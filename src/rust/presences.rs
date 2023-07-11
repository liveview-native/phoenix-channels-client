mod listener;

use atomic_take::AtomicTake;
use std::panic;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::ffi::channel::Channel;
use crate::ffi::presences::{Presences, PresencesShutdownError};
use crate::rust::presence::Presence;
use crate::rust::presences::listener::Listener;

impl Presences {
    /// Spawns a new [Presences] that tracks presence changes on `channel`.
    pub(crate) async fn spawn(channel: Arc<Channel>) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (list_tx, list_rx) = mpsc::channel(10);
        let (sync_tx, _) = broadcast::channel(10);
        let (join_tx, _) = broadcast::channel(10);
        let (leave_tx, _) = broadcast::channel(10);
        let join_handle = Listener::spawn(
            channel.clone(),
            shutdown_rx,
            list_rx,
            sync_tx.clone(),
            join_tx.clone(),
            leave_tx.clone(),
        );

        Self {
            channel,
            shutdown_tx: AtomicTake::new(shutdown_tx),
            list_tx,
            sync_tx,
            join_tx,
            leave_tx,
            join_handle: AtomicTake::new(join_handle),
        }
    }

    /// Propagates panic from [Listener::listen]
    pub(crate) async fn listener_shutdown(&self) -> Result<(), PresencesShutdownError> {
        match self.join_handle.take() {
            Some(join_handle) => match join_handle.await {
                Ok(()) => Ok(()),
                Err(join_error) => panic::resume_unwind(join_error.into_panic()),
            },
            None => Err(PresencesShutdownError::AlreadyJoined),
        }
    }
}

pub struct Join {
    pub key: String,
    pub current: Option<Presence>,
    pub joined: Presence,
}

pub struct Leave {
    pub key: String,
    pub current: Presence,
    pub left: Presence,
}
