pub(crate) mod listener;

use std::panic;
use std::sync::Arc;
use std::time::Duration;

use atomic_take::AtomicTake;
use flexstr::SharedStr;
use log::error;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
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
    ShutdownError, StateCommand,
};
use crate::topic::Topic;
use crate::{channel, Channel, EventPayload};

const PHOENIX_SERIALIZER_VSN: &'static str = "2.0.0";

/// Represents errors that occur when interacting with a [`Socket`]
#[derive(Debug, thiserror::Error)]
pub enum SocketError {
    /// Occurs when the configured url is invalid for some reason
    #[error("invalid url: {0}")]
    InvalidUrl(Url),
    /// Occurs when attempting to connect a client that is already connected
    #[error("already connected")]
    AlreadyConnected,
    /// Occurs when attempting an operation on a disconnected client
    #[error("not connected")]
    NotConnected,
    #[error("not joined to topic {0}")]
    ChannelNotJoined(Arc<String>),
    /// Occurs when the client fails due to a low-level connection error on the socket
    #[error("connection error: {0}")]
    ConnectionError(#[from] tungstenite::Error),
}

/// A [`Socket`] manages the underlying WebSocket connection used to talk to Phoenix.
///
/// It acts as the primary interface (along with [`Channel`]) for working with Phoenix Channels.
///
/// When a client is created, it is disconnected, and must be explicitly connected via [`Self::connect`].
/// Once connected, a worker task is spawned that acts as the broker for messages being sent or
/// received over the socket.
///
/// Once connected, the more useful [`Channel`] instance can be obtained via [`Self::join`]. Most functionality
/// related to channels is exposed there.
pub struct Socket {
    url: Arc<Url>,
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
    /// Spawns a new [Socket] that must be [connect]ed.
    pub async fn spawn(mut url: Url) -> Result<Arc<Self>, SocketError> {
        match url.scheme() {
            "wss" | "ws" => (),
            _ => return Err(SocketError::InvalidUrl(url)),
        }

        // Modify url with given parameters
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("vsn", PHOENIX_SERIALIZER_VSN);
        }

        let url = Arc::new(url);
        let (channel_spawn_tx, channel_spawn_rx) = mpsc::channel(50);
        let (state_command_tx, state_command_rx) = mpsc::channel(50);
        let (channel_state_command_tx, channel_state_command_rx) = mpsc::channel(50);
        let (channel_send_command_tx, channel_send_command_rx) = mpsc::channel(50);
        let join_handle = Listener::spawn(
            url.clone(),
            channel_spawn_rx,
            state_command_rx,
            channel_state_command_rx,
            channel_send_command_rx,
        );

        Ok(Arc::new(Self {
            url,
            channel_spawn_tx,
            state_command_tx,
            channel_state_command_tx,
            channel_send_command_tx,
            join_handle: AtomicTake::new(join_handle),
        }))
    }

    pub fn url(&self) -> Arc<Url> {
        self.url.clone()
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

    /// Propagates panic from [Listener::listen]
    pub async fn shutdown(self) -> Result<(), ShutdownError> {
        self.state_command_tx
            .send(StateCommand::Shutdown)
            .await
            // if already shut down that's OK
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

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("timeout connecting to server")]
    Timeout,
    #[error("websocket error: {0}")]
    WebSocketError(#[from] tungstenite::Error),
    #[error("socket shutting down")]
    SocketShuttingDown,
    #[error("socket already shutdown")]
    SocketShutdown,
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

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
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

#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    #[error("server rejected join")]
    Rejected(Payload),
    #[error("socket was disconnect while channel was being joined")]
    Disconnected,
    #[error("timeout joining channel")]
    Timeout,
    #[error("socket already shutdown")]
    Shutdown,
}
impl From<ShutdownError> for JoinError {
    fn from(_: ShutdownError) -> Self {
        JoinError::Shutdown
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CastError {
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
    #[error("socket already shutdown")]
    Shutdown,
}
impl From<ShutdownError> for CallError {
    fn from(_: ShutdownError) -> Self {
        CallError::Shutdown
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DisconnectError {
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
