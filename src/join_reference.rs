use std::fmt::{Debug, Display, Formatter};

use flexstr::SharedStr;

use crate::reference::Reference;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct JoinReference(Reference);
impl JoinReference {
    pub fn new() -> Self {
        Self(Reference::new())
    }
}
impl Debug for JoinReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "{:#?}", self.0)
        } else {
            write!(f, "JoinReference({:?})", self.0)
        }
    }
}
impl Display for JoinReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl From<&str> for JoinReference {
    fn from(s: &str) -> Self {
        JoinReference(s.into())
    }
}
impl From<String> for JoinReference {
    fn from(string: String) -> Self {
        JoinReference(string.into())
    }
}

impl From<JoinReference> for SharedStr {
    fn from(join_reference: JoinReference) -> Self {
        join_reference.0.into()
    }
}

impl From<JoinReference> for serde_json::Value {
    fn from(join_reference: JoinReference) -> Self {
        serde_json::Value::String(join_reference.to_string())
    }
}
impl From<&JoinReference> for serde_json::Value {
    fn from(join_reference: &JoinReference) -> Self {
        serde_json::Value::String(join_reference.to_string())
    }
}
