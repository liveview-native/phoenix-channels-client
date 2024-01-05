use std::fmt;
use std::fmt::Display;

use tokio_tungstenite::tungstenite::protocol::frame::coding as tungstenite_coding;

/// [tokio_tungstenite::tungstenite::protocol::frame::coding::OpCode], but with `uniffi` support
/// WebSocket message opcode as in RFC 6455.
#[derive(Debug, PartialEq, Eq, Clone, Copy, uniffi::Enum)]
pub enum TungsteniteOpCode {
    /// Data (text or binary).
    Data { data: TungsteniteData },
    /// Control message (close, ping, pong).
    Control { control: TungsteniteControl },
}
impl From<&tungstenite_coding::OpCode> for TungsteniteOpCode {
    fn from(rust_opcode: &tungstenite_coding::OpCode) -> Self {
        match rust_opcode {
            tungstenite_coding::OpCode::Data(data) => Self::Data { data: data.into() },
            tungstenite_coding::OpCode::Control(control) => Self::Control {
                control: control.into(),
            },
        }
    }
}

/// [tokio_tungstenite::tungstenite::protocol::frame::coding::Data], but with `uniffi` support.
/// Data opcodes as in RFC 6455
#[derive(Debug, PartialEq, Eq, Clone, Copy, uniffi::Enum)]
pub enum TungsteniteData {
    /// 0x0 denotes a continuation frame
    Continue,
    /// 0x1 denotes a text frame
    Text,
    /// 0x2 denotes a binary frame
    Binary,
    /// 0x3-7 are reserved for further non-control frames
    Reserved { bits: u8 },
}
impl Display for TungsteniteData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TungsteniteData::Continue => write!(f, "CONTINUE"),
            TungsteniteData::Text => write!(f, "TEXT"),
            TungsteniteData::Binary => write!(f, "BINARY"),
            TungsteniteData::Reserved { bits } => write!(f, "RESERVED_DATA_{}", bits),
        }
    }
}
impl From<&tungstenite_coding::Data> for TungsteniteData {
    fn from(rust_data: &tungstenite_coding::Data) -> Self {
        match rust_data {
            tungstenite_coding::Data::Continue => Self::Continue,
            tungstenite_coding::Data::Text => Self::Text,
            tungstenite_coding::Data::Binary => Self::Binary,
            tungstenite_coding::Data::Reserved(bits) => Self::Reserved { bits: *bits },
        }
    }
}

/// [tokio_tungstenite::tungstenite::protocol::frame::coding::Control], but with `uniffi` support.
/// Control opcodes as in RFC 6455
#[derive(Debug, PartialEq, Eq, Clone, Copy, uniffi::Enum)]
pub enum TungsteniteControl {
    /// 0x8 denotes a connection close
    Close,
    /// 0x9 denotes a ping
    Ping,
    /// 0xa denotes a pong
    Pong,
    /// 0xb-f are reserved for further control frames
    Reserved { bit: u8 },
}
impl From<&tungstenite_coding::Control> for TungsteniteControl {
    fn from(rust_control: &tungstenite_coding::Control) -> Self {
        match rust_control {
            tungstenite_coding::Control::Close => Self::Close,
            tungstenite_coding::Control::Ping => Self::Ping,
            tungstenite_coding::Control::Pong => Self::Pong,
            tungstenite_coding::Control::Reserved(bit) => Self::Reserved { bit: *bit },
        }
    }
}

/// [tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode], but with `uniffi::support`
/// Status code used to indicate why an endpoint is closing the WebSocket connection.
#[derive(Debug, Eq, PartialEq, Clone, Copy, uniffi::Enum)]
pub enum TungsteniteCloseCode {
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
    Tls,
    Reserved { code: u16 },
    Iana { code: u16 },
    Library { code: u16 },
    Bad { code: u16 },
}
impl From<&tungstenite_coding::CloseCode> for TungsteniteCloseCode {
    fn from(rust_close_code: &tungstenite_coding::CloseCode) -> Self {
        match rust_close_code {
            tungstenite_coding::CloseCode::Normal => Self::Normal,
            tungstenite_coding::CloseCode::Away => Self::Away,
            tungstenite_coding::CloseCode::Protocol => Self::Protocol,
            tungstenite_coding::CloseCode::Unsupported => Self::Unsupported,
            tungstenite_coding::CloseCode::Status => Self::Status,
            tungstenite_coding::CloseCode::Abnormal => Self::Abnormal,
            tungstenite_coding::CloseCode::Invalid => Self::Invalid,
            tungstenite_coding::CloseCode::Policy => Self::Policy,
            tungstenite_coding::CloseCode::Size => Self::Size,
            tungstenite_coding::CloseCode::Extension => Self::Extension,
            tungstenite_coding::CloseCode::Error => Self::Error,
            tungstenite_coding::CloseCode::Restart => Self::Restart,
            tungstenite_coding::CloseCode::Again => Self::Again,
            tungstenite_coding::CloseCode::Tls => Self::Tls,
            tungstenite_coding::CloseCode::Reserved(code) => Self::Reserved { code: *code },
            tungstenite_coding::CloseCode::Iana(code) => Self::Iana { code: *code },
            tungstenite_coding::CloseCode::Library(code) => Self::Library { code: *code },
            tungstenite_coding::CloseCode::Bad(code) => Self::Bad { code: *code },
        }
    }
}
