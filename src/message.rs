use std::fmt;
use std::mem;
use std::str::{self, FromStr};
use std::sync::Arc;

use crate::join_reference::JoinReference;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message as SocketMessage;

use crate::reference::Reference;
use crate::topic::Topic;

/// Occurs when decoding messages received from the server
#[doc(hidden)]
#[derive(Debug, thiserror::Error)]
pub enum MessageDecodingError {
    #[error("unexpected eof")]
    UnexpectedEof,
    #[error("invalid utf-8: {0}")]
    InvalidUtf8(#[from] str::Utf8Error),
    #[error("{0}")]
    Invalid(String),
    #[error("invalid binary payload")]
    InvalidBinary,
    #[error("failed to decode json payload: {0}")]
    Json(#[from] serde_json::Error),
}

/// Occurs when encoding messages to send to the server
#[doc(hidden)]
#[derive(Debug, thiserror::Error)]
#[error("unable to encode message")]
pub struct MessageEncodingError;

/// This represents the messages sent to or from Phoenix over the socket
///
/// Channels are implicitly multiplexed over a single connection, by using various
/// fields of the message to determine who the recipients are.
///
/// This structure should not be exposed to users of this library, instead, only the
/// event, and payload are intended to be consumed directly. The topic name is only
/// used when joining a channel, and the references are part of the multiplexing scheme.
#[doc(hidden)]
#[derive(Debug)]
pub enum Message {
    Control(Control),
    Broadcast(Broadcast),
    Reply(Reply),
    Push(Push),
}
impl Message {
    /// Returns a reference to the data associated with this message.
    pub fn payload(&self) -> &Payload {
        match self {
            Self::Control(ref msg) => &msg.payload,
            Self::Broadcast(ref msg) => &msg.payload,
            Self::Reply(ref msg) => &msg.payload,
            Self::Push(ref msg) => &msg.payload,
        }
    }
}

/// Represents a reply to a message
#[doc(hidden)]
#[derive(Debug)]
pub struct Control {
    /// The event for this message type is always a `PhoenixEvent`
    pub event: Event,
    pub payload: Payload,
    /// A unique ID for this message that is used for associating messages with their replies.
    pub reference: Option<Reference>,
}

/// Represents a message broadcast to all members of a channel
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct Broadcast {
    pub topic: Topic,
    pub event: Event,
    pub payload: Arc<Payload>,
}

/// Represents a reply to a message
#[doc(hidden)]
#[derive(Debug)]
pub struct Reply {
    pub topic: Topic,
    /// The event for this message type is always `PhoenixEvent::Reply`
    pub event: Event,
    pub payload: Payload,
    /// The unique reference of the channel member this message is being sent to/from
    ///
    /// This ID is selected by the client when joining a channel, and must be provided whenever sending a message to the channel.
    pub join_reference: JoinReference,
    /// A unique ID for this message that is used for associating messages with their replies.
    pub reference: Reference,
    /// The status of the reply
    pub status: ReplyStatus,
}

/// Represents a message being sent by a specific member of a channel, to the other members of the channel
#[doc(hidden)]
#[derive(Debug)]
pub struct Push {
    pub topic: Topic,
    pub event: Event,
    pub payload: Arc<Payload>,
    /// The unique reference of the channel member this message is being sent to/from
    ///
    /// This ID is selected by the client when joining a channel, and must be provided whenever sending a message to the channel.
    pub join_reference: JoinReference,
    /// A unique ID for this message that is used for associating messages with their replies.
    pub reference: Option<Reference>,
}

impl Message {
    /// Encodes this message for transport over WebSocket
    pub fn encode(self) -> Result<SocketMessage, MessageEncodingError> {
        if self.payload().is_binary() {
            Ok(SocketMessage::Binary(self.encode_binary()?))
        } else {
            Ok(SocketMessage::Text(self.encode_json()?))
        }
    }

    /// The encoding used for client-sent messages differs from that of server-sent messages.
    ///
    /// See the Phoenix Channels js client for reference.
    fn encode_binary(self) -> Result<Vec<u8>, MessageEncodingError> {
        let (join_ref, reference, topic, event, mut payload): (
            Arc<String>,
            Arc<String>,
            Arc<String>,
            String,
            Vec<u8>,
        ) = match self {
            Self::Control(Control {
                event,
                payload,
                reference,
            }) => (
                Arc::new(String::new()),
                reference.map(From::from).unwrap_or_default(),
                Arc::new("phoenix".to_string()),
                event.to_string(),
                payload.into_binary().unwrap(),
            ),
            Self::Broadcast(Broadcast {
                topic,
                event,
                payload,
            }) => (
                Arc::new(String::new()),
                Arc::new(String::new()),
                topic.into(),
                event.to_string(),
                Payload::clone(&payload).into_binary().unwrap(),
            ),
            Self::Reply(Reply {
                topic,
                event,
                payload,
                join_reference,
                reference,
                ..
            }) => (
                join_reference.into(),
                reference.into(),
                topic.into(),
                event.into(),
                payload.into_binary().unwrap(),
            ),
            Self::Push(Push {
                join_reference,
                reference,
                topic,
                event,
                payload,
            }) => (
                join_reference.into(),
                reference
                    .as_ref()
                    .map(From::from)
                    .unwrap_or_else(Default::default),
                topic.into(),
                event.to_string(),
                Payload::clone(&payload).into_binary().unwrap(),
            ),
        };

        let join_ref_size: u8 = join_ref
            .as_bytes()
            .len()
            .try_into()
            .map_err(|_| MessageEncodingError)?;
        let ref_size: u8 = reference
            .as_bytes()
            .len()
            .try_into()
            .map_err(|_| MessageEncodingError)?;
        let topic_size: u8 = topic
            .as_bytes()
            .len()
            .try_into()
            .map_err(|_| MessageEncodingError)?;
        let event_size: u8 = event
            .as_bytes()
            .len()
            .try_into()
            .map_err(|_| MessageEncodingError)?;
        let buffer_size = (5 * mem::size_of::<u8>())
            + (join_ref_size as usize)
            + (ref_size as usize)
            + (topic_size as usize)
            + (event_size as usize)
            + payload.len();

        let mut buffer = Vec::<u8>::with_capacity(buffer_size);
        buffer.push(BinaryMessageType::Push.into());
        buffer.push(join_ref_size);
        buffer.push(ref_size);
        buffer.push(topic_size);
        buffer.push(event_size);
        buffer.extend_from_slice(join_ref.as_bytes());
        buffer.extend_from_slice(reference.as_bytes());
        buffer.extend_from_slice(topic.as_bytes());
        buffer.extend_from_slice(event.as_bytes());
        buffer.append(&mut payload);

        Ok(buffer)
    }

    /// Decodes the given WebSocket message as `Message`
    pub fn decode(message: SocketMessage) -> Result<Self, MessageDecodingError> {
        match message {
            SocketMessage::Text(ref text) => Self::decode_json(text.as_str()),
            SocketMessage::Binary(ref bytes) => Self::decode_binary(bytes.as_slice()),
            other => panic!("invalid message type: {:#?}", &other),
        }
    }

    fn encode_json(self) -> Result<String, MessageEncodingError> {
        let value = Value::Array(match self {
            Self::Control(Control {
                event,
                payload,
                reference: Option::None,
            }) => {
                vec![
                    Value::Null,
                    Value::Null,
                    "phoenix".into(),
                    event.into(),
                    payload.into_value().unwrap(),
                ]
            }
            Self::Control(Control {
                event,
                payload,
                reference: Some(reference),
            }) => {
                vec![
                    Value::Null,
                    reference.into(),
                    "phoenix".into(),
                    event.to_string().into(),
                    payload.into_value().unwrap(),
                ]
            }
            Self::Broadcast(Broadcast {
                topic,
                event,
                payload,
            }) => {
                vec![
                    Value::Null,
                    Value::Null,
                    topic.into(),
                    event.into(),
                    Payload::clone(&payload).into_value().unwrap(),
                ]
            }
            Self::Reply(Reply {
                topic,
                event,
                payload,
                join_reference,
                reference,
                ..
            }) => {
                vec![
                    join_reference.into(),
                    reference.into(),
                    topic.into(),
                    event.into(),
                    payload.into_value().unwrap(),
                ]
            }
            Self::Push(Push {
                join_reference,
                reference,
                topic,
                event,
                payload,
            }) => {
                vec![
                    join_reference.into(),
                    reference.as_ref().map(From::from).unwrap_or(Value::Null),
                    topic.into(),
                    event.into(),
                    Payload::clone(&payload).into_value().unwrap(),
                ]
            }
        });

        serde_json::to_string(&value).map_err(|_| MessageEncodingError)
    }

    /// Parses a JSON-encoded message (using the Phoenix JSON v2 serializer representation)
    fn decode_json(json: &str) -> Result<Self, MessageDecodingError> {
        let value = serde_json::from_str(json)?;
        match value {
            // This is the representation used by the v2 Phoenix serializer
            Value::Array(mut fields) if fields.len() == 5 => {
                let payload = fields.pop().unwrap();
                let event = match fields.pop().unwrap() {
                    Value::String(s) => Event::from(s),
                    other => {
                        return Err(MessageDecodingError::Invalid(format!(
                            "expected event to be a string, got: {:#?}",
                            &other
                        )))
                    }
                };
                let topic = match fields.pop().unwrap() {
                    Value::String(s) => s.into(),
                    other => {
                        return Err(MessageDecodingError::Invalid(format!(
                            "expected topic to be a string, got: {:#?}",
                            &other
                        )))
                    }
                };
                let reference = match fields.pop().unwrap() {
                    Value::Null => None,
                    Value::String(s) => Some(s.into()),
                    other => {
                        return Err(MessageDecodingError::Invalid(format!(
                            "expected ref to be a string or null, got: {:#?}",
                            &other
                        )))
                    }
                };
                let join_reference: Option<JoinReference> = match fields.pop().unwrap() {
                    Value::Null => None,
                    Value::String(s) => Some(s.into()),
                    other => {
                        return Err(MessageDecodingError::Invalid(format!(
                            "expected join_ref to be a string or null, got: {:#?}",
                            &other
                        )))
                    }
                };
                match (join_reference, reference) {
                    (None, None) => Ok(Message::Broadcast(Broadcast {
                        topic,
                        event,
                        payload: Arc::new(Payload::Value(payload)),
                    })),
                    (Some(join_reference), None) => Ok(Message::Push(Push {
                        topic,
                        event,
                        payload: Arc::new(Payload::Value(payload)),
                        join_reference,
                        reference: None,
                    })),
                    (Some(join_reference), Some(reference)) => match event {
                        Event::Phoenix(PhoenixEvent::Reply) => {
                            let (status, payload) = match payload {
                                Value::Object(mut map) => {
                                    match (map.remove("status"), map.remove("response")) {
                                        (Some(Value::String(s)), Some(payload)) => {
                                            (s.into(), payload)
                                        }
                                        other => return Err(MessageDecodingError::Invalid(format!("expected reply payload to be an object with status/response keys, got: {:#?}", &other))),
                                    }
                                }
                                other => return Err(MessageDecodingError::Invalid(format!("expected reply payload to be an object with status/response keys, got: {:#?}", &other))),
                            };

                            Ok(Message::Reply(Reply {
                                topic,
                                event,
                                payload: Payload::Value(payload),
                                join_reference,
                                reference,
                                status,
                            }))
                        }
                        event => Ok(Message::Push(Push {
                            topic,
                            event,
                            payload: Arc::new(Payload::Value(payload)),
                            join_reference,
                            reference: Some(reference),
                        })),
                    },
                    (None, reference) => match event {
                        event @ Event::Phoenix(_) => Ok(Message::Control(Control {
                            event,
                            payload: Payload::Value(payload),
                            reference,
                        })),
                        other => {
                            return Err(MessageDecodingError::Invalid(format!(
                                "unrecognized control event: {:?}",
                                &other
                            )))
                        }
                    },
                }
            }
            other => {
                return Err(MessageDecodingError::Invalid(format!(
                    "expected v2 serializer format, an array of fields, got: {:#?}",
                    &other
                )))
            }
        }
    }

    /// Parses a binary-encoded message, as used by the Phoenix v2 JSON serializer
    fn decode_binary(mut bytes: &[u8]) -> Result<Self, MessageDecodingError> {
        if bytes.is_empty() {
            return Err(MessageDecodingError::InvalidBinary);
        }

        let byte = take_first(&mut bytes).ok_or(MessageDecodingError::UnexpectedEof)?;
        let ty = BinaryMessageType::try_from(byte)?;
        match ty {
            BinaryMessageType::Broadcast => {
                let topic_size =
                    take_first(&mut bytes).ok_or(MessageDecodingError::UnexpectedEof)? as usize;
                let event_size =
                    take_first(&mut bytes).ok_or(MessageDecodingError::UnexpectedEof)? as usize;
                let topic = str::from_utf8(
                    take(&mut bytes, topic_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();
                let event = str::from_utf8(
                    take(&mut bytes, event_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();
                let payload = Arc::new(Payload::Binary(bytes.to_vec()));
                Ok(Message::Broadcast(Broadcast {
                    topic,
                    event,
                    payload,
                }))
            }
            BinaryMessageType::Reply => {
                let join_ref_size =
                    take_first(&mut bytes).ok_or(MessageDecodingError::UnexpectedEof)? as usize;
                let ref_size =
                    take_first(&mut bytes).ok_or(MessageDecodingError::UnexpectedEof)? as usize;
                let topic_size =
                    take_first(&mut bytes).ok_or(MessageDecodingError::UnexpectedEof)? as usize;
                let status_size =
                    take_first(&mut bytes).ok_or(MessageDecodingError::UnexpectedEof)? as usize;
                let join_reference = str::from_utf8(
                    take(&mut bytes, join_ref_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();
                let reference = str::from_utf8(
                    take(&mut bytes, ref_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();
                let topic = str::from_utf8(
                    take(&mut bytes, topic_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();
                let status = str::from_utf8(
                    take(&mut bytes, status_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();

                let payload = Payload::Binary(bytes.to_vec());
                Ok(Message::Reply(Reply {
                    topic,
                    event: Event::Phoenix(PhoenixEvent::Reply),
                    payload,
                    join_reference,
                    reference,
                    status,
                }))
            }
            BinaryMessageType::Push => {
                let join_ref_size =
                    take_first(&mut bytes).ok_or(MessageDecodingError::UnexpectedEof)? as usize;
                let topic_size =
                    take_first(&mut bytes).ok_or(MessageDecodingError::UnexpectedEof)? as usize;
                let event_size =
                    take_first(&mut bytes).ok_or(MessageDecodingError::UnexpectedEof)? as usize;
                let join_reference = str::from_utf8(
                    take(&mut bytes, join_ref_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();
                let topic = str::from_utf8(
                    take(&mut bytes, topic_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();
                let event = str::from_utf8(
                    take(&mut bytes, event_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();
                let payload = Arc::new(Payload::Binary(bytes.to_vec()));
                Ok(Message::Push(Push {
                    topic,
                    event,
                    payload,
                    join_reference,
                    reference: None,
                }))
            }
        }
    }
}

#[cfg(feature = "nightly")]
#[inline(always)]
fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    bytes.take(..n)
}

#[cfg(not(feature = "nightly"))]
#[inline]
fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if n > bytes.len() {
        return None;
    }
    let (taken, rest) = (*bytes).split_at(n);
    *bytes = rest;
    Some(taken)
}

#[cfg(feature = "nightly")]
#[inline(always)]
fn take_first(bytes: &mut &[u8]) -> Option<u8> {
    bytes.take_first().copied()
}

#[cfg(not(feature = "nightly"))]
#[inline]
fn take_first(bytes: &mut &[u8]) -> Option<u8> {
    let (first, rest) = (*bytes).split_first()?;
    *bytes = rest;
    Some(*first)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
enum BinaryMessageType {
    Push = 0,
    Reply = 1,
    Broadcast = 2,
}
impl Into<u8> for BinaryMessageType {
    #[inline]
    fn into(self) -> u8 {
        self as u8
    }
}
impl TryFrom<u8> for BinaryMessageType {
    type Error = MessageDecodingError;

    #[inline]
    fn try_from(byte: u8) -> Result<Self, Self::Error> {
        match byte {
            0 => Ok(Self::Push),
            1 => Ok(Self::Reply),
            2 => Ok(Self::Broadcast),
            n => Err(MessageDecodingError::Invalid(format!(
                "invalid binary message type, got {}",
                n
            ))),
        }
    }
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ReplyStatus {
    Ok,
    Error,
}
impl ReplyStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
        }
    }
}
impl From<&str> for ReplyStatus {
    fn from(s: &str) -> Self {
        match s {
            "ok" => Self::Ok,
            _ => Self::Error,
        }
    }
}
impl From<String> for ReplyStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "ok" => Self::Ok,
            _ => Self::Error,
        }
    }
}
impl fmt::Display for ReplyStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Contains the response payload sent to/received from Phoenix
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// This payload type is used for all non-binary messages
    Value(Value),
    /// This payload type is used for binary messages
    Binary(Vec<u8>),
}

impl Payload {
    pub fn is_value(&self) -> bool {
        match self {
            Self::Value(_) => true,
            _ => false,
        }
    }

    pub fn is_binary(&self) -> bool {
        match self {
            Self::Binary(_) => true,
            _ => false,
        }
    }

    pub fn into_value(self) -> Option<Value> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }

    pub fn into_binary(self) -> Option<Vec<u8>> {
        match self {
            Self::Binary(value) => Some(value),
            _ => None,
        }
    }
}
impl From<Value> for Payload {
    #[inline]
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}
impl From<Vec<u8>> for Payload {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        Self::Binary(value)
    }
}
impl Default for Payload {
    fn default() -> Self {
        Payload::Value(Value::Object(serde_json::Map::new()))
    }
}
impl fmt::Display for Payload {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Value(ref value) => write!(f, "{}", value),
            Self::Binary(ref bytes) => write!(f, "{:?}", bytes.as_slice()),
        }
    }
}

/// This is a strongly typed wrapper around the event associated with a `Message`.
///
/// We discriminate between special Phoenix events and user-defined events, as they have slightly
/// different semantics. Generally speaking, Phoenix events are not exposed to users, and are not
/// permitted to be sent by them either.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Event {
    /// Represents one of the built-in Phoenix channel events, e.g. join
    Phoenix(PhoenixEvent),
    /// Represents a user-defined event
    User(String),
}
impl From<Event> for String {
    fn from(event: Event) -> Self {
        event.to_string()
    }
}
impl From<Event> for Value {
    fn from(event: Event) -> Self {
        Value::String(event.into())
    }
}
impl From<&Event> for Value {
    fn from(event: &Event) -> Self {
        Value::String(event.to_string().into())
    }
}
impl From<PhoenixEvent> for Event {
    #[inline]
    fn from(event: PhoenixEvent) -> Self {
        Self::Phoenix(event)
    }
}
impl From<&str> for Event {
    fn from(s: &str) -> Self {
        match s.parse::<PhoenixEvent>() {
            Ok(e) => Self::Phoenix(e),
            Err(_) => Self::User(s.to_string()),
        }
    }
}
impl From<String> for Event {
    fn from(s: String) -> Self {
        match s.as_str().parse::<PhoenixEvent>() {
            Ok(e) => Self::Phoenix(e),
            Err(_) => Self::User(s),
        }
    }
}
impl PartialEq<PhoenixEvent> for &Event {
    fn eq(&self, other: &PhoenixEvent) -> bool {
        match *self {
            Event::Phoenix(ref this) => this == other,
            Event::User(_) => false,
        }
    }
}
impl PartialEq<PhoenixEvent> for Event {
    fn eq(&self, other: &PhoenixEvent) -> bool {
        match self {
            Self::Phoenix(ref this) => this == other,
            Self::User(_) => false,
        }
    }
}
impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Phoenix(ref e) => write!(f, "{}", e),
            Self::User(ref e) => write!(f, "{}", e),
        }
    }
}

/// Represents special events related to management of Phoenix channels.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
