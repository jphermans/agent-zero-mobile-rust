//! Strongly typed string-backed identifiers for the shared core.
//!
//! All ids serialize transparently as plain JSON strings so they can be
//! embedded in wire DTOs without changing the documented format
//! (protocol inventory sec 5; `Snapshot.context` is a plain string on the wire).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Generates a String-backed newtype id with the full shared trait set:
/// `Debug`, `Clone`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`, serde
/// (transparent string), `Display`, `AsRef<str>` and conversions.
macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a new id from any string-like value.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Borrows the underlying string.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the id and returns the underlying string.
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

define_id!(
    /// Identifies a chat (the `context` string of the wire protocol,
    /// protocol inventory sec 3.1/sec 5.1).
    ChatId
);

define_id!(
    /// Identifies a client-generated message (`message_id` for immediate
    /// sends, `item_id` for queued sends, protocol inventory sec 3.1).
    MessageId
);

define_id!(
    /// Identifies an Agent Zero project by its unique non-empty name
    /// (protocol inventory sec 3.3).
    ProjectId
);

define_id!(
    /// Identifies a locally stored server profile (origin + credentials
    /// holder). This is a client-side domain concept; the wire protocol has
    /// no profile key. TODO(doc-gap): profile identity is not specified in
    /// the phase-1 inventory; the type is intentionally opaque.
    ServerProfileId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_string_backed_and_ordered() {
        let a = ChatId::new("ctx-1");
        let b = ChatId::from("ctx-2");
        assert!(a < b);
        assert_eq!(a.as_str(), "ctx-1");
        assert_eq!(a.to_string(), "ctx-1");
        assert_eq!(ChatId::from(a.clone().into_inner()), a);
        assert_eq!(b.into_inner(), "ctx-2");
    }

    #[test]
    fn ids_serialize_as_plain_json_strings() {
        let id = MessageId::new("m-1");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"m-1\"");
        let back: MessageId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn ids_are_hashable_and_usable_as_map_keys() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ServerProfileId::new("p1"));
        set.insert(ServerProfileId::new("p1"));
        assert_eq!(set.len(), 1);
    }
}
