//! A [Socket] connects to the server through a web socket.  [Socket]s need to be
//! [connected](Socket::connect) when the [Url] params are sent to the server to authorize or
//! customize for this specific [Socket].
//!
//! A [Socket] needs to be created with [Socket::spawn].
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
//! // URL with params for authentication
//! let url = Url::parse_with_params(
//!     "ws://127.0.0.1:9002/socket/websocket",
//!     &[("shared_secret", "supersecret"), ("id", "user-id")],
//! )?;
//!
//! // Create a socket
//! let socket = Socket::spawn(url).await?;
//! # Ok(())
//! # }
//! ```
//!
//! If the [Socket::spawn] [Url] does not have the correct params for authorization, then it will
//! pass back the error from [Socket::connect].
//!
//! ```
//! # use std::time::Duration;
//! #
//! # use serde_json::json;
//!#  use tokio_tungstenite::tungstenite;
//! # use url::Url;
//! #
//! # use phoenix_channels_client::{socket, Error, Socket};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), Error> {
//! // URL with params for authentication
//! use phoenix_channels_client::{ConnectError, socket};
//! let url = Url::parse_with_params(
//!     "ws://127.0.0.1:9002/socket/websocket",
//!     // WITHOUT shared secret
//!     &[("id", "user-id")],
//! )?;
//!
//! // Create a socket
//! let socket = Socket::spawn(url).await?;
//!
//! // Connecting the socket returns the authorization error
//! match socket.connect(Duration::from_secs(5)).await {
//!     Err(socket::ConnectError::WebSocketError(web_socket_error)) => match web_socket_error.as_ref() {
//!        tungstenite::Error::Http(response) => println!("Got status {} from server", response.status()),
//!        web_socket_error => panic!("Got an unexpected web socket error: {:?}", web_socket_error)
//!     },
//!     other => panic!("Didn't get authorization error and instead {:?}", other)
//! }
//! # Ok(())
//! # }
//! ```
//!
//! If the server uses authentication for individual sockets it is important to
//! [monitor the status of the socket](Socket::statuses), to be notified when the [Url] params are
//! no longer valif to authenticate to the socket and a new [Socket] with the new authentication
//! params should be [create](Socket::spawn).
//!
//!```
//! # use std::sync::Arc;
//! # use std::time::Duration;
//! #
//! # use serde_json::{json, Value};
//! # use tokio::time::Instant;
//! # use tokio_tungstenite::tungstenite;
//! # use url::Url;
//! # use uuid::Uuid;
//! #
//! # use phoenix_channels_client::{channel, socket, Error, Payload, Socket};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), Error> {
//! let id = id();
//! let secret = generate_secret(&id).await;
//! let secret_url = Url::parse_with_params(
//!     "ws://127.0.0.1:9002/socket/websocket",
//!     &[("secret", secret.clone()), ("id", id.clone())],
//! )?;
//! let secret_socket = Socket::spawn(secret_url).await?;
//! secret_socket.connect(Duration::from_secs(10)).await?;
//! let mut statuses = secret_socket.statuses();
//!
//! // Deauthorize the socket
//! delete_secret(&id, &secret).await;
//!
//! let until = match statuses.recv().await? {
//!     Ok(socket::Status::WaitingToReconnect(until)) => until,
//!     other => panic!("Didn't wait to reconnect and instead {:?}", other)
//! };
//! println!("Will reconnect in {:?}", until.checked_duration_since(Instant::now()).unwrap_or_else(|| Duration::from_micros(0)));
//! match statuses.recv().await? {
//!     Err(web_socket_error) => match web_socket_error.as_ref() {
//!        tungstenite::Error::Http(response) => println!("Got status {} from server", response.status()),
//!        web_socket_error => panic!("Got an unexpected web socket error: {:?}", web_socket_error)
//!     },
//!     other => panic!("Didn't get authorization error and instead {:?}", other)
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
//! #
//! # async fn generate_secret(id: &str) -> String {
//! #     let url = Url::parse_with_params(
//! #         "ws://127.0.0.1:9002/socket/websocket",
//! #         &[("shared_secret", "supersecret"), ("id", id.clone())],
//! #     ).unwrap();
//! #
//! #     let socket = Socket::spawn(url).await.unwrap();
//! #     socket.connect(Duration::from_secs(10)).await.unwrap();
//! #
//! #     let channel = socket.channel("channel:generate_secret", None).await.unwrap();
//! #     channel.join(Duration::from_secs(10)).await.unwrap();
//! #
//! #     let Payload::Value(value) = channel
//! #         .call("generate_secret", json!({}), Duration::from_secs(10))
//! #         .await
//! #         .unwrap()
//! #     else {
//! #         panic!("secret not returned")
//! #     };
//! #
//! #     let secret = if let Value::String(ref secret) = *value {
//! #         secret.to_owned()
//! #     } else {
//! #         panic!("secret ({:?}) is not a string", value);
//! #     };
//! #
//! #     secret
//! # }
//! #
//! # async fn delete_secret(id: &str, secret: &str) {
//! #    let url = Url::parse_with_params(
//! #        "ws://127.0.0.1:9002/socket/websocket",
//! #        &[("secret", secret), ("id", id.clone())],
//! #    ).unwrap();
//! #
//! #     let socket = Socket::spawn(url).await.unwrap();
//! #     socket.connect(Duration::from_secs(10)).await.unwrap();
//! #
//! #     let channel = socket.channel("channel:secret", None).await.unwrap();
//! #     channel.join(Duration::from_secs(10)).await.unwrap();
//! #
//! #     match channel.call("delete_secret", json!({}), Duration::from_secs(10)).await {
//! #         Ok(payload) => panic!("Deleting secret succeeded without disconnecting socket and returned payload: {:?}", payload),
//! #         Err(channel::CallError::SocketDisconnected) => (),
//! #         Err(other) => panic!("Error other than SocketDisconnected: {:?}", other)
//! #     }
//! # }
pub(crate) mod listener;

use std::panic;
use std::sync::Arc;
use std::time::Duration;

use atomic_take::AtomicTake;
use flexstr::SharedStr;
use log::error;
use thiserror::Error;
use tokio::sync::oneshot;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::error::Elapsed;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite;
use url::Url;

use crate::channel::listener::{JoinedChannelReceivers, LeaveError};
use crate::join_reference::JoinReference;
use crate::message::*;
use crate::socket::listener::{
    ChannelSendCommand, ChannelSpawn, ChannelStateCommand, Connect, Join, Leave, Listener,
    ObservableStatus, StateCommand,
};
pub use crate::socket::listener::{ShutdownError, Status};
use crate::topic::Topic;
use crate::{channel, Channel, EventPayload};

/// Errors when calling [Socket] functions.
#[derive(Error, Debug)]
pub enum Error {
    /// Error when calling [Socket::spawn].
    #[error(transparent)]
    Spawn(#[from] SpawnError),
    /// Error when calling [Socket::connect].
    #[error(transparent)]
    Connect(#[from] ConnectError),
    /// Error when calling [Socket::channel].
    #[error(transparent)]
    Channel(#[from] ChannelError),
    /// Error when calling [Socket::disconnect].
    #[error(transparent)]
    Disconnect(#[from] DisconnectError),
    /// Error when calling [Socket::shutdown].
    #[error(transparent)]
    Shutdown(#[from] ShutdownError),
}

const PHOENIX_SERIALIZER_VSN: &'static str = "2.0.0";

/// A [`Socket`] manages the underlying WebSocket connection used to talk to Phoenix.
///
/// It acts as the primary interface (along with [`Channel`]) for working with Phoenix Channels.
///
/// When a client is created, it is disconnected, and must be explicitly connected via [`Self::connect`].
/// Once connected, a worker task is spawned that acts as the broker for messages being sent or
/// received over the socket.
///
/// Once connected, the more useful [`Channel`] instance can be obtained via [`Self::channel`]. Most functionality
/// related to channels is exposed there.
pub struct Socket {
    url: Arc<Url>,
    status: ObservableStatus,
    state_command_tx: mpsc::Sender<StateCommand>,
    channel_spawn_tx: mpsc::Sender<ChannelSpawn>,
    channel_state_command_tx: mpsc::Sender<ChannelStateCommand>,
    channel_send_command_tx: mpsc::Sender<ChannelSendCommand>,
    /// The join handle corresponding to the socket listener
    /// * Some - spawned task has not been joined.
    /// * None - spawned task has been joined once.
    join_handle: AtomicTake<JoinHandle<Result<(), ShutdownError>>>,
}

impl Socket {
    /// Spawns a new [Socket] that must be [Socket::connect]ed.
    pub async fn spawn(mut url: Url) -> Result<Arc<Self>, SpawnError> {
        match url.scheme() {
            "wss" | "ws" => (),
            _ => return Err(SpawnError::UnsupportedScheme(url)),
        }

        // Modify url with given parameters
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("vsn", PHOENIX_SERIALIZER_VSN);
        }

        let url = Arc::new(url);
        let status = ObservableStatus::new(Status::default());
        let (channel_spawn_tx, channel_spawn_rx) = mpsc::channel(50);
        let (state_command_tx, state_command_rx) = mpsc::channel(50);
        let (channel_state_command_tx, channel_state_command_rx) = mpsc::channel(50);
        let (channel_send_command_tx, channel_send_command_rx) = mpsc::channel(50);
        let join_handle = Listener::spawn(
            url.clone(),
            status.clone(),
            channel_spawn_rx,
            state_command_rx,
            channel_state_command_rx,
            channel_send_command_rx,
        );

        Ok(Arc::new(Self {
            url,
            status,
            channel_spawn_tx,
            state_command_tx,
            channel_state_command_tx,
            channel_send_command_tx,
            join_handle: AtomicTake::new(join_handle),
        }))
    }

    /// The `url` passed to [Socket::spawn]
    pub fn url(&self) -> Arc<Url> {
        self.url.clone()
    }

    /// The current [Status].
    ///
    /// Use [Socket::status] to receive changes to the status.
    pub fn status(&self) -> Status {
        self.status.get()
    }

    /// Broadcasts [Socket::status] changes.
    ///
    /// Use [Socket::status] to see the current status.
    pub fn statuses(&self) -> broadcast::Receiver<Result<Status, Arc<tungstenite::Error>>> {
        self.status.subscribe()
    }

    /// Connects this client to the configured Phoenix Channels endpoint
    ///
    /// This function must be called before using the client to join channels, etc.
    ///
    /// A join handle to the socket worker is returned, we can use this to wait until the worker
    /// exits to ensure graceful termination. Otherwise, when the handle is dropped, it detaches the
    /// worker from the task runtime (though it will continue to run in the background)
    pub async fn connect(&self, timeout: Duration) -> Result<(), ConnectError> {
        let (connected_tx, connected_rx) = oneshot::channel();
        let created_at = Instant::now();
        let deadline = Instant::now() + timeout;

        self.state_command_tx
            .send(StateCommand::Connect(Connect {
                created_at,
                timeout,
                connected_tx,
            }))
            .await?;

        time::timeout_at(deadline, connected_rx).await??
    }

    /// Disconnect the client, regardless of any outstanding channel references
    ///
    /// Connected channels will return `ChannelError::Closed` when next used.
    ///
    /// New channels will need to be obtained from this client after `connect` is
    /// called again.
    pub async fn disconnect(&self) -> Result<(), DisconnectError> {
        let (disconnected_tx, disconnected_rx) = oneshot::channel();

        self.state_command_tx
            .send(StateCommand::Disconnect { disconnected_tx })
            .await?;

        disconnected_rx.await.map_err(From::from)
    }

    /// Propagates panic from async task.
    pub async fn shutdown(&self) -> Result<(), ShutdownError> {
        self.state_command_tx
            .send(StateCommand::Shutdown)
            .await
            .ok();

        self.listener_shutdown().await
    }

    /// Creates a new, unjoined Phoenix Channel
    pub async fn channel<T>(
        self: &Arc<Self>,
        topic: T,
        payload: Option<Payload>,
    ) -> Result<Arc<Channel>, ChannelError>
    where
        T: Into<SharedStr>,
    {
        let (sender, receiver) = oneshot::channel();

        match self
            .channel_spawn_tx
            .send(ChannelSpawn {
                socket: self.clone(),
                topic: topic.into(),
                payload,
                sender,
            })
            .await
        {
            Ok(()) => receiver.await.map(Arc::new).map_err(From::from),
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    pub(crate) async fn join(
        &self,
        topic: Topic,
        join_reference: JoinReference,
        payload: Payload,
        deadline: Instant,
    ) -> Result<oneshot::Receiver<Result<JoinedChannelReceivers, JoinError>>, JoinError> {
        let (joined_tx, joined_rx) = oneshot::channel();

        match self
            .channel_state_command_tx
            .send(ChannelStateCommand::Join(Join {
                topic,
                join_reference,
                payload,
                deadline,
                joined_tx,
            }))
            .await
        {
            Ok(()) => Ok(joined_rx),
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    pub(crate) async fn leave(
        &self,
        topic: Topic,
        join_reference: JoinReference,
    ) -> Result<oneshot::Receiver<Result<(), LeaveError>>, LeaveError> {
        let (left_tx, left_rx) = oneshot::channel();

        match self
            .channel_state_command_tx
            .send(ChannelStateCommand::Leave(Leave {
                topic,
                join_reference,
                left_tx,
            }))
            .await
        {
            Ok(()) => Ok(left_rx),
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    pub(crate) async fn cast(
        &self,
        topic: Topic,
        join_reference: JoinReference,
        event_payload: EventPayload,
    ) -> Result<(), CastError> {
        match self
            .channel_send_command_tx
            .send(ChannelSendCommand::Cast(listener::Cast {
                topic,
                join_reference,
                event_payload,
            }))
            .await
        {
            Ok(()) => Ok(()),
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    pub(crate) async fn call(
        &self,
        topic: Topic,
        join_reference: JoinReference,
        channel_call: channel::Call,
    ) -> Result<(), CallError> {
        match self
            .channel_send_command_tx
            .send(ChannelSendCommand::Call(listener::Call {
                topic,
                join_reference,
                channel_call,
            }))
            .await
        {
            Ok(()) => Ok(()),
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    /// Propagates panic from [Listener::listen]
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

/// Represents errors that occur from [`Socket::spawn`]
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    /// Occurs when the configured url's scheme is not ws or wss.
    #[error("Unsupported scheme in url ({0}). Supported schemes are ws and wss.")]
    UnsupportedScheme(Url),
}

/// Errors from [Socket::connect].
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// Server did not respond before timeout passed to [Socket::connect] expired.
    #[error("timeout connecting to server")]
    Timeout,
    /// A [tokio_tungstenite::WebSocketStream] from the underlying
    /// [tungstenite::protocol::WebSocket].
    #[error("websocket error: {0}")]
    WebSocketError(#[from] Arc<tungstenite::Error>),
    /// [Socket] shutting down because [Socket::shutdown] was called.
    #[error("socket shutting down")]
    SocketShuttingDown,
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("socket already shutdown")]
    SocketShutdown,
    /// The [Socket] is currently waiting until [Instant] to reconnect to not overload the server,
    /// so can't honor the explicit [Socket::connect].
    #[error("waiting to reconnect")]
    WaitingToReconnect(Instant),
}
impl From<Elapsed> for ConnectError {
    fn from(_: Elapsed) -> Self {
        ConnectError::Timeout
    }
}
impl From<mpsc::error::SendError<StateCommand>> for ConnectError {
    fn from(_: mpsc::error::SendError<StateCommand>) -> Self {
        ConnectError::SocketShutdown
    }
}
impl From<oneshot::error::RecvError> for ConnectError {
    fn from(_: oneshot::error::RecvError) -> Self {
        ConnectError::SocketShutdown
    }
}

/// Errors when calling [Socket::channel]
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("socket already shutdown")]
    Shutdown,
}
impl From<oneshot::error::RecvError> for ChannelError {
    fn from(_: oneshot::error::RecvError) -> Self {
        ChannelError::Shutdown
    }
}
impl From<ShutdownError> for ChannelError {
    fn from(_: ShutdownError) -> Self {
        ChannelError::Shutdown
    }
}

/// Errors from the [Socket] when calling [Channel::join].
#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    /// The [Channel::payload] was rejected when attempting to [Channel::join] or automatically
    /// rejoin [Channel::topic].
    #[error("server rejected join")]
    Rejected(Payload),
    /// [Socket::disconnect] was called after [Channel::join] was called while waiting for a
    /// response from the server.
    #[error("socket was disconnect while channel was being joined")]
    Disconnected,
    /// Timeout joining channel
    #[error("timeout joining channel")]
    Timeout,
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("socket already shutdown")]
    Shutdown,
}
impl From<ShutdownError> for JoinError {
    fn from(_: ShutdownError) -> Self {
        JoinError::Shutdown
    }
}

/// Errors that occur when [Socket] attempts to cast to server on behalf of [Channel::cast].
#[derive(Debug, thiserror::Error)]
pub enum CastError {
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("socket already shutdown")]
    Shutdown,
}
impl From<ShutdownError> for CastError {
    fn from(_: ShutdownError) -> Self {
        CastError::Shutdown
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum CallError {
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("socket already shutdown")]
    Shutdown,
}
impl From<ShutdownError> for CallError {
    fn from(_: ShutdownError) -> Self {
        CallError::Shutdown
    }
}

/// Error when calling [Socket::disconnect]
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DisconnectError {
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("socket already shutdown")]
    SocketShutdown,
}
impl From<oneshot::error::RecvError> for DisconnectError {
    fn from(_: oneshot::error::RecvError) -> Self {
        DisconnectError::SocketShutdown
    }
}
impl From<mpsc::error::SendError<StateCommand>> for DisconnectError {
    fn from(_: mpsc::error::SendError<StateCommand>) -> Self {
        DisconnectError::SocketShutdown
    }
}
