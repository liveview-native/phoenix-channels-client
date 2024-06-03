//! A [Socket] connects to the server through a web socket.  [Socket]s need to be
//! [connected](Socket::connect) when the [Url] params are sent to the server to authorize or
//! customize for this specific [Socket].
//!
//! A [Socket] needs to be created with [Socket::spawn].
//!
//! ```rust,no_run
//! # use std::time::Duration;
//! #
//! # use serde_json::json;
//! # use url::Url;
//! #
//! # use phoenix_channels_client::{PhoenixError, Socket};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), PhoenixError> {
//! // URL with params for authentication
//! let url = Url::parse_with_params(
//!     "ws://127.0.0.1:9002/socket/websocket",
//!     &[("shared_secret", "supersecret"), ("id", "user-id")],
//! )?;
//!
//! // Create a socket
//! let socket = Socket::spawn(url, None);
//! # Ok(())
//! # }
//! ```
//!
//! If the [Socket::spawn] [Url] does not have the correct params for authorization, then it will
//! pass back the error from [Socket::connect].
//!
//! ```rust,no_run
//! # use std::time::Duration;
//! #
//! # use serde_json::json;
//!#  use tokio_tungstenite::tungstenite;
//! # use url::Url;
//! #
//! # use phoenix_channels_client::{ConnectError, PhoenixError, Socket, WebSocketError};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), PhoenixError> {
//! // URL with params for authentication
//! let url = Url::parse_with_params(
//!     "ws://127.0.0.1:9002/socket/websocket",
//!     // WITHOUT shared secret
//!     &[("id", "user-id")],
//! )?;
//!
//! // Create a socket
//! let socket = Socket::spawn(url, None)?;
//!
//! // Connecting the socket returns the authorization error
//! match socket.connect(Duration::from_secs(5)).await {
//!     Err(ConnectError::WebSocket { web_socket_error }) => match web_socket_error {
//!        WebSocketError::Http { response } => println!("Got status {} from server", response.status_code),
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
//!```rust,no_run
//! # use std::sync::Arc;
//! # use std::time::{Duration, SystemTime};
//! #
//! # use serde_json::{json, Value};
//! # use tokio::time::Instant;
//! # use tokio_tungstenite::tungstenite;
//! # use url::Url;
//! # use uuid::Uuid;
//! #
//! # use phoenix_channels_client::{
//! #   CallError, PhoenixError, Event, JSON, Payload, Socket, SocketStatus, Topic, WebSocketError
//! # };
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), PhoenixError> {
//! let id = id();
//! let secret = generate_secret(&id).await;
//! let secret_url = Url::parse_with_params(
//!     "ws://127.0.0.1:9002/socket/websocket",
//!     &[("secret", secret.clone()), ("id", id.clone())],
//! )?;
//! let secret_socket = Socket::spawn(secret_url, None).unwrap();
//! secret_socket.connect(Duration::from_secs(10)).await?;
//! let mut statuses = secret_socket.statuses();
//!
//! // Deauthorize the socket
//! delete_secret(&id, &secret).await;
//!
//! let until = match statuses.status().await? {
//!     Ok(SocketStatus::WaitingToReconnect { until }) => until,
//!     other => panic!("Didn't wait to reconnect and instead {:?}", other)
//! };
//! println!("Will reconnect in {:?}", until.duration_since(SystemTime::now()).unwrap_or_else(|_| Duration::from_micros(0)));
//! match statuses.status().await? {
//!     Err(web_socket_error) => match web_socket_error {
//!        WebSocketError::Http { response } => println!("Got status {} from server", response.status_code),
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
//! #     let socket = Socket::spawn(url, None).unwrap();
//! #     socket.connect(Duration::from_secs(10)).await.unwrap();
//! #
//! #     let channel = socket.channel(
//! #         Topic::from_string("channel:generate_secret".to_string()),
//! #         None
//! #     ).await.unwrap();
//! #     channel.join(Duration::from_secs(10)).await.unwrap();
//! #
//! #     let Payload::JSONPayload { json } = channel
//! #         .call(
//! #             Event::from_string("generate_secret".to_string()),
//! #             Payload::json_from_serialized(json!({}).to_string()).unwrap(),
//! #             Duration::from_secs(10)
//! #         )
//! #         .await
//! #         .unwrap()
//! #     else {
//! #         panic!("secret not returned")
//! #     };
//! #
//! #     let secret = if let JSON::Str { string } = json {
//! #         string
//! #     } else {
//! #         panic!("secret ({:?}) is not a string", json);
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
//! #     let socket = Socket::spawn(url, None).unwrap();
//! #     socket.connect(Duration::from_secs(10)).await.unwrap();
//! #
//! #     let channel = socket.channel(
//! #         Topic::from_string("channel:secret".to_string()),
//! #         None
//! #     ).await.unwrap();
//! #     channel.join(Duration::from_secs(10)).await.unwrap();
//! #
//! #     match channel.call(
//! #               Event::from_string("delete_secret".to_string()),
//! #               Payload::json_from_serialized(json!({}).to_string()).unwrap(),
//! #               Duration::from_secs(10)
//!             ).await {
//! #         Ok(payload) => panic!("Deleting secret succeeded without disconnecting socket and returned payload: {:?}", payload),
//! #         Err(CallError::SocketDisconnected) => (),
//! #         Err(other) => panic!("Error other than SocketDisconnected: {:?}", other)
//! #     }
//! # }

use atomic_take::AtomicTake;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::error::Elapsed;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite;
use url::Url;

use crate::ffi::channel::Channel;
use crate::ffi::message::Payload;
use crate::ffi::observable_status::StatusesError;
use crate::ffi::topic::Topic;
use crate::ffi::{http, instant_to_system_time, web_socket};
use crate::rust;
use crate::rust::observable_status;

use crate::rust::socket::listener::{
    ChannelSendCommand, ChannelSpawn, ChannelStateCommand, Connect, Listener, ObservableStatus,
    StateCommand,
};

/// Errors when calling [Socket] functions.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SocketError {
    /// Error when calling [Socket::spawn].
    #[error(transparent)]
    Spawn {
        #[from]
        /// Represents errors that occur from [`Socket::spawn`]
        spawn_error: SpawnError,
    },
    /// Error when calling [Socket::connect].
    #[error(transparent)]
    Connect {
        #[from]
        /// Errors from [Socket::connect].
        connect_error: ConnectError,
    },
    /// Error when calling [Socket::channel].
    #[error(transparent)]
    Channel {
        #[from]
        /// Errors when calling [Socket::channel]
        channel_error: SocketChannelError,
    },
    /// Error when calling [Socket::disconnect].
    #[error(transparent)]
    Disconnect {
        #[from]
        /// Error when calling [Socket::disconnect]
        disconnect_error: DisconnectError,
    },
    /// Error when calling [Socket::shutdown].
    #[error(transparent)]
    Shutdown {
        #[from]
        /// Error from [Socket::shutdown] or from the server itself that caused the [Socket] to shutdown.
        shutdown_error: SocketShutdownError,
    },
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
#[derive(uniffi::Object)]
pub struct Socket {
    url: Arc<Url>,
    status: ObservableStatus,
    state_command_tx: mpsc::Sender<StateCommand>,
    channel_spawn_tx: mpsc::Sender<ChannelSpawn>,
    pub(crate) channel_state_command_tx: mpsc::Sender<ChannelStateCommand>,
    pub(crate) channel_send_command_tx: mpsc::Sender<ChannelSendCommand>,
    /// The join handle corresponding to the socket listener
    /// * Some - spawned task has not been joined.
    /// * None - spawned task has been joined once.
    pub(crate) join_handle: AtomicTake<JoinHandle<Result<(), rust::socket::ShutdownError>>>,
}
#[uniffi::export(async_runtime = "tokio")]
impl Socket {
    /// Spawns a new [Socket] that must be [Socket::connect]ed.
    #[uniffi::constructor]
    pub fn spawn(mut url: Url, cookies: Option<Vec<String>>) -> Result<Arc<Self>, SpawnError> {
        match url.scheme() {
            "wss" | "ws" => (),
            _ => return Err(SpawnError::UnsupportedScheme { url }),
        }

        // Modify url with given parameters
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("vsn", PHOENIX_SERIALIZER_VSN);
        }

        let url = Arc::new(url);
        let status = ObservableStatus::new(rust::socket::Status::default());
        let (channel_spawn_tx, channel_spawn_rx) = mpsc::channel(50);
        let (state_command_tx, state_command_rx) = mpsc::channel(50);
        let (channel_state_command_tx, channel_state_command_rx) = mpsc::channel(50);
        let (channel_send_command_tx, channel_send_command_rx) = mpsc::channel(50);
        let join_handle = Listener::spawn(
            url.clone(),
            cookies.clone(),
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
    pub fn url(&self) -> Url {
        (*self.url).clone()
    }

    /// The current [SocketStatus].
    ///
    /// Use [Socket::status] to receive changes to the status.
    pub fn status(&self) -> SocketStatus {
        self.status.get().into()
    }

    /// Broadcasts [Socket::status] changes.
    ///
    /// Use [Socket::status] to see the current status.
    pub fn statuses(&self) -> Arc<SocketStatuses> {
        Arc::new(self.status.subscribe().into())
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

        match self
            .state_command_tx
            .send(StateCommand::Connect(Connect {
                created_at,
                timeout,
                connected_tx,
            }))
            .await
        {
            Ok(()) => match time::timeout_at(deadline, connected_rx).await? {
                Ok(result) => result.map_err(From::from),
                Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
            },
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    /// Disconnect the client, regardless of any outstanding channel references
    ///
    /// Connected channels will return `ChannelError::Closed` when next used.
    ///
    /// New channels will need to be obtained from this client after `connect` is
    /// called again.
    pub async fn disconnect(&self) -> Result<(), DisconnectError> {
        let (disconnected_tx, disconnected_rx) = oneshot::channel();

        match self
            .state_command_tx
            .send(StateCommand::Disconnect { disconnected_tx })
            .await
        {
            Ok(()) => match disconnected_rx.await {
                Ok(()) => Ok(()),
                Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
            },
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }

    /// Propagates panic from async task.
    pub async fn shutdown(&self) -> Result<(), SocketShutdownError> {
        self.state_command_tx
            .send(StateCommand::Shutdown)
            .await
            .ok();

        self.listener_shutdown().await.map_err(From::from)
    }

    /// Creates a new, unjoined Phoenix Channel
    pub async fn channel(
        self: &Arc<Self>,
        topic: Arc<Topic>,
        payload: Option<Payload>,
    ) -> Result<Arc<Channel>, SocketChannelError> {
        let (sender, receiver) = oneshot::channel();

        match self
            .channel_spawn_tx
            .send(ChannelSpawn {
                socket: self.clone(),
                topic,
                payload: payload.map(From::from),
                sender,
            })
            .await
        {
            Ok(()) => match receiver.await {
                Ok(channel) => Ok(Arc::new(channel)),
                Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
            },
            Err(_) => Err(self.listener_shutdown().await.unwrap_err().into()),
        }
    }
}

/// The status of the [Socket].
#[derive(Copy, Clone, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SocketStatus {
    /// [Socket::connect] has never been called.
    NeverConnected,
    /// [Socket::connect] was called and server responded the socket is connected.
    Connected,
    /// [Socket::connect] was called previously, but the [Socket] was disconnected by the server and
    /// [Socket] needs to wait to reconnect.
    WaitingToReconnect {
        /// When the [Socket] will automatically [Socket::connect] next.
        until: SystemTime,
    },
    /// [Socket::disconnect] was called and the server responded that the socket as disconnected.
    Disconnected,
    /// [Socket::shutdown] was called, but the async task hasn't exited yet.
    ShuttingDown,
    /// The async task has exited.
    ShutDown,
}
impl From<rust::socket::Status> for SocketStatus {
    fn from(rust_status: rust::socket::Status) -> Self {
        match rust_status {
            rust::socket::Status::NeverConnected => Self::NeverConnected,
            rust::socket::Status::Connected => Self::Connected,
            rust::socket::Status::WaitingToReconnect(until) => Self::WaitingToReconnect {
                until: instant_to_system_time(until),
            },
            rust::socket::Status::Disconnected => Self::Disconnected,
            rust::socket::Status::ShuttingDown => Self::ShuttingDown,
            rust::socket::Status::ShutDown => Self::ShutDown,
        }
    }
}

/// A wrapper anound `observable_status::Statuses` because `uniffi` does not support generics
#[derive(uniffi::Object)]
pub struct SocketStatuses(
    observable_status::Statuses<rust::socket::Status, Arc<tungstenite::Error>>,
);

impl SocketStatuses {
    /// Retrieve the current status.
    pub async fn status(
        &self,
    ) -> Result<Result<SocketStatus, web_socket::error::WebSocketError>, StatusesError> {
        self.0.status().await.map(|result| {
            result
                .map(From::from)
                .map_err(|error| error.as_ref().into())
        })
    }
}
impl From<observable_status::Statuses<rust::socket::Status, Arc<tungstenite::Error>>>
    for SocketStatuses
{
    fn from(
        inner: observable_status::Statuses<rust::socket::Status, Arc<tungstenite::Error>>,
    ) -> Self {
        Self(inner)
    }
}

/// Represents errors that occur from [`Socket::spawn`]
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum SpawnError {
    /// Occurs when the configured url's scheme is not ws or wss.
    #[error("Unsupported scheme in url ({url}). Supported schemes are ws and wss.")]
    UnsupportedScheme {
        /// The url for this unsupported scheme.
        url: Url,
    },
}

/// Errors from [Socket::connect].
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum ConnectError {
    /// Server did not respond before timeout passed to [Socket::connect] expired.
    #[error("timeout connecting to server")]
    Timeout,
    /// Error from the underlying
    /// [tokio_tungstenite::tungstenite::protocol::WebSocket].
    #[error("websocket error: {web_socket_error}")]
    WebSocket {
        /// Error from the underlying
        /// [tokio_tungstenite::tungstenite::protocol::WebSocket].
        web_socket_error: web_socket::error::WebSocketError,
    },
    /// [Socket] shutting down because [Socket::shutdown] was called.
    #[error("socket shutting down")]
    ShuttingDown,
    /// Error from [Socket::shutdown] or from the server itself that caused the [Socket] to shutdown.
    #[error("socket shutdown: {shutdown_error}")]
    Shutdown {
        /// Error from [Socket::shutdown] or from the server itself that caused the [Socket] to
        /// shutdown.
        shutdown_error: SocketShutdownError,
    },
    /// The [Socket] is currently waiting until [Instant] to reconnect to not overload the server,
    /// so can't honor the explicit [Socket::connect].
    #[error("waiting to reconnect")]
    WaitingToReconnect {
        /// When the [Socket] will automatically [Socket::connect] next.
        until: SystemTime,
    },
    #[cfg(feature = "native-tls")]
    /// These are TLS errors.
    #[error("tls error: {tls_error}")]
    Tls {
        /// This is the error from [native_tls::Error]
        tls_error: native_tls::Error,
    },
}
impl From<Elapsed> for ConnectError {
    fn from(_: Elapsed) -> Self {
        Self::Timeout
    }
}
impl From<rust::socket::ConnectError> for ConnectError {
    fn from(rust_connect_error: rust::socket::ConnectError) -> Self {
        match rust_connect_error {
            rust::socket::ConnectError::Timeout => Self::Timeout,
            rust::socket::ConnectError::WebSocket(web_socket_error) => Self::WebSocket {
                web_socket_error: web_socket_error.as_ref().into(),
            },
            rust::socket::ConnectError::ShuttingDown => Self::ShuttingDown,
            rust::socket::ConnectError::Shutdown { shutdown_error } => Self::Shutdown {
                shutdown_error: shutdown_error.into(),
            },
            rust::socket::ConnectError::WaitingToReconnect(until) => Self::WaitingToReconnect {
                until: instant_to_system_time(until),
            },
            #[cfg(feature = "native-tls")]
            rust::socket::ConnectError::Tls(tls_error) => Self::Tls { tls_error },
        }
    }
}
impl From<rust::socket::ShutdownError> for ConnectError {
    fn from(rust_shutdown_error: rust::socket::ShutdownError) -> Self {
        Self::Shutdown {
            shutdown_error: rust_shutdown_error.into(),
        }
    }
}

/// Errors when calling [Socket::channel]
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SocketChannelError {
    #[error("socket shutdown: {shutdown_error}")]
    /// The shutdown error for this SocketChannelError
    Shutdown {
        /// The shutdown error for this shutdown
        shutdown_error: SocketShutdownError,
    },
}
impl From<rust::socket::ShutdownError> for SocketChannelError {
    fn from(rust_shutdown_error: rust::socket::ShutdownError) -> Self {
        Self::Shutdown {
            shutdown_error: rust_shutdown_error.into(),
        }
    }
}

/// Error when calling [Socket::disconnect]
#[derive(Debug, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum DisconnectError {
    #[error("socket shutdown: {shutdown_error}")]
    Shutdown { shutdown_error: SocketShutdownError },
}
impl From<rust::socket::ShutdownError> for DisconnectError {
    fn from(rust_shutdown_error: rust::socket::ShutdownError) -> Self {
        Self::Shutdown {
            shutdown_error: rust_shutdown_error.into(),
        }
    }
}

/// Error from [Socket::shutdown] or from the server itself that caused the [Socket] to shutdown.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum SocketShutdownError {
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("listener task was already joined once from another caller")]
    AlreadyJoined,
    /// [CVE-2023-43669](https://nvd.nist.gov/vuln/detail/CVE-2023-43669) attack attempt
    #[error("CVE-2023-43669 attack attempt due to excessive headers from server")]
    AttackAttempt,
    /// Invalid URL.
    #[error("URL error: {url_error}")]
    Url { url_error: String },
    /// HTTP error.
    #[error("HTTP error: {}", response.status_code)]
    Http { response: http::Response },
    /// HTTP format error.
    #[error("HTTP format error: {error}")]
    HttpFormat { error: http::HttpError },
}
impl From<rust::socket::ShutdownError> for SocketShutdownError {
    fn from(rust_socket_error: rust::socket::ShutdownError) -> Self {
        match rust_socket_error {
            rust::socket::ShutdownError::AlreadyJoined => Self::AlreadyJoined,
            rust::socket::ShutdownError::AttackAttempt => Self::AttackAttempt,
            rust::socket::ShutdownError::Url(url_error) => Self::Url {
                url_error: url_error.to_string(),
            },
            rust::socket::ShutdownError::Http(response) => Self::Http {
                response: response.into(),
            },
            rust::socket::ShutdownError::HttpFormat(error) => Self::HttpFormat {
                error: error.into(),
            },
        }
    }
}
impl From<&rust::socket::ShutdownError> for SocketShutdownError {
    fn from(rust_socket_error: &rust::socket::ShutdownError) -> Self {
        match rust_socket_error {
            rust::socket::ShutdownError::AlreadyJoined => Self::AlreadyJoined,
            rust::socket::ShutdownError::AttackAttempt => Self::AttackAttempt,
            rust::socket::ShutdownError::Url(url_error) => Self::Url {
                url_error: url_error.to_string(),
            },
            rust::socket::ShutdownError::Http(response) => Self::Http {
                response: response.into(),
            },
            rust::socket::ShutdownError::HttpFormat(error) => Self::HttpFormat {
                error: error.into(),
            },
        }
    }
}
