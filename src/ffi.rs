//! The FFI  API as opposed to the [Rust](crate::rust) API.
//!
//! [uniffi] should only be used in code under this namespace.

pub mod channel;
mod http;
pub mod io;
pub mod json;
pub mod message;
pub mod observable_status;
pub mod socket;
pub mod topic;
pub mod web_socket;

use std::ops::{Add, Sub};
use std::time::SystemTime;

use tokio::time::error::Elapsed;
use tokio::time::Instant;
use url::Url;

use crate::ffi::observable_status::StatusesError;

/// All errors that can be produced by this library.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PhoenixError {
    /// Timeout elapsed
    #[error("timeout elapsed")]
    Elapsed,
    /// Error when parsing URL
    #[error(transparent)]
    URLParse {
        /// Error parsing URL
        #[from]
        url_parse: URLParseError,
    },
    /// An error from any function on [Socket](crate::Socket).
    #[error(transparent)]
    Socket {
        /// Error from any function on [Socket](crate::Socket).
        #[from]
        socket: socket::SocketError,
    },
    /// An error from any function on [Channel](crate::Channel).
    #[error(transparent)]
    Channel {
        /// An error from any function on [Channel](crate::Channel).
        #[from]
        channel: channel::ChannelError,
    },
    /// An error from calling `status` on [Channel::statuses](crate::Channel::statuses) or
    /// [Socket::statuses](crate::Socket::statuses).
    #[error(transparent)]
    Statuses {
        /// An error from calling `status` on [Channel::statuses](crate::Channel::statuses) or
        /// [Socket::statuses](crate::Socket::statuses).
        #[from]
        statuses: StatusesError,
    },
}
impl From<Elapsed> for PhoenixError {
    fn from(_: Elapsed) -> Self {
        Self::Elapsed
    }
}
impl From<url::ParseError> for PhoenixError {
    fn from(url_parse_error: url::ParseError) -> Self {
        Self::URLParse {
            url_parse: url_parse_error.into(),
        }
    }
}
/// A uniffi supported wrapper around [uniffi::Error].
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum URLParseError {
    /// Empty host
    #[error("empty host")]
    EmptyHost,
    /// Invalid international domain
    #[error("invalid international domain name")]
    IdnaError,
    /// Invalid port number
    #[error("invalid port number")]
    InvalidPort,
    /// Invalid ipv4 address.
    #[error("invalid IPv4 address")]
    InvalidIpv4Address,
    /// Invalid ipv6 address.
    #[error("invalid IPv6 address")]
    InvalidIpv6Address,
    /// Invalid domain character
    #[error("invalid domain character")]
    InvalidDomainCharacter,
    /// Relative URL without a base
    #[error("relative URL without a base")]
    RelativeUrlWithoutBase,
    /// Relative URL with a cannot-be-a-base base"
    #[error("relative URL with a cannot-be-a-base base")]
    RelativeUrlWithCannotBeABaseBase,
    /// A cannot-be-a-base URL doesn’t have a host to set
    #[error("a cannot-be-a-base URL doesn’t have a host to set")]
    SetHostOnCannotBeABaseUrl,
    /// URLs more than 4 GB are not supported
    #[error("URLs more than 4 GB are not supported")]
    Overflow,
}
impl From<url::ParseError> for URLParseError {
    fn from(url_parse_error: url::ParseError) -> Self {
        match url_parse_error {
            url::ParseError::EmptyHost => Self::EmptyHost,
            url::ParseError::IdnaError => Self::IdnaError,
            url::ParseError::InvalidPort => Self::InvalidPort,
            url::ParseError::InvalidIpv4Address => Self::InvalidIpv4Address,
            url::ParseError::InvalidIpv6Address => Self::InvalidIpv6Address,
            url::ParseError::InvalidDomainCharacter => Self::InvalidDomainCharacter,
            url::ParseError::RelativeUrlWithoutBase => Self::RelativeUrlWithoutBase,
            url::ParseError::RelativeUrlWithCannotBeABaseBase => {
                Self::RelativeUrlWithCannotBeABaseBase
            }
            url::ParseError::SetHostOnCannotBeABaseUrl => Self::SetHostOnCannotBeABaseUrl,
            url::ParseError::Overflow => Self::Overflow,
            other => panic!("Unexpected url::ParseError: {:?}", other),
        }
    }
}

fn instant_to_system_time(instant: Instant) -> SystemTime {
    let instant_now = Instant::now();
    let system_time_now = SystemTime::now();

    if instant < instant_now {
        let duration_since = instant_now.duration_since(instant);
        system_time_now.sub(duration_since)
    } else {
        let duration_until = instant.duration_since(instant_now);
        system_time_now.add(duration_until)
    }
}

macro_rules! from_for_error {
    ($t:path, $v:tt, $f:tt) => {
        impl From<$t> for PhoenixError {
            fn from(error: $t) -> Self {
                Self::$v { $f: error.into() }
            }
        }
    };
}
from_for_error!(socket::SpawnError, Socket, socket);
from_for_error!(socket::ConnectError, Socket, socket);
from_for_error!(socket::SocketChannelError, Socket, socket);
from_for_error!(socket::DisconnectError, Socket, socket);
from_for_error!(socket::SocketShutdownError, Socket, socket);
from_for_error!(channel::ChannelJoinError, Channel, channel);
from_for_error!(channel::CallError, Channel, channel);
from_for_error!(channel::CastError, Channel, channel);
from_for_error!(channel::LeaveError, Channel, channel);
from_for_error!(channel::ChannelShutdownError, Channel, channel);

use crate::UniffiCustomTypeConverter;

uniffi::custom_type!(Url, String);
impl UniffiCustomTypeConverter for Url {
    type Builtin = String;

    fn into_custom(string: Self::Builtin) -> uniffi::Result<Self> {
        Url::parse(&string).map_err(From::from)
    }

    fn from_custom(url: Self) -> Self::Builtin {
        url.to_string()
    }
}
