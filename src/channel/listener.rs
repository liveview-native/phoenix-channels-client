use std::fmt::{Debug, Formatter};
use std::mem;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use log::debug;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Instant, Sleep};
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::error::UrlError;
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::http::Response;

use crate::channel::{EventPayload, ObservableStatus, Status};
use crate::join_reference::JoinReference;
use crate::message::{Broadcast, Push};
use crate::socket::listener::{Connectivity, Disconnected};
use crate::topic::Topic;
use crate::{channel, socket, JoinError, Payload, Socket};

pub(super) struct Listener {
    socket: Arc<Socket>,
    socket_connectivity_rx: broadcast::Receiver<Connectivity>,
    topic: Topic,
    payload: Payload,
    channel_status: ObservableStatus,
    shutdown_rx: oneshot::Receiver<()>,
    event_payload_tx: broadcast::Sender<EventPayload>,
    state_command_rx: mpsc::Receiver<StateCommand>,
    state: Option<State>,
    send_command_rx: mpsc::Receiver<SendCommand>,
    join_reference: JoinReference,
}
impl Listener {
    pub(super) fn spawn(
        socket: Arc<Socket>,
        socket_connectivity_rx: broadcast::Receiver<Connectivity>,
        topic: Topic,
        payload: Payload,
        state: State,
        channel_status: ObservableStatus,
        shutdown_rx: oneshot::Receiver<()>,
        event_payload_tx: broadcast::Sender<EventPayload>,
        state_command_rx: mpsc::Receiver<StateCommand>,
        send_command_rx: mpsc::Receiver<SendCommand>,
    ) -> JoinHandle<Result<(), ShutdownError>> {
        let listener = Self::init(
            socket,
            socket_connectivity_rx,
            topic,
            payload,
            state,
            channel_status,
            shutdown_rx,
            event_payload_tx,
            state_command_rx,
            send_command_rx,
        );

        tokio::spawn(listener.listen())
    }

    fn init(
        socket: Arc<Socket>,
        socket_connectivity_rx: broadcast::Receiver<Connectivity>,
        topic: Topic,
        payload: Payload,
        state: State,
        channel_status: ObservableStatus,
        shutdown_rx: oneshot::Receiver<()>,
        event_payload_tx: broadcast::Sender<EventPayload>,
        state_command_rx: mpsc::Receiver<StateCommand>,
        send_command_rx: mpsc::Receiver<SendCommand>,
    ) -> Self {
        Self {
            socket,
            socket_connectivity_rx,
            topic,
            payload,
            state: Some(state),
            channel_status,
            shutdown_rx,
            event_payload_tx,
            state_command_rx,
            send_command_rx,
            join_reference: JoinReference::new(),
        }
    }

    async fn listen(mut self) -> Result<(), ShutdownError> {
        debug!(
            "Channel listener started for topic {} will join as {}",
            &self.topic, &self.join_reference
        );

        let result = loop {
            let mut current_state = self.state.take().unwrap();
            let current_discriminant = mem::discriminant(&current_state);

            let next_state = match current_state {
                State::WaitingForSocketToConnect { .. } => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_rx => current_state.shutdown(),
                    Ok(socket_connectivity) = self.socket_connectivity_rx.recv() => current_state.connectivity_changed(socket_connectivity),
                    else => break Ok(())
                },
                State::WaitingToJoin => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_rx => current_state.shutdown(),
                    Ok(socket_connectivity) = self.socket_connectivity_rx.recv() => current_state.connectivity_changed(socket_connectivity),
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(current_state, state_command).await?,
                    else => break Ok(())
                },
                State::Joining(mut joining) => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_rx => State::Joining(joining).shutdown(),
                    socket_joined_result = &mut joining.socket_joined_rx => self.joined_result_result_received(joining, socket_joined_result).await?,
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(State::Joining(joining), state_command).await?,
                    else => break Ok(())
                },
                State::WaitingToRejoin {
                    ref mut sleep,
                    rejoin,
                } => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_rx => current_state.shutdown(),
                    Ok(socket_connectivity) = self.socket_connectivity_rx.recv() => current_state.connectivity_changed(socket_connectivity),
                    () = sleep => self.rejoin(current_state, rejoin).await?,
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(current_state, state_command).await?,
                    else => break Ok(())
                },
                State::Joined(mut joined) => tokio::select! {
                        // left_rx should be polled before others, so that that the socket
                        // disconnected or is reconnecting is seen before the broadcast_rx's
                        // sender being dropped and a `Err(broadcast::error::RecvError::Closed)`
                        // being received
                        biased;

                        _ = &mut self.shutdown_rx => State::Joined(joined).shutdown(),
                        Ok(socket_connectivity) = self.socket_connectivity_rx.recv() => State::Joined(joined).connectivity_changed(socket_connectivity),
                        Ok(()) = &mut joined.left_rx => State::Left,
                        Some(state_command) = self.state_command_rx.recv() => self.update_state(State::Joined(joined), state_command).await?,
                        Some(send_command) = self.send_command_rx.recv() => self.send(joined, send_command).await,
                        Some(push) = joined.push_rx.recv() => {
                            self.send_event_payload(push);

                            State::Joined(joined)
                        }
                        Ok(broadcast) = joined.broadcast_rx.recv() => {
                            self.send_event_payload(broadcast);

                            State::Joined(joined)
                        }
                        else => break Ok(())
                },
                State::Leaving(mut leaving) => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_rx => State::Leaving(leaving).shutdown(),
                    Ok(socket_connectivity) = self.socket_connectivity_rx.recv() => State::Leaving(leaving).connectivity_changed(socket_connectivity),
                    Ok(left_result) = &mut leaving.socket_left_rx => leaving.left(left_result),
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(State::Leaving(leaving), state_command).await?,
                    else => break Ok(())
                },
                State::Left => tokio::select! {
                    biased;

                    _ = &mut self.shutdown_rx => current_state.shutdown(),
                    Ok(socket_connectivity) = self.socket_connectivity_rx.recv() => current_state.connectivity_changed(socket_connectivity),
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(current_state, state_command).await?,
                    else => break Ok(())
                },
                State::ShuttingDown => break Ok(()),
            };

            self.channel_status.set(next_state.status());

            let next_discriminant = mem::discriminant(&next_state);

            if next_discriminant != current_discriminant {
                debug!("transitioned state to {:#?}", &next_state);
            }

            self.state = Some(next_state);
        };

        self.channel_status.set(Status::ShutDown);

        result
    }

    async fn update_state(
        &self,
        state: State,
        state_command: StateCommand,
    ) -> Result<State, ShutdownError> {
        match state_command {
            StateCommand::Join {
                created_at,
                timeout,
                joined_tx,
            } => self.join(state, created_at, timeout, joined_tx).await,
            StateCommand::Leave { left_tx } => Ok(self.leave(state, left_tx).await),
        }
    }

    async fn join(
        &self,
        mut state: State,
        created_at: Instant,
        timeout: Duration,
        channel_joined_tx: oneshot::Sender<Result<(), JoinError>>,
    ) -> Result<State, ShutdownError> {
        match state {
            State::WaitingForSocketToConnect { .. } => unreachable!(),
            State::WaitingToJoin { .. } | State::Leaving { .. } | State::Left { .. } => {
                self.socket_join_if_not_timed_out(state, created_at, timeout, channel_joined_tx)
                    .await
            }
            State::Joining(Joining {
                ref mut channel_joined_txs,
                ..
            }) => {
                channel_joined_txs.push(channel_joined_tx);

                Ok(state)
            }
            State::WaitingToRejoin { ref sleep, .. } => {
                channel_joined_tx
                    .send(Err(JoinError::WaitingToRejoin(sleep.deadline())))
                    .ok();

                Ok(state)
            }
            State::Joined { .. } => {
                channel_joined_tx.send(Ok(())).ok();

                Ok(state)
            }
            State::ShuttingDown => {
                channel_joined_tx.send(Err(JoinError::ShuttingDown)).ok();

                Ok(state)
            }
        }
    }

    async fn socket_join_if_not_timed_out(
        &self,
        state: State,
        created_at: Instant,
        timeout: Duration,
        channel_joined_tx: oneshot::Sender<Result<(), JoinError>>,
    ) -> Result<State, ShutdownError> {
        let deadline = created_at + timeout;
        let rejoin = Rejoin {
            join_timeout: timeout,
            attempts: 0,
        };

        if Instant::now() <= deadline {
            self.socket_join(state, created_at, rejoin, vec![channel_joined_tx])
                .await
        } else {
            debug!("joining {} timed out before sending to socket", self.topic);

            Ok(rejoin.wait())
        }
    }

    async fn joined_result_result_received(
        &self,
        joining: Joining,
        joined_result_result: Result<
            Result<JoinedChannelReceivers, socket::JoinError>,
            oneshot::error::RecvError,
        >,
    ) -> Result<State, ShutdownError> {
        match joined_result_result {
            Ok(joined_result) => Ok(self.joined_result_received(joining, joined_result)),
            Err(_) => Err(Self::socket_shutdown(State::Joining(joining))),
        }
    }

    fn joined_result_received(
        &self,
        joining: Joining,
        result: Result<JoinedChannelReceivers, socket::JoinError>,
    ) -> State {
        let Joining {
            channel_joined_txs,
            rejoin,
            ..
        } = joining;

        let (channel_joined_result, next_state) = match result {
            Ok(JoinedChannelReceivers {
                push: push_rx,
                broadcast: broadcast_rx,
                left: left_rx,
            }) => (
                Ok(()),
                State::Joined(Joined {
                    push_rx,
                    broadcast_rx,
                    left_rx,
                    join_timeout: rejoin.join_timeout,
                }),
            ),
            Err(socket_join_error) => match socket_join_error {
                socket::JoinError::Shutdown => (Err(JoinError::ShuttingDown), State::ShuttingDown),
                socket::JoinError::Rejected(payload) => {
                    (Err(JoinError::Rejected(payload)), State::Left)
                }
                socket::JoinError::Disconnected => {
                    (Err(JoinError::SocketDisconnected), rejoin.wait())
                }
                socket::JoinError::Timeout => (Err(JoinError::Timeout), rejoin.wait()),
            },
        };

        State::send_to_channel_txs(channel_joined_txs, channel_joined_result);

        next_state
    }

    async fn rejoin(&self, state: State, rejoin: Rejoin) -> Result<State, ShutdownError> {
        self.socket_join(state, Instant::now(), rejoin, vec![])
            .await
    }

    async fn socket_join(
        &self,
        mut state: State,
        created_at: Instant,
        rejoin: Rejoin,
        channel_joined_txs: Vec<oneshot::Sender<Result<(), JoinError>>>,
    ) -> Result<State, ShutdownError> {
        match self
            .socket
            .join(
                self.topic.clone(),
                self.join_reference.clone(),
                self.payload.clone(),
                created_at + rejoin.join_timeout,
            )
            .await
        {
            Ok(socket_joined_rx) => {
                let channel_left_txs = if let State::Leaving(Leaving {
                    ref mut channel_left_txs,
                    ..
                }) = state
                {
                    Some(channel_left_txs.drain(..).collect())
                } else {
                    None
                };

                if let Some(channel_left_txs) = channel_left_txs {
                    State::send_to_channel_txs(channel_left_txs, Err(LeaveError::JoinBeforeLeft));
                };

                Ok(State::Joining(Joining {
                    socket_joined_rx,
                    channel_joined_txs,
                    rejoin,
                }))
            }
            Err(_) => Err(Self::socket_shutdown(state)),
        }
    }

    fn socket_shutdown(state: State) -> ShutdownError {
        match state {
            State::WaitingForSocketToConnect { .. }
            | State::WaitingToJoin { .. }
            | State::WaitingToRejoin { .. }
            | State::Joined { .. }
            | State::Left { .. }
            | State::ShuttingDown => {}
            State::Joining(Joining {
                channel_joined_txs, ..
            }) => {
                State::send_to_channel_txs(channel_joined_txs, Err(JoinError::Shutdown));
            }
            State::Leaving(Leaving {
                channel_left_txs, ..
            }) => State::send_to_channel_txs(channel_left_txs, Err(LeaveError::SocketShutdown)),
        }

        ShutdownError::SocketShutdown
    }

    /// Used during graceful termination to tell the server we're leaving the channel
    async fn leave(
        &self,
        mut state: State,
        left_tx: oneshot::Sender<Result<(), LeaveError>>,
    ) -> State {
        match state {
            State::WaitingForSocketToConnect { .. }
            | State::WaitingToJoin { .. }
            | State::Left { .. }
            | State::ShuttingDown => {
                left_tx.send(Ok(())).ok();

                state
            }
            State::Joining(joining) => {
                match self
                    .socket
                    .leave(self.topic.clone(), self.join_reference.clone())
                    .await
                {
                    Ok(socket_left_rx) => {
                        State::send_to_channel_txs(
                            joining.channel_joined_txs,
                            Err(JoinError::LeavingWhileJoining),
                        );

                        State::Leaving(Leaving {
                            socket_left_rx,
                            channel_left_txs: vec![left_tx],
                        })
                    }
                    Err(leave_error) => {
                        left_tx.send(Err(leave_error)).ok();

                        State::Joining(joining)
                    }
                }
            }
            State::Joined(joined) => match self
                .socket
                .leave(self.topic.clone(), self.join_reference.clone())
                .await
            {
                Ok(socket_left_rx) => State::Leaving(Leaving {
                    socket_left_rx,
                    channel_left_txs: vec![left_tx],
                }),
                Err(leave_error) => {
                    left_tx.send(Err(leave_error)).ok();

                    State::Joined(joined)
                }
            },
            State::WaitingToRejoin { .. } => {
                left_tx.send(Ok(())).ok();

                State::Left
            }
            State::Leaving(Leaving {
                ref mut channel_left_txs,
                ..
            }) => {
                channel_left_txs.push(left_tx);

                state
            }
        }
    }

    async fn send(&self, joined: Joined, send_command: SendCommand) -> State {
        match send_command {
            SendCommand::Cast(cast) => self.cast(joined, cast).await,
            SendCommand::Call(call) => self.call(joined, call).await,
        }
    }

    async fn cast(&self, joined: Joined, event_payload: EventPayload) -> State {
        match self
            .socket
            .cast(
                self.topic.clone(),
                self.join_reference.clone(),
                event_payload,
            )
            .await
        {
            Ok(()) => State::Joined(joined),
            Err(cast_error) => match cast_error {
                socket::CastError::Shutdown => State::ShuttingDown,
            },
        }
    }

    async fn call(&self, joined: Joined, call: Call) -> State {
        match self
            .socket
            .call(self.topic.clone(), self.join_reference.clone(), call)
            .await
        {
            Ok(()) => State::Joined(joined),
            Err(call_error) => match call_error {
                socket::CallError::Shutdown => State::ShuttingDown,
            },
        }
    }

    fn send_event_payload<EP: Into<EventPayload>>(&self, event_payload: EP) {
        self.event_payload_tx.send(event_payload.into()).ok();
    }
}

pub(super) enum StateCommand {
    Join {
        /// When the join was created
        created_at: Instant,
        /// How long after `created_at` must the join complete
        timeout: Duration,
        joined_tx: oneshot::Sender<Result<(), JoinError>>,
    },
    Leave {
        left_tx: oneshot::Sender<Result<(), LeaveError>>,
    },
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum LeaveError {
    #[error("timeout joining channel")]
    Timeout,
    #[error("channel shutting down")]
    ShuttingDown,
    #[error("channel already shutdown")]
    Shutdown,
    #[error("socket already shutdown")]
    SocketShutdown,
    #[error("web socket error {0}")]
    WebSocketError(Arc<tungstenite::Error>),
    #[error("server rejected leave")]
    Rejected(Payload),
    #[error("A join was initiated before the server could respond if leave was successful")]
    JoinBeforeLeft,
    #[error("URL error: {0}")]
    Url(Arc<UrlError>),
    #[error("HTTP error: {}", .0.status())]
    Http(Arc<Response<Option<String>>>),
    /// HTTP format error.
    #[error("HTTP format error: {0}")]
    HttpFormat(Arc<http::Error>),
}
impl From<mpsc::error::SendError<StateCommand>> for LeaveError {
    fn from(_: mpsc::error::SendError<StateCommand>) -> Self {
        LeaveError::Shutdown
    }
}
impl From<socket::listener::ShutdownError> for LeaveError {
    fn from(shutdown_error: socket::listener::ShutdownError) -> Self {
        match shutdown_error {
            socket::listener::ShutdownError::AlreadyJoined => LeaveError::SocketShutdown,
            socket::listener::ShutdownError::Url(url_error) => LeaveError::Url(Arc::new(url_error)),
            socket::listener::ShutdownError::Http(response) => LeaveError::Http(Arc::new(response)),
            socket::listener::ShutdownError::HttpFormat(error) => {
                LeaveError::HttpFormat(Arc::new(error))
            }
        }
    }
}

#[derive(Debug)]
pub(super) enum SendCommand {
    Cast(EventPayload),
    Call(Call),
}

#[derive(Debug)]
pub(crate) struct Call {
    pub event_payload: EventPayload,
    pub reply_tx: oneshot::Sender<Result<Payload, channel::CallError>>,
}

#[must_use]
pub(crate) enum State {
    WaitingForSocketToConnect {
        rejoin: Option<Rejoin>,
    },
    WaitingToJoin,
    Joining(Joining),
    WaitingToRejoin {
        sleep: Pin<Box<Sleep>>,
        rejoin: Rejoin,
    },
    Joined(Joined),
    Leaving(Leaving),
    Left,
    ShuttingDown,
}
impl State {
    pub fn status(&self) -> Status {
        match self {
            State::WaitingForSocketToConnect { .. } => Status::WaitingForSocketToConnect,
            State::WaitingToJoin => Status::WaitingToJoin,
            State::Joining(_) => Status::Joining,
            State::WaitingToRejoin { .. } => Status::WaitingToRejoin,
            State::Joined { .. } => Status::Joined,
            State::Leaving { .. } => Status::Leaving,
            State::Left => Status::Left,
            State::ShuttingDown => Status::ShuttingDown,
        }
    }

    fn connectivity_changed(self, connectivity: Connectivity) -> Self {
        let prefix = format!(
            "Received connectivity change {:?} and transitions from {:#?} to ",
            connectivity, self
        );

        let next_state = match self {
            State::WaitingForSocketToConnect { rejoin } => match connectivity {
                Connectivity::Connected => match rejoin {
                    None => State::WaitingToJoin,
                    Some(rejoin) => rejoin.wait(),
                },
                Connectivity::Disconnected(disconnected) => match disconnected {
                    Disconnected::Disconnect | Disconnected::Reconnect => self,
                    Disconnected::Shutdown => State::ShuttingDown,
                },
            },
            State::WaitingToJoin | State::Left => match connectivity {
                Connectivity::Connected => self,
                Connectivity::Disconnected(disconnected) => match disconnected {
                    Disconnected::Disconnect | Disconnected::Reconnect => {
                        State::WaitingForSocketToConnect { rejoin: None }
                    }
                    Disconnected::Shutdown => State::ShuttingDown,
                },
            },
            State::Joining(Joining {
                channel_joined_txs,
                rejoin,
                ..
            }) => match connectivity {
                Connectivity::Connected | Connectivity::Disconnected(Disconnected::Reconnect) => {
                    Self::send_to_channel_txs(
                        channel_joined_txs,
                        Err(JoinError::SocketDisconnected),
                    );

                    State::WaitingForSocketToConnect {
                        rejoin: Some(rejoin),
                    }
                }
                Connectivity::Disconnected(Disconnected::Disconnect) => {
                    Self::send_to_channel_txs(
                        channel_joined_txs,
                        Err(JoinError::SocketDisconnected),
                    );

                    State::WaitingForSocketToConnect { rejoin: None }
                }
                Connectivity::Disconnected(Disconnected::Shutdown) => {
                    Self::send_to_channel_txs(channel_joined_txs, Err(JoinError::ShuttingDown));

                    State::ShuttingDown
                }
            },
            State::WaitingToRejoin { sleep, rejoin } => match connectivity {
                Connectivity::Connected => State::WaitingToRejoin { sleep, rejoin },
                Connectivity::Disconnected(disconnected) => match disconnected {
                    Disconnected::Disconnect => State::WaitingForSocketToConnect { rejoin: None },
                    Disconnected::Shutdown => State::ShuttingDown,
                    Disconnected::Reconnect => State::WaitingForSocketToConnect {
                        rejoin: Some(rejoin),
                    },
                },
            },
            State::Joined(joined) => match connectivity {
                Connectivity::Connected | Connectivity::Disconnected(Disconnected::Reconnect) => {
                    State::WaitingForSocketToConnect {
                        rejoin: Some(joined.rejoin()),
                    }
                }
                Connectivity::Disconnected(Disconnected::Disconnect) => {
                    State::WaitingForSocketToConnect { rejoin: None }
                }
                Connectivity::Disconnected(Disconnected::Shutdown) => State::ShuttingDown,
            },
            State::Leaving(Leaving {
                channel_left_txs, ..
            }) => {
                Self::send_to_channel_txs(channel_left_txs, Ok(()));

                match connectivity {
                    Connectivity::Connected => State::Left,
                    Connectivity::Disconnected(disconnected) => match disconnected {
                        Disconnected::Disconnect | Disconnected::Reconnect => {
                            State::WaitingForSocketToConnect { rejoin: None }
                        }
                        Disconnected::Shutdown => State::ShuttingDown,
                    },
                }
            }
            State::ShuttingDown => self,
        };

        debug!("{}{:#?}", prefix, next_state);

        next_state
    }

    fn shutdown(self) -> Self {
        match self {
            State::WaitingForSocketToConnect { .. }
            | State::WaitingToJoin { .. }
            | State::WaitingToRejoin { .. }
            | State::Joined(_)
            | State::Left { .. }
            | State::ShuttingDown => (),
            State::Joining(Joining {
                channel_joined_txs, ..
            }) => {
                Self::send_to_channel_txs(channel_joined_txs, Err(JoinError::ShuttingDown));
            }
            State::Leaving(Leaving {
                channel_left_txs, ..
            }) => Self::send_to_channel_txs(channel_left_txs, Err(LeaveError::ShuttingDown)),
        };

        State::ShuttingDown
    }

    fn send_to_channel_txs<E>(
        mut channel_txs: Vec<oneshot::Sender<Result<(), E>>>,
        result: Result<(), E>,
    ) where
        E: Clone,
    {
        for channel_tx in channel_txs.drain(0..) {
            channel_tx.send(result.clone()).ok();
        }
    }
}
impl Debug for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            State::WaitingForSocketToConnect { rejoin, .. } => f
                .debug_struct("WaitingForSocketToConnect")
                .field("rejoin", rejoin)
                .finish_non_exhaustive(),
            State::WaitingToJoin => f.debug_struct("WaitingToJoin").finish_non_exhaustive(),
            State::Joining(joining) => f.debug_tuple("Joining").field(joining).finish(),
            State::WaitingToRejoin { sleep, rejoin, .. } => f
                .debug_struct("WaitingToRejoin")
                .field(
                    "duration_until_sleep_over",
                    &(sleep.deadline() - Instant::now()),
                )
                .field("rejoin", rejoin)
                .finish(),
            State::Joined(joined) => f.debug_tuple("Joined").field(joined).finish(),
            State::Leaving { .. } => f.debug_struct("Leaving").finish_non_exhaustive(),
            State::Left { .. } => f.write_str("Left"),
            State::ShuttingDown => f.write_str("ShuttingDown"),
        }
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub(crate) struct JoinedChannelReceivers {
    pub push: mpsc::Receiver<Push>,
    pub broadcast: broadcast::Receiver<Broadcast>,
    pub left: oneshot::Receiver<()>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Rejoin {
    join_timeout: Duration,
    /// Enough for > 1 week of attempts; u8 would only be 42 minutes of attempts.
    attempts: u16,
}
impl Rejoin {
    fn wait(self) -> State {
        State::WaitingToRejoin {
            sleep: Box::pin(tokio::time::sleep(self.sleep_duration())),
            rejoin: self.next(),
        }
    }

    fn next(self) -> Self {
        Self {
            join_timeout: self.join_timeout,
            attempts: self.attempts.saturating_add(1),
        }
    }

    fn sleep_duration(&self) -> Duration {
        self.join_timeout * self.sleep_duration_multiplier()
    }

    fn sleep_duration_multiplier(&self) -> u32 {
        Self::SLEEP_DURATION_MULTIPLIERS
            [(self.attempts as usize).min(Self::SLEEP_DURATION_MULTIPLIERS.len() - 1)]
    }

    const SLEEP_DURATION_MULTIPLIERS: &'static [u32] = &[0, 1, 2, 5, 10];
}

pub(crate) struct Joining {
    socket_joined_rx: oneshot::Receiver<Result<JoinedChannelReceivers, socket::JoinError>>,
    channel_joined_txs: Vec<oneshot::Sender<Result<(), JoinError>>>,
    rejoin: Rejoin,
}
impl Debug for Joining {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Joining")
            .field("rejoin", &self.rejoin)
            .finish_non_exhaustive()
    }
}

pub(crate) struct Joined {
    push_rx: mpsc::Receiver<Push>,
    broadcast_rx: broadcast::Receiver<Broadcast>,
    left_rx: oneshot::Receiver<()>,
    join_timeout: Duration,
}
impl Joined {
    fn rejoin(self) -> Rejoin {
        Rejoin {
            join_timeout: self.join_timeout,
            attempts: 0,
        }
    }
}
impl Debug for Joined {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Joined")
            .field("join_timeout", &self.join_timeout)
            .finish_non_exhaustive()
    }
}

pub(crate) struct Leaving {
    socket_left_rx: oneshot::Receiver<Result<(), LeaveError>>,
    channel_left_txs: Vec<oneshot::Sender<Result<(), LeaveError>>>,
}
impl Leaving {
    fn left(self, left_result: Result<(), LeaveError>) -> State {
        State::send_to_channel_txs(self.channel_left_txs, left_result);

        State::Left
    }
}

#[derive(Copy, Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ShutdownError {
    #[error("socket already shutdown")]
    SocketShutdown,
    #[error("listener task was already joined once from another caller")]
    AlreadyJoined,
}
