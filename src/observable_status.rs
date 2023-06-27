use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;

#[derive(Clone)]
pub(crate) struct ObservableStatus<S: Copy + Into<usize>, E: Clone> {
    status: Arc<AtomicUsize>,
    tx: broadcast::Sender<Result<S, E>>,
}
impl<S: Copy + Into<usize>, E: Clone> ObservableStatus<S, E> {
    pub fn new(status: S) -> Self {
        let (tx, _) = broadcast::channel(5);

        Self {
            status: Arc::new(AtomicUsize::new(status.into())),
            tx,
        }
    }

    pub fn get(&self) -> S {
        unsafe { core::mem::transmute_copy::<usize, S>(&self.status.load(Ordering::Acquire)) }
    }

    pub fn set(&self, status: S) {
        let status_usize = status.into();

        if self.status.swap(status_usize, Ordering::AcqRel) != status_usize {
            self.tx.send(Ok(status)).ok();
        }
    }

    pub fn error(&self, error: E) {
        self.tx.send(Err(error)).ok();
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Result<S, E>> {
        self.tx.subscribe()
    }
}
