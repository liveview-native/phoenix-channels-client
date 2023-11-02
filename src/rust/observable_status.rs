use std::sync::Arc;

use arc_swap::ArcSwap;
use tokio::sync::{broadcast, Mutex};

use crate::ffi::observable_status::StatusesError;

#[derive(Clone)]
pub struct ObservableStatus<S: Copy + Clone + Eq + PartialEq, E: Clone> {
    status: Arc<ArcSwap<S>>,
    tx: broadcast::Sender<Result<S, E>>,
}
impl<S: Copy + Clone + Eq + PartialEq, E: Clone> ObservableStatus<S, E> {
    pub fn new(status: S) -> Self {
        let (tx, _) = broadcast::channel(10);

        Self {
            status: Arc::new(ArcSwap::from(Arc::new(status))),
            tx,
        }
    }

    pub fn get(&self) -> S {
        *self.status.load_full()
    }

    pub fn set(&self, status: S) {
        if *self.status.swap(Arc::new(status)) != status {
            self.tx.send(Ok(status)).ok();
        }
    }

    pub fn error(&self, error: E) {
        self.tx.send(Err(error)).ok();
    }

    pub fn subscribe(&self) -> Statuses<S, E> {
        Statuses {
            rx: Mutex::new(self.tx.subscribe()),
        }
    }
}

pub struct Statuses<S: Copy + Clone + Eq + PartialEq, E: Clone> {
    rx: Mutex<broadcast::Receiver<Result<S, E>>>,
}
impl<S: Copy + Clone + Eq + PartialEq, E: Clone> Statuses<S, E> {
    pub async fn status(&self) -> Result<Result<S, E>, StatusesError> {
        self.rx.lock().await.recv().await.map_err(From::from)
    }
}

impl From<broadcast::error::RecvError> for StatusesError {
    fn from(recv_error: broadcast::error::RecvError) -> Self {
        match recv_error {
            broadcast::error::RecvError::Closed => Self::NoMoreStatuses,
            broadcast::error::RecvError::Lagged(missed_status_count) => Self::MissedStatuses {
                missed_status_count,
            },
        }
    }
}
