use std::fmt::{Debug, Display, Formatter};

use flexstr::{SharedStr, ToSharedStr};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

impl From<Topic> for serde_json::Value {
    fn from(topic: Topic) -> Self {
        topic.0.to_string().into()
    }
}
impl From<&Topic> for serde_json::Value {
    fn from(topic: &Topic) -> Self {
        topic.0.to_string().into()
    }
}
