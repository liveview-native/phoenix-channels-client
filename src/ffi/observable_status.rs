/// Wraps [tokio::sync::broadcast::error::RecvError] to add `uniffi` support and names specific to
/// [Statuses](crate::rust::observable_status::Statuses).
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum StatusesError {
    #[error("No more statuses left")]
    NoMoreStatuses,
    #[error("Missed {missed_status_count} Statuses; jumping to next Status")]
    MissedStatuses { missed_status_count: u64 },
}
