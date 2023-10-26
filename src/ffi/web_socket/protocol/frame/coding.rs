use std::fmt;
use std::fmt::Display;

use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode as TungsteniteCloseCode;
use tokio_tungstenite::tungstenite::protocol::frame::coding::Control as TungsteniteControl;
use tokio_tungstenite::tungstenite::protocol::frame::coding::Data as TungsteniteData;
use tokio_tungstenite::tungstenite::protocol::frame::coding::OpCode as TungsteniteOpCode;

/// [tokio_tungstenite::tungstenite::protocol::frame::coding::OpCode], but with `uniffi` support
/// WebSocket message opcode as in RFC 6455.
#[derive(Debug, PartialEq, Eq, Clone, Copy, uniffi::Enum)]
pub enum OpCode {
    /// Data (text or binary).
    Data { data: Data },
    /// Control message (close, ping, pong).
    Control { control: Control },
}
impl From<&TungsteniteOpCode> for OpCode {
    fn from(rust_opcode: &TungsteniteOpCode) -> Self {
        match rust_opcode {
            TungsteniteOpCode::Data(data) => Self::Data { data: data.into() },
            TungsteniteOpCode::Control(control) => Self::Control {
                control: control.into(),
            },
        }
    }
}

/// [tokio_tungstenite::tungstenite::protocol::frame::coding::Data], but with `uniffi` support.
/// Data opcodes as in RFC 6455
#[derive(Debug, PartialEq, Eq, Clone, Copy, uniffi::Enum)]
pub enum Data {
    /// 0x0 denotes a continuation frame
    Continue,
    /// 0x1 denotes a text frame
    Text,
    /// 0x2 denotes a binary frame
    Binary,
    /// 0x3-7 are reserved for further non-control frames
    Reserved { bits: u8 },
}
impl Display for Data {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Data::Continue => write!(f, "CONTINUE"),
            Data::Text => write!(f, "TEXT"),
            Data::Binary => write!(f, "BINARY"),
            Data::Reserved { bits } => write!(f, "RESERVED_DATA_{}", bits),
        }
    }
}
impl From<&TungsteniteData> for Data {
    fn from(rust_data: &TungsteniteData) -> Self {
        match rust_data {
            TungsteniteData::Continue => Self::Continue,
            TungsteniteData::Text => Self::Text,
            TungsteniteData::Binary => Self::Binary,
            TungsteniteData::Reserved(bits) => Self::Reserved { bits: *bits },
        }
    }
}

/// [tokio_tungstenite::tungstenite::protocol::frame::coding::Control], but with `uniffi` support.
/// Control opcodes as in RFC 6455
#[derive(Debug, PartialEq, Eq, Clone, Copy, uniffi::Enum)]
pub enum Control {
    /// 0x8 denotes a connection close
    Close,
    /// 0x9 denotes a ping
    Ping,
    /// 0xa denotes a pong
    Pong,
    /// 0xb-f are reserved for further control frames
    Reserved { bit: u8 },
}
impl From<&TungsteniteControl> for Control {
    fn from(rust_control: &TungsteniteControl) -> Self {
        match rust_control {
            TungsteniteControl::Close => Self::Close,
            TungsteniteControl::Ping => Self::Ping,
            TungsteniteControl::Pong => Self::Pong,
            TungsteniteControl::Reserved(bit) => Self::Reserved { bit: *bit },
        }
    }
}

/// [tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode], but with `uniffi::support`
/// Status code used to indicate why an endpoint is closing the WebSocket connection.
#[derive(Debug, Eq, PartialEq, Clone, Copy, uniffi::Enum)]
pub enum CloseCode {
    /// Indicates a normal closure, meaning that the purpose for
    /// which the connection was established has been fulfilled.
    Normal,
    /// Indicates that an endpoint is "going away", such as a server
    /// going down or a browser having navigated away from a page.
    Away,
    /// Indicates that an endpoint is terminating the connection due
    /// to a protocol error.
    Protocol,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a type of data it cannot accept (e.g., an
    /// endpoint that understands only text data MAY send this if it
    /// receives a binary message).
    Unsupported,
    /// Indicates that no status code was included in a closing frame. This
    /// close code makes it possible to use a single method, `on_close` to
    /// handle even cases where no close code was provided.
    Status,
    /// Indicates an abnormal closure. If the abnormal closure was due to an
    /// error, this close code will not be used. Instead, the `on_error` method
    /// of the handler will be called with the error. However, if the connection
    /// is simply dropped, without an error, this close code will be sent to the
    /// handler.
    Abnormal,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received data within a message that was not
    /// consistent with the type of the message (e.g., non-UTF-8 \[RFC3629\]
    /// data within a text message).
    Invalid,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that violates its policy.  This
    /// is a generic status code that can be returned when there is no
    /// other more suitable status code (e.g., Unsupported or Size) or if there
    /// is a need to hide specific details about the policy.
    Policy,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that is too big for it to
    /// process.
    Size,
    /// Indicates that an endpoint (client) is terminating the
    /// connection because it has expected the server to negotiate one or
    /// more extension, but the server didn't return them in the response
    /// message of the WebSocket handshake.  The list of extensions that
    /// are needed should be given as the reason for closing.
    /// Note that this status code is not used by the server, because it
    /// can fail the WebSocket handshake instead.
    Extension,
    /// Indicates that a server is terminating the connection because
    /// it encountered an unexpected condition that prevented it from
    /// fulfilling the request.
    Error,
    /// Indicates that the server is restarting. A client may choose to reconnect,
    /// and if it does, it should use a randomized delay of 5-30 seconds between attempts.
    Restart,
    /// Indicates that the server is overloaded and the client should either connect
    /// to a different IP (when multiple targets exist), or reconnect to the same IP
    /// when a user has performed an action.
    Again,
    #[doc(hidden)]
    Tls,
    #[doc(hidden)]
    Reserved { code: u16 },
    #[doc(hidden)]
    Iana { code: u16 },
    #[doc(hidden)]
    Library { code: u16 },
    #[doc(hidden)]
    Bad { code: u16 },
}
impl From<&TungsteniteCloseCode> for CloseCode {
    fn from(rust_close_code: &TungsteniteCloseCode) -> Self {
        match rust_close_code {
            TungsteniteCloseCode::Normal => Self::Normal,
            TungsteniteCloseCode::Away => Self::Away,
            TungsteniteCloseCode::Protocol => Self::Protocol,
            TungsteniteCloseCode::Unsupported => Self::Unsupported,
            TungsteniteCloseCode::Status => Self::Status,
            TungsteniteCloseCode::Abnormal => Self::Abnormal,
            TungsteniteCloseCode::Invalid => Self::Invalid,
            TungsteniteCloseCode::Policy => Self::Policy,
            TungsteniteCloseCode::Size => Self::Size,
            TungsteniteCloseCode::Extension => Self::Extension,
            TungsteniteCloseCode::Error => Self::Error,
            TungsteniteCloseCode::Restart => Self::Restart,
            TungsteniteCloseCode::Again => Self::Again,
            TungsteniteCloseCode::Tls => Self::Tls,
            TungsteniteCloseCode::Reserved(code) => Self::Reserved { code: *code },
            TungsteniteCloseCode::Iana(code) => Self::Iana { code: *code },
            TungsteniteCloseCode::Library(code) => Self::Library { code: *code },
            TungsteniteCloseCode::Bad(code) => Self::Bad { code: *code },
        }
    }
}
