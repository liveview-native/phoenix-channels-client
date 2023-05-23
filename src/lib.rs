#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "nightly", feature(slice_take))]
#![feature(async_closure)]

mod channel;
mod join_reference;
mod message;
mod reference;
mod socket;
mod topic;

pub use serde_json::Value;

pub use self::channel::*;
pub use self::message::{Event, Payload, PhoenixEvent};
pub use self::socket::{ConnectError, Socket, SocketError};
