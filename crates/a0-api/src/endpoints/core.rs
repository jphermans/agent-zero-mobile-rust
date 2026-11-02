//! Core chat endpoints — inventory §3.1 (`HTTPClient.swift:208-235`,
//! `AgentControls.swift:46-53`) with `StateRequest`/`Snapshot` types from §5.1
//! (`Protocol.swift:39-84`) and fixture §8.4.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::{A0Client, RetryPolicy, RequestSpec, Body};
use crate::error::ApiError;

const CAP_2MIB: usize = 2 * 1024 * 1024;

/// §3.1/§8.4 `StateRequest` (`Protocol.swift:61-84`): `context` is ALWAYS
/// written (JSON null when absent, `ProtocolTests.swift:15`); `forceFull` is
/// NEVER sent (`ProtocolTests.swift:11-18` asserts its absence).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateRequest {
    /// Chat context id, or JSON null for no specific context.
    pub context: Option<String>,
    pub log_from: i64,
    pub notifications_from: i64,
    /// IANA timezone identifier (§10: server-side meaning unknown).
    pub timezone: String,
    pub collections_delta: bool,
}

impl StateRequest {
    /// Fixture §8.4 defaults: null context, zeros, "UTC", delta enabled.
    pub fn new() -> Self {
        Self {
            context: None,
            log_from: 0,
            notifications_from: 0,
            timezone: "UTC".to_string(),
            collections_delta: true,
        }
    }

    pub fn context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    pub fn log_from(mut self, v: i64) -> Self {
        self.log_from = v;
        self
    }

    pub fn notifications_from(mut self, v: i64) -> Self {
        self.notifications_from = v;
        self
    }

    pub fn timezone(mut self, tz: impl Into<String>) -> Self {
        self.timezone = tz.into();
        self
    }

    pub fn collections_delta(mut self, v: bool) -> Self {
        self.collections_delta = v;
        self
    }
}

impl Default for StateRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// §5.1 `LogEntry` (`Protocol.swift:28-37`): `no` required; the rest are
/// optional; `timestamp` is any JSON (number in fixtures); `kvps` is an
/// object when present.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LogEntry {
    pub no: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub entry_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kvps: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agentno: Option<i64>,
}

/// §5.1 `Snapshot` (`Protocol.swift:39-59`; fixtures §8.1/§8.2). `contexts`
/// and `tasks` are arrays or null; notification/context entries are opaque.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Snapshot {
    pub deselect_chat: bool,
    pub context: String,
    #[serde(default)]
    pub contexts: Option<Vec<Value>>,
    #[serde(default)]
    pub tasks: Option<Vec<Value>>,
    #[serde(default)]
    pub logs: Vec<LogEntry>,
    pub log_guid: String,
    pub log_version: i64,
    /// Any JSON (fixture shows a string; §10: other variants unknown).
    pub log_progress: Value,
    pub log_progress_active: bool,
    pub paused: bool,
    #[serde(default)]
    pub notifications: Vec<Value>,
    pub notifications_guid: String,
    pub notifications_version: i64,
}

/// `POST /api/poll` (§3.1, `HTTPClient.swift:208-215`): body `StateRequest`,
/// response `Snapshot` (§5.1). The foreground poll IS a read per §6 —
/// transient failures retry (max 4, table 1/2/4/8 s + seeded jitter); a 403
/// poll failure also clears the session via the §1 uniform handling.
pub async fn poll(client: &A0Client, request: &StateRequest) -> Result<Snapshot, ApiError> {
    let spec = RequestSpec {
        method: reqwest::Method::POST,
        path_and_query: "/api/poll".to_string(),
        body: Body::Json(serde_json::to_value(request).map_err(|e| ApiError::Protocol(format!("encode: {e}")))?),
        response_cap: CAP_2MIB,
        retry: RetryPolicy::TransientRead,
        accept: None,
    };
    client.execute_json(spec).await
}

/// §3.1 `POST /api/chat_create` (`HTTPClient.swift:216-222`): request
/// `{"new_context":"<id>","current_context":""}`; the response must decode
/// `{"ok":true,"ctxid":"<id>"}` with `ctxid` equal to the requested id, else
/// `incompatiblePayload`.
pub async fn chat_create(client: &A0Client, new_context: &str) -> Result<(), ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "new_context": new_context, "current_context": "" });
    let response: Value = client.post_json("/api/chat_create", &payload, CAP_2MIB).await?;
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let ctxid = response.get("ctxid").and_then(Value::as_str).unwrap_or("");
    if !ok || ctxid != new_context {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(())
}

/// §3.1 `POST /api/message_async` JSON variant (immediate send,
/// `HTTPClient.swift:223-235`): `{"context","text","message_id"}`; response
/// must decode `{"context":"<ctx>"}` with the matching context. The multipart
/// attachment variant lives in [`crate::upload::message_async_multipart`] (§7).
pub async fn message_async(client: &A0Client, context: &str, text: &str) -> Result<String, ApiError> {
    client.require_socket_session()?;
    let message_id = client.new_request_id();
    let payload = json!({ "context": context, "text": text, "message_id": message_id });
    let response: Value = client.post_json("/api/message_async", &payload, CAP_2MIB).await?;
    let ctx = response.get("context").and_then(Value::as_str).unwrap_or("");
    if ctx != context {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(message_id)
}

/// §3.1 `POST /api/message_queue_add` request (queued send,
/// `HTTPClient.swift:223-229`; attachments variant `Attachments.swift:79-84`).
#[derive(Debug, Clone)]
pub struct MessageQueueAddRequest {
    pub context: String,
    pub text: String,
    /// Client-generated id echoed verbatim by the response.
    pub item_id: String,
 /// `uploadName`s from a prior `POST /api/upload` (§7); None = bare text.
    pub attachments: Option<Vec<String>>,
}

impl MessageQueueAddRequest {
    pub fn new(context: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            context: context.into(),
            text: text.into(),
            item_id: String::new(),
            attachments: None,
        }
    }

    pub fn item_id(mut self, id: impl Into<String>) -> Self {
        self.item_id = id.into();
        self
    }

    pub fn attachments(mut self, names: Vec<String>) -> Self {
        self.attachments = Some(names);
        self
    }

    fn to_payload(&self) -> Value {
        let mut payload = json!({
            "context": self.context,
            "text": self.text,
            "item_id": self.item_id,
        });
        if let Some(names) = &self.attachments {
            payload["attachments"] = json!(names);
        }
        payload
    }
}

/// §3.1 `POST /api/message_queue_add`: response must be
/// `{"ok":true,"item_id":"<uuid>"}` echoing the client-generated id.
pub async fn message_queue_add(client: &A0Client, request: &MessageQueueAddRequest) -> Result<String, ApiError> {
    client.require_socket_session()?;
    let item_id = if request.item_id.is_empty() {
        client.new_request_id()
    } else {
        request.item_id.clone()
    };
    let mut payload = request.to_payload();
    payload["item_id"] = json!(item_id);
    let response: Value = client.post_json("/api/message_queue_add", &payload, CAP_2MIB).await?;
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let echoed = response.get("item_id").and_then(Value::as_str).unwrap_or("");
    if !ok || echoed != item_id {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(item_id)
}

/// §3.1 `POST /api/message_queue_remove` (`AgentControls.swift:46-53`):
/// request `{"context":"<ctx>"}` (the stop payload is reused); response must
/// be `{"ok":true,"remaining":<int>}`. Ordering invariant (§3.1): send this
/// BEFORE `/api/stop`; a failure here is surfaced after the stop attempt and
/// NEVER retried.
pub async fn message_queue_remove(client: &A0Client, context: &str) -> Result<i64, ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "context": context });
    let response: Value = client.post_json("/api/message_queue_remove", &payload, CAP_2MIB).await?;
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let remaining = response.get("remaining").and_then(Value::as_i64);
    if !ok || remaining.is_none() {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(remaining.unwrap())
}
