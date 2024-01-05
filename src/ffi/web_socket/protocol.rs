use crate::ffi::web_socket::protocol::frame::{CloseFrame, Frame};

pub(crate) mod frame;

/// [tokio_tungstenite::tungstenite::protocol::Message], but with `uniffi` support.
/// An enum representing the various forms of a WebSocket message.
#[derive(Debug, Eq, PartialEq, Clone, uniffi::Enum)]
pub enum WebSocketMessage {
    /// A text WebSocket message
    Text {
        /// The text of the message.
        text: String,
    },
    /// A binary WebSocket message
    Binary {
        /// The binary bytes of the message.
        bytes: Vec<u8>,
    },
    /// A ping message with the specified payload
    ///
    /// The payload here must have a length less than 125 bytes
    Ping {
        /// The bytes that should be sent back in a [WebSocketMessage::Pong] message.
        bytes: Vec<u8>,
    },
    /// A pong message with the specified payload
    ///
    /// The payload here must have a length less than 125 bytes
    Pong {
        /// The bytes sent back in reply to a [WebSocketMessage::Ping].  The `bytes` should match
        /// the last `WebSocketMessage::Pong` to count as an acknowledgement.
        bytes: Vec<u8>,
    },
    /// A close message with the optional close frame.
    Close {
        /// The optional frame sent when the server closes the web socket.  Includes the code and
        /// reason.
        close_frame: Option<CloseFrame>,
    },
    /// Raw frame. Note, that you're not going to get this value while reading the message.
    WebSocketFrame {
        /// A raw frame that hasn't been categorized into the other variants.
        frame: Frame,
    },
}
impl From<&tokio_tungstenite::tungstenite::Message> for WebSocketMessage {
    fn from(rust_message: &tokio_tungstenite::tungstenite::Message) -> Self {
        match rust_message {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                Self::Text { text: text.clone() }
            }
            tokio_tungstenite::tungstenite::Message::Binary(bytes) => Self::Binary {
                bytes: bytes.clone(),
            },
            tokio_tungstenite::tungstenite::Message::Ping(bytes) => Self::Ping {
                bytes: bytes.clone(),
            },
            tokio_tungstenite::tungstenite::Message::Pong(bytes) => Self::Pong {
                bytes: bytes.clone(),
            },
            tokio_tungstenite::tungstenite::Message::Close(close_frame) => Self::Close {
                close_frame: close_frame.as_ref().map(From::from),
            },
            tokio_tungstenite::tungstenite::Message::Frame(frame) => Self::WebSocketFrame {
                frame: frame.into(),
            },
        }
    }
}
