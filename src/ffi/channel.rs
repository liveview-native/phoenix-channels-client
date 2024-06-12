//! Multiple [Channel]s are multiplexed over a single [Socket].  [Channel]s have a specific
//! [topic](Channel::topic) need to be [joined](Channel::join) when additional
//! [payload](Channel::payload) is sent to the server to authorize or customize the [topic](Channel::topic) for this specific [Channel].
//!
//! [Channel] are not created directly, but from a [Socket] with [Socket::channel].
//!
//! ```rust,no_run
//! # use std::time::Duration;
//! #
//! # use serde_json::json;
//! # use url::Url;
//! #
//! # use phoenix_channels_client::{PhoenixError, Payload, Socket};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), PhoenixError> {
//! # // URL with params for authentication
//! # use phoenix_channels_client::Topic;
//! let url = Url::parse_with_params(
//! #     "ws://127.0.0.1:9002/socket/websocket",
//! #     &[("shared_secret", "supersecret"), ("id", "user-id")],
//! # )?;
//! #
//! # // Create a socket
//! # let socket = Socket::spawn(url, None).unwrap();
//! #
//! # // Connect the socket
//! # socket.connect(Duration::from_secs(10)).await?;
//! #
//! // Create a channel without payload
//! let channel_without_payload = socket.channel(
//!   Topic::from_string("channel:without_payload".to_string()),
//!   None
//! ).await?;
//! // Create a channel with JSON payload
//! let channel_with_json_payload = socket.channel(
//!   Topic::from_string("channel:with_json_payload".to_string()),
//!   Some(Payload::json_from_serialized(json!({ "status": "ok", "code": 200u8}).to_string()).unwrap())
//! ).await?;
//! /// Create a channel with binary payload
//! let channel_with_binary_payload = socket.channel(
//!   Topic::from_string("channel:with_binary_payload".to_string()),
//!   Some(Payload::binary_from_bytes(vec![0, 1, 2, 3]))
//! ).await?;
//! #
//! # Ok(())
//! # }
//! ```
//!
//! Once you have a created channel you need to [join](Channel::join) it to tell the server you want
//! to join the channel.
//!
//! ```rust,no_run
//! # use std::time::Duration;
//! #
//! # use serde_json::json;
//! # use url::Url;
//! #
//! # use phoenix_channels_client::{PhoenixError, Socket, Topic};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), PhoenixError> {
//! # // URL with params for authentication
//! let url = Url::parse_with_params(
//! #     "ws://127.0.0.1:9002/socket/websocket",
//! #     &[("shared_secret", "supersecret"), ("id", "user-id")],
//! # )?;
//! #
//! # // Create a socket
//! # let socket = Socket::spawn(url, None).unwrap();
//! #
//! # // Connect the socket
//! # socket.connect(Duration::from_secs(10)).await?;
//! #
//! # // Create a channel without payload
//! # let channel = socket.channel(Topic::from_string("channel:subtopic".to_string()), None).await?;
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
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use std::time::{Duration, SystemTime};
//! #
//! # use serde_json::{json, Value};
//! # use tokio::time::Instant;
//! # use url::Url;
//! # use uuid::Uuid;
//! #
//! # use phoenix_channels_client::{ChannelJoinError, ChannelStatus, PhoenixError,  Event, Payload, Socket,
//! #  Topic};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), PhoenixError> {
//! # let id = id();
//! # // URL with params for authentication
//! # let url = Url::parse_with_params(
//! #     "ws://127.0.0.1:9002/socket/websocket",
//! #     &[("shared_secret", "supersecret".to_string()), ("id", id.clone())],
//! # )?;
//! #
//! # // Create a socket
//! # let socket = Socket::spawn(url, None)?;
//! #
//! # // Connect the socket
//! # socket.connect(Duration::from_secs(10)).await?;
//! #
//! # // Create a channel without payload
//! let topic = "channel:protected";
//! let channel = socket.channel(Topic::from_string(topic.to_string()), None).await?;
//! let mut statuses = channel.statuses();
//! let rejection = match channel.join(Duration::from_secs(10)).await {
//!     Err(ChannelJoinError::Rejected { rejection }) => rejection,
//!     other => panic!("Join wasn't rejected and instead {:?}", other)
//! };
//! println!("user isn't authorized for {}: {}", &topic, rejection);
//! match statuses.status().await {
//!     Ok(ChannelStatus::Joining) => (),
//!     other => panic!("Didn't start joining and instead {:?}", other)
//! }
//! let payload = match statuses.status().await {
//!     Err(payload) => payload,
//!     other => panic!("Didn't get join error and instead {:?}", other)
//! };
//! println!("user isn't authorized for {}: {}", &topic, payload);
//!
//! // rejoin happens immediately, so we'll see failed rejoin attempts before authorize takes effect.
//! let until = match statuses.status().await {
//!     Ok(ChannelStatus::WaitingToRejoin { until }) => until,
//!     other => panic!("Didn't wait to rejoin after being unauthorized instead {:?}", other)
//! };
//! println!("Will rejoin in {:?}", until.duration_since(SystemTime::now()).unwrap_or_else(|_| Duration::from_micros(0)));
//! match statuses.status().await {
//!     Ok(ChannelStatus::Joining) => (),
//!     other => panic!("Didn't start joining after waiting instead {:?}", other)
//! }
//! let payload = match statuses.status().await {
//!     Err(payload) => payload,
//!     other => panic!("Didn't get join error instead {:?}", other)
//! };
//! println!("user isn't authorized for {}: {}", &topic, payload);
//!
//! // rejoin with delay gives us enough time to authorize
//! let until = match statuses.status().await {
//!     Ok(ChannelStatus::WaitingToRejoin { until }) => until,
//!     other => panic!("Didn't wait to rejoin after being unauthorized instead {:?}", other)
//! };
//! println!("Will rejoin in {:?}", until.duration_since(SystemTime::now()).unwrap_or_else(|_| Duration::from_micros(0)));
//!
//! authorize(&id, &topic).await;
//!
//! loop {
//!     match statuses.status().await {
//!         Ok(ChannelStatus::Joining) => (),
//!         other => panic!("Didn't start joining after waiting instead {:?}", other)
//!     }
//!     match statuses.status().await {
//!        Ok(ChannelStatus::WaitingToRejoin { until }) => println!("Will rejoin in {:?}",  until.duration_since(SystemTime::now()).unwrap_or_else(|_| Duration::from_micros(0))),
//!        Ok(ChannelStatus::Joined) => break,
//!        other => panic!("Didn't wait to rejoin or join {:?}", other)
//!     }
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
//! #         &[("shared_secret", "supersecret"), ("id", id)],
//! #     ).unwrap();
//! #
//! #     let socket = Socket::spawn(url, None).unwrap();
//! #     socket.connect(Duration::from_secs(10)).await.unwrap();
//! #
//! #     let channel = socket.channel(
//! #         Topic::from_string("channel:authorize".to_string()),
//! #         None
//! #     ).await.unwrap();
//! #     channel.join(Duration::from_secs(10)).await.unwrap();
//! #
//! #     channel.call(
//! #         Event::from_string("authorize".to_string()),
//! #         Payload::json_from_serialized(
//! #             json!({"channel": channel_name, "id": id}).to_string()
//! #         ).unwrap(),
//! #         Duration::from_secs(10)
//! #     ).await.unwrap();
//! #
//! #     channel.shutdown().await.unwrap();
//! #     socket.shutdown().await.unwrap();
//! # }
//! ```

use std::panic;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use atomic_take::AtomicTake;
use log::{debug, error, trace};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::error::Elapsed;
use tokio::time::Instant;

use crate::ffi::channel::statuses::ChannelStatuses;
use crate::ffi::message::{Event, Payload};
use crate::ffi::socket::SocketShutdownError;
use crate::ffi::topic::Topic;
use crate::ffi::web_socket::error::WebSocketError;
use crate::ffi::{instant_to_system_time, web_socket};
use crate::rust;
use crate::rust::channel::listener::{JoinError, ObservableStatus, SendCommand, StateCommand};
use crate::rust::channel::Call;

pub mod statuses;

/// Errors returned by [Channel] functions.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ChannelError {
    /// Errors when calling [Channel::join].
    #[error(transparent)]
    Join {
        /// Errors when calling [Channel::join].
        #[from]
        join: ChannelJoinError,
    },
    /// Errors when calling [Channel::cast].
    #[error(transparent)]
    Cast {
        /// Errors when calling [Channel::cast].
        #[from]
        cast: CastError,
    },
    /// Errors when calling [Channel::call].
    #[error(transparent)]
    Call {
        /// Errors when calling [Channel::call].
        #[from]
        call: CallError,
    },
    /// Errors when calling [Channel::leave].
    #[error(transparent)]
    Leave {
        /// Errors when calling [Channel::leave].
        #[from]
        leave: LeaveError,
    },
    /// Errors when calling [Channel::shutdown].
    #[error(transparent)]
    Shutdown {
        /// Errors when calling [Channel::shutdown].
        #[from]
        shutdown: ChannelShutdownError,
    },
}

/// A [Channel] is created with [Socket::channel](crate::Socket::channel)
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
#[derive(uniffi::Object)]
pub struct Channel {
    pub(crate) topic: Arc<Topic>,
    pub(crate) payload: crate::rust::message::Payload,
    /// The channel status
    pub(crate) status: ObservableStatus,
    pub(crate) event_payload_tx: broadcast::Sender<crate::rust::message::EventPayload>,
    pub(crate) shutdown_tx: AtomicTake<oneshot::Sender<()>>,
    pub(crate) state_command_tx: mpsc::Sender<StateCommand>,
    pub(crate) send_command_tx: mpsc::Sender<SendCommand>,
    /// The join handle corresponding to the channel listener
    pub(crate) join_handle: AtomicTake<JoinHandle<Result<(), ChannelShutdownError>>>,
}

#[uniffi::export(async_runtime = "tokio")]
impl Channel {
    /// Join [Channel::topic] with [Channel::payload] within `timeout`.
    pub async fn join(&self, timeout: Duration) -> Result<Payload, ChannelJoinError> {
        #[allow(clippy::type_complexity)]
        let (joined_tx, joined_rx): (
            oneshot::Sender<Result<crate::rust::message::Payload, JoinError>>,
            oneshot::Receiver<Result<crate::rust::message::Payload, JoinError>>,
        ) = oneshot::channel();

        match self
            .state_command_tx
            .send(StateCommand::Join {
                created_at: Instant::now(),
                timeout,
                joined_tx,
            })
            .await
        {
            Ok(()) => match time::timeout(timeout, joined_rx).await? {
                Ok(result) => result.map_err(From::from).map(From::from),
                Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
            },
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    /// Returns the topic this channel is connected to
    pub fn topic(&self) -> Arc<Topic> {
        self.topic.clone()
    }

    /// Returns the payload sent to the channel when joined
    pub fn payload(&self) -> Payload {
        (&self.payload).into()
    }

    /// The current [ChannelStatus].
    ///
    /// Use [Channel::statuses] to receive changes to the status.
    pub fn status(&self) -> ChannelStatus {
        self.status.get().into()
    }

    /// Broadcasts [Channel::status] changes.
    ///
    /// Use [Channel::status] to see the current status.
    pub fn statuses(&self) -> Arc<ChannelStatuses> {
        Arc::new(self.status.subscribe().into())
    }

    /// Broadcasts [EventPayload] sent from server.
    pub fn events(&self) -> Arc<Events> {
        Arc::new(self.event_payload_tx.subscribe().into())
    }

    /// Sends `event` with `payload` to this channel, and returns `Ok` if successful.
    ///
    /// This function does not wait for any reply, if you need the reply, then use `send` or `send_with_timeout`.
    pub async fn cast(&self, event: Event, payload: Payload) -> Result<(), CastError> {
        trace!(
            "sending event {:?} with payload {:?}, replies ignored",
            &event,
            &payload
        );

        match self
            .send_command_tx
            .send(SendCommand::Cast(crate::rust::message::EventPayload {
                event: event.into(),
                payload: payload.into(),
            }))
            .await
        {
            Ok(()) => Ok(()),
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    /// Like `send`, except it takes a configurable `timeout` for awaiting the reply.
    ///
    /// If `timeout` is None, it is equivalent to `send`, and waits forever.
    /// If `timeout` is Some(Duration), then waiting for the reply will stop after the duration expires,
    /// and a `SendError::Timeout` will be produced. If the reply is received before that occurs, then
    /// the reply payload will be returned.
    pub async fn call(
        &self,
        event: Event,
        payload: Payload,
        timeout: Duration,
    ) -> Result<Payload, CallError> {
        trace!(
            "sending event {:?} with timeout {:?} and payload {:?}",
            &event,
            &timeout,
            &payload
        );

        let (reply_tx, reply_rx) = oneshot::channel();

        match self
            .send_command_tx
            .send(SendCommand::Call(Call {
                event_payload: crate::rust::message::EventPayload {
                    event: event.into(),
                    payload: payload.into(),
                },
                reply_tx,
            }))
            .await
        {
            Ok(()) => {
                debug!("Waiting for reply for {:?} timeout", &timeout);

                match time::timeout(timeout, reply_rx).await? {
                    Ok(result) => result.map(From::from).map_err(From::from),
                    Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
                }
            }
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    /// Leaves this channel
    pub async fn leave(&self) -> Result<(), LeaveError> {
        let (left_tx, left_rx) = oneshot::channel();

        match self
            .state_command_tx
            .send(StateCommand::Leave { left_tx })
            .await
        {
            Ok(()) => match left_rx.await {
                Ok(result) => result.map_err(From::from),
                Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
            },
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    /// Propagates panic from async task.
    pub async fn shutdown(&self) -> Result<(), ChannelShutdownError> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            shutdown_tx.send(()).ok();
        }

        self.listener_shutdown().await
    }

    /// Propagates panic from async task.
    async fn listener_shutdown(&self) -> Result<(), ChannelShutdownError> {
        match self.join_handle.take() {
            Some(join_handle) => match join_handle.await {
                Ok(result) => result,
                Err(join_error) => panic::resume_unwind(join_error.into_panic()),
            },
            None => Err(ChannelShutdownError::AlreadyJoined),
        }
    }
}

/// Errors when calling [Channel::join].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ChannelJoinError {
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
    /// The [Socket](crate::Socket) the channel is on shutdown
    #[error("socket shutdown: {socket_shutdown_error}")]
    SocketShutdown {
        /// The error that shutdown the [Socket](crate::Socket)
        socket_shutdown_error: SocketShutdownError,
    },
    /// The [Socket](crate::Socket) was disconnected after [Channel::join] was called while waiting
    /// for a response from the server.
    #[error("socket was disconnect while channel was being joined")]
    SocketDisconnected,
    /// [Channel::leave] was called while awaiting a response from the server to a previous
    /// [Channel::join]
    #[error("leaving channel while still waiting to see if join succeeded")]
    LeavingWhileJoining,
    /// The [Channel] is currently waiting `until` [SystemTime] to rejoin to not overload the
    /// server, so can't honor the explicit [Channel::join].
    #[error("waiting to rejoin until {until:?}")]
    WaitingToRejoin {
        /// When the [Channel] will rejoin.
        until: SystemTime,
    },
    /// The [Channel::payload] was rejected when attempting to [Channel::join] or automatically
    /// rejoin [Channel::topic].
    #[error("server rejected join {rejection}")]
    Rejected {
        /// Rejection server sent when attempting to join the [Channel].
        rejection: Payload,
    },
}
impl From<rust::channel::listener::JoinError> for ChannelJoinError {
    fn from(rust_join_error: rust::channel::listener::JoinError) -> Self {
        match rust_join_error {
            rust::channel::listener::JoinError::Timeout => Self::Timeout,
            rust::channel::listener::JoinError::ShuttingDown => Self::ShuttingDown,
            rust::channel::listener::JoinError::Shutdown => Self::Shutdown,
            rust::channel::listener::JoinError::SocketShutdown(shutdown_error) => {
                Self::SocketShutdown {
                    socket_shutdown_error: shutdown_error.as_ref().into(),
                }
            }
            rust::channel::listener::JoinError::SocketDisconnected => Self::SocketDisconnected,
            rust::channel::listener::JoinError::LeavingWhileJoining => Self::LeavingWhileJoining,
            rust::channel::listener::JoinError::WaitingToRejoin(until) => Self::WaitingToRejoin {
                until: instant_to_system_time(until),
            },
            rust::channel::listener::JoinError::Rejected(payload) => Self::Rejected {
                rejection: payload.as_ref().into(),
            },
        }
    }
}
impl From<ChannelShutdownError> for ChannelJoinError {
    fn from(shutdown_error: ChannelShutdownError) -> Self {
        match shutdown_error {
            ChannelShutdownError::SocketShutdown | ChannelShutdownError::AlreadyJoined => {
                ChannelJoinError::Shutdown
            }
        }
    }
}

/// The [EventPayload::event] sent by the server along with the [EventPayload::payload] for that
/// [EventPayload::event].
#[derive(Clone, Debug, uniffi::Record)]
pub struct EventPayload {
    /// The [Event] name.
    pub event: Event,
    /// The data sent for the [EventPayload::event].
    pub payload: Payload,
}
impl From<rust::message::EventPayload> for EventPayload {
    fn from(rust_event_payload: rust::message::EventPayload) -> Self {
        let rust::message::EventPayload { event, payload } = rust_event_payload;

        Self {
            event: event.into(),
            payload: payload.into(),
        }
    }
}

/// Waits for [EventPayload]s sent from the server.
#[derive(uniffi::Object)]
pub struct Events(Mutex<broadcast::Receiver<rust::message::EventPayload>>);
#[uniffi::export]
impl Events {
    /// Wait for next [EventPayload] sent from the server.
    pub async fn event(&self) -> Result<EventPayload, EventsError> {
        self.0
            .lock()
            .await
            .recv()
            .await
            .map(From::from)
            .map_err(From::from)
    }
}
impl From<broadcast::Receiver<rust::message::EventPayload>> for Events {
    fn from(inner: broadcast::Receiver<rust::message::EventPayload>) -> Self {
        Self(Mutex::new(inner))
    }
}

/// Errors when calling [Events::event].
// Wraps [tokio::sync::broadcast::error::RecvError] to add `uniffi` support and names specific to [Events]
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum EventsError {
    /// There are no more events because the [Channel] shutdown.
    #[error("No more events left")]
    NoMoreEvents,
    /// [Events::event] wasn't called often enough and some [EventPayload] won't be sent to not
    /// block the other receivers or the sender.  Call [Events::event] to catch up and get the next
    /// [EventPayload].
    #[error("Missed {missed_event_count} events; jumping to next event")]
    MissedEvents {
        /// How many [EventPayload] were missed.
        missed_event_count: u64,
    },
}
impl From<broadcast::error::RecvError> for EventsError {
    fn from(recv_error: broadcast::error::RecvError) -> Self {
        match recv_error {
            broadcast::error::RecvError::Closed => Self::NoMoreEvents,
            broadcast::error::RecvError::Lagged(missed_event_count) => {
                Self::MissedEvents { missed_event_count }
            }
        }
    }
}

impl From<oneshot::error::RecvError> for ChannelJoinError {
    fn from(_: oneshot::error::RecvError) -> Self {
        ChannelJoinError::Shutdown
    }
}

impl From<mpsc::error::SendError<StateCommand>> for ChannelJoinError {
    fn from(_: mpsc::error::SendError<StateCommand>) -> Self {
        ChannelJoinError::Shutdown
    }
}

impl From<Elapsed> for ChannelJoinError {
    fn from(_: Elapsed) -> Self {
        ChannelJoinError::Timeout
    }
}

/// Errors when calling [Channel::cast].
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CastError {
    /// The async task for the [Channel] was already joined by another call, so the [Result] or
    /// panic from the async task can't be reported here.
    #[error("channel already shutdown")]
    Shutdown,
    /// The async task for the [Socket] was already joined by another call, so the [Result] or panic
    /// from the async task can't be reported here.
    #[error("socket already shutdown")]
    SocketShutdown,
    /// [tokio_tungstenite::tungstenite::error::UrlError] with the `url` passed to [Socket::spawn].  This can include
    /// incorrect scheme ([tokio_tungstenite::tungstenite::error::UrlError::UnsupportedUrlScheme]).
    #[error("URL error: {url_error}")]
    Url { url_error: String },
    /// HTTP error response from server.
    #[error("HTTP error: {}", response.status_code)]
    Http { response: super::http::Response },
    /// HTTP format error.
    #[error("HTTP format error: {error}")]
    HttpFormat { error: super::http::HttpError },
}
impl From<ChannelShutdownError> for CastError {
    fn from(shutdown_error: ChannelShutdownError) -> Self {
        match shutdown_error {
            ChannelShutdownError::SocketShutdown | ChannelShutdownError::AlreadyJoined => {
                Self::Shutdown
            }
        }
    }
}

/// Errors when calling [Channel::call].
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CallError {
    /// The async task for the [Channel] was already joined by another call, so the [Result] or
    /// panic from the async task can't be reported here.
    #[error("channel already shutdown")]
    Shutdown,
    /// Error from [Socket::shutdown](crate::Socket::shutdown) or from the server itself that caused
    /// the [Socket](crate::Socket) to shutdown.
    #[error("socket shutdown: {socket_shutdown_error}")]
    SocketShutdown {
        /// Error from [Socket::shutdown](crate::Socket::shutdown) or from the server itself that
        /// caused the [Socket](crate::Socket) to shutdown.
        socket_shutdown_error: SocketShutdownError,
    },
    /// Timeout passed to [Channel::call] has expired.
    #[error("timeout making call")]
    Timeout,
    /// Error from [Channel]'s [Socket](crate::Socket)'s underlying
    /// [tokio_tungstenite::tungstenite::protocol::WebSocket].
    #[error("web socket error {web_socket_error:?}")]
    WebSocket {
        /// Error from [Channel]'s [Socket](crate::Socket)'s underlying
        /// [tokio_tungstenite::tungstenite::protocol::WebSocket].
        web_socket_error: WebSocketError,
    },
    /// [Socket::disconnect](crate::Socket::disconnect) called after [Channel::call] while waiting
    /// for a reply from the server.
    #[error("socket disconnected while waiting for reply")]
    SocketDisconnected,
    /// An error was returned from the server in reply to [Channel::call]'s `event` and `payload`.
    #[error("error from server {reply:?}")]
    Reply {
        /// Error response from the server.
        reply: Payload,
    },
}
impl From<Elapsed> for CallError {
    fn from(_: Elapsed) -> Self {
        CallError::Timeout
    }
}
impl From<ChannelShutdownError> for CallError {
    fn from(shutdown_error: ChannelShutdownError) -> Self {
        match shutdown_error {
            ChannelShutdownError::SocketShutdown | ChannelShutdownError::AlreadyJoined => {
                Self::Shutdown
            }
        }
    }
}
impl From<rust::channel::CallError> for CallError {
    fn from(rust_call_error: rust::channel::CallError) -> Self {
        match rust_call_error {
            rust::channel::CallError::Shutdown => Self::Shutdown,
            rust::channel::CallError::SocketShutdown(shutdown_error) => Self::SocketShutdown {
                socket_shutdown_error: shutdown_error.into(),
            },
            rust::channel::CallError::Timeout => Self::Timeout,
            rust::channel::CallError::WebSocketError(web_socket_error) => Self::WebSocket {
                web_socket_error: (&web_socket_error).into(),
            },
            rust::channel::CallError::SocketDisconnected => Self::SocketDisconnected,
            rust::channel::CallError::Reply(reply) => Self::Reply {
                reply: reply.into(),
            },
        }
    }
}

/// Errors for when leaving a channel fails.
#[derive(Clone, Debug, thiserror::Error, uniffi::Error)]
pub enum LeaveError {
    /// The leave operation timeout.
    #[error("timeout leaving channel")]
    Timeout,
    /// The channel was shutting down when the client was leaving the channel.
    #[error("channel shutting down")]
    ShuttingDown,
    /// The channel was shutting down when the client was leaving the channel.
    #[error("channel already shutdown")]
    Shutdown,
    /// The channel already shut down when the client was leaving the channel.
    #[error("socket shutdown: {socket_shutdown_error}")]
    SocketShutdown {
        /// The shut down error
        socket_shutdown_error: SocketShutdownError,
    },
    /// There was a websocket error when the client was leaving the channel.
    #[error("web socket error {web_socket_error:?}")]
    WebSocket {
        /// The specific websocket error for this shutdown.
        web_socket_error: web_socket::error::WebSocketError,
    },
    /// The server rejected the clients request to leave the channel.
    #[error("server rejected leave")]
    Rejected {
        /// Response from the server of why the leave was rejected
        rejection: Payload,
    },
    /// A join was initiated before the server could respond if leave was successful
    #[error("A join was initiated before the server could respond if leave was successful")]
    JoinBeforeLeft,
    /// There was a URL error when leaving the channel.
    #[error("URL error: {url_error}")]
    Url {
        /// The url error itself.
        url_error: String,
    },
    /// There was an HTTP error when leaving the cheannel.
    #[error("HTTP error: {}", response.status_code)]
    Http {
        /// The http response for the error.
        response: crate::ffi::http::Response,
    },
    /// HTTP format error.
    #[error("HTTP format error: {error}")]
    HttpFormat {
        /// The http error.
        error: crate::ffi::http::HttpError,
    },
}
impl From<ChannelShutdownError> for LeaveError {
    fn from(shutdown_error: ChannelShutdownError) -> Self {
        match shutdown_error {
            ChannelShutdownError::SocketShutdown | ChannelShutdownError::AlreadyJoined => {
                Self::Shutdown
            }
        }
    }
}
impl From<rust::channel::LeaveError> for LeaveError {
    fn from(rust_leave_error: rust::channel::LeaveError) -> Self {
        match rust_leave_error {
            rust::channel::LeaveError::Timeout => Self::Timeout,
            rust::channel::LeaveError::ShuttingDown => Self::ShuttingDown,
            rust::channel::LeaveError::Shutdown => Self::Shutdown,
            rust::channel::LeaveError::SocketShutdown(socket_shutdown_error) => {
                Self::SocketShutdown {
                    socket_shutdown_error: socket_shutdown_error.as_ref().into(),
                }
            }
            rust::channel::LeaveError::WebSocketError(web_socket_error) => Self::WebSocket {
                web_socket_error: web_socket_error.as_ref().into(),
            },
            rust::channel::LeaveError::Rejected(rejection) => Self::Rejected {
                rejection: rejection.into(),
            },
            rust::channel::LeaveError::JoinBeforeLeft => Self::JoinBeforeLeft,
        }
    }
}

/// The status of the [Channel].
#[derive(Copy, Clone, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum ChannelStatus {
    /// [Channel] is waiting for the [Socket](crate::Socket) to
    /// [Socket::connect](crate::Socket::connect) or automatically reconnect.
    WaitingForSocketToConnect,
    /// [Socket::status](crate::Socket::status) is
    /// [SocketStatus::Connected](crate::SocketStatus::Connected) and [Channel] is waiting for
    /// [Channel::join] to be called.
    WaitingToJoin,
    /// [Channel::join] was called and awaiting response from server.
    Joining,
    /// [Channel::join] was called previously, but the [Socket](crate::Socket) was disconnected and
    /// reconnected.
    WaitingToRejoin {
        /// When the [Channel] will automatically [Channel::join].
        until: SystemTime,
    },
    /// [Channel::join] was called and the server responded that the [Channel::topic] was joined
    /// using [Channel::payload].
    Joined,
    /// [Channel::leave] was called and awaiting response from server.
    Leaving,
    /// [Channel::leave] was called and the server responded that the [Channel::topic] was left.
    Left,
    /// [Channel::shutdown] was called, but the async task hasn't exited yet.
    ShuttingDown,
    /// The async task has exited.
    ShutDown,
}
impl From<rust::channel::Status> for ChannelStatus {
    fn from(rust_status: rust::channel::Status) -> Self {
        match rust_status {
            rust::channel::Status::WaitingForSocketToConnect => Self::WaitingForSocketToConnect,
            rust::channel::Status::WaitingToJoin => Self::WaitingToJoin,
            rust::channel::Status::Joining => Self::Joining,
            rust::channel::Status::WaitingToRejoin(until) => Self::WaitingToRejoin {
                until: instant_to_system_time(until),
            },
            rust::channel::Status::Joined => Self::Joined,
            rust::channel::Status::Leaving => Self::Leaving,
            rust::channel::Status::Left => Self::Left,
            rust::channel::Status::ShuttingDown => Self::ShuttingDown,
            rust::channel::Status::ShutDown => Self::ShutDown,
        }
    }
}

#[derive(Copy, Clone, Debug, thiserror::Error, PartialEq, Eq, uniffi::Error)]
pub enum ChannelShutdownError {
    #[error("socket already shutdown")]
    SocketShutdown,
    #[error("listener task was already joined once from another caller")]
    AlreadyJoined,
}
