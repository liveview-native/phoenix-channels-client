use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;

use uuid::Uuid;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Reference(Arc<String>);
impl Reference {
    pub fn new() -> Self {
        Self::prefixed("")
    }

    fn prefixed(prefix: &str) -> Self {
        Self(Arc::new(format!(
            "{}{}",
            prefix,
            Uuid::new_v4()
                .hyphenated()
                .encode_upper(&mut Uuid::encode_buffer())
        )))
    }

    pub fn heartbeat() -> Self {
        Self::prefixed("heartbeat:")
    }
}
impl Debug for Reference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "{:#?}", self.0)
        } else {
            write!(f, "Reference({:#?})", self.0)
        }
    }
}
impl Default for Reference {
    fn default() -> Self {
        Reference(Arc::new(String::new()))
    }
}
impl Display for Reference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl From<Reference> for Arc<String> {
    fn from(reference: Reference) -> Self {
        reference.0.clone()
    }
}
impl From<Reference> for serde_json::Value {
    fn from(reference: Reference) -> Self {
        serde_json::Value::String(reference.0.to_string())
    }
}
impl From<&Reference> for Arc<String> {
    fn from(reference: &Reference) -> Self {
        reference.0.clone()
    }
}
impl From<&Reference> for serde_json::Value {
    fn from(reference: &Reference) -> Self {
        serde_json::Value::String(reference.0.to_string())
    }
}
impl From<&str> for Reference {
    fn from(s: &str) -> Self {
        s.to_string().into()
    }
}
impl From<String> for Reference {
    fn from(string: String) -> Self {
        Reference(string.into())
    }
}
