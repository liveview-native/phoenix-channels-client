use std::sync::Arc;

use atomic_take::AtomicTake;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;

use crate::ffi::channel::Channel;
use crate::ffi::presence::Presence;
use crate::rust;

/// Errors reutrned by [Presences] functions.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PresencesError {
    /// Errors when calling [PresencesJoins::join].
    #[error(transparent)]
    PresencesJoins {
        #[from]
        presences_joins: PresencesJoinsError,
    },
    /// Errors when calling [PresencesLeaves::leave].
    #[error(transparent)]
    PresencesLeaves {
        #[from]
        presences_leave: PresencesLeavesError,
    },
    /// Errors when calling [PresencesSyncs::sync].
    #[error(transparent)]
    PresencesSyncs {
        #[from]
        presences_syncs: PresencesSyncsError,
    },
    /// Errors when calling [Presences::shutdown].
    #[error(transparent)]
    Shutdown {
        #[from]
        shutdown: PresencesShutdownError,
    },
}

/// All [Presence]s in a [Channel](Presences::channel), such as all users in chat room.
#[derive(uniffi::Object)]
pub struct Presences {
    pub(crate) channel: Arc<Channel>,
    pub(crate) shutdown_tx: AtomicTake<oneshot::Sender<()>>,
    pub(crate) list_tx: mpsc::Sender<oneshot::Sender<Vec<rust::presence::Presence>>>,
    pub(crate) sync_tx: broadcast::Sender<()>,
    pub(crate) join_tx: broadcast::Sender<Arc<rust::presences::Join>>,
    pub(crate) leave_tx: broadcast::Sender<Arc<rust::presences::Leave>>,
    pub(crate) join_handle: AtomicTake<JoinHandle<()>>,
}
#[uniffi::export(async_runtime = "tokio")]
impl Presences {
    /// The [Channel] on which [Presence]s are being tracked.
    pub fn channel(&self) -> Arc<Channel> {
        self.channel.clone()
    }

    /// Broadcasts on changes to [Presences::list].
    pub fn syncs(&self) -> Arc<PresencesSyncs> {
        Arc::new(self.sync_tx.subscribe().into())
    }

    /// Broadcasts when a [Presence] joins the [Presences::channel].
    pub fn joins(&self) -> Arc<PresencesJoins> {
        Arc::new(self.join_tx.subscribe().into())
    }

    /// Broadcasts when a [Presence] leaves the [Presences::channel].
    pub fn leaves(&self) -> Arc<PresencesLeaves> {
        Arc::new(self.leave_tx.subscribe().into())
    }

    /// The [Presence] currently in [Presences::channel].
    pub async fn list(&self) -> Result<Vec<Presence>, PresencesShutdownError> {
        let (tx, rx) = oneshot::channel();

        match self.list_tx.send(tx).await {
            Ok(()) => match rx.await {
                Ok(presence_vec) => Ok(presence_vec.into_iter().map(From::from).collect()),
                Err(_) => self.listener_shutdown().await.map(|_| vec![]),
            },
            Err(_) => self.listener_shutdown().await.map(|_| vec![]),
        }
    }

    /// Propagates panic from the async task.
    pub async fn shutdown(&self) -> Result<(), PresencesShutdownError> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            shutdown_tx.send(()).ok();
        }

        self.listener_shutdown().await
    }
}

#[derive(uniffi::Object)]
pub struct PresencesSyncs(Mutex<broadcast::Receiver<()>>);
#[uniffi::export(async_runtime = "tokio")]
impl PresencesSyncs {
    /// Wait for next time [Presences] changes.  When [Presences] changes call [Presences::list] to get the up-to-date
    /// list of [Presence]s.
    pub async fn sync(&self) -> Result<(), PresencesSyncsError> {
        // because the sync is () and no data is lost, if one is missed just jump to the next one
        loop {
            match self.0.lock().await.recv().await {
                Ok(()) => break Ok(()),
                Err(error) => match error {
                    broadcast::error::RecvError::Lagged(_) => continue,
                    broadcast::error::RecvError::Closed => {
                        break Err(PresencesSyncsError::NoMoreSyncs)
                    }
                },
            }
        }
    }
}
impl From<broadcast::Receiver<()>> for PresencesSyncs {
    fn from(receiver: broadcast::Receiver<()>) -> Self {
        Self(Mutex::new(receiver))
    }
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PresencesSyncsError {
    #[error("No more syncs left")]
    NoMoreSyncs,
}

/// Waits for the next [PresenceJoin](crate::PresencesJoin) when
/// [Presences::list](crate::Presences::list) changes by a user joining.
#[derive(uniffi::Object)]
pub struct PresencesJoins(Mutex<broadcast::Receiver<Arc<rust::presences::Join>>>);

#[uniffi::export(async_runtime = "tokio")]
impl PresencesJoins {
    /// Wait for next time a user joins [Presences].
    pub async fn join(&self) -> Result<PresencesJoin, PresencesJoinsError> {
        self.0
            .lock()
            .await
            .recv()
            .await
            .map(|arc_join| arc_join.as_ref().into())
            .map_err(From::from)
    }
}
impl From<broadcast::Receiver<Arc<rust::presences::Join>>> for PresencesJoins {
    fn from(receiver: broadcast::Receiver<Arc<rust::presences::Join>>) -> Self {
        Self(Mutex::new(receiver))
    }
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PresencesJoinsError {
    #[error("No more PresenceJoins left")]
    NoMoreJoins,
    #[error("Missed {missed_join_count} PresenceJoins; jumping to next Join")]
    MissedJoins { missed_join_count: u64 },
}
impl From<broadcast::error::RecvError> for PresencesJoinsError {
    fn from(recv_error: broadcast::error::RecvError) -> Self {
        match recv_error {
            broadcast::error::RecvError::Closed => Self::NoMoreJoins,
            broadcast::error::RecvError::Lagged(missed_join_count) => {
                Self::MissedJoins { missed_join_count }
            }
        }
    }
}

/// When a join occurs on a channel.
#[derive(Clone, Debug, uniffi::Record)]
pub struct PresencesJoin {
    /// The key used to group multiple [Presence] together, such as the user name when tracking all connection one
    /// user has to the same chat room, such as from multiple devices or multiple browser tabs.
    pub key: String,
    /// The `key`'s [Presence] before this join occurred.
    pub current: Option<Presence>,
    /// The `key`'s updated [Presence] due to this join.
    pub joined: Presence,
}
impl From<&rust::presences::Join> for PresencesJoin {
    fn from(rust_join_ref: &rust::presences::Join) -> Self {
        Self {
            key: rust_join_ref.key.clone(),
            current: rust_join_ref.current.as_ref().map(From::from),
            joined: (&rust_join_ref.joined).into(),
        }
    }
}

/// Waits for the next [PresenceLeave](crate::PresencesLeave) when
/// [Presences::list](crate::Presences::list) changes because a user leaves.
#[derive(uniffi::Object)]
pub struct PresencesLeaves(Mutex<broadcast::Receiver<Arc<rust::presences::Leave>>>);
#[uniffi::export(async_runtime = "tokio")]
impl PresencesLeaves {
    /// Wait for next time a user leaves [Presences].
    pub async fn leave(&self) -> Result<PresencesLeave, PresencesLeavesError> {
        self.0
            .lock()
            .await
            .recv()
            .await
            .map(|arc_leave| arc_leave.as_ref().into())
            .map_err(From::from)
    }
}

impl From<broadcast::Receiver<Arc<rust::presences::Leave>>> for PresencesLeaves {
    fn from(receiver: broadcast::Receiver<Arc<rust::presences::Leave>>) -> Self {
        Self(Mutex::new(receiver))
    }
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PresencesLeavesError {
    #[error("No more PresenceLeaves left")]
    NoMoreLeaves,
    #[error("Missed {missed_leave_count} PresenceLeaves; jumping to next PresenceLeave")]
    MissedLeaves { missed_leave_count: u64 },
}
impl From<broadcast::error::RecvError> for PresencesLeavesError {
    fn from(recv_error: broadcast::error::RecvError) -> Self {
        match recv_error {
            broadcast::error::RecvError::Closed => Self::NoMoreLeaves,
            broadcast::error::RecvError::Lagged(missed_leave_count) => {
                Self::MissedLeaves { missed_leave_count }
            }
        }
    }
}

/// When a leave occurs on a channel.
#[derive(uniffi::Record)]
pub struct PresencesLeave {
    /// The key used to group multiple [Presence] together, such as the user name when tracking all connection one
    /// user has to the same chat room, such as from multiple devices or multiple browser tabs.
    pub key: String,
    /// The `key`'s [Presence] before this leave occurred.
    pub current: Presence,
    /// The `key`'s updated [Presence] due to this leave.
    pub left: Presence,
}
impl From<&rust::presences::Leave> for PresencesLeave {
    fn from(rust_leave_ref: &rust::presences::Leave) -> Self {
        Self {
            key: rust_leave_ref.key.clone(),
            current: (&rust_leave_ref.current).into(),
            left: (&rust_leave_ref.left).into(),
        }
    }
}

#[derive(Copy, Clone, Debug, thiserror::Error, PartialEq, Eq, uniffi::Error)]
pub enum PresencesShutdownError {
    #[error("listener task was already joined once from another caller")]
    AlreadyJoined,
}
