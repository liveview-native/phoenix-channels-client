#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "nightly", feature(slice_take))]
// doc warnings that aren't on by default
//#![warn(missing_docs)]
//#![warn(rustdoc::unescaped_backticks)]

mod ffi;
mod rust;

#[cfg(feature = "browser")]
mod wasm_helpers;

cfg_if::cfg_if! {
    if #[cfg(not(all(target_family = "wasm", target_os = "unknown")))] {
        pub (crate) type Instant = tokio::time::Instant;
    } else {
        pub (crate) type Instant = tokio::time::Instant;
    }
}

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
pub use ffi::socket::{
    ConnectError, Socket, SocketChannelError, SocketError, SocketStatus, SocketStatuses, SpawnError,
};
pub use ffi::topic::Topic;
pub use ffi::web_socket::error::WebSocketError;
pub use ffi::web_socket::protocol::WebSocketMessage;
pub use ffi::{PhoenixError, URLParseError};
pub use url;
uniffi::setup_scaffolding!("phoenix_channels_client");
