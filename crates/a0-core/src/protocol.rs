//! State-sync wire DTOs, transcribed exactly from the phase-1 protocol
//! inventory (`docs/phase1/protocol-inventory.md`).
//!
//! Wire-fidelity notes (all cited):
//! - [`Snapshot`], [`StatePushData`] and [`StateRequest`] use the default
//!   snake_case serde naming, which matches the wire keys verbatim
//!   (inventory sec 5.1, sec 5.2, sec 3.1: `log_from`, `notifications_from`,
//!   `runtime_epoch`, ...).
//! - [`HandshakeAck`] is the one mixed-case structure: `correlationId` and
//!   `handlerId` are camelCase on the wire while its `data.seq_base` key is
//!   snake_case (inventory sec 5.3, fixtures sec 8.5/sec 8.6). `rename_all =
//!   "camelCase"` produces exactly `correlationId`/`handlerId`.
//! - `StateRequest` **always** serializes `context`, as JSON `null` when no
//!   chat is selected, and the crate contains no `forceFull` field at all
//!   (inventory sec 3 intro and sec 3.1; `Protocol.swift:76-83`,
//!   `ProtocolTests.swift:11-18`).
//! - Incremental snapshots reuse the [`Snapshot`] shape: a push is "full"
//!   exactly when `contexts` **and** `tasks` are non-null (inventory sec 5.4,
//!   `SyncReducer.swift:59-61`).
//! - Unknown wire details are modelled with [`non_exhaustive`] and preserved
//!   catch-all extras instead of invented fields (inventory sec 10).

use crate::ids::ChatId;
use crate::json::JSONValue;
use crate::CoreError;
use serde::{Deserialize, Serialize};

// -----------------------------------------------------------------------
// Snapshot (inventory sec 5.1, `Protocol.swift:39-59`, fixtures sec 8.1/sec 8.2)
// -----------------------------------------------------------------------

/// A log entry of the synchronized chat transcript.
///
/// Required key: `no`. All other documented keys are optional
/// (inventory sec 5.1, `Protocol.swift:28-37`). `timestamp` is typed as any
/// JSON (number in fixtures; other variants unknown per inventory sec 10).
/// Unknown additional keys are ignored on decode, never rejected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LogEntry {
    /// Monotonic log sequence number; logs are keyed and sorted by `no`
    /// (inventory sec 5.4, `SyncReducer.swift:65-70`).
    pub no: i64,

    /// Entry id (fixture: `synthetic-user`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Entry type (fixture: `user` / `response`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub entry_type: Option<String>,

    /// Optional heading text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,

    /// Optional body content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Optional key/value payload object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kvps: Option<std::collections::BTreeMap<String, JSONValue>>,

    /// Optional timestamp; any JSON value (inventory sec 5.1, sec 10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<JSONValue>,

    /// Optional agent number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agentno: Option<i64>,
}

/// A notification entry of the versioned notifications collection.
///
/// Described as opaque objects with fixture shape `{id, no, message, read}`
/// (inventory sec 5.1). The fixture fields are typed; any additional server
/// fields are preserved in [`NotificationEntry::extra`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct NotificationEntry {
    /// Notification id; the collection upserts by `id`
    /// (inventory sec 5.4, `SyncReducer.swift:72-79`).
    pub id: String,

    /// Sequence number in the fixture.
    pub no: i64,

    /// Notification text.
    pub message: String,

    /// Read flag (flips between fixtures sec 8.1 and sec 8.2).
    pub read: bool,

    /// Unspecified additional wire fields, preserved verbatim.
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, JSONValue>,
}

/// A chat context entry of the `contexts` collection.
///
/// Described as opaque objects with fixture shape `{id, name, running}`
/// (inventory sec 5.1). Additional server fields are preserved verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ContextSummary {
    /// Context id (equals the chat id on the wire).
    pub id: String,

    /// Display name.
    pub name: String,

    /// Whether the chat is currently running.
    pub running: bool,

    /// Unspecified additional wire fields, preserved verbatim.
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, JSONValue>,
}

/// A task entry of the `tasks` collection.
///
/// TODO(doc-gap): the task entry shape is not specified anywhere in the
/// inventory (sec 10); the fixture only contains an empty array. The entry is
/// therefore a transparent, opaque JSON payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct TaskSummary(pub JSONValue);

/// The state-sync snapshot or delta payload (inventory sec 5.1).
///
/// All listed keys are required on the wire except `contexts`/`tasks`, which
/// are arrays or `null`. Serialization always emits every key, including the
/// collections as `null`, matching the fixture key sets (sec 8.1/sec 8.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Snapshot {
    /// When `true`, the selected chat is deselected and logs are cleared
    /// (inventory sec 5.4, `SyncReducer.swift:62`).
    pub deselect_chat: bool,

    /// The chat this snapshot belongs to.
    pub context: String,

    /// Full contexts collection (full snapshots) or `null` (deltas).
    pub contexts: Option<Vec<ContextSummary>>,

    /// Full tasks collection (full snapshots) or `null` (deltas).
    pub tasks: Option<Vec<TaskSummary>>,

    /// Log entries (incremental upsert by `no`).
    pub logs: Vec<LogEntry>,

    /// Log collection identity; a change forces a full sync (inventory sec 5.4).
    pub log_guid: String,

    /// Log collection version; a regression forces a full sync (sec 5.4).
    pub log_version: u64,

    /// Free-form progress payload; any JSON (fixture: a string) (sec 5.1, sec 10).
    pub log_progress: JSONValue,

    /// Whether progress is currently active.
    pub log_progress_active: bool,

    /// Whether the agent is paused.
    pub paused: bool,

    /// Notification entries (incremental upsert by `id`).
    pub notifications: Vec<NotificationEntry>,

    /// Notifications collection identity; a change forces a full sync (sec 5.4).
    pub notifications_guid: String,

    /// Notifications collection version; regression forces a full sync (sec 5.4).
    pub notifications_version: u64,
}

impl Snapshot {
    /// Returns `true` when this snapshot carries the full collections.
    ///
    /// Per inventory sec 5.4 (`SyncReducer.swift:59-61`) a snapshot is full
    /// exactly when `contexts` **and** `tasks` are non-null.
    pub fn is_full(&self) -> bool {
        self.contexts.is_some() && self.tasks.is_some()
    }
}

// -----------------------------------------------------------------------
// StatePush (inventory sec 5.2, `Protocol.swift:86-94`, fixture sec 8.8)
// -----------------------------------------------------------------------

/// Inner payload of a `state_push` event (inventory sec 5.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StatePushData {
    /// Runtime epoch the push belongs to (epoch fencing, inventory sec 5.4).
    pub runtime_epoch: String,

    /// Monotonic sequence number; applies only at `seq == sequence + 1`
    /// (inventory sec 5.4, `SyncReducer.swift:42-50`).
    pub seq: u64,

    /// Snapshot-or-delta payload (see [`Snapshot::is_full`]).
    pub snapshot: Snapshot,
}

/// Socket.IO `state_push` event wrapper: `{"data": { ...StatePushData... }}`
/// (inventory sec 5.2, fixture sec 8.8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StatePush {
    /// Wrapped push payload.
    pub data: StatePushData,
}

impl StatePush {
    /// Wraps `snapshot` into a push for `runtime_epoch` at `seq`
    /// (constructor helper; the struct is `#[non_exhaustive]`).
    pub fn new(runtime_epoch: impl Into<String>, seq: u64, snapshot: Snapshot) -> Self {
        Self {
            data: StatePushData {
                runtime_epoch: runtime_epoch.into(),
                seq,
                snapshot,
            },
        }
    }
}

// -----------------------------------------------------------------------
// StateRequest (inventory sec 3 intro, sec 3.1, `Protocol.swift:61-84`)
// -----------------------------------------------------------------------

/// Poll and `state_request` payload (inventory sec 3.1, fixtures sec 8.4/sec 8.5).
///
/// `context` always serializes (JSON `null` when absent). The crate
/// deliberately defines no `forceFull` field: it is never sent
/// (inventory sec 3.1, `ProtocolTests.swift:11-18`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StateRequest {
    /// Selected chat or `null` (always emitted, inventory sec 3 intro).
    pub context: Option<ChatId>,

    /// `0` when a full sync is pending, else the log version (sec 5.4,
    /// `SyncReducer.swift:38-41`).
    pub log_from: u64,

    /// `0` when a full sync is pending, else the notifications version (sec 5.4).
    pub notifications_from: u64,

    /// IANA timezone identifier sent by the client (sec 3.1, sec 10).
    pub timezone: String,

    /// Client default `true`; server-side effect unknown (sec 10).
    pub collections_delta: bool,
}

impl StateRequest {
    /// Builds the request for the given reducer bookkeeping values
    /// (inventory sec 5.4, `SyncReducer.swift:38-41`): from-offsets collapse to
    /// `0` whenever a full sync is pending.
    pub fn from_versions(
        context: Option<ChatId>,
        needs_full_sync: bool,
        log_version: u64,
        notifications_version: u64,
        timezone: impl Into<String>,
        collections_delta: bool,
    ) -> Self {
        Self {
            context,
            log_from: if needs_full_sync { 0 } else { log_version },
            notifications_from: if needs_full_sync { 0 } else { notifications_version },
            timezone: timezone.into(),
            collections_delta,
        }
    }
}

// -----------------------------------------------------------------------
// HandshakeAck (inventory sec 5.3, `Protocol.swift:96-113`, fixtures sec 8.5-sec 8.7)
// -----------------------------------------------------------------------

/// Handler result inside a [`HandshakeAck`].
///
/// `handlerId` is camelCase on the wire (sec 5.3). `data` stays opaque because
/// invalid payloads (fixture sec 8.7) must decode and then fail validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AckResult {
    /// Handler identity, e.g. `ws_webui.WsWebui`.
    pub handler_id: String,

    /// Whether the handler accepted the request.
    pub ok: bool,

    /// Opaque result payload (validated by [`HandshakeAck::validated`]).
    #[serde(default)]
    pub data: Option<JSONValue>,

    /// Application error; must be `null`/absent for a valid ack (sec 5.3).
    #[serde(default)]
    pub error: Option<JSONValue>,
}

/// Socket.IO `state_request` ack envelope (inventory sec 5.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HandshakeAck {
    /// Client correlation id echoed by the server.
    pub correlation_id: String,

    /// Per-handler results.
    pub results: Vec<AckResult>,
}

/// Handler ids accepted in a validated ack (inventory sec 5.3,
/// `Protocol.swift:105-113`).
pub const ACCEPTED_HANDLER_IDS: [&str; 2] = ["ws_webui.WsWebui", "api.ws_webui.WsWebui"];

/// Validated handshake data extracted from an ack (inventory sec 5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AckData {
    /// Non-empty runtime epoch echoed in `data.runtime_epoch`.
    pub runtime_epoch: String,

    /// Sequence base the client should arm its reducer with, `>= 0`.
    pub seq_base: u64,
}

impl HandshakeAck {
    /// Validates the ack against the expected correlation id, implementing
    /// inventory sec 5.3 (`Protocol.swift:105-113`) exactly. Any violation
    /// yields [`CoreError::InvalidHandshake`] (Swift `invalidHandshake`).
    ///
    /// Rules: `correlationId` must match; a result for an accepted handler
    /// ([`ACCEPTED_HANDLER_IDS`]) must exist with `ok: true`; its `error`
    /// must be absent or `null`; `data` must be an object with a non-empty
    /// string `runtime_epoch`, an integer `seq_base >= 0`, and no `code` key
    /// (application errors hidden inside a successful ack are rejected,
    /// fixture sec 8.7 / `ProtocolTests.swift:84-88`).
    pub fn validated(&self, expected_correlation_id: &str) -> Result<AckData, CoreError> {
        if self.correlation_id != expected_correlation_id {
            return Err(CoreError::InvalidHandshake(format!(
                "correlationId mismatch: expected {expected_correlation_id:?}, got {:?}",
                self.correlation_id
            )));
        }

        let result = self
            .results
            .iter()
            .find(|r| r.ok && ACCEPTED_HANDLER_IDS.contains(&r.handler_id.as_str()))
            .ok_or_else(|| {
                CoreError::InvalidHandshake(
                    "no accepted handler result with ok:true (ws_webui.WsWebui | api.ws_webui.WsWebui)"
                        .to_string(),
                )
            })?;

        if let Some(error) = &result.error {
            if !error.inner().is_null() {
                return Err(CoreError::InvalidHandshake(
                    "ack result carries a non-null error".to_string(),
                ));
            }
        }

        let data = result.data.as_ref().ok_or_else(|| {
            CoreError::InvalidHandshake("accepted result has no data object".to_string())
        })?;
        let object = data.inner().as_object().ok_or_else(|| {
            CoreError::InvalidHandshake("ack data is not a JSON object".to_string())
        })?;

        if object.contains_key("code") && !object.get("code").is_some_and(|v| v.is_null()) {
            return Err(CoreError::InvalidHandshake(format!(
                "ack data carries application error code: {:?}",
                object.get("code")
            )));
        }

        let runtime_epoch = object
            .get("runtime_epoch")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CoreError::InvalidHandshake(
                    "ack data.runtime_epoch must be a non-empty string".to_string(),
                )
            })?;

        let seq_base = object
            .get("seq_base")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                CoreError::InvalidHandshake(
                    "ack data.seq_base must be an integer >= 0".to_string(),
                )
            })?;

        Ok(AckData {
            runtime_epoch: runtime_epoch.to_string(),
            seq_base,
        })
    }
}

// -----------------------------------------------------------------------
// Section-8 fixtures (inventory sec 8), verbatim wire data for tests.
// -----------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod fixtures {
    /// sec 8.1 full snapshot (`Tests/A0CoreTests/Fixtures/full.json`).
    pub const FULL_SNAPSHOT: &str = r#"{
  "deselect_chat": false,
  "context": "synthetic-chat",
  "contexts": [{"id": "synthetic-chat", "name": "Synthetic fixture", "running": false}],
  "tasks": [],
  "logs": [
    {"no": 0, "id": "synthetic-user", "type": "user", "heading": "", "content": "Synthetic request", "kvps": {}, "timestamp": 0, "agentno": 0},
    {"no": 1, "id": "synthetic-response", "type": "response", "heading": "", "content": "Working", "kvps": {}, "timestamp": 1, "agentno": 0}
  ],
  "log_guid": "synthetic-log",
  "log_version": 2,
  "log_progress": "Working",
  "log_progress_active": true,
  "paused": false,
  "notifications": [{"id": "synthetic-notice", "no": 0, "message": "Synthetic notice", "read": false}],
  "notifications_guid": "synthetic-notifications",
  "notifications_version": 1
}"#;

    /// sec 8.4 poll request (`ProtocolTests.swift:11-18`).
    pub const POLL_REQUEST: &str =
        r#"{"context": null, "log_from": 0, "notifications_from": 0, "timezone": "UTC", "collections_delta": true}"#;

    /// sec 8.5 `state_request` emit (`Realtime.swift:84-85`); `ts` illustrative,
    /// `correlationId` shown with a concrete UUID placeholder value.
    pub const STATE_REQUEST_EMIT: &str = r#"{"ts": 1730000000000, "data": {"context": "synthetic-chat", "log_from": 0, "notifications_from": 0, "timezone": "Europe/Amsterdam", "collections_delta": true}, "correlationId": "00000000-0000-4000-8000-000000000001"}"#;

    /// sec 8.6 valid handshake ack (`ProtocolTests.swift:78-83`).
    pub const HANDSHAKE_ACK_VALID: &str = r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"runtime_epoch":"runtime","seq_base":1}}]}"#;

    /// sec 8.7 application error inside a successful ack; must be rejected
    /// (`ProtocolTests.swift:84-88`).
    pub const HANDSHAKE_ACK_APP_ERROR: &str = r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"code":"INVALID_REQUEST"}}]}"#;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// sec 8.2 delta snapshot: identical key set to sec 8.1 with the documented
    /// differences applied (prose-defined fixture, inventory sec 8.2).
    fn delta_snapshot_json() -> Value {
        let mut value: Value = serde_json::from_str(fixtures::FULL_SNAPSHOT).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.insert("contexts".into(), Value::Null);
        obj.insert("tasks".into(), Value::Null);
        obj.insert(
            "logs".into(),
            json!([{"no": 1, "id": "synthetic-response", "type": "response", "heading": "", "content": "Completed synthetic response", "kvps": {}, "timestamp": 2, "agentno": 0}]),
        );
        obj.insert("log_version".into(), json!(3));
        obj.insert("log_progress".into(), json!(""));
        obj.insert("log_progress_active".into(), json!(false));
        obj["notifications"][0]["read"] = json!(true);
        obj.insert("notifications_version".into(), json!(2));
        value
    }

    #[test]
    fn full_fixture_decodes_and_is_full() {
        let snapshot: Snapshot = serde_json::from_str(fixtures::FULL_SNAPSHOT).unwrap();
        assert_eq!(snapshot.context, "synthetic-chat");
        assert!(!snapshot.deselect_chat);
        assert_eq!(snapshot.log_version, 2);
        assert_eq!(snapshot.notifications_version, 1);
        assert_eq!(snapshot.logs.len(), 2);
        assert_eq!(snapshot.logs[0].no, 0);
        assert_eq!(snapshot.logs[0].entry_type.as_deref(), Some("user"));
        assert_eq!(snapshot.logs[0].timestamp.as_ref().and_then(|t| t.as_i64()), Some(0));
        assert_eq!(snapshot.notifications[0].id, "synthetic-notice");
        assert!(!snapshot.notifications[0].read);
        assert_eq!(snapshot.log_progress.as_string(), Some("Working".to_string()));
        assert!(snapshot.contexts.as_ref().unwrap()[0].running == false);
        assert!(snapshot.is_full());
    }

    #[test]
    fn full_fixture_round_trips_key_set_exactly() {
        let snapshot: Snapshot = serde_json::from_str(fixtures::FULL_SNAPSHOT).unwrap();
        let encoded: Value = serde_json::to_value(&snapshot).unwrap();
        let original: Value = serde_json::from_str(fixtures::FULL_SNAPSHOT).unwrap();
        let encoded_keys: Vec<_> = encoded.as_object().unwrap().keys().collect();
        let original_keys: Vec<_> = original.as_object().unwrap().keys().collect();
        assert_eq!(encoded_keys, original_keys, "wire key set must be identical");
        assert_eq!(encoded, original, "full fixture must round-trip value-exact");
    }

    #[test]
    fn delta_fixture_decodes_with_null_collections() {
        let delta_json = delta_snapshot_json();
        let delta: Snapshot = serde_json::from_value(delta_json.clone()).unwrap();
        assert!(delta.contexts.is_none());
        assert!(delta.tasks.is_none());
        assert!(!delta.is_full());
        assert_eq!(delta.logs.len(), 1);
        assert_eq!(delta.logs[0].content.as_deref(), Some("Completed synthetic response"));
        assert_eq!(delta.log_version, 3);
        assert_eq!(delta.log_progress.as_string(), Some(String::new()));
        assert!(!delta.log_progress_active);
        assert!(delta.notifications[0].read);
        assert_eq!(delta.notifications_version, 2);
        let encoded: Value = serde_json::to_value(&delta).unwrap();
        assert_eq!(encoded, delta_json, "delta must re-serialize null collections");
    }

    #[test]
    fn state_push_wrapper_round_trips_per_fixture_8_8() {
        let wrapper_json = format!(
            r#"{{"data":{{"runtime_epoch":"synthetic-runtime","seq":2,"snapshot":{}}}}}"#,
            fixtures::FULL_SNAPSHOT
        );
        let push: StatePush = serde_json::from_str(&wrapper_json).unwrap();
        assert_eq!(push.data.runtime_epoch, "synthetic-runtime");
        assert_eq!(push.data.seq, 2);
        assert!(push.data.snapshot.is_full());
        assert_eq!(push.data.snapshot.context, "synthetic-chat");
        let encoded = serde_json::to_string(&push).unwrap();
        let decoded: StatePush = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, push);
    }

    #[test]
    fn state_request_serializes_exactly_like_fixture_8_4() {
        let request = StateRequest::from_versions(None, true, 7, 9, "UTC", true);
        let encoded: Value = serde_json::to_value(&request).unwrap();
        let expected: Value = serde_json::from_str(fixtures::POLL_REQUEST).unwrap();
        assert_eq!(encoded, expected);
        let encoded_obj = encoded.as_object().unwrap();
        assert!(encoded_obj.contains_key("context"), "context must always be emitted");
        assert!(!encoded_obj.contains_key("forceFull"), "forceFull must never exist");
    }

    #[test]
    fn state_request_emit_fixture_decodes_payload() {
        let emit: Value = serde_json::from_str(fixtures::STATE_REQUEST_EMIT).unwrap();
        assert_eq!(emit["correlationId"], "00000000-0000-4000-8000-000000000001");
        let request: StateRequest = serde_json::from_value(emit["data"].clone()).unwrap();
        assert_eq!(request.context, Some(ChatId::new("synthetic-chat")));
        assert_eq!(request.timezone, "Europe/Amsterdam");
        assert!(request.collections_delta);
    }

    #[test]
    fn valid_ack_fixture_validates_per_fixture_8_6() {
        let ack: HandshakeAck = serde_json::from_str(fixtures::HANDSHAKE_ACK_VALID).unwrap();
        let data = ack.validated("request-1").unwrap();
        assert_eq!(data.runtime_epoch, "runtime");
        assert_eq!(data.seq_base, 1);
    }

    #[test]
    fn api_prefixed_handler_id_is_also_accepted() {
        let json = r#"{"correlationId":"request-1","results":[{"handlerId":"api.ws_webui.WsWebui","ok":true,"data":{"runtime_epoch":"runtime","seq_base":0}}]}"#;
        let ack: HandshakeAck = serde_json::from_str(json).unwrap();
        assert_eq!(ack.validated("request-1").unwrap().seq_base, 0);
    }

    #[test]
    fn error_null_result_still_validates() {
        let json = r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"runtime_epoch":"e","seq_base":2},"error":null}]}"#;
        let ack: HandshakeAck = serde_json::from_str(json).unwrap();
        assert!(ack.validated("request-1").is_ok());
    }

    #[test]
    fn app_error_inside_successful_ack_is_rejected_per_fixture_8_7() {
        let ack: HandshakeAck = serde_json::from_str(fixtures::HANDSHAKE_ACK_APP_ERROR).unwrap();
        let err = ack.validated("request-1").unwrap_err();
        assert!(matches!(err, CoreError::InvalidHandshake(_)));
        assert!(err.to_string().contains("INVALID_REQUEST"));
    }

    #[test]
    fn validation_rejects_every_documented_violation() {
        let wrong_correlation: HandshakeAck = serde_json::from_str(
            r#"{"correlationId":"other","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"runtime_epoch":"e","seq_base":0}}]}"#,
        )
        .unwrap();
        assert!(wrong_correlation.validated("request-1").is_err());

        let missing_handler: HandshakeAck = serde_json::from_str(
            r#"{"correlationId":"request-1","results":[{"handlerId":"other.Handler","ok":true,"data":{"runtime_epoch":"e","seq_base":0}}]}"#,
        )
        .unwrap();
        assert!(missing_handler.validated("request-1").is_err());

        let not_ok: HandshakeAck = serde_json::from_str(
            r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":false,"data":{"runtime_epoch":"e","seq_base":0}}]}"#,
        )
        .unwrap();
        assert!(not_ok.validated("request-1").is_err());

        let null_data: HandshakeAck = serde_json::from_str(
            r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true}]}"#,
        )
        .unwrap();
        assert!(null_data.validated("request-1").is_err());

        let non_object_data: HandshakeAck = serde_json::from_str(
            r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":"str"}]}"#,
        )
        .unwrap();
        assert!(non_object_data.validated("request-1").is_err());

        let empty_epoch: HandshakeAck = serde_json::from_str(
            r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"runtime_epoch":"","seq_base":0}}]}"#,
        )
        .unwrap();
        assert!(empty_epoch.validated("request-1").is_err());

        let negative_seq_base: HandshakeAck = serde_json::from_str(
            r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"runtime_epoch":"e","seq_base":-1}}]}"#,
        )
        .unwrap();
        assert!(negative_seq_base.validated("request-1").is_err());

        let non_null_error: HandshakeAck = serde_json::from_str(
            r#"{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"runtime_epoch":"e","seq_base":0},"error":"boom"}]}"#,
        )
        .unwrap();
        assert!(non_null_error.validated("request-1").is_err());
    }
}
