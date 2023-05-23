use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use log::debug;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task;
use tokio::task::JoinHandle;
use tokio::time::{Instant, Sleep};
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::error::UrlError;
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::http::Response;

use crate::channel::Status;
use crate::join_reference::JoinReference;
use crate::message::{Broadcast, Push};
use crate::socket::listener::Left;
use crate::topic::Topic;
use crate::{channel, socket, Event, EventHandler, Payload, Socket, SubscriptionReference};

pub(super) struct Listener {
    socket: Arc<Socket>,
    topic: Topic,
    payload: Arc<Payload>,
    channel_status: Arc<AtomicU32>,
    subscription_command_rx: mpsc::Receiver<SubscriptionCommand>,
    handler_by_subscription_reference_by_event:
        HashMap<Event, HashMap<SubscriptionReference, EventHandler>>,
    state_command_rx: mpsc::Receiver<StateCommand>,
    state: Option<State>,
    send_command_rx: mpsc::Receiver<SendCommand>,
    join_reference: JoinReference,
}
impl Listener {
    pub(super) fn spawn(
        socket: Arc<Socket>,
        topic: Topic,
        payload: Arc<Payload>,
        channel_status: Arc<AtomicU32>,
        subscription_command_rx: mpsc::Receiver<SubscriptionCommand>,
        state_command_rx: mpsc::Receiver<StateCommand>,
        send_command_rx: mpsc::Receiver<SendCommand>,
    ) -> JoinHandle<Result<(), ShutdownError>> {
        let listener = Self::init(
            socket,
            topic,
            payload,
            channel_status,
            subscription_command_rx,
            state_command_rx,
            send_command_rx,
        );

        tokio::spawn(listener.listen_wrapper())
    }

    fn init(
        socket: Arc<Socket>,
        topic: Topic,
        payload: Arc<Payload>,
        channel_status: Arc<AtomicU32>,
        subscription_command_rx: mpsc::Receiver<SubscriptionCommand>,
        state_command_rx: mpsc::Receiver<StateCommand>,
        send_command_rx: mpsc::Receiver<SendCommand>,
    ) -> Self {
        Self {
            socket,
            topic,
            payload,
            channel_status,
            subscription_command_rx,
            handler_by_subscription_reference_by_event: Default::default(),
            state_command_rx,
            state: Some(State::NeverJoined),
            send_command_rx,
            join_reference: JoinReference::new(),
        }
    }

    async fn listen_wrapper(self) -> Result<(), ShutdownError> {
        let result = self.listen().await;

        debug!("Listen returned {:#?}", result);

        result
    }

    async fn listen(mut self) -> Result<(), ShutdownError> {
        debug!(
            "Channel listener started for topic {} will join as {}",
            &self.topic, &self.join_reference
        );

        loop {
            let mut current_state = self.state.take().unwrap();

            let next_state = match current_state {
                State::NeverJoined | State::Left => tokio::select! {
                    Some(subscription_command) = self.subscription_command_rx.recv() => {
                        self.update_subscriptions(subscription_command).await;

                        current_state
                    },
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(current_state, state_command).await?,
                    else => break Ok(())
                },
                State::Joining(mut joining) => tokio::select! {
                    socket_joined_result = &mut joining.socket_joined_rx => self.joined_result_result_received(joining, socket_joined_result).await?,
                    Some(subscription_command) = self.subscription_command_rx.recv() => {
                        self.update_subscriptions(subscription_command).await;

                        State::Joining(joining)
                    },
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(State::Joining(joining), state_command).await?,
                    else => break Ok(())
                },
                State::WaitingToRejoin {
                    ref mut sleep,
                    rejoin,
                } => tokio::select! {
                    () = sleep => self.rejoin(current_state, rejoin).await?,
                    Some(subscription_command) = self.subscription_command_rx.recv() => {
                        self.update_subscriptions(subscription_command).await;

                        current_state
                    },
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(current_state, state_command).await?,
                    else => break Ok(())
                },
                State::Joined(mut joined) => {
                    tokio::select! {
                        // left_rx should be polled before others, so that that the socket
                        // disconnected or is reconnecting is seen before the broadcast_rx's
                        // sender being dropped and a `Err(broadcast::error::RecvError::Closed)`
                        // being received
                        biased;

                        left_result = &mut joined.left_rx => self.left_result_received(joined, left_result),
                        Some(subscription_command) = self.subscription_command_rx.recv() => {
                            self.update_subscriptions(subscription_command).await;

                            State::Joined(joined)
                        },
                        Some(state_command) = self.state_command_rx.recv() => self.update_state(State::Joined(joined), state_command).await?,
                        Some(send_command) = self.send_command_rx.recv() => self.send(joined, send_command).await,
                        Some(push) = joined.push_rx.recv() => {
                            self.push_received(push).await;

                            State::Joined(joined)
                        },
                        broadcast_result = joined.broadcast_rx.recv() => self.broadcast_result_received(joined, broadcast_result).await?,
                        else => break Ok(())
                    }
                }
                State::Leaving {
                    ref mut socket_left_rx,
                    ..
                } => tokio::select! {
                    left_result_result = socket_left_rx => self.left_result_result_received(current_state, left_result_result).await?,
                    Some(subscription_command) = self.subscription_command_rx.recv() => {
                        self.update_subscriptions(subscription_command).await;

                        current_state
                    },
                    Some(state_command) = self.state_command_rx.recv() => self.update_state(current_state, state_command).await?,
                    else => break Ok(())
                },
                State::ShuttingDown => break Ok(()),
            };

            let channel_status = match next_state {
                State::NeverJoined => Status::NeverJoined,
                State::Joining { .. } => Status::Joining,
                State::WaitingToRejoin { .. } => Status::WaitingToRejoin,
                State::Joined { .. } => Status::Joined,
                State::Leaving { .. } => Status::Leaving,
                State::Left => Status::Left,
                State::ShuttingDown => Status::ShuttingDown,
            };
            self.channel_status
                .store(channel_status as u32, Ordering::SeqCst);

            debug!("next state is {:#?}", &next_state);

            self.state = Some(next_state);
        }
    }

    async fn update_subscriptions(&mut self, subscription_command: SubscriptionCommand) {
        match subscription_command {
            SubscriptionCommand::Subscribe(subscribe) => self.subscribe(subscribe).await,
            SubscriptionCommand::UnsubscribeSubscription(unsubscribe_subscription) => {
                self.unsubscribe_subscription(unsubscribe_subscription)
            }
            SubscriptionCommand::UnsubscribeEvent(event) => self.unsubscribe_event(event),
        }
    }

    async fn subscribe(&mut self, subscribe: Subscribe) {
        let Subscribe {
            event,
            handler,
            subscription_reference_tx,
        } = subscribe;

        let subscription_reference = SubscriptionReference::new();

        self.handler_by_subscription_reference_by_event
            .entry(event)
            .or_default()
            .insert(subscription_reference.clone(), handler);

        subscription_reference_tx
            .send(subscription_reference)
            // Don't care if [Channel::on] does away.
            .ok();
    }

    fn unsubscribe_subscription(&mut self, unsubscribe_subscription: UnsubscribeSubscription) {
        let UnsubscribeSubscription {
            event,
            subscription_reference,
        } = unsubscribe_subscription;

        if let Entry::Occupied(mut handler_by_subscription_reference_entry) = self
            .handler_by_subscription_reference_by_event
            .entry(event.clone())
        {
            let handler_by_subscription_reference =
                handler_by_subscription_reference_entry.get_mut();

            if let Some(_) = handler_by_subscription_reference.remove(&subscription_reference) {
                if handler_by_subscription_reference.is_empty() {
                    handler_by_subscription_reference_entry.remove();
                }
            }
        }
    }

    fn unsubscribe_event(&mut self, event: Event) {
        self.handler_by_subscription_reference_by_event
            .remove(&event);
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
            StateCommand::Shutdown => Ok(State::ShuttingDown),
        }
    }

    async fn join(
        &self,
        mut state: State,
        created_at: Instant,
        timeout: Duration,
        channel_joined_tx: oneshot::Sender<Result<(), channel::JoinError>>,
    ) -> Result<State, ShutdownError> {
        match state {
            State::NeverJoined | State::Leaving { .. } | State::Left => {
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
                    .send(Err(channel::JoinError::WaitingToRejoin(sleep.deadline())))
                    .ok();

                Ok(state)
            }
            State::Joined { .. } => {
                channel_joined_tx.send(Ok(())).ok();

                Ok(state)
            }
            State::ShuttingDown => {
                channel_joined_tx
                    .send(Err(channel::JoinError::ShuttingDown))
                    .ok();

                Ok(state)
            }
        }
    }

    async fn socket_join_if_not_timed_out(
        &self,
        state: State,
        created_at: Instant,
        timeout: Duration,
        channel_joined_tx: oneshot::Sender<Result<(), channel::JoinError>>,
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
                    join_timeout: Default::default(),
                }),
            ),
            Err(socket_join_error) => match socket_join_error {
                socket::JoinError::Shutdown => {
                    (Err(channel::JoinError::ShuttingDown), State::ShuttingDown)
                }
                socket::JoinError::Rejected(payload) => {
                    (Err(channel::JoinError::Rejected(payload)), State::Left)
                }
                socket::JoinError::Disconnected => {
                    (Err(channel::JoinError::SocketDisconnected), rejoin.wait())
                }
                socket::JoinError::Timeout => (Err(channel::JoinError::Timeout), rejoin.wait()),
            },
        };

        Self::send_to_channel_txs(channel_joined_txs, channel_joined_result);

        next_state
    }

    async fn rejoin(&self, state: State, rejoin: Rejoin) -> Result<State, ShutdownError> {
        self.socket_join(state, Instant::now(), rejoin, vec![])
            .await
    }

    async fn socket_join(
        &self,
        state: State,
        created_at: Instant,
        rejoin: Rejoin,
        channel_joined_txs: Vec<oneshot::Sender<Result<(), channel::JoinError>>>,
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
                if let State::Leaving {
                    channel_left_txs, ..
                } = state
                {
                    Self::send_to_channel_txs(channel_left_txs, Err(LeaveError::JoinBeforeLeft));
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
            State::NeverJoined
            | State::WaitingToRejoin { .. }
            | State::Joined { .. }
            | State::Left
            | State::ShuttingDown => {}
            State::Joining(Joining {
                channel_joined_txs, ..
            }) => {
                Self::send_to_channel_txs(channel_joined_txs, Err(channel::JoinError::Shutdown));
            }
            State::Leaving {
                channel_left_txs, ..
            } => Self::send_to_channel_txs(channel_left_txs, Err(LeaveError::SocketShutdown)),
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
            State::NeverJoined | State::Left | State::ShuttingDown => {
                left_tx.send(Ok(())).ok();

                state
            }
            State::Joining { .. } => match self.socket_leave(state, left_tx).await {
                (
                    State::Joining(Joining {
                        channel_joined_txs, ..
                    }),
                    Some(next_state @ State::Leaving { .. }),
                ) => {
                    Self::send_to_channel_txs(
                        channel_joined_txs,
                        Err(channel::JoinError::LeavingWhileJoining),
                    );

                    next_state
                }
                (_, Some(next_state)) => next_state,
                (unchanged_state, None) => unchanged_state,
            },
            State::Joined { .. } => match self.socket_leave(state, left_tx).await {
                (_, Some(next_state)) => next_state,
                (unchanged_state, None) => unchanged_state,
            },
            State::WaitingToRejoin { .. } => {
                left_tx.send(Ok(())).ok();

                State::Left
            }
            State::Leaving {
                ref mut channel_left_txs,
                ..
            } => {
                channel_left_txs.push(left_tx);

                state
            }
        }
    }

    async fn socket_leave(
        &self,
        state: State,
        left_tx: oneshot::Sender<Result<(), LeaveError>>,
    ) -> (State, Option<State>) {
        match self
            .socket
            .leave(self.topic.clone(), self.join_reference.clone())
            .await
        {
            Ok(socket_left_rx) => (
                state,
                Some(State::Leaving {
                    socket_left_rx,
                    channel_left_txs: vec![left_tx],
                }),
            ),
            Err(leave_error) => {
                left_tx.send(Err(leave_error)).ok();

                (state, None)
            }
        }
    }

    async fn send(&self, joined: Joined, send_command: SendCommand) -> State {
        match send_command {
            SendCommand::Cast(cast) => self.cast(joined, cast).await,
            SendCommand::Call(call) => self.call(joined, call).await,
        }
    }

    async fn cast(&self, joined: Joined, cast: Cast) -> State {
        match self
            .socket
            .cast(self.topic.clone(), self.join_reference.clone(), cast)
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

    async fn push_received(&self, push: Push) {
        self.handle_event(&push.event, push.payload.clone()).await
    }

    async fn broadcast_result_received(
        &self,
        joined: Joined,
        broadcast_result: Result<Broadcast, broadcast::error::RecvError>,
    ) -> Result<State, ShutdownError> {
        match broadcast_result {
            Ok(broadcast) => {
                self.broadcast_received(broadcast).await;

                Ok(State::Joined(joined))
            }
            Err(recv_error) => match recv_error {
                broadcast::error::RecvError::Closed => {
                    Err(Self::socket_shutdown(State::Joined(joined)))
                }
                broadcast::error::RecvError::Lagged(lag) => {
                    debug!("broadcasts lagged {} messages", lag);

                    Ok(State::Joined(joined))
                }
            },
        }
    }

    async fn broadcast_received(&self, broadcast: Broadcast) {
        self.handle_event(&broadcast.event, broadcast.payload.clone())
            .await
    }

    fn left_result_received(
        &self,
        joined: Joined,
        left_result: Result<Left, oneshot::error::RecvError>,
    ) -> State {
        match left_result {
            Ok(left) => match left {
                Left::ServerDisconnected => {
                    debug!("Server told socket to disconnect.  Will not reconnect socket, so will not rejoin channel.");

                    State::Left
                }
                Left::SocketDisconnect => {
                    debug!("Socket::disconnected() called.  Will not reconnect socket, so will not rejoin channel.");

                    State::Left
                }
                Left::SocketShutdown => {
                    debug!("Socket::shutdown() called.  Will not reconnect socket, so will not rejoin channel.");

                    State::ShuttingDown
                }
                Left::SocketReconnect => {
                    debug!(
                        "Socket disconnected implicitly.  Will rejoin channel {} as {}.",
                        &self.topic, &self.join_reference
                    );

                    joined.wait_to_rejoin()
                }
                Left::Left => {
                    debug!("Channel left explicitly.  Will not rejoin.");

                    State::Left
                }
            },
            Err(_) => {
                debug!("Got receive error instead of disconnect notification from socket.  Attempting to rejoin channel {} as {}.", &self.topic, &self.join_reference);

                joined.wait_to_rejoin()
            }
        }
    }

    async fn handle_event(&self, event: &Event, payload: Arc<Payload>) {
        if let Some(handler_by_subscription_reference) =
            self.handler_by_subscription_reference_by_event.get(event)
        {
            for handler in handler_by_subscription_reference.values() {
                tokio::spawn(handler(payload.clone()));
                task::consume_budget().await;
            }
        }
    }

    async fn left_result_result_received(
        &self,
        state: State,
        left_result_result: Result<Result<(), LeaveError>, oneshot::error::RecvError>,
    ) -> Result<State, ShutdownError> {
        match left_result_result {
            Ok(left_result) => {
                if let State::Leaving {
                    channel_left_txs, ..
                } = state
                {
                    Self::send_to_channel_txs(channel_left_txs, left_result);
                }

                Ok(State::Left)
            }
            Err(_) => Err(Self::socket_shutdown(state)),
        }
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

pub(super) enum SubscriptionCommand {
    Subscribe(Subscribe),
    UnsubscribeSubscription(UnsubscribeSubscription),
    UnsubscribeEvent(Event),
}

pub(super) struct Subscribe {
    pub event: Event,
    pub handler: EventHandler,
    pub subscription_reference_tx: oneshot::Sender<SubscriptionReference>,
}

pub(super) struct UnsubscribeSubscription {
    pub event: Event,
    pub subscription_reference: SubscriptionReference,
}

pub(super) enum StateCommand {
    Join {
        /// When the join was created
        created_at: Instant,
        /// How long after `created_at` must the join complete
        timeout: Duration,
        joined_tx: oneshot::Sender<Result<(), channel::JoinError>>,
    },
    Leave {
        left_tx: oneshot::Sender<Result<(), LeaveError>>,
    },
    Shutdown,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum LeaveError {
    #[error("timeout joining channel")]
    Timeout,
    #[error("channel already shutdown")]
    ChannelShutdown,
    #[error("socket already shutdown")]
    SocketShutdown,
    #[error("web socket error {0}")]
    WebSocketError(Arc<tungstenite::Error>),
    #[error("server rejected leave")]
    Rejected(Arc<Payload>),
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
        LeaveError::ChannelShutdown
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
    Cast(Cast),
    Call(Call),
}

#[derive(Debug)]
pub(crate) struct Call {
    pub event: Event,
    pub payload: Payload,
    pub reply_tx: oneshot::Sender<Result<Payload, channel::CallError>>,
}

#[derive(Debug)]
pub(crate) struct Cast {
    pub event: Event,
    pub payload: Payload,
}

#[must_use]
enum State {
    NeverJoined,
    Joining(Joining),
    WaitingToRejoin {
        sleep: Pin<Box<Sleep>>,
        rejoin: Rejoin,
    },
    Joined(Joined),
    Leaving {
        socket_left_rx: oneshot::Receiver<Result<(), LeaveError>>,
        channel_left_txs: Vec<oneshot::Sender<Result<(), LeaveError>>>,
    },
    Left,
    ShuttingDown,
}
impl Debug for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            State::NeverJoined => f.write_str("NeverJoined"),
            State::Joining(joining) => f.debug_tuple("Joining").field(joining).finish(),
            State::WaitingToRejoin { sleep, rejoin } => f
                .debug_struct("WaitingToRejoin")
                .field(
                    "duration_until_sleep_over",
                    &(sleep.deadline() - Instant::now()),
                )
                .field("rejoin", rejoin)
                .finish(),
            State::Joined(joined) => f.debug_tuple("Joined").field(joined).finish(),
            State::Leaving { .. } => f.debug_struct("Leaving").finish_non_exhaustive(),
            State::Left => f.write_str("Left"),
            State::ShuttingDown => f.write_str("ShuttingDown"),
        }
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub(crate) struct JoinedChannelReceivers {
    pub push: mpsc::Receiver<Push>,
    pub broadcast: broadcast::Receiver<Broadcast>,
    pub left: oneshot::Receiver<Left>,
}

#[derive(Copy, Clone, Debug)]
struct Rejoin {
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

    const SLEEP_DURATIONS: &'static [Duration] = &[
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_secs(5),
        Duration::from_secs(10),
    ];

    fn sleep_duration(&self) -> Duration {
        Self::SLEEP_DURATIONS[(self.attempts as usize).min(Self::SLEEP_DURATIONS.len() - 1)]
    }
}

struct Joining {
    socket_joined_rx: oneshot::Receiver<Result<JoinedChannelReceivers, socket::JoinError>>,
    channel_joined_txs: Vec<oneshot::Sender<Result<(), channel::JoinError>>>,
    rejoin: Rejoin,
}
impl Debug for Joining {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Joining")
            .field("rejoin", &self.rejoin)
            .finish_non_exhaustive()
    }
}

struct Joined {
    push_rx: mpsc::Receiver<Push>,
    broadcast_rx: broadcast::Receiver<Broadcast>,
    left_rx: oneshot::Receiver<Left>,
    join_timeout: Duration,
}
impl Joined {
    fn wait_to_rejoin(self) -> State {
        self.rejoin().wait()
    }

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

#[derive(Copy, Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ShutdownError {
    #[error("socket already shutdown")]
    SocketShutdown,
    #[error("listener task was already joined once from another caller")]
    AlreadyJoined,
}
