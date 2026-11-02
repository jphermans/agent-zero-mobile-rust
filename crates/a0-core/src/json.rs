//! A thin [`serde_json::Value`] newtype with protocol-oriented helpers.
//!
//! `log_progress` is typed as any-JSON by the wire protocol (protocol
//! inventory sec 5.1, `Protocol.swift:47`) and handshake ack `data` payloads are
//! validated as free-form objects (sec 5.3), so the core carries a small JSON
//! wrapper instead of inventing typed structs for unspecified shapes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Keys whose values must be masked by [`JSONValue::redacted_debug`].
/// Matching is ASCII case-insensitive and exact-name only, as specified.
const SENSITIVE_KEYS: [&str; 6] = [
    "token",
    "secret",
    "password",
    "authorization",
    "csrf",
    "api_key",
];

/// Returns `true` when the key name is in the sensitive set (case-insensitive).
fn key_is_sensitive(key: &str) -> bool {
    SENSITIVE_KEYS.iter().any(|s| s.eq_ignore_ascii_case(key))
}

/// Renders `value` into `out`, recursively masking sensitive key values.
fn render_redacted(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push_str(": ");
                if key_is_sensitive(key) {
                    out.push_str("[REDACTED]");
                } else {
                    render_redacted(item, out);
                }
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_redacted(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// A thin newtype around [`serde_json::Value`].
///
/// It serializes transparently, so it is wire-compatible with raw JSON values.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JSONValue(Value);

impl JSONValue {
    /// Wraps a raw [`serde_json::Value`].
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    /// Borrows the inner value.
    pub fn inner(&self) -> &Value {
        &self.0
    }

    /// Consumes the wrapper and returns the inner value.
    pub fn into_inner(self) -> Value {
        self.0
    }

    /// Resolves a dot-separated path against nested objects and arrays.
    ///
    /// Numeric segments index arrays (e.g. `"logs.0.no"`); all other segments
    /// are object keys. Returns a cloned [`JSONValue`] when the path resolves.
    pub fn get_path(&self, path: &str) -> Option<JSONValue> {
        let mut cursor: &Value = &self.0;
        for segment in path.split('.') {
            if segment.is_empty() {
                return None;
            }
            cursor = match cursor {
                Value::Object(map) => map.get(segment)?,
                Value::Array(items) => {
                    let index: usize = segment.parse().ok()?;
                    items.get(index)?
                }
                _ => return None,
            };
        }
        Some(JSONValue(cursor.clone()))
    }

    /// Returns the string value when the value is a JSON string.
    ///
    /// Owned return so the wrapper composes with [`JSONValue::get_path`]
    /// (which yields an owned wrapper per lookup).
    pub fn as_string(&self) -> Option<String> {
        self.0.as_str().map(|s| s.to_string())
    }

    /// Returns the integer value when the value is a JSON integer.
    pub fn as_i64(&self) -> Option<i64> {
        self.0.as_i64()
    }

    /// Renders a `Debug`-style representation with sensitive values masked.
    ///
    /// Values under keys named `token`, `secret`, `password`, `authorization`,
    /// `csrf` or `api_key` (ASCII case-insensitive) are replaced by
    /// `[REDACTED]`. Object keys render in `serde_json`'s deterministic order.
    pub fn redacted_debug(&self) -> String {
        let mut out = String::new();
        render_redacted(&self.0, &mut out);
        out
    }
}

impl From<Value> for JSONValue {
    fn from(value: Value) -> Self {
        Self(value)
    }
}

impl From<JSONValue> for Value {
    fn from(value: JSONValue) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_path_resolves_objects_arrays_and_missing() {
        let value = JSONValue::new(json!({"data": {"logs": [{"no": 0}, {"no": 1}], "seq": 3}}));
        assert_eq!(value.get_path("data.seq").and_then(|v| v.as_i64()), Some(3));
        assert_eq!(
            value.get_path("data.logs.1.no").and_then(|v| v.as_i64()),
            Some(1)
        );
        assert!(value.get_path("data.logs.9").is_none());
        assert!(value.get_path("data.missing").is_none());
        assert!(value.get_path("data.seq.deeper").is_none());
        assert!(value.get_path("").is_none());
    }

    #[test]
    fn as_string_and_as_i64_are_type_strict() {
        let value = JSONValue::new(json!({"s": "x", "n": 7, "f": 1.5}));
        assert_eq!(
            value.get_path("s").and_then(|v| v.as_string()),
            Some("x".to_string())
        );
        assert_eq!(value.get_path("n").and_then(|v| v.as_i64()), Some(7));
        assert_eq!(value.get_path("f").and_then(|v| v.as_i64()), None);
    }

    #[test]
    fn redacted_debug_masks_all_sensitive_key_names() {
        let value = JSONValue::new(json!({
            "token": "t1",
            "TOKEN": "t2",
            "secret": "s1",
            "password": "p1",
            "authorization": "a1",
            "csrf": "c1",
            "api_key": "k1",
            "nested": {"api_key": "k2", "keep": "visible"}
        }));
        let rendered = value.redacted_debug();
        for secret in ["t1", "t2", "s1", "p1", "a1", "c1", "k1", "k2"] {
            assert!(!rendered.contains(secret), "leaked {secret} in {rendered}");
        }
        assert!(rendered.contains("[REDACTED]"));
        assert!(rendered.contains("visible"));
    }

    #[test]
    fn redacted_debug_keeps_non_sensitive_values() {
        let value = JSONValue::new(json!({"context": "ctx", "seq": 2, "ok": true, "none": null}));
        let rendered = value.redacted_debug();
        assert!(rendered.contains("ctx") && rendered.contains("2") && rendered.contains("true"));
    }

    #[test]
    fn transparent_round_trip_preserves_the_value() {
        let original = json!([1, "two", {"three": null}]);
        let wrapped = JSONValue::from(original.clone());
        let encoded = serde_json::to_string(&wrapped).unwrap();
        let decoded: JSONValue = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.inner(), &original);
    }
}
