pub(crate) mod listener;

use std::panic;
use std::sync::Arc;

use log::error;
use tokio::sync::oneshot;
use tokio::time::error::Elapsed;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::error::UrlError;
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::http::Response;

use crate::ffi::socket::Socket;
use crate::ffi::topic::Topic;
use crate::rust::channel;
use crate::rust::channel::listener::{JoinedChannelReceivers, LeaveError};
use crate::rust::join_reference::JoinReference;
use crate::rust::message::*;
pub use crate::rust::socket::listener::Status;
use crate::rust::socket::listener::{ChannelSendCommand, ChannelStateCommand, Join, Leave};

// non-uniffi::export functions
impl Socket {
    pub(crate) async fn join(
        &self,
        topic: Arc<Topic>,
        join_reference: JoinReference,
        payload: Payload,
        deadline: Instant,
    ) -> Result<oneshot::Receiver<Result<JoinedChannelReceivers, JoinError>>, ShutdownError> {
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
            Err(_) => Err(self.listener_shutdown().await.unwrap_err()),
        }
    }

    pub(crate) async fn leave(
        &self,
        topic: Arc<Topic>,
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
        topic: Arc<Topic>,
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
        topic: Arc<Topic>,
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
    pub(crate) async fn listener_shutdown(&self) -> Result<(), ShutdownError> {
        match self.join_handle.take() {
            Some(join_handle) => match join_handle.await {
                Ok(result) => result,
                Err(join_error) => panic::resume_unwind(join_error.into_panic()),
            },
            None => Err(ShutdownError::AlreadyJoined),
        }
    }
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
    ShuttingDown,
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("socket already shutdown")]
    Shutdown { shutdown_error: ShutdownError },
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
    /// The socket shutdown
    #[error("socket shutdown: {0}")]
    Shutdown(ShutdownError),
}
impl From<ShutdownError> for JoinError {
    fn from(shutdown_error: ShutdownError) -> Self {
        Self::Shutdown(shutdown_error)
    }
}

/// Errors that occur when [Socket] attempts to call to server on behalf of [Channel::call].
#[derive(Debug, thiserror::Error)]
pub enum CallError {
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here
    #[error("socket already shutdown {0}")]
    Shutdown(ShutdownError),
}
impl From<ShutdownError> for CallError {
    fn from(shutdown_error: ShutdownError) -> Self {
        Self::Shutdown(shutdown_error)
    }
}

/// Errors that occur when [Socket] attempts to cast to server on behalf of [Channel::cast].
#[derive(Debug, thiserror::Error)]
pub enum CastError {
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("socket already shutdown {0}")]
    Shutdown(ShutdownError),
}
impl From<ShutdownError> for CastError {
    fn from(shutdown_error: ShutdownError) -> Self {
        Self::Shutdown(shutdown_error)
    }
}

/// Error from [Socket::shutdown] or from the server itself that caused the [Socket] to shutdown.
#[derive(Debug, thiserror::Error)]
pub enum ShutdownError {
    /// The async task was already joined by another call, so the [Result] or panic from the async
    /// task can't be reported here.
    #[error("listener task was already joined once from another caller")]
    AlreadyJoined,
    /// [tungstenite::error::UrlError] with the `url` passed to [Socket::spawn].  This can include
    /// incorrect scheme ([tungstenite::error::UrlError::UnsupportedUrlScheme]).
    #[error("URL error: {0}")]
    Url(#[from] UrlError),
    /// HTTP error response from server.
    #[error("HTTP error: {}", .0.status())]
    Http(Response<Option<String>>),
    /// HTTP format error.
    #[error("HTTP format error: {0}")]
    HttpFormat(#[from] http::Error),
}
