use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::mem;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use log::debug;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task;
use tokio::task::JoinHandle;
use tokio::time::{Instant, Sleep};
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::error::UrlError;
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::http::Response;

use crate::channel::Status;
use crate::join_reference::JoinReference;
use crate::message::{Broadcast, Push};
use crate::socket::listener::{Connectivity, Disconnected};
use crate::topic::Topic;
use crate::{
    channel, socket, Event, EventHandler, JoinError, Payload, Socket, SubscriptionReference,
};

pub(super) struct Listener {
    socket: Arc<Socket>,
    socket_connectivity_receiver: broadcast::Receiver<Connectivity>,
    topic: Topic,
    payload: Arc<Payload>,
    channel_status: Arc<AtomicU32>,
    shutdown_receiver: oneshot::Receiver<()>,
    subscription_command_receiver: mpsc::Receiver<SubscriptionCommand>,
    handler_by_subscription_reference_by_event:
        HashMap<Event, HashMap<SubscriptionReference, EventHandler>>,
    state_command_receiver: mpsc::Receiver<StateCommand>,
    state: Option<State>,
    send_command_receiver: mpsc::Receiver<SendCommand>,
    join_reference: JoinReference,
}
impl Listener {
    pub(super) fn spawn(
        socket: Arc<Socket>,
        socket_connectivity_receiver: broadcast::Receiver<Connectivity>,
        topic: Topic,
        payload: Arc<Payload>,
        state: State,
        channel_status: Arc<AtomicU32>,
        shutdown_receiver: oneshot::Receiver<()>,
        subscription_command_receiver: mpsc::Receiver<SubscriptionCommand>,
        state_command_receiver: mpsc::Receiver<StateCommand>,
        send_command_receiver: mpsc::Receiver<SendCommand>,
    ) -> JoinHandle<Result<(), ShutdownError>> {
        let listener = Self::init(
            socket,
            socket_connectivity_receiver,
            topic,
            payload,
            state,
            channel_status,
            shutdown_receiver,
            subscription_command_receiver,
            state_command_receiver,
            send_command_receiver,
        );

        tokio::spawn(listener.listen())
    }

    fn init(
        socket: Arc<Socket>,
        socket_connectivity_receiver: broadcast::Receiver<Connectivity>,
        topic: Topic,
        payload: Arc<Payload>,
        state: State,
        channel_status: Arc<AtomicU32>,
        shutdown_receiver: oneshot::Receiver<()>,
        subscription_command_receiver: mpsc::Receiver<SubscriptionCommand>,
        state_command_receiver: mpsc::Receiver<StateCommand>,
        send_command_receiver: mpsc::Receiver<SendCommand>,
    ) -> Self {
        Self {
            socket,
            socket_connectivity_receiver,
            topic,
            payload,
            state: Some(state),
            channel_status,
            shutdown_receiver,
            subscription_command_receiver,
            handler_by_subscription_reference_by_event: Default::default(),
            state_command_receiver,
            send_command_receiver,
            join_reference: JoinReference::new(),
        }
    }

    async fn listen(mut self) -> Result<(), ShutdownError> {
        debug!(
            "Channel listener started for topic {} will join as {}",
            &self.topic, &self.join_reference
        );

        loop {
            let mut current_state = self.state.take().unwrap();
            let current_discriminant = mem::discriminant(&current_state);

            let next_state = match current_state {
                State::WaitingForSocketToConnect { .. } => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_receiver => current_state.shutdown(),
                    Ok(socket_connectivity) = self.socket_connectivity_receiver.recv() => current_state.connectivity_changed(socket_connectivity),
                    Some(subscription_command) = self.subscription_command_receiver.recv() => {
                        self.update_subscriptions(subscription_command).await;

                        current_state
                    },
                    else => break Ok(())
                },
                State::WaitingToJoin => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_receiver => current_state.shutdown(),
                    Ok(socket_connectivity) = self.socket_connectivity_receiver.recv() => current_state.connectivity_changed(socket_connectivity),
                    Some(subscription_command) = self.subscription_command_receiver.recv() => {
                        self.update_subscriptions(subscription_command).await;

                        current_state
                    },
                    Some(state_command) = self.state_command_receiver.recv() => self.update_state(current_state, state_command).await?,
                    else => break Ok(())
                },
                State::Joining(mut joining) => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_receiver => State::Joining(joining).shutdown(),
                    socket_joined_result = &mut joining.socket_joined_receiver => self.joined_result_result_received(joining, socket_joined_result).await?,
                    Some(subscription_command) = self.subscription_command_receiver.recv() => {
                        self.update_subscriptions(subscription_command).await;

                        State::Joining(joining)
                    },
                    Some(state_command) = self.state_command_receiver.recv() => self.update_state(State::Joining(joining), state_command).await?,
                    else => break Ok(())
                },
                State::WaitingToRejoin {
                    ref mut sleep,
                    rejoin,
                } => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_receiver => current_state.shutdown(),
                    Ok(socket_connectivity) = self.socket_connectivity_receiver.recv() => current_state.connectivity_changed(socket_connectivity),
                    () = sleep => self.rejoin(current_state, rejoin).await?,
                    Some(subscription_command) = self.subscription_command_receiver.recv() => {
                        self.update_subscriptions(subscription_command).await;

                        current_state
                    },
                    Some(state_command) = self.state_command_receiver.recv() => self.update_state(current_state, state_command).await?,
                    else => break Ok(())
                },
                State::Joined(mut joined) => tokio::select! {
                        // left_receiver should be polled before others, so that that the socket
                        // disconnected or is reconnecting is seen before the broadcast_receiver's
                        // sender being dropped and a `Err(broadcast::error::RecvError::Closed)`
                        // being received
                        biased;

                        _ = &mut self.shutdown_receiver => State::Joined(joined).shutdown(),
                        Ok(socket_connectivity) = self.socket_connectivity_receiver.recv() => State::Joined(joined).connectivity_changed(socket_connectivity),
                        Ok(()) = &mut joined.left_receiver => State::Left,
                        Some(subscription_command) = self.subscription_command_receiver.recv() => {
                            self.update_subscriptions(subscription_command).await;

                            State::Joined(joined)
                        },
                        Some(state_command) = self.state_command_receiver.recv() => self.update_state(State::Joined(joined), state_command).await?,
                        Some(send_command) = self.send_command_receiver.recv() => self.send(joined, send_command).await,
                        Some(push) = joined.push_receiver.recv() => {
                            self.push_received(push).await;

                            State::Joined(joined)
                        }
                        Ok(broadcast) = joined.broadcast_receiver.recv() => {
                            self.broadcast_received(broadcast).await;

                            State::Joined(joined)
                        }
                        else => break Ok(())
                },
                State::Leaving(mut leaving) => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_receiver => State::Leaving(leaving).shutdown(),
                    Ok(socket_connectivity) = self.socket_connectivity_receiver.recv() => State::Leaving(leaving).connectivity_changed(socket_connectivity),
                    Ok(left_result) = &mut leaving.socket_left_receiver => leaving.left(left_result),
                    Some(subscription_command) = self.subscription_command_receiver.recv() => {
                        self.update_subscriptions(subscription_command).await;

                         State::Leaving(leaving)
                    },
                    Some(state_command) = self.state_command_receiver.recv() => self.update_state(State::Leaving(leaving), state_command).await?,
                    else => break Ok(())
                },
                State::Left => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_receiver => current_state.shutdown(),
                    Ok(socket_connectivity) = self.socket_connectivity_receiver.recv() => current_state.connectivity_changed(socket_connectivity),
                    Some(subscription_command) = self.subscription_command_receiver.recv() => {
                        self.update_subscriptions(subscription_command).await;

                        current_state
                    },
                    Some(state_command) = self.state_command_receiver.recv() => self.update_state(current_state, state_command).await?,
                    else => break Ok(())
                },
                State::ShuttingDown => break Ok(()),
            };

            self.channel_status
                .store(next_state.status() as u32, Ordering::SeqCst);

            let next_discriminant = mem::discriminant(&next_state);

            if next_discriminant != current_discriminant {
                debug!("transitioned state to {:#?}", &next_state);
            }

            self.state = Some(next_state);
        }
    }

    async fn update_subscriptions(&mut self, subscription_command: SubscriptionCommand) {
        match subscription_command {
            SubscriptionCommand::Subscribe(subscribe) => self.subscribe(subscribe).await,
            SubscriptionCommand::UnsubscribeSubscription(unsubscribe_subscription) => {
                self.unsubscribe_subscription(unsubscribe_subscription)
            }
            SubscriptionCommand::UnsubscribeEvent(event) => self.unsubscribe_event(event),
        }
    }

    async fn subscribe(&mut self, subscribe: Subscribe) {
        let Subscribe {
            event,
            handler,
            subscription_reference_sender,
        } = subscribe;

        let subscription_reference = SubscriptionReference::new();

        self.handler_by_subscription_reference_by_event
            .entry(event)
            .or_default()
            .insert(subscription_reference.clone(), handler);

        subscription_reference_sender
            .send(subscription_reference)
            // Don't care if [Channel::on] does away.
            .ok();
    }

    fn unsubscribe_subscription(&mut self, unsubscribe_subscription: UnsubscribeSubscription) {
        let UnsubscribeSubscription {
            event,
            subscription_reference,
        } = unsubscribe_subscription;

        if let Entry::Occupied(mut handler_by_subscription_reference_entry) = self
            .handler_by_subscription_reference_by_event
            .entry(event.clone())
        {
            let handler_by_subscription_reference =
                handler_by_subscription_reference_entry.get_mut();

            if let Some(_) = handler_by_subscription_reference.remove(&subscription_reference) {
                if handler_by_subscription_reference.is_empty() {
                    handler_by_subscription_reference_entry.remove();
                }
            }
        }
    }

    fn unsubscribe_event(&mut self, event: Event) {
        self.handler_by_subscription_reference_by_event
            .remove(&event);
    }

    async fn update_state(
        &self,
        state: State,
        state_command: StateCommand,
    ) -> Result<State, ShutdownError> {
        match state_command {
            StateCommand::Join {
                created_at,
                timeout,
                joined_sender,
            } => self.join(state, created_at, timeout, joined_sender).await,
            StateCommand::Leave { left_sender } => Ok(self.leave(state, left_sender).await),
        }
    }

    async fn join(
        &self,
        mut state: State,
        created_at: Instant,
        timeout: Duration,
        channel_joined_sender: oneshot::Sender<Result<(), JoinError>>,
    ) -> Result<State, ShutdownError> {
        match state {
            State::WaitingForSocketToConnect { .. } => unreachable!(),
            State::WaitingToJoin { .. } | State::Leaving { .. } | State::Left { .. } => {
                self.socket_join_if_not_timed_out(state, created_at, timeout, channel_joined_sender)
                    .await
            }
            State::Joining(Joining {
                ref mut channel_joined_senders,
                ..
            }) => {
                channel_joined_senders.push(channel_joined_sender);

                Ok(state)
            }
            State::WaitingToRejoin { ref sleep, .. } => {
                channel_joined_sender
                    .send(Err(JoinError::WaitingToRejoin(sleep.deadline())))
                    .ok();

                Ok(state)
            }
            State::Joined { .. } => {
                channel_joined_sender.send(Ok(())).ok();

                Ok(state)
            }
            State::ShuttingDown => {
                channel_joined_sender
                    .send(Err(JoinError::ShuttingDown))
                    .ok();

                Ok(state)
            }
        }
    }

    async fn socket_join_if_not_timed_out(
        &self,
        state: State,
        created_at: Instant,
        timeout: Duration,
        channel_joined_sender: oneshot::Sender<Result<(), JoinError>>,
    ) -> Result<State, ShutdownError> {
        let deadline = created_at + timeout;
        let rejoin = Rejoin {
            join_timeout: timeout,
            attempts: 0,
        };

        if Instant::now() <= deadline {
            self.socket_join(state, created_at, rejoin, vec![channel_joined_sender])
                .await
        } else {
            debug!("joining {} timed out before sending to socket", self.topic);

            Ok(rejoin.wait())
        }
    }

    async fn joined_result_result_received(
        &self,
        joining: Joining,
        joined_result_result: Result<
            Result<JoinedChannelReceivers, socket::JoinError>,
            oneshot::error::RecvError,
        >,
    ) -> Result<State, ShutdownError> {
        match joined_result_result {
            Ok(joined_result) => Ok(self.joined_result_received(joining, joined_result)),
            Err(_) => Err(Self::socket_shutdown(State::Joining(joining))),
        }
    }

    fn joined_result_received(
        &self,
        joining: Joining,
        result: Result<JoinedChannelReceivers, socket::JoinError>,
    ) -> State {
        let Joining {
            channel_joined_senders,
            rejoin,
            ..
        } = joining;

        let (channel_joined_result, next_state) = match result {
            Ok(JoinedChannelReceivers {
                push: push_receiver,
                broadcast: broadcast_receiver,
                left: left_receiver,
            }) => (
                Ok(()),
                State::Joined(Joined {
                    push_receiver,
                    broadcast_receiver,
                    left_receiver,
                    join_timeout: rejoin.join_timeout,
                }),
            ),
            Err(socket_join_error) => match socket_join_error {
                socket::JoinError::Shutdown => (Err(JoinError::ShuttingDown), State::ShuttingDown),
                socket::JoinError::Rejected(payload) => {
                    (Err(JoinError::Rejected(payload)), State::Left)
                }
                socket::JoinError::Disconnected => {
                    (Err(JoinError::SocketDisconnected), rejoin.wait())
                }
                socket::JoinError::Timeout => (Err(JoinError::Timeout), rejoin.wait()),
            },
        };

        State::send_to_channel_senders(channel_joined_senders, channel_joined_result);

        next_state
    }

    async fn rejoin(&self, state: State, rejoin: Rejoin) -> Result<State, ShutdownError> {
        self.socket_join(state, Instant::now(), rejoin, vec![])
            .await
    }

    async fn socket_join(
        &self,
        mut state: State,
        created_at: Instant,
        rejoin: Rejoin,
        channel_joined_senders: Vec<oneshot::Sender<Result<(), JoinError>>>,
    ) -> Result<State, ShutdownError> {
        match self
            .socket
            .join(
                self.topic.clone(),
                self.join_reference.clone(),
                self.payload.clone(),
                created_at + rejoin.join_timeout,
            )
            .await
        {
            Ok(socket_joined_receiver) => {
                let channel_left_senders = if let State::Leaving(Leaving {
                    ref mut channel_left_senders,
                    ..
                }) = state
                {
                    Some(channel_left_senders.drain(..).collect())
                } else {
                    None
                };

                if let Some(channel_left_senders) = channel_left_senders {
                    State::send_to_channel_senders(
                        channel_left_senders,
                        Err(LeaveError::JoinBeforeLeft),
                    );
                };

                Ok(State::Joining(Joining {
                    socket_joined_receiver,
                    channel_joined_senders,
                    rejoin,
                }))
            }
            Err(_) => Err(Self::socket_shutdown(state)),
        }
    }

    fn socket_shutdown(state: State) -> ShutdownError {
        match state {
            State::WaitingForSocketToConnect { .. }
            | State::WaitingToJoin { .. }
            | State::WaitingToRejoin { .. }
            | State::Joined { .. }
            | State::Left { .. }
            | State::ShuttingDown => {}
            State::Joining(Joining {
                channel_joined_senders,
                ..
            }) => {
                State::send_to_channel_senders(channel_joined_senders, Err(JoinError::Shutdown));
            }
            State::Leaving(Leaving {
                channel_left_senders,
                ..
            }) => State::send_to_channel_senders(
                channel_left_senders,
                Err(LeaveError::SocketShutdown),
            ),
        }

        ShutdownError::SocketShutdown
    }

    /// Used during graceful termination to tell the server we're leaving the channel
    async fn leave(
        &self,
        mut state: State,
        left_sender: oneshot::Sender<Result<(), LeaveError>>,
    ) -> State {
        match state {
            State::WaitingForSocketToConnect { .. }
            | State::WaitingToJoin { .. }
            | State::Left { .. }
            | State::ShuttingDown => {
                left_sender.send(Ok(())).ok();

                state
            }
            State::Joining(joining) => {
                match self
                    .socket
                    .leave(self.topic.clone(), self.join_reference.clone())
                    .await
                {
                    Ok(socket_left_receiver) => {
                        State::send_to_channel_senders(
                            joining.channel_joined_senders,
                            Err(JoinError::LeavingWhileJoining),
                        );

                        State::Leaving(Leaving {
                            socket_left_receiver,
                            channel_left_senders: vec![left_sender],
                        })
                    }
                    Err(leave_error) => {
                        left_sender.send(Err(leave_error)).ok();

                        State::Joining(joining)
                    }
                }
            }
            State::Joined(joined) => match self
                .socket
                .leave(self.topic.clone(), self.join_reference.clone())
                .await
            {
                Ok(socket_left_receiver) => State::Leaving(Leaving {
                    socket_left_receiver,
                    channel_left_senders: vec![left_sender],
                }),
                Err(leave_error) => {
                    left_sender.send(Err(leave_error)).ok();

                    State::Joined(joined)
                }
            },
            State::WaitingToRejoin { .. } => {
                left_sender.send(Ok(())).ok();

                State::Left
            }
            State::Leaving(Leaving {
                ref mut channel_left_senders,
                ..
            }) => {
                channel_left_senders.push(left_sender);

                state
            }
        }
    }

    async fn send(&self, joined: Joined, send_command: SendCommand) -> State {
        match send_command {
            SendCommand::Cast(cast) => self.cast(joined, cast).await,
            SendCommand::Call(call) => self.call(joined, call).await,
        }
    }

    async fn cast(&self, joined: Joined, cast: Cast) -> State {
        match self
            .socket
            .cast(self.topic.clone(), self.join_reference.clone(), cast)
            .await
        {
            Ok(()) => State::Joined(joined),
            Err(cast_error) => match cast_error {
                socket::CastError::Shutdown => State::ShuttingDown,
            },
        }
    }

    async fn call(&self, joined: Joined, call: Call) -> State {
        match self
            .socket
            .call(self.topic.clone(), self.join_reference.clone(), call)
            .await
        {
            Ok(()) => State::Joined(joined),
            Err(call_error) => match call_error {
                socket::CallError::Shutdown => State::ShuttingDown,
            },
        }
    }

    async fn push_received(&self, push: Push) {
        self.handle_event(&push.event, push.payload.clone()).await
    }

    async fn broadcast_received(&self, broadcast: Broadcast) {
        self.handle_event(&broadcast.event, broadcast.payload.clone())
            .await
    }

    async fn handle_event(&self, event: &Event, payload: Arc<Payload>) {
        if let Some(handler_by_subscription_reference) =
            self.handler_by_subscription_reference_by_event.get(event)
        {
            for handler in handler_by_subscription_reference.values() {
                tokio::spawn(handler(payload.clone()));
                task::consume_budget().await;
            }
        }
    }
}

pub(super) enum SubscriptionCommand {
    Subscribe(Subscribe),
    UnsubscribeSubscription(UnsubscribeSubscription),
    UnsubscribeEvent(Event),
}

pub(super) struct Subscribe {
    pub event: Event,
    pub handler: EventHandler,
    pub subscription_reference_sender: oneshot::Sender<SubscriptionReference>,
}

pub(super) struct UnsubscribeSubscription {
    pub event: Event,
    pub subscription_reference: SubscriptionReference,
}

pub(super) enum StateCommand {
    Join {
        /// When the join was created
        created_at: Instant,
        /// How long after `created_at` must the join complete
        timeout: Duration,
        joined_sender: oneshot::Sender<Result<(), JoinError>>,
    },
    Leave {
        left_sender: oneshot::Sender<Result<(), LeaveError>>,
    },
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum LeaveError {
    #[error("timeout joining channel")]
    Timeout,
    #[error("channel shutting down")]
    ShuttingDown,
    #[error("channel already shutdown")]
    Shutdown,
    #[error("socket already shutdown")]
    SocketShutdown,
    #[error("web socket error {0}")]
    WebSocketError(Arc<tungstenite::Error>),
    #[error("server rejected leave")]
    Rejected(Arc<Payload>),
    #[error("A join was initiated before the server could respond if leave was successful")]
    JoinBeforeLeft,
    #[error("URL error: {0}")]
    Url(Arc<UrlError>),
    #[error("HTTP error: {}", .0.status())]
    Http(Arc<Response<Option<String>>>),
    /// HTTP format error.
    #[error("HTTP format error: {0}")]
    HttpFormat(Arc<http::Error>),
}
impl From<mpsc::error::SendError<StateCommand>> for LeaveError {
    fn from(_: mpsc::error::SendError<StateCommand>) -> Self {
        LeaveError::Shutdown
    }
}
impl From<socket::listener::ShutdownError> for LeaveError {
    fn from(shutdown_error: socket::listener::ShutdownError) -> Self {
        match shutdown_error {
            socket::listener::ShutdownError::AlreadyJoined => LeaveError::SocketShutdown,
            socket::listener::ShutdownError::Url(url_error) => LeaveError::Url(Arc::new(url_error)),
            socket::listener::ShutdownError::Http(response) => LeaveError::Http(Arc::new(response)),
            socket::listener::ShutdownError::HttpFormat(error) => {
                LeaveError::HttpFormat(Arc::new(error))
            }
        }
    }
}

#[derive(Debug)]
pub(super) enum SendCommand {
    Cast(Cast),
    Call(Call),
}

#[derive(Debug)]
pub(crate) struct Call {
    pub event: Event,
    pub payload: Payload,
    pub reply_sender: oneshot::Sender<Result<Payload, channel::CallError>>,
}

#[derive(Debug)]
pub(crate) struct Cast {
    pub event: Event,
    pub payload: Payload,
}

#[must_use]
pub(crate) enum State {
    WaitingForSocketToConnect {
        rejoin: Option<Rejoin>,
    },
    WaitingToJoin,
    Joining(Joining),
    WaitingToRejoin {
        sleep: Pin<Box<Sleep>>,
        rejoin: Rejoin,
    },
    Joined(Joined),
    Leaving(Leaving),
    Left,
    ShuttingDown,
}
impl State {
    pub fn status(&self) -> Status {
        match self {
            State::WaitingForSocketToConnect { .. } => Status::WaitingForSocketToConnect,
            State::WaitingToJoin => Status::WaitingToJoin,
            State::Joining(_) => Status::Joining,
            State::WaitingToRejoin { .. } => Status::WaitingToRejoin,
            State::Joined { .. } => Status::Joined,
            State::Leaving { .. } => Status::Leaving,
            State::Left => Status::Left,
            State::ShuttingDown => Status::ShuttingDown,
        }
    }

    fn connectivity_changed(self, connectivity: Connectivity) -> Self {
        let prefix = format!(
            "Received connectivity change {:?} and transitions from {:#?} to ",
            connectivity, self
        );

        let next_state = match self {
            State::WaitingForSocketToConnect { rejoin } => match connectivity {
                Connectivity::Connected => match rejoin {
                    None => State::WaitingToJoin,
                    Some(rejoin) => rejoin.wait(),
                },
                Connectivity::Disconnected(disconnected) => match disconnected {
                    Disconnected::ServerDisconnected
                    | Disconnected::Disconnect
                    | Disconnected::Reconnect => self,
                    Disconnected::Shutdown => State::ShuttingDown,
                },
            },
            State::WaitingToJoin | State::Left => match connectivity {
                Connectivity::Connected => self,
                Connectivity::Disconnected(disconnected) => match disconnected {
                    Disconnected::ServerDisconnected
                    | Disconnected::Disconnect
                    | Disconnected::Reconnect => State::WaitingForSocketToConnect { rejoin: None },
                    Disconnected::Shutdown => State::ShuttingDown,
                },
            },
            State::Joining(Joining {
                channel_joined_senders,
                rejoin,
                ..
            }) => match connectivity {
                Connectivity::Connected | Connectivity::Disconnected(Disconnected::Reconnect) => {
                    Self::send_to_channel_senders(
                        channel_joined_senders,
                        Err(JoinError::SocketDisconnected),
                    );

                    State::WaitingForSocketToConnect {
                        rejoin: Some(rejoin),
                    }
                }
                Connectivity::Disconnected(Disconnected::ServerDisconnected)
                | Connectivity::Disconnected(Disconnected::Disconnect) => {
                    State::WaitingForSocketToConnect { rejoin: None }
                }
                Connectivity::Disconnected(Disconnected::Shutdown) => State::ShuttingDown,
            },
            State::WaitingToRejoin { sleep, rejoin } => match connectivity {
                Connectivity::Connected => State::WaitingToRejoin { sleep, rejoin },
                Connectivity::Disconnected(disconnected) => match disconnected {
                    Disconnected::ServerDisconnected | Disconnected::Disconnect => {
                        State::WaitingForSocketToConnect { rejoin: None }
                    }
                    Disconnected::Shutdown => State::ShuttingDown,
                    Disconnected::Reconnect => State::WaitingForSocketToConnect {
                        rejoin: Some(rejoin),
                    },
                },
            },
            State::Joined(joined) => match connectivity {
                Connectivity::Connected | Connectivity::Disconnected(Disconnected::Reconnect) => {
                    State::WaitingForSocketToConnect {
                        rejoin: Some(joined.rejoin()),
                    }
                }
                Connectivity::Disconnected(Disconnected::ServerDisconnected)
                | Connectivity::Disconnected(Disconnected::Disconnect) => {
                    State::WaitingForSocketToConnect { rejoin: None }
                }
                Connectivity::Disconnected(Disconnected::Shutdown) => State::ShuttingDown,
            },
            State::Leaving(Leaving {
                channel_left_senders,
                ..
            }) => match connectivity {
                Connectivity::Connected => {
                    Self::send_to_channel_senders(channel_left_senders, Ok(()));

                    State::Left
                }
                Connectivity::Disconnected(disconnected) => match disconnected {
                    Disconnected::ServerDisconnected | Disconnected::Disconnect => {
                        State::WaitingForSocketToConnect { rejoin: None }
                    }
                    Disconnected::Shutdown => State::ShuttingDown,
                    Disconnected::Reconnect => {
                        Self::send_to_channel_senders(channel_left_senders, Ok(()));

                        State::WaitingForSocketToConnect { rejoin: None }
                    }
                },
            },
            State::ShuttingDown => self,
        };

        debug!("{}{:#?}", prefix, next_state);

        next_state
    }

    fn shutdown(self) -> Self {
        match self {
            State::WaitingForSocketToConnect { .. }
            | State::WaitingToJoin { .. }
            | State::WaitingToRejoin { .. }
            | State::Joined(_)
            | State::Left { .. }
            | State::ShuttingDown => (),
            State::Joining(Joining {
                channel_joined_senders,
                ..
            }) => {
                Self::send_to_channel_senders(channel_joined_senders, Err(JoinError::ShuttingDown));
            }
            State::Leaving(Leaving {
                channel_left_senders,
                ..
            }) => {
                Self::send_to_channel_senders(channel_left_senders, Err(LeaveError::ShuttingDown))
            }
        };

        State::ShuttingDown
    }

    fn send_to_channel_senders<E>(
        mut channel_senders: Vec<oneshot::Sender<Result<(), E>>>,
        result: Result<(), E>,
    ) where
        E: Clone,
    {
        for channel_sender in channel_senders.drain(0..) {
            channel_sender.send(result.clone()).ok();
        }
    }
}
impl Debug for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            State::WaitingForSocketToConnect { rejoin, .. } => f
                .debug_struct("WaitingForSocketToConnect")
                .field("rejoin", rejoin)
                .finish_non_exhaustive(),
            State::WaitingToJoin => f.debug_struct("WaitingToJoin").finish_non_exhaustive(),
            State::Joining(joining) => f.debug_tuple("Joining").field(joining).finish(),
            State::WaitingToRejoin { sleep, rejoin, .. } => f
                .debug_struct("WaitingToRejoin")
                .field(
                    "duration_until_sleep_over",
                    &(sleep.deadline() - Instant::now()),
                )
                .field("rejoin", rejoin)
                .finish(),
            State::Joined(joined) => f.debug_tuple("Joined").field(joined).finish(),
            State::Leaving { .. } => f.debug_struct("Leaving").finish_non_exhaustive(),
            State::Left { .. } => f.write_str("Left"),
            State::ShuttingDown => f.write_str("ShuttingDown"),
        }
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub(crate) struct JoinedChannelReceivers {
    pub push: mpsc::Receiver<Push>,
    pub broadcast: broadcast::Receiver<Broadcast>,
    pub left: oneshot::Receiver<()>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Rejoin {
    join_timeout: Duration,
    /// Enough for > 1 week of attempts; u8 would only be 42 minutes of attempts.
    attempts: u16,
}
impl Rejoin {
    fn wait(self) -> State {
        State::WaitingToRejoin {
            sleep: Box::pin(tokio::time::sleep(self.sleep_duration())),
            rejoin: self.next(),
        }
    }

    fn next(self) -> Self {
        Self {
            join_timeout: self.join_timeout,
            attempts: self.attempts.saturating_add(1),
        }
    }

    fn sleep_duration(&self) -> Duration {
        self.join_timeout * self.sleep_duration_multiplier()
    }

    fn sleep_duration_multiplier(&self) -> u32 {
        Self::SLEEP_DURATION_MULTIPLIERS
            [(self.attempts as usize).min(Self::SLEEP_DURATION_MULTIPLIERS.len() - 1)]
    }

    const SLEEP_DURATION_MULTIPLIERS: &'static [u32] = &[0, 1, 2, 5, 10];
}

pub(crate) struct Joining {
    socket_joined_receiver: oneshot::Receiver<Result<JoinedChannelReceivers, socket::JoinError>>,
    channel_joined_senders: Vec<oneshot::Sender<Result<(), JoinError>>>,
    rejoin: Rejoin,
}
impl Debug for Joining {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Joining")
            .field("rejoin", &self.rejoin)
            .finish_non_exhaustive()
    }
}

pub(crate) struct Joined {
    push_receiver: mpsc::Receiver<Push>,
    broadcast_receiver: broadcast::Receiver<Broadcast>,
    left_receiver: oneshot::Receiver<()>,
    join_timeout: Duration,
}
impl Joined {
    fn rejoin(self) -> Rejoin {
        Rejoin {
            join_timeout: self.join_timeout,
            attempts: 0,
        }
    }
}
impl Debug for Joined {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Joined")
            .field("join_timeout", &self.join_timeout)
            .finish_non_exhaustive()
    }
}

pub(crate) struct Leaving {
    socket_left_receiver: oneshot::Receiver<Result<(), LeaveError>>,
    channel_left_senders: Vec<oneshot::Sender<Result<(), LeaveError>>>,
}
impl Leaving {
    fn left(self, left_result: Result<(), LeaveError>) -> State {
        State::send_to_channel_senders(self.channel_left_senders, left_result);

        State::Left
    }
}

#[derive(Copy, Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ShutdownError {
    #[error("socket already shutdown")]
    SocketShutdown,
    #[error("listener task was already joined once from another caller")]
    AlreadyJoined,
}
