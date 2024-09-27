use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;

use log::{debug, error};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::ffi::channel::Channel;
use crate::rust::message::{Event, EventPayload, Payload};
use crate::rust::presence::{Meta, Presence};
use crate::rust::presences::{Join, Leave};

pub(super) struct Listener {
    channel: Arc<Channel>,
    shutdown_rx: oneshot::Receiver<()>,
    list_rx: mpsc::Receiver<oneshot::Sender<Vec<Presence>>>,
    sync_tx: broadcast::Sender<()>,
    join_tx: broadcast::Sender<Arc<Join>>,
    leave_tx: broadcast::Sender<Arc<Leave>>,
    presence_by_key: PresenceByKey,
}
impl Listener {
    pub(super) fn spawn(
        channel: Arc<Channel>,
        shutdown_rx: oneshot::Receiver<()>,
        list_rx: mpsc::Receiver<oneshot::Sender<Vec<Presence>>>,
        sync_tx: broadcast::Sender<()>,
        join_tx: broadcast::Sender<Arc<Join>>,
        leave_tx: broadcast::Sender<Arc<Leave>>,
    ) -> JoinHandle<()> {
        let listener = Self::init(channel, shutdown_rx, list_rx, sync_tx, join_tx, leave_tx);

        tokio::spawn(listener.listen())
    }

    fn init(
        channel: Arc<Channel>,
        shutdown_rx: oneshot::Receiver<()>,
        list_rx: mpsc::Receiver<oneshot::Sender<Vec<Presence>>>,
        sync_tx: broadcast::Sender<()>,
        join_tx: broadcast::Sender<Arc<Join>>,
        leave_tx: broadcast::Sender<Arc<Leave>>,
    ) -> Self {
        Self {
            channel,
            shutdown_rx,
            list_rx,
            sync_tx,
            join_tx,
            leave_tx,
            presence_by_key: Default::default(),
        }
    }

    async fn listen(mut self) {
        debug!("Presence started for channel {}", self.channel);

        let mut events = self.channel.event_payload_tx.subscribe();

        loop {
            tokio::select! {
                _ = &mut self.shutdown_rx => break,
                option = self.list_rx.recv() => match option {
                    Some(list_tx) => {
                        list_tx.send(self.presence_by_key.0.values().map(ToOwned::to_owned).collect()).ok();
                    }
                    None => break,
                },
                result = events.recv() => match result {
                    Ok(EventPayload { event, payload }) => match event {
                        Event::Phoenix(_) => continue,
                        Event::User(user_event) => match user_event.as_str() {
                            "presence_state" => self.presence_state(payload),
                            "presence_diff" => self.presence_diff(payload),
                            _ => continue
                        }
                    },
                    Err(RecvError::Closed) => break,
                    Err(RecvError::Lagged(_)) => continue
                }
            }
        }
    }

    fn presence_state(&mut self, new_state: Payload) {
        debug!("{} received presence_state {:?}", self.channel, new_state);

        match new_state.try_into() {
            Ok(new_presence_by_key) => {
                debug!("{} received presence_state that will be synced as {:?} with current presence_by_key {:?}", self.channel, new_presence_by_key, self.presence_by_key);

                self.presence_by_key
                    .sync(new_presence_by_key, &self.join_tx, &self.leave_tx);

                self.sync_tx.send(()).ok();
            }
            Err(error) => error!(
                "Could not convert new presence state into PresenceByKey: {:?}",
                error
            ),
        }
    }

    fn presence_diff(&mut self, diff_payload: Payload) {
        debug!("{} receive presence_diff {:?}", self.channel, diff_payload);
        let diff_result: Result<Diff, _> = diff_payload.try_into();

        match diff_result {
            Ok(diff) => {
                debug!(
                    "{} received presence_diff {:?} that will be merged with current presence_by_key",
                    self.channel, diff
                );

                self.presence_by_key.sync_diff(
                    diff.joins,
                    diff.leaves,
                    &self.join_tx,
                    &self.leave_tx,
                );

                self.sync_tx.send(()).ok();
            }
            Err(error) => error!("Could not convert presence diff into Diff: {:?}", error),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct PresenceByKey(HashMap<String, Presence>);
impl PresenceByKey {
    fn sync(
        &mut self,
        new: PresenceByKey,
        join_tx: &broadcast::Sender<Arc<Join>>,
        leave_tx: &broadcast::Sender<Arc<Leave>>,
    ) {
        let mut left_presence_by_key: HashMap<String, Presence> = self
            .0
            .extract_if(|key, _presence| !new.0.contains_key(key))
            .collect();
        let mut joined_presence_by_key: HashMap<String, Presence> = Default::default();

        for (new_key, mut new_presence) in new.0 {
            match self.0.get(&new_key) {
                Some(current_presence) => {
                    let new_join_references = new_presence.join_references();
                    let current_join_references = current_presence.join_references();
                    let joined_metas: Vec<Meta> = new_presence
                        .metas
                        .iter()
                        .filter(|meta| !current_join_references.contains(&meta.join_reference))
                        .map(ToOwned::to_owned)
                        .collect();
                    let left_metas: Vec<Meta> = current_presence
                        .metas
                        .iter()
                        .filter(|meta| !new_join_references.contains(&meta.join_reference))
                        .map(ToOwned::to_owned)
                        .collect();

                    if !joined_metas.is_empty() {
                        new_presence.metas = joined_metas;
                        joined_presence_by_key.insert(new_key.clone(), new_presence);
                    }

                    if !left_metas.is_empty() {
                        let mut left_presence = current_presence.clone();
                        left_presence.metas = left_metas;
                        left_presence_by_key.insert(new_key, left_presence);
                    }
                }
                None => {
                    joined_presence_by_key.insert(new_key, new_presence);
                }
            };
        }

        self.sync_diff(
            joined_presence_by_key,
            left_presence_by_key,
            join_tx,
            leave_tx,
        )
    }

    fn sync_diff(
        &mut self,
        joined_presence_by_key: HashMap<String, Presence>,
        left_presence_by_key: HashMap<String, Presence>,
        join_tx: &broadcast::Sender<Arc<Join>>,
        leave_tx: &broadcast::Sender<Arc<Leave>>,
    ) {
        for (key, joined_presence) in joined_presence_by_key {
            let current_presence = self.0.get(&key);
            let mut merged_presence = joined_presence.clone();

            if let Some(current_presence) = current_presence {
                let joined_references = merged_presence.join_references();
                let mut current_metas: Vec<Meta> = current_presence
                    .metas
                    .iter()
                    .filter(|meta| !joined_references.contains(&meta.join_reference))
                    .map(ToOwned::to_owned)
                    .collect();
                merged_presence.metas.append(&mut current_metas);
            }

            let current = current_presence.map(ToOwned::to_owned);
            self.0.insert(key.clone(), merged_presence.clone());

            join_tx
                .send(Arc::new(Join {
                    key,
                    current,
                    joined: merged_presence,
                }))
                .ok();
        }

        for (key, left_presence) in left_presence_by_key {
            if let Entry::Occupied(mut current_presence_entry) = self.0.entry(key.clone()) {
                let current_presence = current_presence_entry.get_mut();
                let join_references_to_remove = left_presence.join_references();
                current_presence
                    .metas
                    .retain(|meta| !join_references_to_remove.contains(&meta.join_reference));

                leave_tx
                    .send(Arc::new(Leave {
                        key,
                        current: current_presence.to_owned(),
                        left: left_presence,
                    }))
                    .ok();

                if current_presence.metas.is_empty() {
                    current_presence_entry.remove_entry();
                }
            }
        }
    }
}
impl TryFrom<Payload> for PresenceByKey {
    type Error = TryFromPayloadError;

    fn try_from(payload: Payload) -> Result<Self, Self::Error> {
        match payload {
            Payload::Value(value) => {
                serde_json::from_value(Value::clone(&value)).map_err(TryFromPayloadError::Json)
            }
            Payload::Binary(_) => Err(TryFromPayloadError::Binary),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Diff {
    joins: HashMap<String, Presence>,
    leaves: HashMap<String, Presence>,
}
impl TryFrom<Payload> for Diff {
    type Error = TryFromPayloadError;

    fn try_from(payload: Payload) -> Result<Self, Self::Error> {
        match payload {
            Payload::Value(value) => {
                serde_json::from_value(Value::clone(&value)).map_err(TryFromPayloadError::Json)
            }
            Payload::Binary(_) => Err(TryFromPayloadError::Binary),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum TryFromPayloadError {
    #[error("Binary payload instead of JSON")]
    Binary,
    #[error("JSON payload in incorrect format: {0}")]
    Json(#[from] serde_json::Error),
}
