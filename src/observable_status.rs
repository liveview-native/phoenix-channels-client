use std::sync::Arc;

use arc_swap::ArcSwap;
use tokio::sync::broadcast;

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

    pub fn subscribe(&self) -> broadcast::Receiver<Result<S, E>> {
        self.tx.subscribe()
    }
}
