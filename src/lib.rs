#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "nightly", feature(slice_take))]
#![feature(async_closure)]

pub mod channel;
mod join_reference;
mod message;
mod observable_status;
mod reference;
pub mod socket;
mod topic;

pub use serde_json::Value;
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::time::error::Elapsed;

pub use self::channel::*;
pub use self::message::{Event, Payload, PhoenixEvent};
pub use self::socket::{ConnectError, Socket, SpawnError};

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Elapsed(#[from] Elapsed),
    #[error(transparent)]
    Broadcast(#[from] broadcast::error::RecvError),
    #[error(transparent)]
    Socket(#[from] socket::Error),
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
