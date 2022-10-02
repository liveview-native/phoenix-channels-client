use std::fmt;
use std::ops::Deref;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use futures::future::FutureExt;
use fxhash::FxHashMap;
use log::debug;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task;
use tokio::time;

use crate::client::{ClientCommand, Leave};
use crate::message::*;

/// Represents errors that occur when interacting with [`Channel`]
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// Occurs when you perform an operation on a channel that is closing/closed
    #[error("channel is closed")]
    Closed,
    /// Occurs when a join/send operation times out
    #[error("operation failed due to timeout")]
    Timeout,
    /// Occurs when the join message for a channel receives an error reply from the server
    #[error("error occurred while joining channel: {0}")]
    JoinFailed(Box<Payload>),
}
impl From<tokio::time::error::Elapsed> for ChannelError {
    fn from(_err: tokio::time::error::Elapsed) -> Self {
        Self::Timeout
    }
}
impl From<tokio::sync::oneshot::error::RecvError> for ChannelError {
    fn from(_err: tokio::sync::oneshot::error::RecvError) -> Self {
        Self::Closed
    }
}

/// Represents errors that occur when sending messages via [`Channel`]
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    /// Occurs when sending a message that waits for a reply times out
    #[error("timeout occurred waiting for reply")]
    Timeout,
    /// Occurs when sending a message to a closing/closed channel
    #[error("channel is closed")]
    Closed,
    /// Occurs when the the reply to a sent message contains an error produced by the server
    #[error("received an error reply from the server: {0}")]
    Reply(Box<Payload>),
}
impl From<tokio::time::error::Elapsed> for SendError {
    fn from(_err: tokio::time::error::Elapsed) -> Self {
        Self::Timeout
    }
}
impl From<tokio::sync::oneshot::error::RecvError> for SendError {
    fn from(_err: tokio::sync::oneshot::error::RecvError) -> Self {
        Self::Closed
    }
}
impl<T> From<tokio::sync::mpsc::error::SendError<T>> for SendError {
    fn from(_err: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Self::Closed
    }
}

/// A simple copyable reference to a specific topic
#[doc(hidden)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelId(u32);
impl ChannelId {
    #[inline]
    pub fn new(topic: &str) -> Self {
        Self(fxhash::hash32(topic))
    }
}
impl fmt::Display for ChannelId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// A simple copyable reference to a specific channel member
///
/// It refers to a specific pair of channel topic and channel join reference.
/// The channel id is derived from the hash of the topic name; and the channel
/// reference is unique for each joiner, i.e. when you have multiple joins to
/// the same channel.
#[doc(hidden)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelRef {
    pub(crate) channel_id: ChannelId,
    pub(crate) join_ref: u32,
}
impl ChannelRef {
    pub fn new(channel_id: ChannelId, join_ref: u32) -> Self {
        Self {
            channel_id,
            join_ref,
        }
    }
}
impl fmt::Display for ChannelRef {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", &self.channel_id, self.join_ref)
    }
}

/// A reference to a specific event handler
///
/// This can be used to unsubscribe a single event handler from a channel
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubscriptionRef {
    pub(super) channel_ref: ChannelRef,
    pub(super) reference: u32,
}
impl SubscriptionRef {
    pub fn new(channel_ref: ChannelRef, reference: u32) -> Self {
        Self {
            channel_ref,
            reference,
        }
    }
}
impl fmt::Display for SubscriptionRef {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}+{}", &self.channel_ref, self.reference)
    }
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum ChannelStatus {
    Pending,
    Joined,
    Leaving,
    Closing,
}

/// An event handler is a thread-safe callback which is invoked every time an event
/// is received on a channel to which the handler is subscribed.
///
/// Handlers receive the channel and payload of the event as arguments, which provides
/// a fair amount of flexibility for reacting to messages; however, they must adhere to
/// a few rules:
///
/// * They must implement `Send` and `Sync`, this is because the callbacks may be invoked
/// on any thread; and while the `Sync` bound is onerous, it is required due to the
/// compiler being unable to see that the callbacks are not able to be invoked concurrently,
/// though this is not actually the case, as callbacks are only invoked by the listener task.
///
/// * They should avoid blocking. These callbacks are invoked from an async context, and so
/// any blocking they do will block the async runtime from making progress on that thread. If
/// you need to perform blocking work in the callback, you should invoke `tokio::spawn` with
/// a new async task; or `tokio::spawn_blocking` for truly blocking tasks.
pub type EventHandler = Box<dyn Fn(Arc<Channel>, &Payload) + Send + Sync + 'static>;

/// A `Channel` is created when one joins a topic via `Client::join`.
///
/// It represents a unique connection to the topic, and as a result, you may join
/// the same topic many times, each time receiving a new, unique `Channel` instance.
///
/// To leave a topic/channel, you can either await the result of `Channel::leave`, or
/// drop the channel. Once all references to a specific `Channel` are dropped, if it
/// hasn't yet left its channel, this is done at that point.
///
/// You have two ways of sending messages to the channel:
///
/// * `send`/`send_with_timeout`, to send a message and await a reply from the server
/// * `send_noreply`, to send a message and ignore any replies
///
pub struct Channel {
    /// The unique id for this channel. This is always the same for a channel of a given `topic`
    id: ChannelId,
    /// The unique id of this channel's session, once joined to the topic
    join_ref: u32,
    /// The channel status
    status: Arc<AtomicU32>,
    /// The name of the channel topic this handle is joined to
    topic: Arc<String>,
    /// Used to send messages to the socket
    client: mpsc::Sender<ClientCommand>,
    /// Used to issue commands to the channel listener
    listener: mpsc::Sender<Command>,
    /// Counter for unique message reference identifiers
    next_reference_id: Arc<AtomicU32>,
}
impl Channel {
    pub(crate) fn new(
        topic: String,
        join_ref: u32,
        client: mpsc::Sender<ClientCommand>,
    ) -> (Arc<Self>, oneshot::Sender<ChannelJoined>) {
        let id = ChannelId::new(topic.as_str());
        let channel_ref = ChannelRef::new(id, join_ref);
        let topic = Arc::new(topic);
        // Create a channel for communicating with the listener
        let (commands_tx, commands_rx) = mpsc::channel(10);
        // Create a channel to signal when the channel is joined and send along remaining handles
        let (tx, rx) = oneshot::channel::<ChannelJoined>();

        let status = Arc::new(AtomicU32::new(ChannelStatus::Pending as u32));
        let next_reference_id = Arc::new(AtomicU32::new(0));

        let listener = ChannelListener {
            channel_ref,
            topic: topic.clone(),
            join_ref,
            status: status.clone(),
            handlers: Default::default(),
            commands: commands_rx,
            client: client.clone(),
            next_reference_id: next_reference_id.clone(),
        };

        // Spawn the listener, which will block until we send the ChannelJoined message, or the channel is dropped
        tokio::spawn(listen(listener, rx));

        let channel = Arc::new(Self {
            id,
            join_ref,
            status,
            topic,
            client,
            listener: commands_tx,
            next_reference_id,
        });

        (channel, tx)
    }

    /// Returns the topic this channel is connected to
    pub fn topic(&self) -> &str {
        self.topic.as_str()
    }

    /// Returns true if this channel is currently joined to its topic
    pub fn is_joined(&self) -> bool {
        self.status() == ChannelStatus::Joined
    }

    /// Returns true if this channel is currently leaving its topic
    pub fn is_leaving(&self) -> bool {
        self.status() == ChannelStatus::Leaving
    }

    /// Returns true if this channel is currently closing
    pub fn is_closing(&self) -> bool {
        self.status() == ChannelStatus::Closing
    }

    #[inline]
    fn status(&self) -> ChannelStatus {
        unsafe { core::mem::transmute::<u32, ChannelStatus>(self.status.load(Ordering::SeqCst)) }
    }

    /// Registers an event handler to be run the next time this channel receives `event`.
    ///
    /// Returns a `SubscriptionRef` which can be used to unsubscribe the handler.
    ///
    /// You may also unsubscribe all handlers for `event` using `Channel::clear`.
    pub async fn on<E, F>(&self, event: E, handler: F) -> Result<SubscriptionRef, ChannelError>
    where
        E: Into<Event>,
        F: Fn(Arc<Channel>, &Payload) + Send + Sync + 'static,
    {
        let event = event.into();
        let handler = Box::new(handler);
        let (tx, rx) = oneshot::channel::<SubscriptionRef>();
        self.listener
            .send(Command::Subscribe(event.into(), handler, tx))
            .await
            .ok();
        Ok(rx.await?)
    }

    /// Unsubscribes `subscription` from `event`
    ///
    /// To remove all subscriptions for `event`, use `Channel::clear`.
    pub async fn off<E>(&self, event: E, subscription: SubscriptionRef)
    where
        E: Into<Event>,
    {
        self.listener
            .send(Command::Unsubscribe(event.into(), subscription))
            .await
            .ok();
    }

    /// Unsubscribes all handlers for `event`.
    pub async fn clear<E>(&self, event: E)
    where
        E: Into<Event>,
    {
        self.listener
            .send(Command::UnsubscribeAll(event.into()))
            .await
            .ok();
    }

    /// Sends `event` with `payload` to this channel, and awaits a reply.
    ///
    /// If you don't care about a reply, use `send_noreply` instead, as it has less overhead.
    ///
    /// This function sets no timeout on the reply, so awaiting its result will block forever. As a result,
    /// you should generally prefer to use `send_with_timeout` with a reasonable default, to ensure that
    /// you don't accidentally block your workers.
    pub async fn send<E, T>(&self, event: E, payload: T) -> Result<Payload, SendError>
    where
        E: Into<Event>,
        T: Into<Value>,
    {
        self.send_with_timeout(event, payload, None).await
    }

    /// Like `send`, except it takes a configurable `timeout` for awaiting the reply.
    ///
    /// If `timeout` is None, it is equivalent to `send`, and waits forever.
    /// If `timeout` is Some(Duration), then waiting for the reply will stop after the duration expires,
    /// and a `SendError::Timeout` will be produced. If the reply is received before that occurs, then
    /// the reply payload will be returned.
    pub async fn send_with_timeout<E, T>(
        &self,
        event: E,
        payload: T,
        timeout: Option<Duration>,
    ) -> Result<Payload, SendError>
    where
        E: Into<Event>,
        T: Into<Value>,
    {
        let event = event.into();
        let payload = Payload::Value(payload.into());

        debug!(
            "sending event {:?} with timeout {} and payload {:#?}",
            &event, &payload, &timeout
        );

        let reference_id = self.next_reference_id.fetch_add(1, Ordering::SeqCst);
        let reference = reference_id.to_string();

        let message = Push {
            topic: self.topic.deref().clone(),
            event,
            payload,
            join_ref: self.join_ref.to_string(),
            reference: Some(reference),
        };

        let (tx, rx) = oneshot::channel();

        self.client
            .send(ClientCommand::Call(message, tx))
            .then(|result| async move {
                // Gather the reply
                let reply = match result {
                    Ok(_) => match timeout {
                        Some(timeout) => time::timeout(timeout, rx).await??,
                        None => rx.await?,
                    },
                    Err(err) => return Err(err.into()),
                };
                match reply.status {
                    ReplyStatus::Ok => Ok(reply.payload),
                    ReplyStatus::Error => Err(SendError::Reply(Box::new(reply.payload))),
                }
            })
            .await
    }

    /// Sends `event` with `payload` to this channel, and returns `Ok` if successful.
    ///
    /// This function does not wait for any reply, if you need the reply, then use `send` or `send_with_timeout`.
    pub async fn send_noreply<E, T>(&self, event: E, payload: T) -> Result<(), SendError>
    where
        E: Into<Event>,
        T: Into<Value>,
    {
        let event = event.into();
        let payload = Payload::Value(payload.into());

        debug!(
            "sending event {:?} with payload {:#?}, replies ignored",
            &event, &payload
        );

        let message = Push {
            topic: self.topic.deref().clone(),
            event,
            payload,
            join_ref: self.join_ref.to_string(),
            reference: None,
        };

        Ok(self.client.send(ClientCommand::Cast(message)).await?)
    }

    /// Leaves this channel
    pub async fn leave(&self) {
        debug!("leaving channel {}", &self.id);
        let message = Leave {
            topic: self.topic.deref().clone(),
            channel_ref: self.channel_ref(),
        };
        self.client.send(ClientCommand::Leave(message)).await.ok();
        self.listener.send(Command::Close).await.ok();
    }

    /// Returns the unique id for this channel
    #[inline]
    pub(super) fn channel_ref(&self) -> ChannelRef {
        ChannelRef::new(self.id, self.join_ref)
    }
}

#[doc(hidden)]
pub(super) struct ChannelJoined {
    /// We send a weak reference to the channel, since we consider the ability to
    /// upgrade it to a strong reference a signal that the channel should be kept
    /// alive.
    pub channel: Weak<Channel>,
    pub pushes: mpsc::Receiver<Push>,
    pub broadcasts: broadcast::Receiver<Broadcast>,
}

enum Command {
    Subscribe(Event, EventHandler, oneshot::Sender<SubscriptionRef>),
    Unsubscribe(Event, SubscriptionRef),
    UnsubscribeAll(Event),
    Close,
}

struct ChannelListener {
    channel_ref: ChannelRef,
    /// The topic this channel is related to
    topic: Arc<String>,
    /// The join ref of this channel
    join_ref: u32,
    /// The status of this channel
    status: Arc<AtomicU32>,
    /// Event handlers that have been registered to this channel
    handlers: FxHashMap<Event, FxHashMap<SubscriptionRef, EventHandler>>,
    /// Used to receive commands for interacting with the channel
    commands: mpsc::Receiver<Command>,
    /// Used to send leave commands to the client when shutting down gracefully
    client: mpsc::Sender<ClientCommand>,
    /// Shared reference id generator
    next_reference_id: Arc<AtomicU32>,
}
impl ChannelListener {
    /// Used during graceful termination to tell the server we're leaving the channel
    async fn leave(&self) {
        self.status
            .store(ChannelStatus::Leaving as u32, Ordering::SeqCst);
        let message = Push {
            topic: self.topic.as_str().to_string(),
            event: PhoenixEvent::Leave.into(),
            payload: Value::Null.into(),
            join_ref: self.join_ref.to_string(),
            reference: None,
        };
        self.client.send(ClientCommand::Cast(message)).await.ok();
    }

    /// Terminates this listener, and returns the final result
    fn shutdown(self, reason: Result<(), ChannelError>) -> Result<(), ChannelError> {
        self.status
            .store(ChannelStatus::Closing as u32, Ordering::SeqCst);
        reason
    }

    /// Register a new event handler
    fn register_handler(&mut self, event: Event, handler: EventHandler) -> SubscriptionRef {
        use std::collections::hash_map::Entry;

        let subscription = self.next_subscription_ref();
        match self.handlers.entry(event) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().insert(subscription, handler);
            }
            Entry::Vacant(entry) => {
                let mut subs = FxHashMap::default();
                subs.insert(subscription, handler);
                entry.insert(subs);
            }
        }
        subscription
    }

    /// Unregisters a previously registered event handler
    fn unregister_handler(&mut self, event: Event, subscriber: SubscriptionRef) {
        if let Some(subscribers) = self.handlers.get_mut(&event) {
            subscribers.remove(&subscriber);
        }
    }

    /// Unregisters all handlers for `event`
    #[inline]
    fn clear_handlers(&mut self, event: Event) {
        self.handlers.remove(&event);
    }

    /// Handles a received event
    async fn handle_event(&self, channel: Arc<Channel>, event: Event, payload: Payload) {
        if let Some(subscribers) = self.handlers.get(&event) {
            for handler in subscribers.values() {
                handler(channel.clone(), &payload);
                task::consume_budget().await
            }
        }
    }

    /// Returns a new unique subscription id
    #[inline]
    fn next_subscription_ref(&self) -> SubscriptionRef {
        SubscriptionRef::new(
            self.channel_ref,
            self.next_reference_id.fetch_add(1, Ordering::SeqCst),
        )
    }
}

async fn listen(
    mut listener: ChannelListener,
    joined: oneshot::Receiver<ChannelJoined>,
) -> Result<(), ChannelError> {
    use tokio::sync::broadcast::error::RecvError;

    let ChannelJoined {
        channel,
        mut broadcasts,
        mut pushes,
    } = joined.await?;

    listener
        .status
        .store(ChannelStatus::Joined as u32, Ordering::SeqCst);

    let mut interval = time::interval(Duration::from_secs(30));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            cmd = listener.commands.recv() => {
                interval.reset();
                match cmd {
                    Some(Command::Subscribe(event, handler, reply_to)) => {
                        let subscription = listener.register_handler(event, handler);
                        reply_to.send(subscription).ok();
                    }
                    Some(Command::Unsubscribe(event, subscription)) => {
                        listener.unregister_handler(event, subscription);
                    }
                    Some(Command::UnsubscribeAll(event)) => {
                        listener.clear_handlers(event);
                    }
                    Some(Command::Close) | None => {
                        listener.leave().await;
                        return listener.shutdown(Ok(()));
                    }
                }
            }
            msg = pushes.recv() => {
                interval.reset();
                if let Some(channel) = channel.upgrade() {
                    match msg {
                        Some(msg) => listener.handle_event(channel.clone(), msg.event, msg.payload).await,
                        None => return listener.shutdown(Err(ChannelError::Closed)),
                    }
                } else {
                    listener.leave().await;
                    return listener.shutdown(Err(ChannelError::Closed));
                }
            },
            msg = broadcasts.recv() => {
                interval.reset();
                if let Some(channel) = channel.upgrade() {
                    match msg {
                        Ok(msg) => listener.handle_event(channel.clone(), msg.event, msg.payload).await,
                        Err(RecvError::Closed) => return listener.shutdown(Err(ChannelError::Closed)),
                        Err(RecvError::Lagged(_)) => continue,
                    }
                } else {
                    listener.leave().await;
                    return listener.shutdown(Err(ChannelError::Closed));
                }
            }
            _ = interval.tick() => {
                // If we haven't received any messages for 30s, check to make sure the channel still has strong references
                match channel.strong_count() {
                    0 => {
                        listener.leave().await;
                        return listener.shutdown(Err(ChannelError::Closed));
                    }
                    _ => continue,
                }
            }
        }
    }
}
