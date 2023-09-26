//! Multiple [Channel]s are multiplexed over a single [Socket].  [Channel]s have a specific
//! [topic](Channel::topic) need to be [joined](Channel::join) when additional
//! [payload](Channel::payload) is sent to the server to authorize or customize the [topic](Channel::topic) for this specific [Channel].
//!
//! [Channel] are not created directly, but from a [Socket] with [Socket::channel].
//!
//! ```
//! # use std::time::Duration;
//! #
//! # use serde_json::json;
//! # use url::Url;
//! #
//! # use phoenix_channels_client::{Error, Socket};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), Error> {
//! # // URL with params for authentication
//! # let url = Url::parse_with_params(
//! #     "ws://127.0.0.1:9002/socket/websocket",
//! #     &[("shared_secret", "supersecret"), ("id", "user-id")],
//! # )?;
//! #
//! # // Create a socket
//! # let socket = Socket::spawn(url).await?;
//! #
//! # // Connect the socket
//! # socket.connect(Duration::from_secs(10)).await?;
//! #
//! // Create a channel without payload
//! let channel_without_payload = socket.channel("channel:without_payload", None).await?;
//! // Create a channel with JSON payload
//! let channel_with_json_payload = socket.channel("channel:with_json_payload", Some(json!({ "status": "ok", "code": 200u8}).into())).await?;
//! /// Create a channel with binary payload
//! let channel_with_binary_payload = socket.channel("channel:with_binary_payload", Some(vec![0, 1, 2, 3].into())).await?;
//! #
//! # Ok(())
//! # }
//! ```
//!
//! Once you have a created channel you need to [join](Channel::join) it to tell the server you want
//! to join the channel.
//!
//! ```
//! # use std::time::Duration;
//! #
//! # use serde_json::json;
//! # use url::Url;
//! #
//! # use phoenix_channels_client::{Error, Socket};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), Error> {
//! # // URL with params for authentication
//! # let url = Url::parse_with_params(
//! #     "ws://127.0.0.1:9002/socket/websocket",
//! #     &[("shared_secret", "supersecret"), ("id", "user-id")],
//! # )?;
//! #
//! # // Create a socket
//! # let socket = Socket::spawn(url).await?;
//! #
//! # // Connect the socket
//! # socket.connect(Duration::from_secs(10)).await?;
//! #
//! # // Create a channel without payload
//! # let channel = socket.channel("channel:subtopic", None).await?;
//! channel.join(Duration::from_secs(10)).await?;
//! # Ok(())
//! }
//! ```
//!
//! If the server uses authentication for individual channels it is important to
//! [monitor the status of the channel](Channel::statuses), to be notified when the
//! [payload](Channel::payload) is no longer valid to authenticate to the channel and a new
//! [Channel] with the new authentication payload should be [created](Socket::channel).
//!
//! ```
//! # use std::sync::Arc;
//! # use std::time::Duration;
//! #
//! # use serde_json::{json, Value};
//! # use tokio::time::Instant;
//! # use url::Url;
//! # use uuid::Uuid;
//! #
//! # use phoenix_channels_client::{channel, Error, JoinError, Payload, Socket};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), Error> {
//! # let id = id();
//! # // URL with params for authentication
//! # let url = Url::parse_with_params(
//! #     "ws://127.0.0.1:9002/socket/websocket",
//! #     &[("shared_secret", "supersecret".to_string()), ("id", id.clone())],
//! # )?;
//! #
//! # // Create a socket
//! # let socket = Socket::spawn(url).await?;
//! #
//! # // Connect the socket
//! # socket.connect(Duration::from_secs(10)).await?;
//! #
//! # // Create a channel without payload
//! let topic = "channel:protected";
//! let channel = socket.channel(topic, None).await?;
//! let mut statuses = channel.statuses();
//! let payload = match channel.join(Duration::from_secs(10)).await {
//!     Err(JoinError::Rejected(payload)) => payload,
//!     other => panic!("Join wasn't rejected and instead {:?}", other)
//! };
//! println!("user isn't authorized for {}: {}", &topic, payload);
//! match statuses.recv().await? {
//!     Ok(channel::Status::Joining) => (),
//!     other => panic!("Didn't start joining and instead {:?}", other)
//! }
//! let payload = match statuses.recv().await? {
//!     Err(payload) => payload,
//!     other => panic!("Didn't get join error and instead {:?}", other)
//! };
//! println!("user isn't authorized for {}: {}", &topic, payload);
//!
//! // rejoin happens immediately, so we'll see failed rejoin attempts before authorize takes effect.
//! let until = match statuses.recv().await? {
//!     Ok(channel::Status::WaitingToRejoin(until)) => until,
//!     other => panic!("Didn't wait to rejoin after being unauthorized instead {:?}", other)
//! };
//! println!("Will rejoin in {:?}", until.checked_duration_since(Instant::now()).unwrap_or_else(|| Duration::from_micros(0)));
//! match statuses.recv().await? {
//!     Ok(channel::Status::Joining) => (),
//!     other => panic!("Didn't start joining after waiting instead {:?}", other)
//! }
//! let payload = match statuses.recv().await? {
//!     Err(payload) => payload,
//!     other => panic!("Didn't get join error instead {:?}", other)
//! };
//! println!("user isn't authorized for {}: {}", &topic, payload);
//!
//! // rejoin with delay gives us enough time to authorize
//! let until = match statuses.recv().await? {
//!     Ok(channel::Status::WaitingToRejoin(until)) => until,
//!     other => panic!("Didn't wait to rejoin after being unauthorized instead {:?}", other)
//! };
//! println!("Will rejoin in {:?}", until.checked_duration_since(Instant::now()).unwrap_or_else(|| Duration::from_micros(0)));
//!
//! authorize(&id, &topic).await;
//!
//! match statuses.recv().await? {
//!     Ok(channel::Status::Joining) => (),
//!     other => panic!("Didn't start joining after waiting instead {:?}", other)
//! }
//! match statuses.recv().await? {
//!     Ok(channel::Status::Joined)  => (),
//!     other => panic!("Didn't join after being authorized instead {:?}", other)
//! }
//! # Ok(())
//! # }
//! #
//! # fn id() -> String {
//! #     Uuid::new_v4()
//! #         .hyphenated()
//! #         .encode_upper(&mut Uuid::encode_buffer())
//! #         .to_string()
//! # }
//! # async fn authorize(id: &str, channel_name: &str) {
//! #     let url = Url::parse_with_params(
//! #         "ws://127.0.0.1:9002/socket/websocket",
//! #         &[("shared_secret", "supersecret"), ("id", id.clone())],
//! #     ).unwrap();
//! #
//! #     let socket = Socket::spawn(url).await.unwrap();
//! #     socket.connect(Duration::from_secs(10)).await.unwrap();
//! #
//! #     let channel = socket.channel("channel:authorize", None).await.unwrap();
//! #     channel.join(Duration::from_secs(10)).await.unwrap();
//! #
//! #     channel.call("authorize", json!({"channel": channel_name, "id": id}), Duration::from_secs(10)).await.unwrap();
//! #
//! #     channel.shutdown().await.unwrap();
//! #     socket.shutdown().await.unwrap();
//! # }
//! ```

pub(crate) mod listener;

use atomic_take::AtomicTake;
use std::panic;
use std::sync::Arc;
use std::time::Duration;

use flexstr::SharedStr;
use log::{debug, error};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::error::Elapsed;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::error::UrlError;
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::http::Response;

pub use crate::channel::listener::Status;
pub(crate) use crate::channel::listener::{Call, LeaveError, ShutdownError};
use crate::channel::listener::{Listener, ObservableStatus, SendCommand, StateCommand};
use crate::message::*;
use crate::socket;
use crate::socket::listener::Connectivity;
use crate::socket::Socket;
use crate::topic::Topic;

/// Errors returned by [Channel] functions.
#[derive(Error, Debug)]
pub enum Error {
    /// Errors when calling [Channel::join].
    #[error(transparent)]
    Join(#[from] JoinError),
    /// Errors when calling [Channel::cast].
    #[error(transparent)]
    Cast(#[from] CastError),
    /// Errors when calling [Channel::call].
    #[error(transparent)]
    Call(#[from] CallError),
    /// Errors when calling [Channel::leave].
    #[error(transparent)]
    Leave(#[from] LeaveError),
    /// Errors when calling [Channel::shutdown].
    #[error(transparent)]
    Shutdown(#[from] ShutdownError),
}

/// The [EventPayload::event] sent by the server along with the [EventPayload::payload] for that
/// [EventPayload::event].
#[derive(Clone, Debug)]
pub struct EventPayload {
    /// The [Event] name.
    pub event: Event,
    /// The data sent for the [EventPayload::event].
    pub payload: Payload,
}
impl From<Broadcast> for EventPayload {
    fn from(broadcast: Broadcast) -> Self {
        broadcast.event_payload
    }
}
impl From<Push> for EventPayload {
    fn from(push: Push) -> Self {
        push.event_payload
    }
}

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
/// * [Channel::call] to send a message and await a reply from the server
/// * [Channel::cast] to send a message and ignore any replies
///
pub struct Channel {
    topic: Topic,
    payload: Payload,
    /// The channel status
    status: ObservableStatus,
    event_payload_tx: broadcast::Sender<EventPayload>,
    shutdown_tx: AtomicTake<oneshot::Sender<()>>,
    state_command_tx: mpsc::Sender<StateCommand>,
    send_command_tx: mpsc::Sender<SendCommand>,
    /// The join handle corresponding to the channel listener
    join_handle: AtomicTake<JoinHandle<Result<(), ShutdownError>>>,
}
impl Channel {
    /// Spawns a new [Channel] that must be [join]ed.  The `topic` and `payload` is sent on the
    /// first [join] and any rejoins if the underlying `socket` is disconnected and
    /// reconnects.
    pub(crate) async fn spawn(
        socket: Arc<Socket>,
        socket_connectivity_rx: broadcast::Receiver<Connectivity>,
        topic: SharedStr,
        payload: Option<Payload>,
        state: listener::State,
    ) -> Self {
        let topic: Topic = topic.into();
        let payload = payload.unwrap_or_default();
        let status = ObservableStatus::new(state.status());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (event_payload_tx, _) = broadcast::channel(10);
        let (state_command_tx, state_command_rx) = mpsc::channel(10);
        let (send_command_tx, send_command_rx) = mpsc::channel(10);
        let join_handle = Listener::spawn(
            socket,
            socket_connectivity_rx,
            topic.clone(),
            payload.clone(),
            state,
            status.clone(),
            shutdown_rx,
            event_payload_tx.clone(),
            state_command_rx,
            send_command_rx,
        );

        Self {
            topic,
            payload,
            status,
            event_payload_tx,
            shutdown_tx: AtomicTake::new(shutdown_tx),
            state_command_tx,
            send_command_tx,
            join_handle: AtomicTake::new(join_handle),
        }
    }

    /// Join [Channel::topic] with [Channel::payload] within `timeout`.
    pub async fn join(&self, timeout: Duration) -> Result<(), JoinError> {
        let (joined_tx, joined_rx) = oneshot::channel();

        self.state_command_tx
            .send(StateCommand::Join {
                created_at: Instant::now(),
                timeout,
                joined_tx,
            })
            .await?;

        time::timeout(timeout, joined_rx).await??
    }

    /// Returns the topic this channel is connected to
    pub fn topic(&self) -> Topic {
        self.topic.clone()
    }

    /// Returns the payload sent to the channel when joined
    pub fn payload(&self) -> Payload {
        self.payload.clone()
    }

    /// The current [Status].
    ///
    /// Use [Channel::statuses] to receive changes to the status.
    pub fn status(&self) -> Status {
        self.status.get()
    }

    /// Broadcasts [Socket::status] changes.
    ///
    /// Use [Socket::status] to see the current status.
    pub fn statuses(&self) -> broadcast::Receiver<Result<Status, Arc<Payload>>> {
        self.status.subscribe()
    }

    /// Broadcasts [EventPayload] sent from server.
    pub fn events(&self) -> broadcast::Receiver<EventPayload> {
        self.event_payload_tx.subscribe()
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

        self.send_command_tx
            .send(SendCommand::Cast(EventPayload { event, payload }))
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

        let (reply_tx, reply_rx) = oneshot::channel();

        self.send_command_tx
            .send(SendCommand::Call(Call {
                event_payload: EventPayload { event, payload },
                reply_tx,
            }))
            .await?;

        debug!("Waiting for reply for {:?} timeout", &timeout);

        time::timeout(timeout, reply_rx).await??
    }

    /// Leaves this channel
    pub async fn leave(&self) -> Result<(), LeaveError> {
        let (left_tx, left_rx) = oneshot::channel();

        self.state_command_tx
            .send(StateCommand::Leave { left_tx })
            .await?;

        match left_rx.await {
            Ok(result) => result,
            Err(_) => Err(LeaveError::Shutdown),
        }
    }

    /// Propagates panic from async task.
    pub async fn shutdown(&self) -> Result<(), ShutdownError> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            shutdown_tx.send(()).ok();
        }

        self.listener_shutdown().await
    }

    /// Propagates panic from async task.
    async fn listener_shutdown(&self) -> Result<(), ShutdownError> {
        match self.join_handle.take() {
            Some(join_handle) => match join_handle.await {
                Ok(result) => result,
                Err(join_error) => panic::resume_unwind(join_error.into_panic()),
            },
            None => Err(ShutdownError::AlreadyJoined),
        }
    }
}

/// Errors when calling [Channel::join].
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum JoinError {
    /// Timeout joining channel
    #[error("timeout joining channel")]
    Timeout,
    /// [Channel] shutting down because [Channel::shutdown] was called.
    #[error("channel shutting down")]
    ShuttingDown,
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("channel already shutdown")]
    Shutdown,
    /// The [Socket] was disconnected after [Channel::join] was called while waiting for a response
    /// from the server.
    #[error("socket was disconnect while channel was being joined")]
    SocketDisconnected,
    /// [Channel::leave] was called while awaiting a response from the server to a previous
    /// [Channel::join]
    #[error("leaving channel while still waiting to see if join succeeded")]
    LeavingWhileJoining,
    /// The [Channel] is currently waiting until [Instant] to rejoin to not overload the server, so
    /// can't honor the explicit [Channel::join].
    #[error("waiting to rejoin")]
    WaitingToRejoin(Instant),
    /// The [Channel::payload] was rejected when attempting to [Channel::join] or automatically
    /// rejoin [Channel::topic].
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

/// Errors when calling [Channel::cast].
#[derive(Debug, thiserror::Error)]
pub enum CastError {
    /// The async task for the [Channel] was already joined by another call, so the [Result] or
    /// panic from the async task can't be reported here.
    #[error("channel already shutdown")]
    Shutdown,
    /// The async task for the [Socket] was already joined by another call, so the [Result] or panic
    /// from the async task can't be reported here.
    #[error("socket already shutdown")]
    SocketShutdown,
    /// [tungstenite::error::UrlError] with the `url` passed to [Socket::spawn].  This can include
    /// incorrect scheme ([tungstenite::error::UrlError::UnsupportedUrlScheme]).
    #[error("URL error: {0}")]
    Url(UrlError),
    /// HTTP error response from server.
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

/// Errors when calling [Channel::call].
#[derive(Debug, thiserror::Error)]
pub enum CallError {
    /// The async task for the [Channel] was already joined by another call, so the [Result] or
    /// panic from the async task can't be reported here.
    #[error("channel already shutdown")]
    Shutdown,
    /// Timeout passed to [Channel::call] has expired.
    #[error("timeout making call")]
    Timeout,
    /// A [tokio_tungstenite::WebSocketStream] from [Channel]'s [Socket]'s underlying
    /// [tungstenite::protocol::WebSocket].
    #[error("web socket error {0}")]
    WebSocketError(tungstenite::Error),
    /// [Socket::disconnect] called after [Channel::call] while waiting for a reply from the server.
    #[error("socket disconnected while waiting for reply")]
    SocketDisconnected,
    /// An error was returned from the server in reply to [Channel::call]'s `event` and `payload`.
    #[error("error from server {0:?}")]
    Reply(Payload),
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
