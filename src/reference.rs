use std::fmt::{Debug, Display, Formatter};

use flexstr::{shared_fmt, SharedStr, ToSharedStr};
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Reference(pub SharedStr);
impl Reference {
    pub fn new() -> Self {
        Self::prefixed("")
    }

    fn prefixed(prefix: &str) -> Self {
        Self(shared_fmt!(
            "{}{}",
            prefix,
            Uuid::new_v4()
                .hyphenated()
                .encode_upper(&mut Uuid::encode_buffer())
        ))
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
        Reference("".to_shared_str())
    }
}
impl Display for Reference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl From<&str> for Reference {
    fn from(s: &str) -> Self {
        Reference(s.into())
    }
}
impl From<String> for Reference {
    fn from(string: String) -> Self {
        Reference(string.into())
    }
}

impl From<Reference> for SharedStr {
    fn from(reference: Reference) -> Self {
        reference.0
    }
}
impl From<&Reference> for SharedStr {
    fn from(reference: &Reference) -> Self {
        reference.0.clone()
    }
}

impl From<Reference> for serde_json::Value {
    fn from(reference: Reference) -> Self {
        serde_json::Value::String(reference.0.to_string())
    }
}
impl From<&Reference> for serde_json::Value {
    fn from(reference: &Reference) -> Self {
        serde_json::Value::String(reference.0.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_debug_without_alternate() {
        let reference: Reference = "reference".into();

        assert_eq!(format!("{:?}", reference), "Reference(\"reference\")")
    }

    #[test]
    fn reference_debug_with_alternate() {
        let reference: Reference = "reference".into();

        assert_eq!(format!("{:#?}", reference), "\"reference\"")
    }
}

