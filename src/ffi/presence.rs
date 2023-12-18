use std::collections::HashMap;

use crate::ffi::json::JSON;
use crate::rust;

/// A Presence tracks connections to a [Channel](crate::Channel), such as users in a chat room.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Presence {
    /// Meta data about this presence
    pub metas: Vec<Meta>,
}
impl From<rust::presence::Presence> for Presence {
    fn from(rust_presence: rust::presence::Presence) -> Self {
        Self {
            metas: rust_presence.metas.into_iter().map(From::from).collect(),
        }
    }
}
impl From<&rust::presence::Presence> for Presence {
    fn from(rust_presence_ref: &rust::presence::Presence) -> Self {
        Self {
            metas: rust_presence_ref.metas.iter().map(From::from).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Meta {
    pub join_reference: String,
    pub others: HashMap<String, JSON>,
}
impl From<rust::presence::Meta> for Meta {
    fn from(rust_meta: rust::presence::Meta) -> Self {
        Self {
            join_reference: rust_meta.join_reference,
            others: rust_meta
                .others
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        }
    }
}
impl From<&rust::presence::Meta> for Meta {
    fn from(rust_meta_ref: &rust::presence::Meta) -> Self {
        Self {
            join_reference: rust_meta_ref.join_reference.clone(),
            others: rust_meta_ref
                .others
                .iter()
                .map(|(key, value)| (key.clone(), value.into()))
                .collect(),
        }
    }
}
