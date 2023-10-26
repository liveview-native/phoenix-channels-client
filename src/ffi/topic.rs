use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A [Channel](crate::Channel) topic.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, uniffi::Object)]
pub struct Topic(String);

#[uniffi::export]
impl Topic {
    /// Create [Topic] from string.
    #[uniffi::constructor]
    pub fn from_string(topic: String) -> Arc<Self> {
        Arc::new(Topic(topic))
    }
}

impl Debug for Topic {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "{:#?}", self.0)
        } else {
            write!(f, "Topic({:#?})", self.0)
        }
    }
}
impl Display for Topic {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl From<&str> for Topic {
    fn from(s: &str) -> Self {
        s.to_string().into()
    }
}
impl From<String> for Topic {
    fn from(string: String) -> Self {
        Self(string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_deserialize() {
        let topic: Topic = "topic".into();

        assert_eq!(
            topic,
            serde_json::from_value(serde_json::Value::String("topic".to_string())).unwrap()
        )
    }

    #[test]
    fn topic_serialize() {
        let topic: Topic = "topic".into();
        let json = serde_json::to_value(topic).unwrap();

        assert_eq!(json, serde_json::Value::String("topic".to_string()))
    }
}
