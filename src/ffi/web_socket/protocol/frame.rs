use coding::{TungsteniteCloseCode, TungsteniteOpCode};

pub(crate) mod coding;

/// [tokio_tungstenite::tungstenite::protocol::frame::Frame], but with `uniffi` support.
/// A struct representing a WebSocket frame.
#[derive(Debug, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Frame {
    header: FrameHeader,
    payload: Vec<u8>,
}
impl From<&tokio_tungstenite::tungstenite::protocol::frame::Frame> for Frame {
    fn from(rust_frame: &tokio_tungstenite::tungstenite::protocol::frame::Frame) -> Self {
        Self {
            header: rust_frame.header().into(),
            payload: rust_frame.payload().clone(),
        }
    }
}

/// [tokio_tungstenite::tungstenite::protocol::frame::FrameHeader], but with `uniffi` support.
/// A struct representing a WebSocket frame header.
#[allow(missing_copy_implementations)]
#[derive(Debug, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct FrameHeader {
    /// Indicates that the frame is the last one of a possibly fragmented message.
    pub is_final: bool,
    /// Reserved for protocol extensions.
    pub rsv1: bool,
    /// Reserved for protocol extensions.
    pub rsv2: bool,
    /// Reserved for protocol extensions.
    pub rsv3: bool,
    /// WebSocket protocol opcode.
    pub opcode: TungsteniteOpCode,
    /// A frame mask, if any.
    pub mask: Option<Vec<u8>>,
}
impl From<&tokio_tungstenite::tungstenite::protocol::frame::FrameHeader> for FrameHeader {
    fn from(
        rust_frame_header: &tokio_tungstenite::tungstenite::protocol::frame::FrameHeader,
    ) -> Self {
        let tokio_tungstenite::tungstenite::protocol::frame::FrameHeader {
            is_final,
            rsv1,
            rsv2,
            rsv3,
            opcode,
            mask,
        } = rust_frame_header;

        Self {
            is_final: *is_final,
            rsv1: *rsv1,
            rsv2: *rsv2,
            rsv3: *rsv3,
            opcode: opcode.into(),
            mask: mask.map(|array| array.to_vec()),
        }
    }
}

/// [tokio_tungstenite::tungstenite::protocol::frame::CloseFrame], but with `uniffi::support`
/// A struct representing the close command.
#[derive(Debug, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct CloseFrame {
    /// The reason as a code.
    pub code: TungsteniteCloseCode,
    /// The reason as text string.
    pub reason: String,
}
impl<'a> From<&tokio_tungstenite::tungstenite::protocol::frame::CloseFrame<'a>> for CloseFrame {
    fn from(rust_close_frame: &tokio_tungstenite::tungstenite::protocol::CloseFrame<'a>) -> Self {
        let tokio_tungstenite::tungstenite::protocol::CloseFrame { code, reason } =
            rust_close_frame;

        Self {
            code: code.into(),
            reason: reason.clone().to_string(),
        }
    }
}
