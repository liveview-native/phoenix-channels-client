use std::mem;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use fxhash::FxHashMap;
use log::{debug, error};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::{self, JoinHandle};
use tokio::time;
use tokio_tungstenite::tungstenite::{Error as SocketError, Message as SocketMessage};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::*;

const PHOENIX_SERIALIZER_VSN: &'static str = "2.0.0";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("invalid url: {0}")]
    InvalidUrl(Url),
    #[error("invalid parameter, serialization failed: {0}")]
    InvalidParameter(#[from] serde_json::Error),
    #[error("invalid configuration")]
    ConfigError,
    #[error("already connected")]
    AlreadyConnected,
    #[error("not connected")]
    NotConnected,
    #[error("connection error: {0}")]
    ConnectionError(#[from] SocketError),
}

/// This struct contains configuration for Clients
#[derive(Clone)]
pub struct Config {
    /// The endpoint URL to connect to, by default this is `ws://localhost:4000`
    url: Url,
    /// The set of query parameters to send during connection setup. Defaults to an empty set.
    params: FxHashMap<String, String>,
    /// Determines whether or not to automatically reconnect on error/unexpected disconnect. Defaults to false.
    reconnect: bool,
    /// If `reconnect` is `true`, this determines the maximum number of reconnect attempts to make. Defaults to 3.
    max_attempts: u8,
}
impl Config {
    pub fn new<E, U: TryInto<Url, Error = E>>(url: U) -> Result<Self, E> {
        Ok(Self {
            url: url.try_into()?,
            params: FxHashMap::default(),
            reconnect: false,
            max_attempts: 3,
        })
    }

    pub fn reconnect(&mut self, enabled: bool) -> &mut Self {
        self.reconnect = enabled;
        self
    }

    pub fn max_attempts(&mut self, max: u8) -> &mut Self {
        self.max_attempts = max;
        self
    }

    pub fn set<K: Into<String>, V: ToString>(&mut self, key: K, value: V) -> &mut Self {
        self.params.insert(key.into(), value.to_string());
        self
    }
}
impl Default for Config {
    fn default() -> Self {
        Self {
            url: Url::parse("ws://localhost:4000").unwrap(),
            params: FxHashMap::default(),
            reconnect: false,
            max_attempts: 3,
        }
    }
}

/// A `Client` manages the underlying WebSocket connection used to talk to Phoenix.
///
/// It acts as the primary interface (along with `Channel`) for working with Phoenix Channels.
///
/// When a client is created, it is disconnected, and must be explicitly connected via `connect`.
/// Once connected, a worker task is spawned that acts as the broker for messages being sent or
/// received over the socket. The worker uses two different types of channels for communication
/// across tasks/threads:
///
/// * A broadcast channel, to which all members of the same topic subscribe. This is used to send
/// messages to all members of a channel.
/// * A mpsc channel, one for each channel member. This is used to send messages to specific `Channel`
/// instances. These are created/destroyed in concert with their associated `Channel` clients.
///
/// Once connected, the more useful `Channel` instance can be obtained via `join`. Most functionality
/// related to channels is exposed there.
pub struct Client {
    config: Config,
    /// A channel for communicating with the socket listener
    sender: mpsc::Sender<ClientCommand>,
    /// Contains a the `Receiver` that corresponds to `sender`
    ///
    /// When `None`, indicates that the client is connected; `Some` indicates the client is not yet connected.
    ///
    /// Can only be modified by `connect`, which requires a mutable reference
    listener: Option<mpsc::Receiver<ClientCommand>>,
    /// The join handle corresponding to the socket listener
    handle: Option<JoinHandle<Result<(), ClientError>>>,
    /// A unique counter used for making unique join refs
    next_join_ref: AtomicU32,
}
impl Client {
    /// Creates a new, disconnected Phoenix Channels client
    pub fn new(mut config: Config) -> Result<Self, ClientError> {
        match config.url.scheme() {
            "wss" | "ws" => (),
            _ => return Err(ClientError::InvalidUrl(config.url)),
        }

        // Modify url with given parameters
        {
            let mut query = config.url.query_pairs_mut();
            for (k, v) in config.params.iter() {
                query.append_pair(k.as_str(), v.as_str());
            }
            query.append_pair("vsn", PHOENIX_SERIALIZER_VSN);
        }

        let (sender, listener) = mpsc::channel::<ClientCommand>(50);

        Ok(Self {
            config,
            sender,
            listener: Some(listener),
            handle: None,
            next_join_ref: AtomicU32::new(1),
        })
    }

    /// Connects this client to the configured Phoenix Channels endpoint
    ///
    /// This function must be called before using the client to join channels, etc.
    ///
    /// A join handle to the socket worker is returned, we can use this to wait until the worker
    /// exits to ensure graceful termination. Otherwise, when the handle is dropped, it detaches the
    /// worker from the task runtime (though it will continue to run in the background)
    pub async fn connect(&mut self) -> Result<(), ClientError> {
        if self.handle.is_some() {
            return Err(ClientError::AlreadyConnected);
        }

        debug!("connecting to {}", &self.config.url);

        let (socket, _) = tokio_tungstenite::connect_async(&self.config.url).await?;

        // Take channel for sending commands to the socket listener
        let commands = self.listener.take().unwrap();

        // Create the listener which will handle incoming/outgoing traffic
        let worker = SocketListener {
            socket,
            commands,
            channels: FxHashMap::default(),
            joined: FxHashMap::default(),
            joining: FxHashMap::default(),
            replies: FxHashMap::default(),
        };

        self.handle = Some(tokio::spawn(listen(worker)));

        Ok(())
    }

    /// Disconnect the client, regardless of any outstanding channel references
    ///
    /// Connected channels will return `ChannelError::Closed` when next used.
    ///
    /// New channels will need to be obtained from this client after `connect` is
    /// called again.
    pub async fn disconnect(&mut self) -> Result<(), ClientError> {
        if self.handle.is_none() {
            return Ok(());
        }

        let (sender, listener) = mpsc::channel::<ClientCommand>(50);

        {
            let prev_sender = mem::replace(&mut self.sender, sender);
            prev_sender.send(ClientCommand::Close).await.ok();

            if let Some(prev_listener) = self.listener.as_mut() {
                drop(mem::replace(prev_listener, listener));
            }
        }

        let handle = self.handle.take().unwrap();
        match handle.await {
            Ok(_) => Ok(()),
            Err(err) if err.is_cancelled() => Ok(()),
            Err(err) => std::panic::resume_unwind(err.into_panic()),
        }
    }

    pub fn shutdown(mut self) -> JoinHandle<Result<(), ClientError>> {
        match self.handle.take() {
            Some(handle) => handle,
            None => task::spawn(async { Ok(()) }),
        }
    }

    /// Joins `topic` with optional timeout duration.
    ///
    /// If successful, returns a `Channel` which can be used to interact with the connected channel.
    pub async fn join(
        &self,
        topic: &str,
        timeout: Option<Duration>,
    ) -> Result<Arc<Channel>, ChannelError> {
        let join_ref = self.next_join_ref.fetch_add(1, Ordering::SeqCst);

        // Get a clone of the client sender to send messages
        let channel_tx = self.sender.clone();

        let (notify, waiter) = oneshot::channel();

        // Create a channel which is pending join. The `on_join` channel is used to tell
        // the channel listener that the channel was successfully joined, and to communicate
        // some last minute details
        let (channel, on_join) = Channel::new(topic.to_string(), join_ref, channel_tx);

        let join = Box::new(Join {
            instant: Instant::now(),
            channel: Arc::downgrade(&channel),
            notify,
            on_join,
        });

        // Spawn a new task for the join
        let sender = self.sender.clone();
        tokio::spawn(async move { sender.send(ClientCommand::Join(join)).await });

        match timeout {
            None => match waiter.await? {
                Ok(_) => {
                    debug!("joined topic '{}'", topic);
                    Ok(channel)
                }
                Err(err) => {
                    debug!("error occurred while joining channel: {}", &err);
                    Err(err)
                }
            },
            Some(duration) => match time::timeout(duration, waiter).await?? {
                Ok(_) => {
                    debug!("joined topic '{}'", topic);
                    Ok(channel)
                }
                Err(err) => {
                    debug!("error occurred while joining channel: {}", &err);
                    Err(err)
                }
            },
        }
    }
}

/// This enum encodes the various commands that can be used to control the socket listener
pub(super) enum ClientCommand {
    /// Tells the client to join `channel` to its desired topic, and then:
    ///
    /// * If joining was successful, notify the channel listener via `on_join`, and then tell
    /// the caller the result of the overall join operation via `notify`
    /// * If joining was unsuccessful, tell the caller via `notify`, and drop `on_join` to notify the
    /// channel listener that the channel is being closed.
    Join(Box<Join>),
    /// Tells the client to leave the given channel
    Leave(Leave),
    /// Tells the client to send `push`, and if successful, send the reply to `on_reply`
    Call(Push, oneshot::Sender<Reply>),
    /// Tells the client to send `push`, but to ignore any replies
    Cast(Push),
    /// Tells the client to shutdown the socket, and disconnect all channels
    Close,
}

/// Represents a command to the socket listener to join a channel
pub(super) struct Join {
    /// The instant at which this join was created
    instant: Instant,
    /// The channel being joined
    channel: Weak<Channel>,
    /// The oneshot channel used to notify the caller on success/error
    notify: oneshot::Sender<Result<(), ChannelError>>,
    /// The oneshot channel used to tell the channel listener that the channel is active
    on_join: oneshot::Sender<ChannelJoined>,
}
impl Join {
    #[inline]
    fn timeout(&self) -> bool {
        const TIMEOUT: Duration = Duration::from_secs(30);

        self.instant.elapsed() >= TIMEOUT
    }
}

/// Represents a command to the socket listener to leave a channel
pub(super) struct Leave {
    pub topic: String,
    pub channel_ref: ChannelRef,
}

struct SocketListener {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    commands: mpsc::Receiver<ClientCommand>,
    channels: FxHashMap<ChannelId, broadcast::Sender<Broadcast>>,
    joining: FxHashMap<SubscriptionRef, Box<Join>>,
    joined: FxHashMap<ChannelRef, mpsc::Sender<Push>>,
    replies: FxHashMap<SubscriptionRef, oneshot::Sender<Reply>>,
}
impl SocketListener {
    async fn shutdown(mut self, reason: Result<(), ClientError>) -> Result<(), ClientError> {
        debug!("client is shutting down: {:#?}", &reason);
        self.socket.close(None).await.ok();
        reason
    }

    async fn leave(&mut self, leave: Leave) -> Result<(), ClientError> {
        debug!(
            "client is leaving channel '{}' ({})",
            &leave.topic, &leave.channel_ref
        );
        let message = Message::Push(Push {
            topic: leave.topic,
            event: PhoenixEvent::Leave.into(),
            payload: Value::Null.into(),
            join_ref: leave.channel_ref.join_ref.to_string(),
            reference: None,
        });
        let data = message.encode().unwrap();
        Ok(self.socket.send(data).await?)
    }

    async fn start_join(&mut self, join: Box<Join>) -> Result<(), ClientError> {
        let (channel, subscription) = match join.channel.upgrade() {
            Some(channel) => {
                let channel_ref = channel.channel_ref();
                let subscription_ref = SubscriptionRef::new(channel_ref, 0);
                debug!(
                    "client is attempting to join '{}', with ref {}",
                    channel.topic(),
                    &subscription_ref
                );
                (channel, subscription_ref)
            }
            // If the channel was already dropped, there's nothing to do
            None => {
                debug!("client was about to attempt a join, but was cancelled by the caller");
                return Ok(());
            }
        };

        self.joining.insert(subscription, join);

        // Send the join message, and register the join
        let message = Message::Push(Push {
            topic: channel.topic().to_string(),
            event: PhoenixEvent::Join.into(),
            payload: Value::Object(serde_json::Map::new()).into(),
            join_ref: subscription.channel_ref.join_ref.to_string(),
            reference: Some(subscription.reference.to_string()),
        });
        let data = message.encode().unwrap();
        Ok(self.socket.send(data).await?)
    }

    fn finish_join(&mut self, id: SubscriptionRef, reply: Reply, join: Box<Join>) {
        use std::collections::hash_map::Entry;

        // If the join failed, then notify the waiter and bail
        if reply.status.is_error() {
            debug!(
                "client received error in response to join ref {}: {}",
                &id, &reply.payload
            );
            // Notify the caller that the join failed
            join.notify
                .send(Err(ChannelError::JoinFailed(Box::new(reply.payload))))
                .ok();
            return;
        }

        debug!(
            "client received ok in response to join ref {}, finalizing..",
            &id
        );

        // Obtain a new broadcast subscription for the joined channel
        let broadcasts = match self.channels.entry(id.channel_ref.channel_id) {
            Entry::Occupied(entry) => entry.get().subscribe(),
            Entry::Vacant(entry) => {
                let (broadcasts_tx, broadcasts_rx) = broadcast::channel::<Broadcast>(50);
                entry.insert(broadcasts_tx);
                broadcasts_rx
            }
        };

        // Create a new channel for receiving direct messages from the server
        let (pusher, pushes) = mpsc::channel::<Push>(10);

        // Register the channel so we can handle relaying on its behalf
        self.joined.insert(id.channel_ref, pusher);

        // Send the channel join event to the channel listener
        let joined = ChannelJoined {
            channel: join.channel,
            broadcasts,
            pushes,
        };
        debug!("notifying channel listener for {}", &id);
        join.on_join.send(joined).ok();

        // Notify the caller that the channel joined successfully
        debug!("notifying caller for {}", &id);
        join.notify.send(Ok(())).ok();
    }

    fn handle_reply(&mut self, reply: Reply) -> Result<(), ClientError> {
        let subscription_ref = reply.subscription_ref();

        // Check if this is a join reply
        if let Some(join) = self.joining.remove(&subscription_ref) {
            return Ok(self.finish_join(subscription_ref, reply, join));
        }

        // Check if we have a waiter for this reply
        if let Some(reply_to) = self.replies.remove(&subscription_ref) {
            debug!(
                "received reply to message ref {}, status is {}",
                &subscription_ref, &reply.status
            );
            reply_to.send(reply).ok();
        } else {
            debug!(
                "received reply to message ref {} with status {}, but it was ignored",
                &subscription_ref, &reply.status
            );
        }

        Ok(())
    }

    async fn handle_push(&self, push: Push) -> Result<(), ClientError> {
        debug!("received push: {:#?}", &push);
        if let Some(pusher) = self.joined.get(&push.channel_ref()) {
            pusher.send(push).await.ok();
        }

        Ok(())
    }

    fn handle_broadcast(&self, broadcast: Broadcast) -> Result<(), ClientError> {
        debug!("received broadcast: {:#?}", &broadcast);
        if let Some(broadcaster) = self.channels.get(&broadcast.channel_id()) {
            broadcaster.send(broadcast).ok();
        }

        Ok(())
    }

    async fn call(
        &mut self,
        push: Push,
        reply_to: oneshot::Sender<Reply>,
    ) -> Result<(), ClientError> {
        let subscription_ref = push.subscription_ref().unwrap();

        debug!(
            "sending message with ref {} to topic '{}', will wait for reply",
            &subscription_ref, &push.topic
        );

        let message = Message::Push(push);
        let data = message.encode().unwrap();

        self.replies.insert(subscription_ref, reply_to);
        Ok(self.socket.send(data).await?)
    }

    async fn cast(&mut self, push: Push) -> Result<(), ClientError> {
        debug!(
            "sending message for channel ref {}, to topic '{}', will ignore replies",
            &push.channel_ref(),
            &push.topic
        );
        let message = Message::Push(push);
        let data = message.encode().unwrap();
        Ok(self.socket.send(data).await?)
    }

    fn maintenance(&mut self) {
        debug!("performing maintenance on socket listener");
        // Clean up any timed out joins
        for (sub, join) in self.joining.drain_filter(|_, join| join.timeout()) {
            debug!("forcing timeout of join attempt {}", &sub);
            drop(join);
        }
    }
}

async fn listen(mut listener: SocketListener) -> Result<(), ClientError> {
    debug!("starting client worker");

    // Start a heartbeat timer
    let duration = Duration::from_secs(30);
    let mut heartbeat = time::interval_at((Instant::now() + duration).into(), duration);
    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let mut heartbeats: u64 = 0;
    let mut pending_heartbeat_ref = None;

    // Start a maintenance timer
    let mut maintenance = time::interval_at((Instant::now() + duration).into(), duration);
    maintenance.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            recv = listener.commands.recv() => {
                match recv {
                    // A channel is being joined
                    Some(ClientCommand::Join(join)) => listener.start_join(join).await?,
                    // A channel is being left
                    Some(ClientCommand::Leave(leave)) => listener.leave(leave).await?,
                    // A channel message is being sent, with a reply expected
                    Some(ClientCommand::Call(push, reply_to)) => listener.call(push, reply_to).await?,
                    // A channel message is being sent, replies ignored
                    Some(ClientCommand::Cast(push)) => listener.cast(push).await?,
                    Some(ClientCommand::Close) => return listener.shutdown(Ok(())).await,
                    // The sender has been dropped, meaning the client has been dropped, so we should shutdown immediately
                    None => {
                        debug!("the client has been dropped, shutting down immediately");
                        return listener.shutdown(Ok(())).await;
                    }
                }
            }
            next = listener.socket.next() => {
                match next.ok_or(ClientError::NotConnected)? {
                    Ok(SocketMessage::Ping(data)) => {
                        debug!("client received ping");
                        listener.socket.send(SocketMessage::Pong(data)).await?;
                        continue;
                    }
                    Ok(SocketMessage::Close(frame)) => {
                        debug!("client received close from server, closing socket");
                        listener.socket.close(frame).await?;
                        return Ok(());
                    }
                    Ok(SocketMessage::Pong(_)) => {
                        debug!("client received pong");
                        continue;
                    }
                    Ok(msg) => {
                        match Message::decode(msg) {
                            Ok(Message::Control(control)) => {
                                debug!("received control message: {:#?}", &control);
                                match control.event {
                                    Event::Phoenix(PhoenixEvent::Reply) => {
                                        if control.reference.as_deref() == pending_heartbeat_ref.as_deref() {
                                            // Reset heartbeat timeout
                                            debug!("received heartbeat reply, resetting heartbeat timeout");
                                            pending_heartbeat_ref = None;
                                            heartbeat.reset();
                                        }
                                    }
                                    _ => continue,
                                }
                            }
                            Ok(Message::Reply(reply)) if reply.event == PhoenixEvent::Heartbeat => {
                                debug!("received heartbeat reply not in control format: {:#?}", &reply);
                                continue
                            }
                            Ok(Message::Reply(reply)) => listener.handle_reply(reply)?,
                            Ok(Message::Push(push)) => listener.handle_push(push).await?,
                            Ok(Message::Broadcast(broadcast)) => listener.handle_broadcast(broadcast)?,
                            Err(err) => {
                                debug!("dropping invalid message received from server, due to error decoding: {}", &err);
                            }
                        }
                    }
                    // Connection terminated normally
                    Err(SocketError::ConnectionClosed) => {
                        debug!("connection closed");
                        return Ok(());
                    },
                    Err(SocketError::AlreadyClosed) => {
                        // This shouldn't ever be reached since we handle ConnectionClosed, but treat it the same
                        error!("socket already closed");
                        return Ok(());
                    }
                    Err(err @ SocketError::Io(_)) => {
                        error!("io error encountered while reading from socket: {:?}", &err);
                        return Err(err.into());
                    }
                    Err(SocketError::Capacity(_)) => {
                        debug!("socket capacity exhausted, dropping message");
                        continue;
                    }
                    Err(err) => {
                        error!("socket error: {:?}", &err);
                        return Err(err.into())
                    }
                }
            }
            // Maintain the heartbeat
            _ = heartbeat.tick() => {
                match pending_heartbeat_ref.take() {
                    Some(_) => {
                        debug!("a heartbeat interval passed with no reply to our previous heartbeat, extending deadline..");
                        continue;
                    }
                    None => {
                        // Send heartbeat
                        debug!("sending heartbeat..");
                        heartbeats += 1;
                        let heartbeat_ref = format!("heartbeat:{}", &heartbeats);
                        pending_heartbeat_ref = Some(heartbeat_ref.clone());
                        listener.socket.send(heartbeat_message(heartbeat_ref)).await?;
                    }
                }
            }
            // Perform maintenance since we have a moment to breathe
            _ = maintenance.tick() => {
                listener.maintenance();
                maintenance.reset();
            }
        }
    }
}

#[inline]
fn heartbeat_message(reference: String) -> SocketMessage {
    Message::encode(Message::Control(Control {
        event: Event::Phoenix(PhoenixEvent::Heartbeat),
        reference: Some(reference),
        payload: Payload::Value(Value::Null),
    }))
    .unwrap()
}
