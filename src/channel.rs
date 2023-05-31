pub(crate) mod listener;

use std::panic;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use log::{debug, error};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::error::Elapsed;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::error::UrlError;
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::http::Response;

pub(crate) use crate::channel::listener::{Call, Cast};
use crate::channel::listener::{
    LeaveError, Listener, SendCommand, ShutdownError, StateCommand, Subscribe, SubscriptionCommand,
    UnsubscribeSubscription,
};
use crate::message::*;
use crate::reference::Reference;
use crate::socket;
use crate::socket::listener::Connectivity;
use crate::socket::Socket;
use crate::topic::Topic;

/// Represents errors that occur when interacting with [`Channel`]
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChannelError {
    /// Occurs when [Channel::join] is called when a channel is already joined
    #[error("already joined")]
    AlreadyJoined,
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
impl From<oneshot::error::RecvError> for ChannelError {
    fn from(_err: oneshot::error::RecvError) -> Self {
        Self::Closed
    }
}

/// A reference to a specific event handler
///
/// This can be used to unsubscribe a single event handler from a channel
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SubscriptionReference(Reference);
impl SubscriptionReference {
    pub fn new() -> Self {
        Self(Reference::new())
    }
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum Status {
    WaitingForSocketToConnect,
    WaitingToJoin,
    Joining,
    WaitingToRejoin,
    Joined,
    Leaving,
    Left,
    ShuttingDown,
}

/// An event handler is a thread-safe callback which is invoked every time an event
/// is received on a channel to which the handler is subscribed.
///
/// Handlers receive the payload of the event as arguments, which provides
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
pub type EventHandler = Box<dyn Fn(Arc<Payload>) -> BoxFuture<'static, ()> + Send + Sync>;

/// A [Channel] is created with [Socket::channel()]
///
/// It represents a unique connection to the topic, and as a result, you may join
/// the same topic many times, each time receiving a new, unique `Channel` instance.
///
/// To leave a topic/channel, you can either await the result of `Channel::leave`, or
/// drop the channel. Once all references to a specific `Channel` are dropped, if it
/// hasn't yet left its channel, this is
// /// You have two ways o done at that point.
///f sending messages to the channel:
///
/// * [Channel::send]/[Channel::cast], to send a message and await a reply from the server
/// * [Channel::cast], to send a message and ignore any replies
///
pub struct Channel {
    topic: Topic,
    payload: Arc<Payload>,
    /// The channel status
    status: Arc<AtomicU32>,
    subscription_command_sender: mpsc::Sender<SubscriptionCommand>,
    shutdown_sender: oneshot::Sender<()>,
    state_command_sender: mpsc::Sender<StateCommand>,
    send_command_sender: mpsc::Sender<SendCommand>,
    /// The join handle corresponding to the channel listener
    join_handle: JoinHandle<Result<(), ShutdownError>>,
}
impl Channel {
    /// Spawns a new [Channel] that must be [join]ed.  The `topic` and `payload` is sent on the
    /// first [join] and any rejoins if the underlying `socket` is disconnected and
    /// reconnects.
    pub(crate) async fn spawn(
        socket: Arc<Socket>,
        socket_connectivity_receiver: broadcast::Receiver<Connectivity>,
        topic: String,
        payload: Option<Payload>,
        state: listener::State,
    ) -> Self {
        let topic: Topic = topic.into();
        let payload = Arc::new(payload.unwrap_or_default());
        let status = Arc::new(AtomicU32::new(state.status() as u32));
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let (subscription_command_sender, subscription_command_receiver) = mpsc::channel(10);
        let (state_command_sender, state_command_receiver) = mpsc::channel(10);
        let (send_command_sender, send_command_receiver) = mpsc::channel(10);
        let join_handle = Listener::spawn(
            socket,
            socket_connectivity_receiver,
            topic.clone(),
            payload.clone(),
            state,
            status.clone(),
            shutdown_receiver,
            subscription_command_receiver,
            state_command_receiver,
            send_command_receiver,
        );

        Self {
            topic,
            payload,
            status,
            subscription_command_sender,
            shutdown_sender,
            state_command_sender,
            send_command_sender,
            join_handle,
        }
    }

    pub async fn join(&self, timeout: Duration) -> Result<(), JoinError> {
        let (joined_sender, joined_receiver) = oneshot::channel();

        self.state_command_sender
            .send(StateCommand::Join {
                created_at: Instant::now(),
                timeout,
                joined_sender,
            })
            .await?;

        time::timeout(timeout, joined_receiver).await??
    }

    /// Returns the topic this channel is connected to
    pub fn topic(&self) -> Topic {
        self.topic.clone()
    }

    /// Returns the payload sent to the channel when joined
    pub fn payload(&self) -> Arc<Payload> {
        self.payload.clone()
    }

    /// Returns true if this channel is currently joined to its topic
    pub fn is_joined(&self) -> bool {
        self.get_status() == Status::Joined
    }

    /// Returns true if this channel is currently leaving its topic
    pub fn is_leaving(&self) -> bool {
        self.get_status() == Status::Leaving
    }

    #[inline]
    fn get_status(&self) -> Status {
        unsafe { core::mem::transmute::<u32, Status>(self.status.load(Ordering::SeqCst)) }
    }

    /// Registers an event handler to be run the next time this channel receives `event`.
    ///
    /// Returns a `SubscriptionRef` which can be used to unsubscribe the handler.
    ///
    /// You may also unsubscribe all handlers for `event` using `Channel::clear`.
    pub async fn on<E>(
        &self,
        event: E,
        handler: EventHandler,
    ) -> Result<SubscriptionReference, SubscribeError>
    where
        E: Into<Event>,
    {
        let (subscription_reference_sender, subscription_reference_receiver) = oneshot::channel();
        self.subscription_command_sender
            .send(SubscriptionCommand::Subscribe(Subscribe {
                event: event.into(),
                handler,
                subscription_reference_sender,
            }))
            .await?;

        subscription_reference_receiver.await.map_err(From::from)
    }

    /// Unsubscribes `subscription` from `event`
    ///
    /// To remove all subscriptions for `event`, use [Channel::clear].
    pub async fn off<E>(&self, event: E, subscription_reference: SubscriptionReference)
    where
        E: Into<Event>,
    {
        self.subscription_command_sender
            .send(SubscriptionCommand::UnsubscribeSubscription(
                UnsubscribeSubscription {
                    event: event.into(),
                    subscription_reference,
                },
            ))
            .await
            .ok();
    }

    /// Unsubscribes all handlers for `event`.
    pub async fn clear<E>(&self, event: E)
    where
        E: Into<Event>,
    {
        self.subscription_command_sender
            .send(SubscriptionCommand::UnsubscribeEvent(event.into()))
            .await
            .ok();
    }

    /// Sends `event` with `payload` to this channel, and returns `Ok` if successful.
    ///
    /// This function does not wait for any reply, if you need the reply, then use `send` or `send_with_timeout`.
    pub async fn cast<E, T>(&self, event: E, payload: T) -> Result<(), CastError>
    where
        E: Into<Event>,
        T: Into<Payload>,
    {
        let event = event.into();
        let payload = payload.into();

        debug!(
            "sending event {:?} with payload {:#?}, replies ignored",
            &event, &payload
        );

        self.send_command_sender
            .send(SendCommand::Cast(Cast { event, payload }))
            .await
            .map_err(From::from)
    }

    /// Like `send`, except it takes a configurable `timeout` for awaiting the reply.
    ///
    /// If `timeout` is None, it is equivalent to `send`, and waits forever.
    /// If `timeout` is Some(Duration), then waiting for the reply will stop after the duration expires,
    /// and a `SendError::Timeout` will be produced. If the reply is received before that occurs, then
    /// the reply payload will be returned.
    pub async fn call<E, T>(
        &self,
        event: E,
        payload: T,
        timeout: Duration,
    ) -> Result<Payload, CallError>
    where
        E: Into<Event>,
        T: Into<Payload>,
    {
        let event = event.into();
        let payload = payload.into();

        debug!(
            "sending event {:?} with timeout {:?} and payload {:#?}",
            &event, &timeout, &payload
        );

        let (reply_sender, reply_receiver) = oneshot::channel();

        self.send_command_sender
            .send(SendCommand::Call(Call {
                event,
                payload,
                reply_sender,
            }))
            .await?;

        debug!("Waiting for reply for {:?} timeout", &timeout);

        time::timeout(timeout, reply_receiver).await??
    }

    /// Leaves this channel
    pub async fn leave(&self) -> Result<(), LeaveError> {
        let (left_sender, left_receiver) = oneshot::channel();

        self.state_command_sender
            .send(StateCommand::Leave { left_sender })
            .await?;

        match left_receiver.await {
            Ok(result) => result,
            Err(_) => Err(LeaveError::Shutdown),
        }
    }

    /// Propagates panic from [Listener::listen]
    pub async fn shutdown(self) -> Result<(), ShutdownError> {
        self.shutdown_sender
            .send(())
            // if already shut down that's OK
            .ok();

        match self.join_handle.await {
            Ok(result) => result,
            Err(join_error) => panic::resume_unwind(join_error.into_panic()),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SubscribeError {
    #[error("channel already shutdown")]
    Shutdown,
}
impl From<mpsc::error::SendError<SubscriptionCommand>> for SubscribeError {
    fn from(_: mpsc::error::SendError<SubscriptionCommand>) -> Self {
        SubscribeError::Shutdown
    }
}
impl From<oneshot::error::RecvError> for SubscribeError {
    fn from(_: oneshot::error::RecvError) -> Self {
        SubscribeError::Shutdown
    }
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum JoinError {
    #[error("timeout joining channel")]
    Timeout,
    #[error("channel shutting down")]
    ShuttingDown,
    #[error("channel already shutdown")]
    Shutdown,
    #[error("server disconnected the socket")]
    ServerDisconnected,
    #[error("socket was disconnect while channel was being joined")]
    SocketDisconnected,
    #[error("leaving channel while still waiting to see if join succeeded")]
    LeavingWhileJoining,
    #[error("waiting to rejoin")]
    WaitingToRejoin(Instant),
    #[error("server rejected join")]
    Rejected(Arc<Payload>),
}
impl From<oneshot::error::RecvError> for JoinError {
    fn from(_: oneshot::error::RecvError) -> Self {
        JoinError::Shutdown
    }
}
impl From<mpsc::error::SendError<StateCommand>> for JoinError {
    fn from(_: mpsc::error::SendError<StateCommand>) -> Self {
        JoinError::Shutdown
    }
}
impl From<Elapsed> for JoinError {
    fn from(_: Elapsed) -> Self {
        JoinError::Timeout
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CastError {
    #[error("channel already shutdown")]
    Shutdown,
    #[error("socket already shutdown")]
    SocketShutdown,
    #[error("URL error: {0}")]
    Url(UrlError),
    #[error("HTTP error: {}", .0.status())]
    Http(Response<Option<String>>),
    /// HTTP format error.
    #[error("HTTP format error: {0}")]
    HttpFormat(http::Error),
}
impl From<mpsc::error::SendError<SendCommand>> for CastError {
    fn from(_: mpsc::error::SendError<SendCommand>) -> Self {
        CastError::Shutdown
    }
}
impl From<socket::listener::ShutdownError> for CastError {
    fn from(shutdown_error: socket::listener::ShutdownError) -> Self {
        match shutdown_error {
            socket::listener::ShutdownError::AlreadyJoined => CastError::SocketShutdown,
            socket::listener::ShutdownError::Url(url_error) => CastError::Url(url_error),
            socket::listener::ShutdownError::Http(response) => CastError::Http(response),
            socket::listener::ShutdownError::HttpFormat(error) => CastError::HttpFormat(error),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error("channel already shutdown")]
    Shutdown,
    #[error("timeout making call")]
    Timeout,
    #[error("web socket error {0}")]
    WebSocketError(tungstenite::Error),
    #[error("socket disconnected while waiting for reply")]
    SocketDisconnected,
}
impl From<mpsc::error::SendError<SendCommand>> for CallError {
    fn from(_: mpsc::error::SendError<SendCommand>) -> Self {
        CallError::Shutdown
    }
}
impl From<Elapsed> for CallError {
    fn from(_: Elapsed) -> Self {
        CallError::Timeout
    }
}
impl From<oneshot::error::RecvError> for CallError {
    fn from(_: oneshot::error::RecvError) -> Self {
        CallError::Shutdown
    }
}
