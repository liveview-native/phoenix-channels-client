use std::sync::Arc;

use crate::ffi::message::Payload;
use crate::ffi::observable_status::StatusesError;
use crate::rust;
use crate::rust::observable_status;
use crate::ChannelStatus;

/// Waits for [ChannelStatus] changes from the [Channel](crate::Channel).
// Can't be generic because `uniffi` does not support generics
#[cfg_attr(
    feature = "uniffi",
    derive(uniffi::Object)
)]
pub struct ChannelStatuses(
    observable_status::Statuses<rust::channel::Status, Arc<rust::message::Payload>>,
);
/*
 *TODO: Nested results do not work when running uniffi-bindgen on build library.
#[cfg_attr(
    feature = "uniffi",
    uniffi::export,
)]
*/
impl ChannelStatuses {
    /// Wait for next [ChannelStatus] when the [Channel::status](super::Channel::status) changes.
    pub async fn status(
        &self,
    ) -> Result<Result<ChannelStatus, ChannelStatusJoinError>, StatusesError> {
        self.0
            .status()
            .await
            .map(|result| result.map(From::from).map_err(From::from))
    }
}
impl From<observable_status::Statuses<rust::channel::Status, Arc<rust::message::Payload>>>
    for ChannelStatuses
{
    fn from(
        inner: observable_status::Statuses<rust::channel::Status, Arc<rust::message::Payload>>,
    ) -> Self {
        Self(inner)
    }
}

/// Errors when calling [Channel::join](super::Channel::join).
#[derive(Clone, Debug, thiserror::Error)]
#[cfg_attr(
    feature = "uniffi",
    derive(uniffi::Error)
)]
pub enum ChannelStatusJoinError {
    /// The [Channel::payload](super::Channel::payload) was rejected when attempting to
    /// [Channel::join](super::Channel::join) or automatically rejoin
    /// [Channel::topic](super::Channel::topic).
    #[error("server rejected join")]
    Rejected {
        /// Error response from the serrve.
        response: Payload,
    },
}
impl From<Arc<rust::message::Payload>> for ChannelStatusJoinError {
    fn from(rust_payload: Arc<rust::message::Payload>) -> Self {
        Self::Rejected {
            response: rust_payload.as_ref().into(),
        }
    }
}
