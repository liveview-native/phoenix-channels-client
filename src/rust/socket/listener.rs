use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::hash::Hash;
use std::mem;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::FuturesUnordered;
use futures::SinkExt;
use futures::StreamExt;
use log::{debug, error, trace};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::{Instant, Interval, Sleep};
use tokio_tungstenite::{tungstenite, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::ffi::channel::Channel;
use crate::ffi::message::PhoenixEvent;
use crate::ffi::socket::Socket;
use crate::ffi::topic::Topic;
use crate::rust::channel::listener::{JoinedChannelReceivers, LeaveError};
use crate::rust::channel::CallError;
use crate::rust::join_reference::JoinReference;
use crate::rust::message::{
    Broadcast, Control, Event, EventPayload, Message, Payload, Push, Reply, ReplyStatus,
};
use crate::rust::reference::Reference;
use crate::rust::socket::{ConnectError, ShutdownError};
use crate::rust::{channel, socket};

pub(crate) struct Listener {
    url: Arc<Url>,
    cookies: Option<Vec<String>>,
    channel_spawn_rx: mpsc::Receiver<ChannelSpawn>,
    state_command_rx: mpsc::Receiver<StateCommand>,
    channel_state_command_rx: mpsc::Receiver<ChannelStateCommand>,
    channel_send_command_rx: mpsc::Receiver<ChannelSendCommand>,
    connectivity_tx: broadcast::Sender<Connectivity>,
    state: Option<State>,
    socket_status: ObservableStatus,
}
impl Listener {
    pub(crate) fn spawn(
        url: Arc<Url>,
        cookies: Option<Vec<String>>,
        socket_status: ObservableStatus,
        channel_spawn_rx: mpsc::Receiver<ChannelSpawn>,
        state_command_rx: mpsc::Receiver<StateCommand>,
        channel_state_command_rx: mpsc::Receiver<ChannelStateCommand>,
        channel_send_command_rx: mpsc::Receiver<ChannelSendCommand>,
    ) -> JoinHandle<Result<(), ShutdownError>> {
        let listener = Self::init(
            url,
            cookies,
            socket_status,
            channel_spawn_rx,
            state_command_rx,
            channel_state_command_rx,
            channel_send_command_rx,
        );

        tokio::spawn(listener.listen())
    }

    fn init(
        url: Arc<Url>,
        cookies: Option<Vec<String>>,
        socket_status: ObservableStatus,
        channel_spawn_rx: mpsc::Receiver<ChannelSpawn>,
        state_command_rx: mpsc::Receiver<StateCommand>,
        channel_state_command_rx: mpsc::Receiver<ChannelStateCommand>,
        channel_send_command_rx: mpsc::Receiver<ChannelSendCommand>,
    ) -> Self {
        let (connectivity_tx, _) = broadcast::channel(1);

        Self {
            url,
            cookies,
            socket_status,
            channel_spawn_rx,
            state_command_rx,
            channel_state_command_rx,
            channel_send_command_rx,
            connectivity_tx,
            state: Some(State::NeverConnected),
        }
    }

    async fn listen(mut self) -> Result<(), ShutdownError> {
        debug!("starting client worker");

        let result = loop {
            let mut current_state = self.state.take().unwrap();
            let current_discriminant = mem::discriminant(&current_state);

            let next_state = match current_state {
                State::NeverConnected { .. } | State::Disconnected { .. } => tokio::select! {
                    Some(channel_spawn) = self.channel_spawn_rx.recv() => self.spawn_channel(current_state, channel_spawn).await,
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(current_state, state_command).await,
                    else => break Ok(())
                },
                State::Connected(mut connected) => tokio::select! {
                    Some(socket_result) = connected.socket.next() => self.handle_socket_result(connected, socket_result).await?,
                    Some(channel_spawn) = self.channel_spawn_rx.recv() => self.spawn_channel(State::Connected(connected), channel_spawn).await,
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(State::Connected(connected), state_command).await,
                    Some(channel_state_command) = self.channel_state_command_rx.recv() => self.update_channel_state(connected, channel_state_command).await,
                    Some(channel_send_command) = self.channel_send_command_rx.recv() => self.send(connected, channel_send_command).await,
                    Some(join_key) = connected.join_timeouts.next() => {
                        connected.join_timed_out(join_key);

                        State::Connected(connected)
                    },
                    _ = connected.heartbeat.tick() => self.heartbeat(connected).await,
                    else => break Ok(())
                },
                State::WaitingToReconnect {
                    ref mut sleep,
                    reconnect,
                } => tokio::select! {
                    () = sleep => self.reconnect(reconnect).await,
                    Some(channel_spawn) = self.channel_spawn_rx.recv() => self.spawn_channel(current_state, channel_spawn).await,
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(current_state, state_command).await,
                    else => break Ok(())
                },
                State::ShuttingDown | State::ShutDown => break Ok(()),
            };

            self.socket_status.set(next_state.status());

            let next_discriminant = mem::discriminant(&next_state);

            if next_discriminant != current_discriminant {
                debug!("transitioned state to {:#?}", &next_state);
            }

            self.state = Some(next_state);
        };

        let state = State::ShutDown;
        self.socket_status.set(state.status());
        self.state = Some(state);

        result
    }

    async fn spawn_channel(&self, state: State, channel_spawn: ChannelSpawn) -> State {
        let ChannelSpawn {
            socket,
            topic,
            payload,
            sender,
        } = channel_spawn;

        let channel_state = match &state {
            State::NeverConnected | State::Disconnected | State::WaitingToReconnect { .. } => {
                channel::listener::State::WaitingForSocketToConnect { rejoin: None }
            }
            State::Connected(_) => channel::listener::State::WaitingToJoin,
            State::ShuttingDown => channel::listener::State::ShuttingDown,
            State::ShutDown => channel::listener::State::ShutDown,
        };

        let connectivity_rx = self.connectivity_tx.subscribe();

        let channel = Channel::spawn(socket, connectivity_rx, topic, payload, channel_state).await;

        if let Err(channel) = sender.send(channel) {
            channel.shutdown().await.ok();
        }

        state
    }

    async fn update_state(&self, state: State, state_command: StateCommand) -> State {
        match state_command {
            StateCommand::Connect(connect) => self.connect(state, connect).await,
            StateCommand::Disconnect { disconnected_tx } => {
                self.disconnect(state, disconnected_tx).await
            }
            StateCommand::Shutdown => self.shutdown(state).await,
        }
    }

    async fn connect(&self, state: State, connect: Connect) -> State {
        let (connect_result, next_state) = match state {
            State::NeverConnected | State::Disconnected => {
                match self
                    .socket_connect(connect.created_at, connect.reconnect())
                    .await
                {
                    Ok(state) => (Ok(()), state),
                    Err((connect_error, reconnect)) => (Err(connect_error), reconnect.wait()),
                }
            }
            State::WaitingToReconnect { ref sleep, .. } => (
                Err(ConnectError::WaitingToReconnect(sleep.deadline())),
                state,
            ),
            State::Connected { .. } => (Ok(()), state),
            State::ShuttingDown => (Err(ConnectError::ShuttingDown), state),
            State::ShutDown => unreachable!("Connect should not be called when socket is shutdown"),
        };

        connect.connected_tx.send(connect_result).ok();

        next_state
    }

    async fn disconnect(&self, state: State, disconnected_tx: oneshot::Sender<()>) -> State {
        let next_state = match state {
            State::NeverConnected { .. }
            | State::WaitingToReconnect { .. }
            | State::Disconnected { .. }
            | State::ShuttingDown
            | State::ShutDown => state,
            State::Connected(mut connected) => {
                if let Err(error) = connected.socket.close(None).await {
                    debug!("Web socket error while disconnecting: {}", error);
                };

                self.disconnect_connected(connected)
            }
        };

        disconnected_tx.send(()).ok();

        next_state
    }

    fn disconnect_connected(&self, mut connected: Connected) -> State {
        self.send_disconnected(&mut connected, Disconnected::Disconnect);

        State::Disconnected
    }

    fn shutdown_connected(&self, mut connected: Connected) -> State {
        self.send_disconnected(&mut connected, Disconnected::Shutdown);

        State::ShuttingDown
    }

    fn wait_to_reconnect_connected(&self, mut connected: Connected) -> State {
        self.send_disconnected(&mut connected, Disconnected::Reconnect);

        Reconnect {
            connect_timeout: connected.connect_timeout,
            attempts: 0,
        }
        .wait()
    }

    fn send_disconnected(&self, connected: &mut Connected, disconnected: Disconnected) {
        let connectivity = Connectivity::Disconnected(disconnected);
        debug!("Sending connectivity change: {:?}", connectivity);
        self.connectivity_tx.send(connectivity).ok();

        for (_, mut join_by_join_reference) in connected.join_by_reference_by_topic.drain() {
            for (_, join) in join_by_join_reference.drain() {
                join.joined_tx
                    .send(Err(socket::JoinError::Disconnected))
                    .ok();
            }
        }

        // self.joined_channel_txs_by_join_reference_by_topic left senders are not notified as they should get the disconnect message above instead
        connected
            .joined_channel_txs_by_join_reference_by_topic
            .clear();

        for (topic, mut reply_tx_by_reference_by_join_reference) in connected
            .reply_tx_by_reference_by_join_reference_by_topic
            .drain()
        {
            debug!(
                "Sending SocketDisconnected to topic {:#?} channel reply senders",
                topic
            );

            for (join_reference, mut reply_tx_by_reference) in
                reply_tx_by_reference_by_join_reference.drain()
            {
                debug!("Sending SocketDisconnected to topic {:#?} channel joined as {:#?} reply senders", topic, join_reference);

                for (reference, reply_tx) in reply_tx_by_reference.drain() {
                    debug!("Sending SocketDisconnected to topic {:#?} channel joined as {:#?} message {:#?} reply senders", topic, join_reference, reference);

                    reply_tx
                        .send(Err(channel::CallError::SocketDisconnected))
                        .ok();
                }
            }
        }
    }

    async fn handle_socket_result(
        &self,
        connected: Connected,
        result: Result<tungstenite::Message, tungstenite::Error>,
    ) -> Result<State, ShutdownError> {
        match result {
            Ok(message) => Ok(self.handle_socket_message(connected, message).await),
            Err(error) => self.handle_socket_error(connected, error).await,
        }
    }

    async fn handle_socket_message(
        &self,
        mut connected: Connected,
        message: tungstenite::Message,
    ) -> State {
        match message {
            tungstenite::Message::Ping(data) => {
                debug!("client received ping");
                match connected
                    .socket
                    .send(tungstenite::Message::Pong(data))
                    .await
                {
                    Ok(()) => State::Connected(connected),
                    Err(_) => self.wait_to_reconnect_connected(connected),
                }
            }
            tungstenite::Message::Pong(_) => {
                debug!("client received pong");

                State::Connected(connected)
            }
            tungstenite::Message::Close(close_frame) => {
                let infix = match &close_frame {
                    Some(close_frame) => {
                        format!(" ({})", close_frame)
                    }
                    None => "".to_string(),
                };

                debug!(
                    "Client received close{} from server. Waiting to reconnect.",
                    infix
                );

                connected.socket.close(close_frame).await.ok();

                self.wait_to_reconnect_connected(connected)
            }
            msg @ tungstenite::Message::Binary(_) | msg @ tungstenite::Message::Text(_) => {
                match Message::decode(msg) {
                    Ok(Message::Control(control)) => {
                        debug!("received control message: {:#?}", &control);
                        if let Event::Phoenix(PhoenixEvent::Reply) = control.event {
                            if control.reference == connected.sent_heartbeat_reference {
                                // Reset heartbeat timeout
                                debug!("received heartbeat reply, resetting heartbeat timeout");
                                connected.sent_heartbeat_reference = None;
                                connected.heartbeat.reset();
                            }
                        }
                    }
                    Ok(Message::Reply(reply)) if reply.event == PhoenixEvent::Heartbeat => {
                        debug!(
                            "received heartbeat reply not in control format: {:#?}",
                            &reply
                        );
                    }
                    Ok(Message::Reply(reply)) => {
                        connected.handle_reply(reply);
                    }
                    Ok(Message::Push(push)) => {
                        connected.handle_push(push).await;
                    }
                    Ok(Message::Broadcast(broadcast)) => connected.handle_broadcast(broadcast),
                    Err(err) => {
                        debug!("dropping invalid message received from server, due to error decoding: {}", &err);
                    }
                }

                State::Connected(connected)
            }
            tungstenite::Message::Frame(_) => unreachable!(),
        }
    }

    async fn handle_socket_error(
        &self,
        connected: Connected,
        error: tungstenite::Error,
    ) -> Result<State, ShutdownError> {
        match error {
            tungstenite::Error::ConnectionClosed => {
                debug!("connection closed");

                Ok(self.wait_to_reconnect_connected(connected))
            }
            tungstenite::Error::AlreadyClosed => {
                // This shouldn't ever be reached since we handle ConnectionClosed, but treat it the same
                error!("socket already closed");

                Ok(self.wait_to_reconnect_connected(connected))
            }
            tungstenite::Error::Io(_) => {
                error!(
                    "io error encountered while reading from socket: {:?}",
                    &error
                );

                Ok(self.wait_to_reconnect_connected(connected))
            }
            tungstenite::Error::Tls(_) => {
                error!(
                    "TLS error encountered while reading from socket: {:?}",
                    &error
                );

                Ok(self.wait_to_reconnect_connected(connected))
            }
            tungstenite::Error::Capacity(_) => {
                debug!("socket capacity exhausted, dropping message");

                Ok(State::Connected(connected))
            }
            tungstenite::Error::Protocol(_) => {
                debug!("web socket protocol error: {:?}", &error);

                Ok(self.wait_to_reconnect_connected(connected))
            }
            tungstenite::Error::WriteBufferFull(_) => {
                debug!("web socket write buffer full: {:?}", &error);

                Ok(State::Connected(connected))
            }
            tungstenite::Error::Utf8 => {
                debug!("UTF-8 encoding error");

                Ok(State::Connected(connected))
            }
            tungstenite::Error::AttackAttempt => {
                debug!("potential CVE-2023-43669 attempt due to long headers from server");

                Err(ShutdownError::AttackAttempt)
            }
            tungstenite::Error::Url(url_error) => Err(ShutdownError::Url(url_error)),
            tungstenite::Error::Http(response) => Err(ShutdownError::Http(response)),
            tungstenite::Error::HttpFormat(http_error) => {
                Err(ShutdownError::HttpFormat(http_error))
            }
        }
    }

    async fn update_channel_state(
        &self,
        connected: Connected,
        channel_state_command: ChannelStateCommand,
    ) -> State {
        match channel_state_command {
            ChannelStateCommand::Join(join) => self.start_join(connected, join).await,
            ChannelStateCommand::Leave(leave) => self.leave(connected, leave).await,
        }
    }

    async fn start_join(&self, mut connected: Connected, join: Join) -> State {
        let topic = join.topic.clone();
        let join_reference = join.join_reference.clone();
        let reference = Reference::new();

        debug!(
            "attempting to join topic '{}' as {} with ref {}",
            &topic, join_reference, reference
        );

        // Send the join message, and register the join
        let message = Message::Push(Push {
            topic: topic.clone(),
            event_payload: EventPayload {
                event: PhoenixEvent::Join.into(),
                payload: join.payload.clone(),
            },
            join_reference: join_reference.clone(),
            reference: Some(reference),
        });

        let data = message.encode().unwrap();

        match connected.socket.send(data).await {
            Ok(()) => {
                let deadline = join.deadline;

                connected
                    .join_by_reference_by_topic
                    .entry(topic.clone())
                    .or_default()
                    .insert(join_reference.clone(), join);

                connected.join_timeouts.push(Box::pin(Self::join_timeout(
                    deadline,
                    topic,
                    join_reference,
                )));

                State::Connected(connected)
            }
            Err(_) => self.wait_to_reconnect_connected(connected),
        }
    }

    async fn join_timeout(
        deadline: Instant,
        topic: Arc<Topic>,
        join_reference: JoinReference,
    ) -> JoinKey {
        time::sleep_until(deadline.into()).await;

        JoinKey {
            topic,
            join_reference,
        }
    }

    async fn leave(&self, mut connected: Connected, leave: Leave) -> State {
        debug!(
            "client is leaving channel '{}' ({})",
            &leave.topic, &leave.join_reference
        );

        let message = Message::Push(Push {
            topic: leave.topic.clone(),
            event_payload: EventPayload {
                event: PhoenixEvent::Leave.into(),
                payload: Value::Null.into(),
            },
            join_reference: leave.join_reference.clone(),
            reference: None,
        });
        let data = message.encode().unwrap();

        let (next_state, left_result) = match connected.socket.send(data).await {
            Ok(()) => {
                if let Some(JoinedChannelSenders { left: left_tx, .. }) =
                    connected.remove_joined_channel_txs(leave.topic, leave.join_reference)
                {
                    left_tx.send(()).ok();
                }

                (State::Connected(connected), Ok(()))
            }
            Err(web_socket_error) => (
                self.wait_to_reconnect_connected(connected),
                Err(LeaveError::WebSocketError(Arc::new(web_socket_error))),
            ),
        };

        leave.left_tx.send(left_result).ok();

        next_state
    }

    async fn send(&self, connected: Connected, channel_send_command: ChannelSendCommand) -> State {
        match channel_send_command {
            ChannelSendCommand::Call(call) => self.call(connected, call).await,
            ChannelSendCommand::Cast(cast) => self.cast(connected, cast).await,
        }
    }

    async fn call(&self, mut connected: Connected, call: Call) -> State {
        let Call {
            topic,
            join_reference,
            channel_call:
                channel::Call {
                    event_payload,
                    reply_tx,
                },
        } = call;
        let reference = Reference::new();

        trace!(
            "sending event {:?} with payload {:?} to topic {} joined as {} with ref {}, will wait for reply",
            &event_payload.event, &event_payload.payload, &topic, &join_reference, &reference
        );

        let message = Message::Push(Push {
            topic: topic.clone(),
            event_payload,
            join_reference: join_reference.clone(),
            reference: Some(reference.clone()),
        });
        let data = message.encode().unwrap();

        match connected.socket.send(data).await {
            Ok(()) => {
                connected
                    .reply_tx_by_reference_by_join_reference_by_topic
                    .entry(topic)
                    .or_default()
                    .entry(join_reference)
                    .or_default()
                    .insert(reference, reply_tx);

                State::Connected(connected)
            }
            Err(web_socket_error) => {
                reply_tx
                    .send(Err(channel::CallError::WebSocketError(web_socket_error)))
                    .ok();

                self.wait_to_reconnect_connected(connected)
            }
        }
    }

    async fn cast(&self, mut connected: Connected, cast: Cast) -> State {
        let Cast {
            topic,
            join_reference,
            event_payload,
        } = cast;
        let reference = Reference::new();

        debug!(
            "sending message for ref {}, to topic '{}', will ignore replies",
            &reference, &topic
        );
        let message = Message::Push(Push {
            topic,
            event_payload,
            join_reference,
            reference: Some(reference),
        });
        let data = message.encode().unwrap();

        match connected.socket.send(data).await {
            Ok(()) => State::Connected(connected),
            Err(_) => self.wait_to_reconnect_connected(connected),
        }
    }

    async fn heartbeat(&self, mut connected: Connected) -> State {
        match connected.sent_heartbeat_reference {
            Some(_) => {
                debug!("a heartbeat interval passed with no reply to our previous heartbeat, extending deadline..");

                State::Connected(connected)
            }
            None => {
                // Send heartbeat
                debug!("sending heartbeat..");
                let heartbeat_reference = Reference::heartbeat();

                match connected
                    .socket
                    .send(heartbeat_message(heartbeat_reference.clone()))
                    .await
                {
                    Ok(()) => {
                        connected.sent_heartbeat_reference = Some(heartbeat_reference);

                        State::Connected(connected)
                    }
                    Err(_) => self.wait_to_reconnect_connected(connected),
                }
            }
        }
    }

    async fn reconnect(&self, reconnect: Reconnect) -> State {
        match self.socket_connect(Instant::now(), reconnect).await {
            Ok(state) => state,
            Err((_connect_error, reconnect)) => reconnect.wait(),
        }
    }

    async fn socket_connect(
        &self,
        created_at: Instant,
        reconnect: Reconnect,
    ) -> Result<State, (ConnectError, Reconnect)> {
        let url = self.url.clone();
        let host = url.host().expect("Failed to get host from url");
        use tokio_tungstenite::tungstenite::handshake::client::{generate_key, Request};

        // This comes from
        // https://github.com/snapview/tungstenite-rs/blob/2ee05d10803d95ad48b3ad03d9d9a03164060e76/src/client.rs#L219-L242
        let mut request = Request::builder()
            .method("GET")
            .header("Host", host.to_string())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            .uri(url.as_str());

        if let Some(cookies) = &self.cookies {
            for cookie in cookies {
                request = request.header("Cookie", cookie);
            }
        }
        #[cfg(feature = "native-tls")]
        let connector = match url.scheme() {
            "ws" => Some(tokio_tungstenite::Connector::Plain),
            "wss" => {
                let tls_con = native_tls::TlsConnector::new()
                    .map_err(|e| (ConnectError::from(e), reconnect))?;
                Some(tokio_tungstenite::Connector::NativeTls(tls_con))
            }
            other => {
                error!("Scheme {other} is not supported! Use either ws or wss");
                None
            }
        };
        #[cfg(feature = "native-tls")]
        let connector = tokio_tungstenite::connect_async_tls_with_config(
            request.body(()).expect("Failed to build http request"),
            None,
            false,
            connector,
        );
        #[cfg(not(feature = "native-tls"))]
        let connector = tokio_tungstenite::connect_async_with_config(
            request.body(()).expect("Failed to build http request"),
            None,
            false,
        );

        match time::timeout_at(created_at + reconnect.connect_timeout, connector).await {
            Ok(connect_result) => match connect_result {
                Ok((socket, _response)) => {
                    let duration = Duration::from_secs(30);
                    let mut heartbeat =
                        time::interval_at((Instant::now() + duration).into(), duration);
                    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

                    self.connectivity_tx.send(Connectivity::Connected).ok();

                    Ok(State::Connected(Connected {
                        socket,
                        heartbeat,
                        sent_heartbeat_reference: None,
                        join_by_reference_by_topic: Default::default(),
                        join_timeouts: Default::default(),
                        broadcast_by_topic: Default::default(),
                        joined_channel_txs_by_join_reference_by_topic: Default::default(),
                        reply_tx_by_reference_by_join_reference_by_topic: Default::default(),
                        connect_timeout: reconnect.connect_timeout,
                    }))
                }
                Err(error) => {
                    let arc_error = Arc::new(error);
                    debug!("Error connecting to {}: {}", self.url, arc_error);
                    self.socket_status.error(arc_error.clone());

                    Err((arc_error.into(), reconnect))
                }
            },
            Err(_) => Err((ConnectError::Timeout, reconnect)),
        }
    }

    async fn shutdown(&self, state: State) -> State {
        match state {
            State::Connected(mut connected) => {
                debug!("socket is shutting down");
                connected.socket.close(None).await.ok();
                self.shutdown_connected(connected)
            }
            State::NeverConnected { .. }
            | State::WaitingToReconnect { .. }
            | State::Disconnected { .. }
            | State::ShuttingDown
            | State::ShutDown => State::ShuttingDown,
        }
    }
}

#[must_use]
enum State {
    /// [Socket::connect] has never been called.
    NeverConnected,
    /// [Socket::connect] was called and server responded the socket is connected.
    Connected(Connected),
    /// [Socket::connect] was called previously, but the [Socket] was disconnected by the server and
    /// [Socket] needs to wait to reconnect.
    WaitingToReconnect {
        /// When the [Socket] can attempt to automatically reconnect.
        sleep: Pin<Box<Sleep>>,
        /// How long to wait for the next reconnect if this one fails and how many times
        /// reconnecting has been attempted already.
        reconnect: Reconnect,
    },
    /// [Socket::disconnect] was called and the server responded that the socket as disconnected.
    Disconnected,
    /// [Socket::shutdown] was called, but the async task hasn't exited yet.
    ShuttingDown,
    /// The async task has exited.
    ShutDown,
}
impl State {
    pub fn status(&self) -> Status {
        match self {
            State::NeverConnected => Status::NeverConnected,
            State::Connected(_) => Status::Connected,
            State::WaitingToReconnect { sleep, .. } => Status::WaitingToReconnect(sleep.deadline()),
            State::Disconnected => Status::Disconnected,
            State::ShuttingDown => Status::ShuttingDown,
            State::ShutDown => Status::ShutDown,
        }
    }
}
impl Debug for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            State::NeverConnected { .. } => write!(f, "NeverConnected"),
            State::Disconnected { .. } => write!(f, "Disconnected"),
            State::ShuttingDown => write!(f, "ShuttingDown"),
            State::Connected(connected) => f.debug_tuple("Connected").field(connected).finish(),
            State::WaitingToReconnect {
                sleep, reconnect, ..
            } => f
                .debug_struct("WaitingToReconnect")
                .field(
                    "duration_until_sleep_over",
                    &(sleep.deadline() - Instant::now()),
                )
                .field("reconnect", reconnect)
                .finish(),
            State::ShutDown => write!(f, "ShutDown"),
        }
    }
}

/// The status of the [Socket].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Status {
    /// [Socket::connect] has never been called.
    NeverConnected,
    /// [Socket::connect] was called and server responded the socket is connected.
    Connected,
    /// [Socket::connect] was called previously, but the [Socket] was disconnected by the server and
    /// [Socket] needs to wait to reconnect.
    WaitingToReconnect(Instant),
    /// [Socket::disconnect] was called and the server responded that the socket as disconnected.
    Disconnected,
    /// [Socket::shutdown] was called, but the async task hasn't exited yet.
    ShuttingDown,
    /// The async task has exited.
    ShutDown,
}
impl Default for Status {
    fn default() -> Self {
        Status::NeverConnected
    }
}

pub(crate) type ObservableStatus =
    crate::rust::observable_status::ObservableStatus<Status, Arc<tungstenite::Error>>;

#[must_use]
struct Connected {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    heartbeat: Interval,
    sent_heartbeat_reference: Option<Reference>,
    join_by_reference_by_topic: HashMap<Arc<Topic>, HashMap<JoinReference, Join>>,
    join_timeouts: FuturesUnordered<Pin<Box<dyn Future<Output = JoinKey> + Send + Sync + 'static>>>,
    broadcast_by_topic: HashMap<Arc<Topic>, broadcast::Sender<Broadcast>>,
    joined_channel_txs_by_join_reference_by_topic:
        HashMap<Arc<Topic>, HashMap<JoinReference, JoinedChannelSenders>>,
    reply_tx_by_reference_by_join_reference_by_topic: HashMap<
        Arc<Topic>,
        HashMap<
            JoinReference,
            HashMap<Reference, oneshot::Sender<Result<Payload, channel::CallError>>>,
        >,
    >,
    connect_timeout: Duration,
}
impl Connected {
    fn handle_reply(&mut self, reply: Reply) {
        // Check if this is a join reply
        if let Some(join) = self.remove_join(reply.topic.clone(), reply.join_reference.clone()) {
            self.finish_join(reply, join);
        } else if let Some(reply_tx) = self.remove_reply_tx(
            reply.topic.clone(),
            reply.join_reference.clone(),
            reply.reference.clone(),
        ) {
            debug!(
                "received reply on topic {} joined as {} to message ref {}, status is {}",
                &reply.topic, &reply.join_reference, &reply.reference, &reply.status
            );
            let result = match reply.status {
                ReplyStatus::Ok => Ok(reply.payload),
                ReplyStatus::Error => Err(CallError::Reply(reply.payload)),
            };
            reply_tx.send(result).ok();
        } else {
            debug!(
                "received reply on topic {} joined as {} to message ref {} with status {}, but it was ignored",
                &reply.topic, &reply.join_reference, &reply.reference, &reply.status
            );
        }
    }

    fn remove_join(&mut self, topic: Arc<Topic>, join_reference: JoinReference) -> Option<Join> {
        Self::remove_by_reference_by_topic(
            &mut self.join_by_reference_by_topic,
            topic,
            join_reference,
        )
    }

    fn finish_join(&mut self, reply: Reply, join: Join) {
        // If the join failed, then notify the waiter and bail
        let joined_result = match reply.status {
            ReplyStatus::Error => {
                debug!(
                    "client received error in response to joining topic {} as {} with reference {}: {}",
                    &join.topic, &reply.join_reference, &reply.reference, &reply.payload
                );

                Err(socket::JoinError::Rejected(reply.payload))
            }
            ReplyStatus::Ok => {
                debug!(
                    "client received ok in response to joining topic {} as {} with reference {}, finalizing..",
                    &join.topic, &reply.join_reference, &reply.reference
                );

                // Create a new channel for receiving direct messages from the server
                let (push_tx, push_rx) = mpsc::channel(10);

                // Obtain a new broadcast subscription for the joined channel
                let broadcast_rx = match self.broadcast_by_topic.entry(join.topic.clone()) {
                    Entry::Occupied(entry) => entry.get().subscribe(),
                    Entry::Vacant(entry) => {
                        let (broadcasts_tx, broadcast_rx) = broadcast::channel::<Broadcast>(50);
                        entry.insert(broadcasts_tx);
                        broadcast_rx
                    }
                };

                let (left_tx, left_rx) = oneshot::channel();

                // Register the channel so we can handle relaying on its behalf
                self.joined_channel_txs_by_join_reference_by_topic
                    .entry(join.topic.clone())
                    .or_default()
                    .insert(
                        reply.join_reference.clone(),
                        JoinedChannelSenders {
                            push: push_tx,
                            left: left_tx,
                        },
                    );

                // Send the channel join event to the channel listener
                let joined = JoinedChannelReceivers {
                    push: push_rx,
                    broadcast: broadcast_rx,
                    left: left_rx,
                    payload: reply.payload,
                };

                Ok(joined)
            }
        };

        join.joined_tx
            .send(joined_result)
            // Don't care if `join` method went away
            .ok();
    }

    fn remove_joined_channel_txs(
        &mut self,
        topic: Arc<Topic>,
        join_reference: JoinReference,
    ) -> Option<JoinedChannelSenders> {
        Self::remove_by_reference_by_topic(
            &mut self.joined_channel_txs_by_join_reference_by_topic,
            topic,
            join_reference,
        )
    }

    fn remove_reply_tx(
        &mut self,
        topic: Arc<Topic>,
        join_reference: JoinReference,
        reference: Reference,
    ) -> Option<oneshot::Sender<Result<Payload, channel::CallError>>> {
        let Entry::Occupied(mut reply_tx_by_reference_by_join_reference_entry) = self
            .reply_tx_by_reference_by_join_reference_by_topic
            .entry(topic.clone())
        else {
            return None;
        };

        let reply_tx_by_reference_by_join_reference =
            reply_tx_by_reference_by_join_reference_entry.get_mut();

        let Entry::Occupied(mut reply_tx_by_reference_entry) =
            reply_tx_by_reference_by_join_reference.entry(join_reference)
        else {
            return None;
        };

        let reply_tx_by_reference = reply_tx_by_reference_entry.get_mut();

        let Entry::Occupied(reply_tx_entry) = reply_tx_by_reference.entry(reference) else {
            return None;
        };
        let reply_tx = reply_tx_entry.remove();

        if reply_tx_by_reference.is_empty() {
            reply_tx_by_reference_entry.remove();

            if reply_tx_by_reference_by_join_reference.is_empty() {
                reply_tx_by_reference_by_join_reference_entry.remove();
            }
        }

        Some(reply_tx)
    }

    fn remove_by_reference_by_topic<V, R>(
        value_by_reference_by_topic: &mut HashMap<Arc<Topic>, HashMap<R, V>>,
        topic: Arc<Topic>,
        reference: R,
    ) -> Option<V>
    where
        R: Eq + PartialEq + Hash,
    {
        if let Entry::Occupied(mut value_by_reference_entry) =
            value_by_reference_by_topic.entry(topic.clone())
        {
            let value_by_reference = value_by_reference_entry.get_mut();

            if let Entry::Occupied(value_entry) = value_by_reference.entry(reference) {
                let value = value_entry.remove();

                if value_by_reference.is_empty() {
                    value_by_reference_entry.remove();
                }

                Some(value)
            } else {
                None
            }
        } else {
            None
        }
    }

    async fn handle_push(&self, push: Push) {
        debug!("received push: {:#?}", &push);

        if let Some(JoinedChannelSenders { push: push_tx, .. }) = self
            .joined_channel_txs_by_join_reference_by_topic
            .get(&push.topic)
            .and_then(|push_tx_by_reference| push_tx_by_reference.get(&push.join_reference.clone()))
        {
            push_tx.send(push).await.ok();
        }
    }

    fn handle_broadcast(&self, broadcast: Broadcast) {
        debug!("received broadcast: {:#?}", &broadcast);
        if let Some(broadcaster) = self.broadcast_by_topic.get(&broadcast.topic) {
            broadcaster.send(broadcast).ok();
        }
    }

    fn join_timed_out(&mut self, join_key: JoinKey) {
        if let Some(Join { joined_tx, .. }) =
            self.remove_join(join_key.topic, join_key.join_reference)
        {
            joined_tx
                .send(Err(socket::JoinError::Timeout))
                // Don't care if join method went away
                .ok();
        }
    }
}
impl Debug for Connected {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut join_references_by_topic: HashMap<&Topic, Vec<&JoinReference>> = HashMap::new();

        for (topic, joined_channel_txs_by_join_reference) in
            &self.joined_channel_txs_by_join_reference_by_topic
        {
            join_references_by_topic
                .insert(topic, joined_channel_txs_by_join_reference.keys().collect());
        }

        let mut reply_references_by_join_reference_by_topic: HashMap<
            &Topic,
            HashMap<&JoinReference, Vec<&Reference>>,
        > = HashMap::new();

        for (topic, reply_tx_by_reference_by_join_reference) in
            &self.reply_tx_by_reference_by_join_reference_by_topic
        {
            let mut reply_references_by_join_reference = HashMap::new();

            for (join_reference, reply_tx_by_reference) in reply_tx_by_reference_by_join_reference {
                reply_references_by_join_reference
                    .insert(join_reference, reply_tx_by_reference.keys().collect());
            }

            reply_references_by_join_reference_by_topic
                .insert(topic, reply_references_by_join_reference);
        }

        f.debug_struct("Connected")
            // skip `socket` because it's too noisy
            // skip `heartbeat` because it's to noisy
            .field("sent_heartbeat_reference", &self.sent_heartbeat_reference)
            .field(
                "join_by_reference_by_topic",
                &self.join_by_reference_by_topic,
            )
            // join_timeouts has nothing helpful
            .field("broadcast_topics", &self.broadcast_by_topic.keys())
            .field("join_references_by_topic", &join_references_by_topic)
            .field(
                "reply_references_by_join_reference_by_topic",
                &reply_references_by_join_reference_by_topic,
            )
            .field("connect_timeout", &self.connect_timeout)
            .finish_non_exhaustive()
    }
}

#[derive(Copy, Clone, Debug)]
struct Reconnect {
    connect_timeout: Duration,
    /// Enough for > 1 week of attempts; u8 would only be 42 minutes of attempts.
    attempts: u16,
}
impl Reconnect {
    fn wait(self) -> State {
        State::WaitingToReconnect {
            sleep: Box::pin(time::sleep(self.sleep_duration())),
            reconnect: self.next(),
        }
    }

    fn next(self) -> Self {
        Self {
            connect_timeout: self.connect_timeout,
            attempts: self.attempts.saturating_add(1),
        }
    }

    fn sleep_duration(&self) -> Duration {
        self.connect_timeout * self.sleep_duration_multiplier()
    }

    fn sleep_duration_multiplier(&self) -> u32 {
        Self::SLEEP_DURATION_MULTIPLIERS
            [(self.attempts as usize).min(Self::SLEEP_DURATION_MULTIPLIERS.len() - 1)]
    }

    const SLEEP_DURATION_MULTIPLIERS: &'static [u32] = &[0, 1, 2, 5, 10];
}

struct JoinKey {
    topic: Arc<Topic>,
    join_reference: JoinReference,
}

#[derive(Debug)]
struct JoinedChannelSenders {
    push: mpsc::Sender<Push>,
    left: oneshot::Sender<()>,
}

#[derive(Clone, Debug)]
pub(crate) enum Connectivity {
    Connected,
    Disconnected(Disconnected),
}

#[derive(Clone, Debug)]
pub(crate) enum Disconnected {
    /// [Socket::disconnect]
    Disconnect,
    /// [Socket::shutdown]
    Shutdown,
    /// We're reconnecting automatically; channels should rejoin when [Connectivity::Connected] occurs next.
    Reconnect,
}

pub(crate) struct ChannelSpawn {
    pub socket: Arc<Socket>,
    pub topic: Arc<Topic>,
    pub payload: Option<Payload>,
    pub sender: oneshot::Sender<Channel>,
}

pub(crate) enum StateCommand {
    Connect(Connect),
    Disconnect {
        disconnected_tx: oneshot::Sender<()>,
    },
    /// Tells the client to shutdown the socket, and disconnect all channels
    Shutdown,
}

pub(crate) enum ChannelStateCommand {
    /// Tells the client to join `channel` to its desired topic, and then:
    ///
    /// * If joining was successful, notify the channel listener via `on_join`, and then tell
    /// the caller the result of the overall join operation via `notify`
    /// * If joining was unsuccessful, tell the caller via `notify`, and drop `on_join` to notify the
    /// channel listener that the channel is being closed.
    Join(Join),
    /// Tells the client to leave the given channel
    Leave(Leave),
}

/// Represents a command to the socket listener to join a channel
#[doc(hidden)]
pub(crate) struct Join {
    /// [Channel] topic
    pub topic: Arc<Topic>,
    /// The [Channel] join reference
    pub join_reference: JoinReference,
    /// The params sent when `topic` is joined.
    pub payload: Payload,
    /// The instant at which this join must complete
    pub deadline: Instant,
    /// Sends back to [Channel::join] from the server
    pub joined_tx: oneshot::Sender<Result<JoinedChannelReceivers, socket::JoinError>>,
}
impl Debug for Join {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Join")
            .field("topic", &self.topic)
            .field("join_reference", &self.join_reference)
            .field("payload", &self.payload)
            .field("duration_until_deadline", &(self.deadline - Instant::now()))
            // skip `joined_tx` because its debug is not useful
            .finish_non_exhaustive()
    }
}

/// Represents a command to the socket listener to leave a channel
#[doc(hidden)]
#[derive(Debug)]
pub(crate) struct Leave {
    pub topic: Arc<Topic>,
    pub join_reference: JoinReference,
    pub left_tx: oneshot::Sender<Result<(), LeaveError>>,
}

/// This enum encodes the various commands that can be used to control the socket listener
#[doc(hidden)]
#[derive(Debug)]
pub(crate) enum ChannelSendCommand {
    /// Tells the client to send `push`, and if successful, send the reply to `on_reply`
    Call(Call),
    /// Tells the client to send `push`, but to ignore any replies
    Cast(Cast),
}

#[derive(Debug)]
pub(crate) struct Call {
    pub topic: Arc<Topic>,
    pub join_reference: JoinReference,
    pub channel_call: channel::Call,
}

#[derive(Debug)]
pub(crate) struct Cast {
    pub topic: Arc<Topic>,
    pub join_reference: JoinReference,
    pub event_payload: EventPayload,
}

pub struct Connect {
    /// When the connect was created
    pub created_at: Instant,
    /// How long after `created_at` must the connect complete
    pub timeout: Duration,
    pub connected_tx: oneshot::Sender<Result<(), ConnectError>>,
}
impl Connect {
    fn reconnect(&self) -> Reconnect {
        Reconnect {
            connect_timeout: self.timeout,
            attempts: 0,
        }
    }
}

#[inline]
fn heartbeat_message(reference: Reference) -> tungstenite::Message {
    Message::encode(Message::Control(Control {
        event: Event::Phoenix(PhoenixEvent::Heartbeat),
        reference: Some(reference),
        payload: Value::Null.into(),
    }))
    .unwrap()
}
