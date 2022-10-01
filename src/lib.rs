#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "nightly", feature(slice_take))]

mod channel;
mod client;
mod message;

pub use self::channel::*;
pub use self::client::{Client, ClientError, Config};
pub use self::message::{Event, Payload, PhoenixEvent};
pub use serde_json::Value;
