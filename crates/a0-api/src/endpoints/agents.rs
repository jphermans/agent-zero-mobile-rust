//! Agents & scope options — inventory §3.5 (`Plugins.swift:133-142`):
//! `POST /api/agents` with `{"action":"list"}` and the projects
//! `list_options` variant share the same parser; response shape
//! `{"ok":true,"data":[{"key":"…","label":"…"}],…}`, max 5000 rows,
//! unique keys.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::A0Client;
use crate::error::ApiError;

/// §3.5 row: `{"key":"…","label":"…"}`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct OptionRow {
    pub key: String,
    pub label: String,
}

fn parse_options(response: Value) -> Result<Vec<OptionRow>, ApiError> {
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let Some(rows) = response.get("data").and_then(Value::as_array) else {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    };
    if rows.len() > 5000 {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let parsed: OptionRow = serde_json::from_value(row.clone())
            .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
        if parsed.key.is_empty() {
            return Err(ApiError::Protocol("incompatiblePayload".to_string()));
        }
        if out.iter().any(|r: &OptionRow| r.key == parsed.key) {
            return Err(ApiError::Protocol("incompatiblePayload".to_string()));
        }
        out.push(parsed);
    }
    if !ok {
        return Err(ApiError::Protocol("rejected".to_string()));
    }
    Ok(out)
}

/// §3.5 `POST /api/agents` `{"action":"list"}` → agent profiles list.
pub async fn list(client: &A0Client) -> Result<Vec<OptionRow>, ApiError> {
    let payload = json!({ "action": "list" });
    let response: Value = client
        .post_json("/api/agents", &payload, 2 * 1024 * 1024)
        .await?;
    parse_options(response)
}

/// §3.5 `POST /api/projects` `{"action":"list_options"}` → project options
/// (same parser as [`list`], `Plugins.swift:133-142`).
pub async fn list_project_options(client: &A0Client) -> Result<Vec<OptionRow>, ApiError> {
    let payload = json!({ "action": "list_options" });
    let response: Value = client
        .post_json("/api/projects", &payload, 2 * 1024 * 1024)
        .await?;
    parse_options(response)
}
