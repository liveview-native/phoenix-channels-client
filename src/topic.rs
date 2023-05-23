use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Topic(Arc<String>);

impl AsRef<String> for Topic {
    fn as_ref(&self) -> &String {
        self.0.as_ref()
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
impl From<&str> for Topic {
    fn from(s: &str) -> Self {
        s.to_string().into()
    }
}
impl From<String> for Topic {
    fn from(string: String) -> Self {
        Topic(Arc::new(string))
    }
}
impl From<Topic> for Arc<String> {
    fn from(topic: Topic) -> Self {
        topic.0.clone()
    }
}
impl From<Topic> for serde_json::Value {
    fn from(topic: Topic) -> Self {
        topic.0.to_string().into()
    }
}
impl From<&Topic> for Arc<String> {
    fn from(topic: &Topic) -> Self {
        topic.0.clone()
    }
}
impl From<&Topic> for serde_json::Value {
    fn from(topic: &Topic) -> Self {
        topic.0.to_string().into()
    }
}
impl Display for Topic {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
