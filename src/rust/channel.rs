pub(crate) mod listener;

use atomic_take::AtomicTake;
use std::sync::Arc;

use log::error;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::error::Elapsed;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::error::UrlError;
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::http::Response;

use crate::ffi::channel::Channel;
use crate::ffi::socket::Socket;
use crate::ffi::topic::Topic;
pub(crate) use crate::rust::channel::listener::{Call, LeaveError, Status};
use crate::rust::channel::listener::{Listener, ObservableStatus, SendCommand};
use crate::rust::message::Payload;
use crate::rust::socket;
use crate::rust::socket::listener::Connectivity;
use crate::rust::socket::ShutdownError;

// non-uniffi::export
impl Channel {
    /// Spawns a new [Channel] that must be [join]ed.  The `topic` and `payload` is sent on the
    /// first [join] and any rejoins if the underlying `socket` is disconnected and
    /// reconnects.
    pub(crate) async fn spawn(
        socket: Arc<Socket>,
        socket_connectivity_rx: broadcast::Receiver<Connectivity>,
        topic: Arc<Topic>,
        payload: Option<Payload>,
        state: listener::State,
    ) -> Self {
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
impl From<socket::ShutdownError> for CastError {
    fn from(shutdown_error: socket::ShutdownError) -> Self {
        match shutdown_error {
            socket::ShutdownError::AlreadyJoined => CastError::SocketShutdown,
            socket::ShutdownError::Url(url_error) => CastError::Url(url_error),
            socket::ShutdownError::Http(response) => CastError::Http(response),
            socket::ShutdownError::HttpFormat(error) => CastError::HttpFormat(error),
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
    /// The async task for the [Socket] was already joined by another call, so the [Result] or panic
    /// from the async task can't be reported here.
    #[error("socket already shutdown")]
    SocketShutdown(ShutdownError),
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
impl From<ShutdownError> for CallError {
    fn from(shutdown_error: ShutdownError) -> Self {
        Self::SocketShutdown(shutdown_error)
    }
}
