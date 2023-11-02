use std::fmt::{Debug, Display, Formatter};

use flexstr::SharedStr;
use serde::{Deserialize, Serialize};

use crate::rust::reference::Reference;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_reference_deserialize() {
        let join_reference: JoinReference = "join_reference".into();

        assert_eq!(
            join_reference,
            serde_json::from_value(serde_json::Value::String("join_reference".to_string()))
                .unwrap()
        )
    }

    #[test]
    fn join_reference_serialize() {
        let join_reference: JoinReference = "join_reference".into();
        let json = serde_json::to_value(join_reference).unwrap();

        assert_eq!(
            json,
            serde_json::Value::String("join_reference".to_string())
        )
    }

    #[test]
    fn join_reference_debug_without_alternate() {
        let join_reference: JoinReference = "join_reference".into();

        assert_eq!(
            format!("{:?}", join_reference),
            "JoinReference(Reference(\"join_reference\"))"
        )
    }

    #[test]
    fn join_reference_debug_with_alternate() {
        let join_reference: JoinReference = "join_reference".into();

        assert_eq!(format!("{:#?}", join_reference), "\"join_reference\"")
    }
}
