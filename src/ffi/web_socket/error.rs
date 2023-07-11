use tokio_tungstenite::tungstenite::error::{
    CapacityError as TungsteniteCapacityError, ProtocolError as TungsteniteProtocolError,
};
use tokio_tungstenite::tungstenite::Error as TungsteniteError;

use crate::ffi::http;
use crate::ffi::io;
use crate::ffi::web_socket::protocol::frame::coding::TungsteniteData;
use crate::ffi::web_socket::protocol::WebSocketMessage;

/// [tokio_tungstenite::tungstenite::error::Error], but with `uniffi` support
#[derive(Clone, Debug, thiserror::Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum WebSocketError {
    /// WebSocket connection closed normally. This informs you of the close.
    /// It's not an error as such and nothing wrong happened.
    ///
    /// This is returned as soon as the close handshake is finished (we have both sent and
    /// received a close frame) on the server end and as soon as the server has closed the
    /// underlying connection if this endpoint is a client.
    ///
    /// Thus when you receive this, it is safe to drop the underlying connection.
    ///
    /// Receiving this error means that the WebSocket object is not usable anymore and the
    /// only meaningful action with it is dropping it.
    #[error("Connection closed normally")]
    ConnectionClosed,
    /// Trying to work with already closed connection.
    ///
    /// Trying to read or write after receiving `ConnectionClosed` causes this.
    ///
    /// As opposed to `ConnectionClosed`, this indicates your code tries to operate on the
    /// connection when it really shouldn't anymore, so this really indicates a programmer
    /// error on your part.
    #[error("Trying to work with closed connection")]
    AlreadyClosed,
    /// Input-output error. Apart from WouldBlock, these are generally errors with the
    /// underlying connection and you should probably consider them fatal.
    #[error("IO error: {io_error}")]
    Io {
        /// Input-output error. Apart from WouldBlock, these are generally errors with the
        /// underlying connection and you should probably consider them fatal.
        io_error: io::error::IoError,
    },
    /// TLS error.
    #[error("TLS error: {tls_error}")]
    Tls {
        /// TLS error
        tls_error: String,
    },
    /// - When reading: buffer capacity exhausted.
    /// - When writing: your message is bigger than the configured max message size
    ///   (64MB by default).
    #[error("Space limit exceeded: {capacity_error}")]
    Capacity {
        /// When there are too many headers or the message is too long
        capacity_error: CapacityError,
    },
    /// Protocol violation.
    #[error("WebSocket protocol error: {protocol_error}")]
    Protocol {
        /// Protocol violation
        protocol_error: ProtocolError,
    },
    /// Message write buffer is full.
    #[error("Write buffer is full")]
    WriteBufferFull {
        /// The [WebSocketMessage] that could not fit in the write buffer.  It should be resent
        /// later or the error needs to propagate up.
        message: WebSocketMessage,
    },
    /// UTF coding error.
    #[error("UTF-8 encoding error")]
    Utf8,
    /// [CVE-2023-43669](https://nvd.nist.gov/vuln/detail/CVE-2023-43669) attack attempt
    #[error("CVE-2023-43669 attack attempt due to excessive headers from server")]
    AttackAttempt,
    /// Invalid URL.
    #[error("URL error: {url_error}")]
    Url {
        /// The URL error as a string
        url_error: String,
    },
    /// HTTP error.
    #[error("HTTP error: {}", response.status_code)]
    Http {
        /// Error response from the server.
        response: http::Response,
    },
    /// HTTP format error.
    #[error("HTTP format error: {error}")]
    HttpFormat {
        /// HTTP format error.
        error: http::HttpError,
    },
}
impl From<&TungsteniteError> for WebSocketError {
    fn from(tungstenite_error: &TungsteniteError) -> Self {
        match tungstenite_error {
            TungsteniteError::ConnectionClosed => Self::ConnectionClosed,
            TungsteniteError::AlreadyClosed => Self::AlreadyClosed,
            TungsteniteError::Io(io_error) => Self::Io {
                io_error: io_error.into(),
            },
            TungsteniteError::Tls(tls_error) => Self::Tls {
                tls_error: tls_error.to_string(),
            },
            TungsteniteError::Capacity(capacity_error) => Self::Capacity {
                capacity_error: capacity_error.into(),
            },
            TungsteniteError::Protocol(protocol_error) => Self::Protocol {
                protocol_error: protocol_error.into(),
            },
            TungsteniteError::WriteBufferFull(message) => Self::WriteBufferFull {
                message: message.into(),
            },
            TungsteniteError::Utf8 => Self::Utf8,
            TungsteniteError::AttackAttempt => Self::AttackAttempt,
            TungsteniteError::Url(url_error) => Self::Url {
                url_error: url_error.to_string(),
            },
            TungsteniteError::Http(response) => Self::Http {
                response: response.into(),
            },
            TungsteniteError::HttpFormat(error) => Self::HttpFormat {
                error: error.into(),
            },
        }
    }
}

/// [tungstenite::error::CapacityError], but with `uniffi` support.
/// Indicates the specific type/cause of a capacity error.
#[derive(Debug, PartialEq, Eq, Clone, Copy, thiserror::Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum CapacityError {
    /// Too many headers provided (see [`httparse::Error::TooManyHeaders`]).
    #[error("Too many headers")]
    TooManyHeaders,
    /// Received header is too long.
    /// Message is bigger than the maximum allowed size.
    #[error("Message too long: {size} > {max_size}")]
    MessageTooLong {
        /// The size of the message.
        size: u64,
        /// The maximum allowed message size.
        max_size: u64,
    },
}
impl From<&TungsteniteCapacityError> for CapacityError {
    fn from(rust_capacity_error: &TungsteniteCapacityError) -> Self {
        match rust_capacity_error {
            TungsteniteCapacityError::TooManyHeaders => Self::TooManyHeaders,
            TungsteniteCapacityError::MessageTooLong { size, max_size } => Self::MessageTooLong {
                size: *size as u64,
                max_size: *max_size as u64,
            },
        }
    }
}

/// [tokio_tungstenite::tungstenite::error::ProtocolError], but with `uniffi` support.
/// Indicates the specific type/cause of a protocol error.
#[derive(Debug, PartialEq, Eq, Clone, thiserror::Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum ProtocolError {
    /// Use of the wrong HTTP method (the WebSocket protocol requires the GET method be used).
    #[error("Unsupported HTTP method used - only GET is allowed")]
    WrongHttpMethod,
    /// Wrong HTTP version used (the WebSocket protocol requires version 1.1 or higher).
    #[error("HTTP version must be 1.1 or higher")]
    WrongHttpVersion,
    /// Missing `Connection: upgrade` HTTP header.
    #[error("No \"Connection: upgrade\" header")]
    MissingConnectionUpgradeHeader,
    /// Missing `Upgrade: websocket` HTTP header.
    #[error("No \"Upgrade: websocket\" header")]
    MissingUpgradeWebSocketHeader,
    /// Missing `Sec-WebSocket-Version: 13` HTTP header.
    #[error("No \"Sec-WebSocket-Version: 13\" header")]
    MissingSecWebSocketVersionHeader,
    /// Missing `Sec-WebSocket-Key` HTTP header.
    #[error("No \"Sec-WebSocket-Key\" header")]
    MissingSecWebSocketKey,
    /// The `Sec-WebSocket-Accept` header is either not present or does not specify the correct key value.
    #[error("Key mismatch in \"Sec-WebSocket-Accept\" header")]
    SecWebSocketAcceptKeyMismatch,
    /// Garbage data encountered after client request.
    #[error("Junk after client request")]
    JunkAfterRequest,
    /// Custom responses must be unsuccessful.
    #[error("Custom response must not be successful")]
    CustomResponseSuccessful,
    /// Invalid header is passed. Or the header is missing in the request. Or not present at all. Check the request that you pass.
    #[error("Missing, duplicated or incorrect header {header}")]
    InvalidHeader { header: String },
    /// No more data while still performing handshake.
    #[error("Handshake not finished")]
    HandshakeIncomplete,
    /// Wrapper around a [`httparse::Error`] value.
    #[error("httparse error: {httparse_error}")]
    HttparseError {
        httparse_error: http::parse::HTTParseError,
    },
    /// Not allowed to send after having sent a closing frame.
    #[error("Sending after closing is not allowed")]
    SendAfterClosing,
    /// Remote sent data after sending a closing frame.
    #[error("Remote sent after having closed")]
    ReceivedAfterClosing,
    /// Reserved bits in frame header are non-zero.
    #[error("Reserved bits are non-zero")]
    NonZeroReservedBits,
    /// The server must close the connection when an unmasked frame is received.
    #[error("Received an unmasked frame from client")]
    UnmaskedFrameFromClient,
    /// The client must close the connection when a masked frame is received.
    #[error("Received a masked frame from server")]
    MaskedFrameFromServer,
    /// Control frames must not be fragmented.
    #[error("Fragmented control frame")]
    FragmentedControlFrame,
    /// Control frames must have a payload of 125 bytes or less.
    #[error("Control frame too big (payload must be 125 bytes or less)")]
    ControlFrameTooBig,
    /// Type of control frame not recognised.
    #[error("Unknown control frame type: {control_frame_type}")]
    UnknownControlFrameType { control_frame_type: u8 },
    /// Type of data frame not recognised.
    #[error("Unknown data frame type: {data_frame_type}")]
    UnknownDataFrameType { data_frame_type: u8 },
    /// Received a continue frame despite there being nothing to continue.
    #[error("Continue frame but nothing to continue")]
    UnexpectedContinueFrame,
    /// Received data while waiting for more fragments.
    #[error("While waiting for more fragments received: {data}")]
    ExpectedFragment { data: TungsteniteData },
    /// Connection closed without performing the closing handshake.
    #[error("Connection reset without closing handshake")]
    ResetWithoutClosingHandshake,
    /// Encountered an invalid opcode.
    #[error("Encountered invalid opcode: {opcode}")]
    InvalidOpcode { opcode: u8 },
    /// The payload for the closing frame is invalid.
    #[error("Invalid close sequence")]
    InvalidCloseSequence,
}
impl From<&TungsteniteProtocolError> for ProtocolError {
    fn from(rust_protocol_error: &TungsteniteProtocolError) -> Self {
        match rust_protocol_error {
            TungsteniteProtocolError::WrongHttpMethod => Self::WrongHttpMethod,
            TungsteniteProtocolError::WrongHttpVersion => Self::WrongHttpVersion,
            TungsteniteProtocolError::MissingConnectionUpgradeHeader => {
                Self::MissingConnectionUpgradeHeader
            }
            TungsteniteProtocolError::MissingUpgradeWebSocketHeader => {
                Self::MissingUpgradeWebSocketHeader
            }
            TungsteniteProtocolError::MissingSecWebSocketVersionHeader => {
                Self::MissingSecWebSocketVersionHeader
            }
            TungsteniteProtocolError::MissingSecWebSocketKey => Self::MissingSecWebSocketKey,
            TungsteniteProtocolError::SecWebSocketAcceptKeyMismatch => {
                Self::SecWebSocketAcceptKeyMismatch
            }
            TungsteniteProtocolError::JunkAfterRequest => Self::JunkAfterRequest,
            TungsteniteProtocolError::CustomResponseSuccessful => Self::CustomResponseSuccessful,
            TungsteniteProtocolError::InvalidHeader(header_name) => Self::InvalidHeader {
                header: header_name.to_string(),
            },
            TungsteniteProtocolError::HandshakeIncomplete => Self::HandshakeIncomplete,
            TungsteniteProtocolError::HttparseError(error) => Self::HttparseError {
                httparse_error: error.into(),
            },
            TungsteniteProtocolError::SendAfterClosing => Self::SendAfterClosing,
            TungsteniteProtocolError::ReceivedAfterClosing => Self::ReceivedAfterClosing,
            TungsteniteProtocolError::NonZeroReservedBits => Self::NonZeroReservedBits,
            TungsteniteProtocolError::UnmaskedFrameFromClient => Self::UnmaskedFrameFromClient,
            TungsteniteProtocolError::MaskedFrameFromServer => Self::MaskedFrameFromServer,
            TungsteniteProtocolError::FragmentedControlFrame => Self::FragmentedControlFrame,
            TungsteniteProtocolError::ControlFrameTooBig => Self::ControlFrameTooBig,
            TungsteniteProtocolError::UnknownControlFrameType(control_frame_type) => {
                Self::UnknownControlFrameType {
                    control_frame_type: *control_frame_type,
                }
            }
            TungsteniteProtocolError::UnknownDataFrameType(data_frame_type) => {
                Self::UnknownDataFrameType {
                    data_frame_type: *data_frame_type,
                }
            }
            TungsteniteProtocolError::UnexpectedContinueFrame => Self::UnexpectedContinueFrame,
            TungsteniteProtocolError::ExpectedFragment(data) => {
                Self::ExpectedFragment { data: data.into() }
            }
            TungsteniteProtocolError::ResetWithoutClosingHandshake => {
                Self::ResetWithoutClosingHandshake
            }
            TungsteniteProtocolError::InvalidOpcode(opcode) => {
                Self::InvalidOpcode { opcode: *opcode }
            }
            TungsteniteProtocolError::InvalidCloseSequence => Self::InvalidCloseSequence,
        }
    }
}
