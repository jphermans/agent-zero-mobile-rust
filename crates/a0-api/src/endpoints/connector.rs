//! Host connector — inventory §3.8 (`HostConnection.swift:37-151`):
//! `POST /api/plugins/_a0_connector/v1/<route>`. Shared wrapper: socket
//! session guard, standard 401/403 handling, 404 tolerated ONLY where noted,
//! response cap 64 KiB (`HostConnection.swift:142-151`).
//!
//! NOTE (task-brief deviation): the brief lists a `host_viewer` route, but
//! §3.8/§3.10 enumerate exactly {capabilities, host_status,
//! launcher_gateway_status, host_setup, host_task}; `host_viewer` is not in
//! the binding contract. [`raw_route`] provides a generic passthrough so an
//! unknown future route can still be called explicitly.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::A0Client;
use crate::error::ApiError;

const CAP_64KIB: usize = 64 * 1024;
const BASE: &str = "/api/plugins/_a0_connector/v1";

/// §3.8 (`HostConnection.swift:102-105`): `target_id`/`generation` are
/// exactly 64 lowercase-hex characters.
fn validate_hex64(value: &str) -> Result<(), ApiError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(ApiError::Protocol("invalidHex64".to_string()));
    }
    Ok(())
}

/// Generic §3.8 dispatch: session guard, POST, 64 KiB cap. `allow_404`
/// returns `Ok(Value::Null)` when the connector route is missing
/// (`HostConnection.swift:112,147`).
async fn call_route(
    client: &A0Client,
    route: &str,
    payload: Value,
    allow_404: bool,
) -> Result<Value, ApiError> {
    // §2.5 feature-method guard before mutating endpoints.
    client.require_socket_session()?;
    let response: Result<Value, ApiError> = client
        .post_json(&format!("{BASE}/{route}"), &payload, CAP_64KIB)
        .await;
    match response {
        Ok(v) => Ok(v),
        Err(ApiError::Status { code: 404 }) if allow_404 => Ok(Value::Null),
        Err(e) => Err(e),
    }
}

/// §3.8 `capabilities` response (`HostConnection.swift:111-115`):
/// `{"protocol":"a0-connector.v1","features":[…]}`; HTTP 404 ⇒ connector
/// absent → `Ok(None)`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Capabilities {
    pub protocol: String,
    pub features: Vec<String>,
}

/// §3.8 `POST .../capabilities` with `{}`; 404 tolerated → None.
pub async fn capabilities(client: &A0Client) -> Result<Option<Capabilities>, ApiError> {
    match call_route(client, "capabilities", json!({}), true).await? {
        Value::Null => Ok(None),
        v => Ok(Some(serde_json::from_value(v).map_err(|_| {
            ApiError::Protocol("incompatiblePayload".to_string())
        })?)),
    }
}

/// §3.8 per-capability state object (`HostConnection.swift:37-56`):
/// `{"state":"ready|off|unsupported|needs_attention|container","ready":bool}`
/// with the invariant `ready == (state == "ready")`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CapabilityState {
    pub state: String,
    pub ready: bool,
}

fn validate_capability_states(capabilities: &Value) -> Result<(), ApiError> {
    let Some(map) = capabilities.as_object() else {
        return Ok(()); // absent capabilities are tolerated by the wrapper type
    };
    for (_name, entry) in map {
        let state = entry.get("state").and_then(Value::as_str).unwrap_or("");
        let ready = entry.get("ready").and_then(Value::as_bool).unwrap_or(false);
        if !matches!(
            state,
            "ready" | "off" | "unsupported" | "needs_attention" | "container"
        ) {
            return Err(ApiError::Protocol("incompatiblePayload".to_string()));
        }
        if ready != (state == "ready") {
            return Err(ApiError::Protocol("incompatiblePayload".to_string()));
        }
    }
    Ok(())
}

/// §3.8 `host_status` response (`HostConnection.swift:37-56,117-120`).
/// Unspecified inner capability payloads are opaque `Value`s (only the
/// state/ready contract is enforced).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HostStatus {
    pub version: i64,
    pub context_id: String,
    /// ≤64 chars (`HostConnection.swift:117-120`).
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    pub bound: bool,
    pub binding_current: bool,
    /// `browser`, `computer_use`, `files`, `file_write`, `code_execution`.
    pub capabilities: Value,
}

/// §3.8 `POST .../host_status` with `{"context":"<ctx>"}`.
pub async fn host_status(client: &A0Client, context: &str) -> Result<HostStatus, ApiError> {
    let v = call_route(client, "host_status", json!({ "context": context }), false).await?;
    let status: HostStatus = serde_json::from_value(v)
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
    if status.state.len() > 64 {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    if let Some(id) = &status.target_id {
        validate_hex64(id)?;
    }
    if let Some(gen) = &status.generation {
        validate_hex64(gen)?;
    }
    validate_capability_states(&status.capabilities)?;
    Ok(status)
}

/// §3.8 `launcher_gateway_status` gateway object (`HostConnection.swift:57-96`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayInfo {
    pub version: i64,
    pub kind: String,
    pub id: String,
    pub host_label: String,
    pub master_enabled: bool,
    /// `browser`, `computer_use`, `files`, `file_write`, `code_execution`.
    pub scopes: Value,
}

/// §3.8 `launcher_gateway_status` response: `{connected, multiple_hosts?,
/// gateway?}` (parsed `HostConnection.swift:57-96`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LauncherGatewayStatus {
    pub connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiple_hosts: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<GatewayInfo>,
}

/// §3.8 `POST .../launcher_gateway_status` with `{}`.
pub async fn launcher_gateway_status(client: &A0Client) -> Result<LauncherGatewayStatus, ApiError> {
    let v = call_route(client, "launcher_gateway_status", json!({}), false).await?;
    serde_json::from_value(v).map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))
}

/// §3.8 `host_setup` action `status`: the response shape is
/// **unknown** (§3.8/§10: `ComputerSetupSnapshot` struct not read);
/// returned as raw JSON — TODO(doc-gap): type once the struct is inventoried.
/// HTTP 404 tolerated (`HostConnection.swift:122-124`) → `Ok(None)`.
pub async fn host_setup_status(client: &A0Client) -> Result<Option<Value>, ApiError> {
    // TODO(doc-gap): ComputerSetupSnapshot wire shape is unknown (§10).
    match call_route(client, "host_setup", json!({ "action": "status" }), true).await? {
        Value::Null => Ok(None),
        v => Ok(Some(v)),
    }
}

/// §3.8 `host_task` response (`HostConnection.swift:134-141`):
/// `{"ok":true,"context":"<ctx>","message_id":"<id>","queued":bool}`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HostTaskResult {
    pub context: String,
    pub message_id: String,
    pub queued: bool,
}

/// §3.8 `host_task` request (`HostConnection.swift:134-141`). The wire maps
/// `queued` to the STRING `"true"|"false"` in the request (a bool in the
/// response); `capability` is `"browser"` or `"computer_use"`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HostTaskRequest {
    pub context: String,
    pub text: String,
    pub message_id: String,
    pub queued: bool,
    pub target_id: String,
    pub generation: String,
    pub capability: String,
}

impl HostTaskRequest {
    fn validate(&self) -> Result<(), ApiError> {
        validate_hex64(&self.target_id)?;
        validate_hex64(&self.generation)?;
        if !matches!(self.capability.as_str(), "browser" | "computer_use") {
            return Err(ApiError::Protocol("invalidCapability".to_string()));
        }
        Ok(())
    }
}

/// §3.8 `POST .../host_task` (`HostConnection.swift:134-141`).
pub async fn host_task(
    client: &A0Client,
    request: &HostTaskRequest,
) -> Result<HostTaskResult, ApiError> {
    request.validate()?;
    let payload = json!({
        "context": request.context.as_str(),
        "text": request.text.as_str(),
        "message_id": request.message_id.as_str(),
        "queued": if request.queued { "true" } else { "false" },
        "target_id": request.target_id.as_str(),
        "generation": request.generation.as_str(),
        "capability": request.capability.as_str(),
    });
    let v = call_route(client, "host_task", payload, false).await?;
    let ok = v.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let result: HostTaskResult = serde_json::from_value(v.clone())
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
    if !ok || result.context != request.context || result.message_id != request.message_id {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(result)
}

/// Generic passthrough for a connector route not enumerated in §3.8
/// (e.g. the task-brief's `host_viewer`, which is NOT part of the binding
/// contract — TODO(doc-gap): remove once the route set is authoritative).
pub async fn raw_route(client: &A0Client, route: &str, payload: Value) -> Result<Value, ApiError> {
    // TODO(doc-gap): route not present in §3.10 summary; contract unclear.
    call_route(client, route, payload, false).await
}
