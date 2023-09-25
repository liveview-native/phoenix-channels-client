#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "nightly", feature(slice_take))]
#![feature(async_closure)]
// doc warnings that aren't on by default
#![warn(missing_docs)]
#![warn(rustdoc::unescaped_backticks)]

pub mod channel;
mod join_reference;
mod message;
mod observable_status;
mod reference;
pub mod socket;
mod topic;

pub use serde_json::Value;
use strum_macros::Display;
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::time::error::Elapsed;

pub use self::channel::*;
pub use self::message::{Event, Payload, PhoenixEvent};
pub use self::socket::{ConnectError, Socket, SpawnError};

/// All errors that can be produced by this library.
#[derive(Error, Display, Debug)]
pub enum Error {
    /// Timeout expired
    #[error(transparent)]
    Elapsed(#[from] Elapsed),
    /// Errors listening for [Socket::status] or [Channel::status].
    #[error(transparent)]
    Broadcast(#[from] broadcast::error::RecvError),
    /// Error when parsing URL
    URLParse(#[from] url::ParseError),
    /// An error from any function on [Socket].
    #[error(transparent)]
    Socket(#[from] socket::Error),
    /// An error from any function on [Channel].
    #[error(transparent)]
    Channel(#[from] channel::Error),
}

macro_rules! from_for_error {
    ($t:path, $v:tt) => {
        impl From<$t> for Error {
            fn from(error: $t) -> Self {
                Self::$v(error.into())
            }
        }
    };
}
from_for_error!(socket::SpawnError, Socket);
from_for_error!(socket::ConnectError, Socket);
from_for_error!(socket::ChannelError, Socket);
from_for_error!(socket::DisconnectError, Socket);
from_for_error!(socket::ShutdownError, Socket);
from_for_error!(channel::JoinError, Channel);
from_for_error!(channel::CallError, Channel);
from_for_error!(channel::CastError, Channel);
from_for_error!(channel::LeaveError, Channel);
from_for_error!(channel::ShutdownError, Channel);
