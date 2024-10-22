#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "nightly", feature(slice_take))]
// doc warnings that aren't on by default
#![warn(missing_docs)]
#![warn(rustdoc::unescaped_backticks)]

mod ffi;
mod rust;

// All types should be at the root as `uniffi` only exposes one namespace to foreign code
pub use ffi::channel::statuses::{ChannelStatusJoinError, ChannelStatuses};
pub use ffi::channel::{
    CallError, Channel, ChannelError, ChannelJoinError, ChannelStatus, EventPayload, Events,
    EventsError, LeaveError,
};
pub use ffi::io::error::IoError;
pub use ffi::json::{JSONDeserializationError, Number, JSON};
pub use ffi::message::{Event, Payload, PhoenixEvent};
pub use ffi::observable_status::StatusesError;
pub use ffi::presence::Presence;
pub use ffi::presences::{
    Presences, PresencesJoin, PresencesJoins, PresencesLeave, PresencesLeaves,
};
pub use ffi::socket::{
    ConnectError, Socket, SocketChannelError, SocketError, SocketStatus, SocketStatuses, SpawnError,
};
pub use ffi::topic::Topic;
pub use ffi::web_socket::error::WebSocketError;
pub use ffi::web_socket::protocol::WebSocketMessage;
pub use ffi::{PhoenixError, URLParseError};
pub use url;

uniffi::setup_scaffolding!("phoenix_channels_client");
