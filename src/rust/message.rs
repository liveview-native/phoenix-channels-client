use std::fmt;
use std::mem;
use std::str;
use std::sync::Arc;

use bytes::{BufMut, Bytes};
use flexstr::SharedStr;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message as SocketMessage;

use crate::ffi;
use crate::ffi::message::PhoenixEvent;
use crate::ffi::topic::Topic;
use crate::rust::join_reference::JoinReference;
use crate::rust::reference::Reference;

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
            Self::Broadcast(ref msg) => &msg.event_payload.payload,
            Self::Reply(ref msg) => &msg.payload,
            Self::Push(ref msg) => &msg.event_payload.payload,
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
    pub topic: Arc<Topic>,
    pub event_payload: EventPayload,
}

/// Represents a reply to a message
#[doc(hidden)]
#[derive(Debug)]
pub struct Reply {
    pub topic: Arc<Topic>,
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
    pub topic: Arc<Topic>,
    pub event_payload: EventPayload,
    /// The unique reference of the channel member this message is being sent to/from
    ///
    /// This ID is selected by the client when joining a channel, and must be provided whenever sending a message to the channel.
    pub join_reference: JoinReference,
    /// A unique ID for this message that is used for associating messages with their replies.
    pub reference: Option<Reference>,
}

/// The [EventPayload::event] sent by the server along with the [EventPayload::payload] for that
/// [EventPayload::event].
#[derive(Clone, Debug)]
pub struct EventPayload {
    /// The [Event] name.
    pub event: Event,
    /// The data sent for the [EventPayload::event].
    pub payload: Payload,
}
impl From<Broadcast> for EventPayload {
    fn from(broadcast: Broadcast) -> Self {
        broadcast.event_payload
    }
}
impl From<Push> for EventPayload {
    fn from(push: Push) -> Self {
        push.event_payload
    }
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
        let (join_ref, reference, topic, event, payload_bytes): (
            SharedStr,
            SharedStr,
            String,
            String,
            Bytes,
        ) = match self {
            Self::Control(Control {
                event,
                payload,
                reference,
            }) => (
                "".into(),
                reference.map(From::from).unwrap_or_default(),
                "phoenix".into(),
                event.to_string(),
                payload.into_binary().unwrap(),
            ),
            Self::Broadcast(Broadcast {
                topic,
                event_payload,
            }) => (
                "".into(),
                "".into(),
                topic.to_string(),
                event_payload.event.to_string(),
                Payload::clone(&event_payload.payload)
                    .into_binary()
                    .unwrap(),
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
                topic.to_string(),
                event.into(),
                payload.into_binary().unwrap(),
            ),
            Self::Push(Push {
                join_reference,
                reference,
                topic,
                event_payload,
            }) => (
                join_reference.into(),
                reference
                    .as_ref()
                    .map(From::from)
                    .unwrap_or_else(Default::default),
                topic.to_string(),
                event_payload.event.to_string(),
                Payload::clone(&event_payload.payload)
                    .into_binary()
                    .unwrap(),
            ),
        };

        let join_reference_bytes = join_ref.as_bytes();
        let join_reference_size: u8 = join_reference_bytes
            .len()
            .try_into()
            .map_err(|_| MessageEncodingError)?;
        let reference_bytes = reference.as_bytes();
        let reference_size: u8 = reference_bytes
            .len()
            .try_into()
            .map_err(|_| MessageEncodingError)?;
        let topic_bytes = topic.as_bytes();
        let topic_size: u8 = topic_bytes
            .len()
            .try_into()
            .map_err(|_| MessageEncodingError)?;
        let event_bytes = event.as_bytes();
        let event_size: u8 = event_bytes
            .len()
            .try_into()
            .map_err(|_| MessageEncodingError)?;
        let buffer_size = (5 * mem::size_of::<u8>())
            + (join_reference_size as usize)
            + (reference_size as usize)
            + (topic_size as usize)
            + (event_size as usize)
            + payload_bytes.len();

        let mut buffer = Vec::with_capacity(buffer_size);
        buffer.put_u8(BinaryMessageType::Push.into());
        buffer.put_u8(join_reference_size);
        buffer.put_u8(reference_size);
        buffer.put_u8(topic_size);
        buffer.put_u8(event_size);
        buffer.put_slice(join_reference_bytes);
        buffer.put_slice(reference_bytes);
        buffer.put_slice(topic_bytes);
        buffer.put_slice(event_bytes);
        buffer.put(payload_bytes);

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
                    serde_json::to_value("phoenix").unwrap(),
                    event.into(),
                    Value::clone(&payload.into_value().unwrap()),
                ]
            }
            Self::Control(Control {
                event,
                payload,
                reference: Some(reference),
            }) => {
                vec![
                    Value::Null,
                    serde_json::to_value(reference).unwrap(),
                    serde_json::to_value("phoenix").unwrap(),
                    event.to_string().into(),
                    Value::clone(payload.value().unwrap()),
                ]
            }
            Self::Broadcast(Broadcast {
                topic,
                event_payload,
            }) => {
                vec![
                    Value::Null,
                    Value::Null,
                    serde_json::to_value(topic.as_ref()).unwrap(),
                    event_payload.event.into(),
                    Value::clone(event_payload.payload.value().unwrap()),
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
                    serde_json::to_value(join_reference).unwrap(),
                    serde_json::to_value(reference).unwrap(),
                    serde_json::to_value(topic.as_ref()).unwrap(),
                    event.into(),
                    Value::clone(payload.value().unwrap()),
                ]
            }
            Self::Push(Push {
                join_reference,
                reference,
                topic,
                event_payload,
            }) => {
                vec![
                    serde_json::to_value(join_reference).unwrap(),
                    serde_json::to_value(reference).unwrap(),
                    serde_json::to_value(topic.as_ref()).unwrap(),
                    event_payload.event.into(),
                    Value::clone(event_payload.payload.value().unwrap()),
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
                    Value::String(s) => Topic::from_string(s),
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
                        event_payload: EventPayload {
                            event,
                            payload: payload.into(),
                        },
                    })),
                    (Some(join_reference), None) => Ok(Message::Push(Push {
                        topic,
                        event_payload: EventPayload {
                            event,
                            payload: payload.into(),
                        },
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
                                payload: payload.into(),
                                join_reference,
                                reference,
                                status,
                            }))
                        }
                        event => Ok(Message::Push(Push {
                            topic,
                            event_payload: EventPayload {
                                event,
                                payload: payload.into(),
                            },
                            join_reference,
                            reference: Some(reference),
                        })),
                    },
                    (None, reference) => match event {
                        event @ Event::Phoenix(_) => Ok(Message::Control(Control {
                            event,
                            payload: payload.into(),
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
                let topic = Topic::from_string(
                    str::from_utf8(
                        take(&mut bytes, topic_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                    )
                    .map_err(MessageDecodingError::InvalidUtf8)?
                    .to_string(),
                );
                let event = str::from_utf8(
                    take(&mut bytes, event_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();
                let payload = Bytes::copy_from_slice(bytes).into();
                Ok(Message::Broadcast(Broadcast {
                    topic,
                    event_payload: EventPayload { event, payload },
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
                let topic = Topic::from_string(
                    str::from_utf8(
                        take(&mut bytes, topic_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                    )
                    .map_err(MessageDecodingError::InvalidUtf8)?
                    .to_string(),
                );
                let status = str::from_utf8(
                    take(&mut bytes, status_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();

                let payload = Bytes::copy_from_slice(bytes).into();
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
                let topic = Topic::from_string(
                    str::from_utf8(
                        take(&mut bytes, topic_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                    )
                    .map_err(MessageDecodingError::InvalidUtf8)?
                    .to_string(),
                );
                let event = str::from_utf8(
                    take(&mut bytes, event_size).ok_or(MessageDecodingError::UnexpectedEof)?,
                )
                .map_err(MessageDecodingError::InvalidUtf8)?
                .into();
                let payload = Bytes::copy_from_slice(bytes).into();
                Ok(Message::Push(Push {
                    topic,
                    event_payload: EventPayload { event, payload },
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
    Value(Arc<Value>),
    /// This payload type is used for binary messages
    Binary(Bytes),
}
impl Payload {
    /// * `true` - this is a JSON payload.
    /// * `false` - this is a binary payload.
    pub fn is_value(&self) -> bool {
        match self {
            Self::Value(_) => true,
            _ => false,
        }
    }

    /// * `true` - this is a binary payload.
    /// * `false` - this a JSON payload.
    pub fn is_binary(&self) -> bool {
        match self {
            Self::Binary(_) => true,
            _ => false,
        }
    }

    /// * [Some(&Value)] - the JSON payload.
    /// * [None] - this is a binary payload.
    pub fn value(&self) -> Option<&Value> {
        match self {
            Self::Value(value) => Some(value.as_ref()),
            _ => None,
        }
    }

    /// Consume the payload and turn it into its inner JSON if this is a JSON payload.
    ///
    /// * `Some(Arc<Value>)` - this is a JSON payload.
    /// * `None` - this is a binary payload.
    pub fn into_value(self) -> Option<Arc<Value>> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }

    /// Consume the payload and turn it into its inner binary bytes if this is a binary payload.
    ///
    /// * `Some(Bytes)` - this is a binary payload.
    /// * `None` - this is a JSON payload.
    pub fn into_binary(self) -> Option<Bytes> {
        match self {
            Self::Binary(value) => Some(value),
            _ => None,
        }
    }
}
impl From<Value> for Payload {
    #[inline]
    fn from(value: Value) -> Self {
        Self::Value(Arc::new(value))
    }
}
impl From<Bytes> for Payload {
    fn from(bytes: Bytes) -> Self {
        Self::Binary(bytes)
    }
}
impl From<Vec<u8>> for Payload {
    fn from(byte_vec: Vec<u8>) -> Self {
        Self::Binary(byte_vec.into())
    }
}
impl From<ffi::message::Payload> for Payload {
    fn from(ffi_payload: ffi::message::Payload) -> Self {
        match ffi_payload {
            ffi::message::Payload::JSONPayload { json } => Self::Value(Arc::new(json.into())),
            ffi::message::Payload::Binary { bytes } => Self::Binary(bytes.into()),
        }
    }
}
impl Default for Payload {
    fn default() -> Self {
        Payload::Value(Arc::new(Value::Object(serde_json::Map::new())))
    }
}
impl fmt::Display for Payload {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Value(ref value) => write!(f, "{}", value),
            Self::Binary(ref bytes) => write!(f, "{:?}", bytes),
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
impl From<ffi::message::Event> for Event {
    fn from(ffi_event: ffi::message::Event) -> Self {
        match ffi_event {
            ffi::message::Event::Phoenix { phoenix } => Self::Phoenix(phoenix.into()),
            ffi::message::Event::User { user } => Self::User(user),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_encode_with_control_with_binary_payload() {
        assert_eq!(
            Message::Control(Control {
                event: Event::User("event_name".to_string()),
                payload: binary_payload(),
                reference: Some("reference".into()),
            })
            .encode()
            .unwrap(),
            SocketMessage::Binary(vec![
                0, 0, 9, 7, 10, 114, 101, 102, 101, 114, 101, 110, 99, 101, 112, 104, 111, 101,
                110, 105, 120, 101, 118, 101, 110, 116, 95, 110, 97, 109, 101, 0, 1, 2, 3
            ])
        );
    }

    #[test]
    fn message_encode_with_control_with_json_payload() {
        assert_eq!(
            Message::Control(Control {
                event: Event::User("event_name".to_string()),
                payload: json_payload(),
                reference: Some("reference".into()),
            })
            .encode()
            .unwrap(),
            SocketMessage::Text(
                "[null,\"reference\",\"phoenix\",\"event_name\",{\"key\":\"value\"}]".to_string()
            )
        );
    }

    #[test]
    fn message_encode_with_broadcast_with_binary_payload() {
        assert_eq!(
            Message::Broadcast(Broadcast {
                topic: Topic::from_string("channel_topic".to_string()),
                event_payload: EventPayload {
                    event: Event::User("event_name".to_string()),
                    payload: binary_payload()
                }
            })
            .encode()
            .unwrap(),
            SocketMessage::Binary(vec![
                0, 0, 0, 13, 10, 99, 104, 97, 110, 110, 101, 108, 95, 116, 111, 112, 105, 99, 101,
                118, 101, 110, 116, 95, 110, 97, 109, 101, 0, 1, 2, 3
            ])
        )
    }

    #[test]
    fn message_encode_with_broadcast_with_json_payload() {
        assert_eq!(
            Message::Broadcast(Broadcast {
                topic: Topic::from_string("channel_topic".to_string()),
                event_payload: EventPayload {
                    event: Event::User("event_name".to_string()),
                    payload: json_payload()
                }
            })
            .encode()
            .unwrap(),
            SocketMessage::Text(
                "[null,null,\"channel_topic\",\"event_name\",{\"key\":\"value\"}]".to_string()
            )
        )
    }

    #[test]
    fn message_encode_with_reply_with_binary_payload() {
        assert_eq!(
            Message::Reply(Reply {
                topic: Topic::from_string("channel_topic".to_string()),
                event: Event::User("event_name".to_string()),
                payload: binary_payload(),
                join_reference: "join_reference".into(),
                reference: "reference".into(),
                status: ReplyStatus::Ok,
            })
            .encode()
            .unwrap(),
            SocketMessage::Binary(vec![
                0, 14, 9, 13, 10, 106, 111, 105, 110, 95, 114, 101, 102, 101, 114, 101, 110, 99,
                101, 114, 101, 102, 101, 114, 101, 110, 99, 101, 99, 104, 97, 110, 110, 101, 108,
                95, 116, 111, 112, 105, 99, 101, 118, 101, 110, 116, 95, 110, 97, 109, 101, 0, 1,
                2, 3
            ])
        )
    }

    #[test]
    fn message_encode_with_reply_with_json_payload() {
        assert_eq!(
            Message::Reply(Reply {
                topic: Topic::from_string("channel_topic".to_string()),
                event: Event::User("event_name".to_string()),
                payload: json_payload(),
                join_reference: "join_reference".into(),
                reference: "reference".into(),
                status: ReplyStatus::Ok,
            })
            .encode()
            .unwrap(),
            SocketMessage::Text("[\"join_reference\",\"reference\",\"channel_topic\",\"event_name\",{\"key\":\"value\"}]".to_string())
        )
    }

    #[test]
    fn message_encode_with_push_with_binary_payload() {
        assert_eq!(
            Message::Push(Push {
                topic: Topic::from_string("channel_topic".to_string()),
                event_payload: EventPayload {
                    event: Event::User("event_name".to_string()),
                    payload: binary_payload(),
                },
                join_reference: "join_reference".into(),
                reference: Some("reference".into()),
            })
            .encode()
            .unwrap(),
            SocketMessage::Binary(vec![
                0, 14, 9, 13, 10, 106, 111, 105, 110, 95, 114, 101, 102, 101, 114, 101, 110, 99,
                101, 114, 101, 102, 101, 114, 101, 110, 99, 101, 99, 104, 97, 110, 110, 101, 108,
                95, 116, 111, 112, 105, 99, 101, 118, 101, 110, 116, 95, 110, 97, 109, 101, 0, 1,
                2, 3
            ])
        )
    }

    #[test]
    fn message_encode_with_push_with_json_payload() {
        assert_eq!(
            Message::Push(Push {
                topic: Topic::from_string("channel_topic".to_string()),
                event_payload: EventPayload {
                    event: Event::User("event_name".to_string()),
                    payload: json_payload()
                },
                join_reference: "join_reference".into(),
                reference: Some("reference".into()),
            })
            .encode()
            .unwrap(),
            SocketMessage::Text("[\"join_reference\",\"reference\",\"channel_topic\",\"event_name\",{\"key\":\"value\"}]".to_string())
        )
    }

    fn binary_payload() -> Payload {
        vec![0, 1, 2, 3].into()
    }

    fn json_payload() -> Payload {
        json!({ "key": "value" }).into()
    }
}
