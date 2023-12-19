use crate::ffi::channel::statuses::ChannelStatusJoinError;
use crate::ffi::web_socket::error::WebSocketError;

/// Wraps [tokio::sync::broadcast::error::RecvError] to add `uniffi` support and names specific to
/// [ChannelStatuses](crate::ChannelStatuses).
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum StatusesError {
    /// No more statuses
    #[error("No more statuses left")]
    NoMoreStatuses,
    /// Missed statuses, jump to the next status
    #[error("Missed {missed_status_count} Statuses; jumping to next Status")]
    MissedStatuses {
        /// Number of missed statuses.
        missed_status_count: u64
    },
    /// Failed to join a Channel (due to rejection).
    #[error("Join Error {join_error}")]
    ChannelStatusJoin {
        /// The join Error
        #[from]
        join_error: ChannelStatusJoinError,
    },
    /// An error with WebSockets
    #[error("Websocket Error: {websocket_error}")]
    WebSocket {
        /// The websocket error
        #[from]
        websocket_error: WebSocketError
    },
}
