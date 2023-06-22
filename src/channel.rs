pub(crate) mod listener;

use std::panic;
use std::sync::atomic::{AtomicU32, Ordering};
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

pub(crate) use crate::channel::listener::Call;
use crate::channel::listener::{LeaveError, Listener, SendCommand, ShutdownError, StateCommand};
use crate::message::*;
use crate::socket;
use crate::socket::listener::Connectivity;
use crate::socket::Socket;
use crate::topic::Topic;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Join(#[from] JoinError),
    #[error(transparent)]
    Cast(#[from] CastError),
    #[error(transparent)]
    Call(#[from] CallError),
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

#[derive(Clone, Debug)]
pub struct EventPayload {
    pub event: Event,
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
/// * [Channel::send]/[Channel::cast], to send a message and await a reply from the server
/// * [Channel::cast], to send a message and ignore any replies
///
pub struct Channel {
    topic: Topic,
    payload: Payload,
    /// The channel status
    status: Arc<AtomicU32>,
    event_payload_tx: broadcast::Sender<EventPayload>,
    shutdown_tx: oneshot::Sender<()>,
    state_command_tx: mpsc::Sender<StateCommand>,
    send_command_tx: mpsc::Sender<SendCommand>,
    /// The join handle corresponding to the channel listener
    join_handle: JoinHandle<Result<(), ShutdownError>>,
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
        let status = Arc::new(AtomicU32::new(state.status() as u32));
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
            shutdown_tx,
            state_command_tx,
            send_command_tx,
            join_handle,
        }
    }

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

    /// Propagates panic from [Listener::listen]
    pub async fn shutdown(self) -> Result<(), ShutdownError> {
        self.shutdown_tx
            .send(())
            // if already shut down that's OK
            .ok();

        match self.join_handle.await {
            Ok(result) => result,
            Err(join_error) => panic::resume_unwind(join_error.into_panic()),
        }
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
    #[error("socket was disconnect while channel was being joined")]
    SocketDisconnected,
    #[error("leaving channel while still waiting to see if join succeeded")]
    LeavingWhileJoining,
    #[error("waiting to rejoin")]
    WaitingToRejoin(Instant),
    #[error("server rejected join")]
    Rejected(Payload),
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
