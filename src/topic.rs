use std::fmt::{Debug, Display, Formatter};

use flexstr::{SharedStr, ToSharedStr};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Topic(SharedStr);

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
        s.to_shared_str().into()
    }
}
impl From<SharedStr> for Topic {
    fn from(shared_str: SharedStr) -> Self {
        Topic(shared_str)
    }
}
impl From<String> for Topic {
    fn from(string: String) -> Self {
        string.to_shared_str().into()
    }
}

impl From<Topic> for SharedStr {
    fn from(topic: Topic) -> Self {
        topic.0
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
