//! Agent controls — inventory §3.2 (`AgentControls.swift:9-38,42-67`).
//! All five responses are capped at 2 MiB and guarded by the standard auth
//! handling (`AgentControls.swift:19-21,60-67`) plus the §2.5 socket-session
//! guard (`AgentControls.swift:43,61`). All five are mutations → never
//! retried (§6). Ordering invariant (§3.1): send `message_queue_remove`
//! BEFORE `stop` (see [`crate::endpoints::core::message_queue_remove`]).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::A0Client;
use crate::error::ApiError;

const CAP_2MIB: usize = 2 * 1024 * 1024;

/// §3.2 `POST /api/stop` (`AgentControls.swift:9-21`): request
/// `{"context":"<ctx>"}`; required response keys `context` == ctx and
/// `stopped`: bool. Returns the echoed `stopped` value.
pub async fn stop(client: &A0Client, context: &str) -> Result<bool, ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "context": context });
    let response: Value = client.post_json("/api/stop", &payload, CAP_2MIB).await?;
    let ctx = response.get("context").and_then(Value::as_str).unwrap_or("");
    let stopped = response.get("stopped").and_then(Value::as_bool);
    if ctx != context || stopped.is_none() {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(stopped.unwrap())
}

/// §3.2 `POST /api/pause` (`AgentControls.swift:22-31`): request
/// `{"context":"<ctx>","paused":bool}`; the response key `pause` must equal
/// the requested bool.
pub async fn pause(client: &A0Client, context: &str, paused: bool) -> Result<(), ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "context": context, "paused": paused });
    let response: Value = client.post_json("/api/pause", &payload, CAP_2MIB).await?;
    let echoed = response.get("pause").and_then(Value::as_bool);
    if echoed != Some(paused) {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(())
}

/// §3.2 `POST /api/nudge` (`AgentControls.swift:32-38`): request uses the key
/// `ctxid` (NOTE the key, not `context`); response `ctxid` must equal ctx.
pub async fn nudge(client: &A0Client, context: &str) -> Result<(), ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "ctxid": context });
    let response: Value = client.post_json("/api/nudge", &payload, CAP_2MIB).await?;
    let echoed = response.get("ctxid").and_then(Value::as_str).unwrap_or("");
    if echoed != context {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(())
}

/// §3.2 `POST /api/history_get` response: `history`: string; optional
/// `tokens`: number (`AgentControls.swift:42-50`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HistoryContent {
    pub history: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<f64>,
}

/// §3.2 `POST /api/history_get`: request `{"context":"<ctx>"}`.
pub async fn history_get(client: &A0Client, context: &str) -> Result<HistoryContent, ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "context": context });
    let response: Value = client.post_json("/api/history_get", &payload, CAP_2MIB).await?;
    serde_json::from_value(response).map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))
}

/// §3.2 `POST /api/ctx_window_get` response: `content`: string; optional
/// `tokens`: number (`AgentControls.swift:42-50,60-67`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CtxWindowContent {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<f64>,
}

/// §3.2 `POST /api/ctx_window_get`: request `{"context":"<ctx>"}`.
pub async fn ctx_window_get(client: &A0Client, context: &str) -> Result<CtxWindowContent, ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "context": context });
    let response: Value = client.post_json("/api/ctx_window_get", &payload, CAP_2MIB).await?;
    serde_json::from_value(response).map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))
}
