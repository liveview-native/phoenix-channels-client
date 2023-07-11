use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use crate::ffi::json::{JSONDeserializationError, JSON};
use crate::rust;

/// This is a strongly typed wrapper around the event associated with a `Message`.
///
/// We discriminate between special Phoenix events and user-defined events, as they have slightly
/// different semantics. Generally speaking, Phoenix events are not exposed to users, and are not
/// permitted to be sent by them either.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum Event {
    /// Represents one of the built-in Phoenix channel events, e.g. join
    Phoenix {
        /// The built-in event name
        phoenix: PhoenixEvent,
    },
    /// Represents a user-defined event
    User {
        /// The user-defined event name
        user: String,
    },
}
impl Event {
    /// Creates an [Event] from a string and ensures that special Phoenix control events are turned
    /// into [Event::Phoenix] while others are [Event::User].
    pub fn from_string(name: String) -> Self {
        let rust_event: rust::message::Event = name.into();

        rust_event.into()
    }
}
impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Phoenix { phoenix } => write!(f, "{}", phoenix),
            Event::User { user } => f.write_str(user),
        }
    }
}
impl From<rust::message::Event> for Event {
    fn from(rust_event: rust::message::Event) -> Self {
        match rust_event {
            rust::message::Event::Phoenix(phoenix) => Self::Phoenix { phoenix },
            rust::message::Event::User(user) => Self::User { user },
        }
    }
}

/// Represents special events related to management of Phoenix channels.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum PhoenixEvent {
    /// Used when sending a message to join a channel
    Join = 0,
    /// Used when sending a message to leave a channel
    Leave,
    /// Sent/received when a channel is closed
    Close,
    /// Sent/received with replies
    Reply,
    /// Sent by the server when an error occurs
    Error,
    /// Sent/received as a keepalive for the underlying socket
    Heartbeat,
}
impl fmt::Display for PhoenixEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Join => f.write_str("phx_join"),
            Self::Leave => f.write_str("phx_leave"),
            Self::Close => f.write_str("phx_close"),
            Self::Reply => f.write_str("phx_reply"),
            Self::Error => f.write_str("phx_error"),
            Self::Heartbeat => f.write_str("heartbeat"),
        }
    }
}
impl FromStr for PhoenixEvent {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "phx_join" => Ok(Self::Join),
            "phx_leave" => Ok(Self::Leave),
            "phx_close" => Ok(Self::Close),
            "phx_error" => Ok(Self::Error),
            "phx_reply" => Ok(Self::Reply),
            "heartbeat" => Ok(Self::Heartbeat),
            _ => Err(()),
        }
    }
}

/// Contains the response payload sent to/received from Phoenix
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum Payload {
    /// A JSON payload
    JSON {
        /// The JSON payload
        json: JSON,
    },
    /// A binary payload
    Binary {
        /// The bytes of the binary payload.
        bytes: Vec<u8>,
    },
}
impl Payload {
    /// Deserializes JSON serialized to a string to [Payload::JSON] or errors with a
    /// [JSONDeserializationError] if the string is invalid JSON.
    pub fn json_from_serialized(serialized_json: String) -> Result<Self, JSONDeserializationError> {
        JSON::deserialize(serialized_json).map(|json| Self::JSON { json })
    }

    /// Stores bytes in [Payload::Binary].
    pub fn binary_from_bytes(bytes: Vec<u8>) -> Self {
        Self::Binary { bytes }
    }
}
impl Display for Payload {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::JSON { json } => write!(f, "{}", json),
            Self::Binary { bytes } => {
                f.write_str("<<")?;
                let mut first = true;

                for byte in bytes {
                    if first {
                        first = false;
                    } else {
                        f.write_str(", ")?;
                    }

                    write!(f, "{:#04x}", byte)?;
                }

                f.write_str(">>")
            }
        }
    }
}
impl From<rust::message::Payload> for Payload {
    fn from(rust_payload: rust::message::Payload) -> Self {
        match rust_payload {
            rust::message::Payload::Value(value) => Payload::JSON {
                json: value.as_ref().into(),
            },
            rust::message::Payload::Binary(bytes) => Payload::Binary {
                bytes: bytes.to_vec(),
            },
        }
    }
}
impl From<&rust::message::Payload> for Payload {
    fn from(rust_payload_ref: &rust::message::Payload) -> Self {
        match rust_payload_ref {
            rust::message::Payload::Value(value_ref) => Payload::JSON {
                json: value_ref.as_ref().into(),
            },
            rust::message::Payload::Binary(bytes_ref) => Payload::Binary {
                bytes: bytes_ref.to_vec(),
            },
        }
    }
}
