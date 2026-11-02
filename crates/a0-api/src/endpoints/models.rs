//! Model config plugin — inventory §3.6 (`ModelPresets.swift:14-198`,
//! `ModelCatalog.swift:10-51`) with the §3.6 403 exception on `set_preset`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::A0Client;
use crate::error::{ApiError, AuthErrorKind};

const CAP_2MIB: usize = 2 * 1024 * 1024;
const OVERRIDE_DISABLED_BODY: &str = "Per-chat override is disabled";

/// §3.6 model slot (`ModelSlot`, `ModelPresets.swift:14-18`): keys
/// `chat, vision, utility, embedding`; slot objects contain `provider`,
/// `name`, optional `kwargs` etc.; `kwargs` is opaque and never stripped
/// (`ModelPresets.swift:37-46,61-63`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelPreset {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utility: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Value>,
}

/// §3.6 `model_presets` `get` response (`ModelPresets.swift:139-168`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelPresetsGet {
    pub presets: Vec<ModelPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_preset: Option<String>,
}

fn validate_presets(presets: &[ModelPreset]) -> Result<(), ApiError> {
    // §3.6 (`ModelPresets.swift:140,151-161`): max 1000 presets, unique
    // lower-cased names; `Default` must carry chat, utility and embedding.
    if presets.len() > 1000 {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    let mut seen = std::collections::HashSet::new();
    for preset in presets {
        let key = preset.name.to_lowercase();
        if key.is_empty() || !seen.insert(key) {
            return Err(ApiError::Protocol("incompatiblePayload".to_string()));
        }
        if preset.name == "Default"
            && (preset.chat.is_none() || preset.utility.is_none() || preset.embedding.is_none())
        {
            return Err(ApiError::Protocol("incompatiblePayload".to_string()));
        }
    }
    Ok(())
}

/// §3.6 `POST /api/plugins/_model_config/model_presets` action `get`.
pub async fn presets_get(
    client: &A0Client,
    context_id: Option<&str>,
) -> Result<ModelPresetsGet, ApiError> {
    client.require_socket_session()?;
    let mut payload = json!({ "action": "get" });
    if let Some(ctx) = context_id {
        payload["context_id"] = json!(ctx);
    }
    let response: Value = client
        .post_json(
            "/api/plugins/_model_config/model_presets",
            &payload,
            CAP_2MIB,
        )
        .await?;
    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(ApiError::Protocol("rejected".to_string()));
    }
    let parsed: ModelPresetsGet = serde_json::from_value(response)
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
    validate_presets(&parsed.presets)?;
    Ok(parsed)
}

/// §3.6 preset rename entry for `save`.
#[derive(Debug, Clone, Serialize)]
pub struct PresetRename {
    pub from: String,
    pub to: String,
}

/// §3.6 `save` (`ModelPresets.swift:63-71`): each preset is stripped of
/// top-level `_`-prefixed keys and `api_key`, likewise inside each slot.
pub fn strip_preset_secrets(preset: &mut Value) {
    if let Some(map) = preset.as_object_mut() {
        let remove: Vec<String> = map
            .keys()
            .filter(|k| k.starts_with('_') || k.as_str() == "api_key")
            .cloned()
            .collect();
        for key in remove {
            map.remove(&key);
        }
        for slot_key in ["chat", "vision", "utility", "embedding"] {
            if let Some(slot) = map.get_mut(slot_key).and_then(Value::as_object_mut) {
                let remove: Vec<String> = slot
                    .keys()
                    .filter(|k| k.starts_with('_') || k.as_str() == "api_key")
                    .cloned()
                    .collect();
                for key in remove {
                    slot.remove(&key);
                }
                // `kwargs` is opaque and NEVER stripped (`ModelPresets.swift:61-63`).
            }
        }
    }
}

/// §3.6 `POST .../model_presets` action `save`: `{ok,"presets":[…]}`.
pub async fn presets_save(
    client: &A0Client,
    mut presets: Vec<Value>,
    renames: Vec<PresetRename>,
) -> Result<Vec<ModelPreset>, ApiError> {
    client.require_socket_session()?;
    for preset in &mut presets {
        strip_preset_secrets(preset);
    }
    let payload = json!({ "action": "save", "presets": presets, "renames": renames });
    let response: Value = client
        .post_json(
            "/api/plugins/_model_config/model_presets",
            &payload,
            CAP_2MIB,
        )
        .await?;
    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(ApiError::Protocol("rejected".to_string()));
    }
    let parsed: Vec<ModelPreset> = response
        .get("presets")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?
        .unwrap_or_default();
    Ok(parsed)
}

/// §3.6 `POST .../model_presets` action `reset`: `{ok,"presets":[…]}`.
pub async fn presets_reset(client: &A0Client) -> Result<Vec<ModelPreset>, ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "action": "reset" });
    let response: Value = client
        .post_json(
            "/api/plugins/_model_config/model_presets",
            &payload,
            CAP_2MIB,
        )
        .await?;
    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(ApiError::Protocol("rejected".to_string()));
    }
    response
        .get("presets")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?
        .ok_or(ApiError::Protocol("incompatiblePayload".to_string()))
}

/// §3.6 `POST .../model_presets` action `select` (`ModelPresets.swift:130-136`):
/// scoped keys `project_name`/`agent_profile` added when provided; response
/// `{ok,"selected_preset":"…"}` must echo the requested name.
pub async fn presets_select(
    client: &A0Client,
    name: &str,
    scope: Option<(&str, &str)>,
    context_id: Option<&str>,
) -> Result<(), ApiError> {
    client.require_socket_session()?;
    let mut payload = json!({ "action": "select", "name": name });
    if let Some((project_name, agent_profile)) = scope {
        payload["project_name"] = json!(project_name);
        payload["agent_profile"] = json!(agent_profile);
    }
    if let Some(ctx) = context_id {
        payload["context_id"] = json!(ctx);
    }
    let response: Value = client
        .post_json(
            "/api/plugins/_model_config/model_presets",
            &payload,
            CAP_2MIB,
        )
        .await?;
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let selected = response
        .get("selected_preset")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !ok || selected != name {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(())
}

/// §3.6 `model_override` `get` response (`ModelPresets.swift:186-190`):
/// `{"allowed":bool,"configured_preset":"…","effective_preset":"…",
/// "override":object|null}`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelOverrideGet {
    pub allowed: bool,
    pub configured_preset: String,
    pub effective_preset: String,
    #[serde(rename = "override")]
    pub override_: Option<Value>,
}

/// §3.6 `POST /api/plugins/_model_config/model_override` action `get`.
pub async fn override_get(
    client: &A0Client,
    context_id: &str,
) -> Result<ModelOverrideGet, ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "action": "get", "context_id": context_id });
    let response: Value = client
        .post_json(
            "/api/plugins/_model_config/model_override",
            &payload,
            CAP_2MIB,
        )
        .await?;
    serde_json::from_value(response)
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))
}

/// §3.6 `model_override` `set_preset` (`ModelPresets.swift:191-198`) WITH the
/// 403 exception — **seen** (`ModelPresets.swift:191-195`): HTTP 403 with body
/// EXACTLY `Per-chat override is disabled` maps to [`AuthErrorKind::OverrideDisabled`]
/// WITHOUT clearing the session. Any other 403 follows the §1 uniform rule
/// (`csrfRejected`, session cleared).
pub async fn override_set_preset(
    client: &A0Client,
    context_id: &str,
    preset_name: &str,
) -> Result<(), ApiError> {
    let payload =
        json!({ "action": "set_preset", "context_id": context_id, "preset_name": preset_name });
    let spec = crate::client::RequestSpec {
        method: reqwest::Method::POST,
        path_and_query: "/api/plugins/_model_config/model_override".to_string(),
        body: crate::client::Body::Json(payload),
        response_cap: CAP_2MIB,
        retry: crate::client::RetryPolicy::Never,
        accept: None,
    };
    client.require_socket_session()?;
    let (status, location, body) = client.execute_preserving_status(spec).await?;
    if (300..400).contains(&status) {
        // §1: a login redirect (same-origin /login Location) → requiresLogin
        // with the session cleared; any other 3xx is a Status error (never
        // followed, §1).
        let login_redirect = location
            .as_deref()
            .and_then(|loc| client.origin().url().join(loc).ok())
            .map(|resolved| client.origin().same_origin(&resolved) && resolved.path() == "/login")
            .unwrap_or(false);
        if login_redirect {
            client.clear_session();
            return Err(ApiError::Auth {
                kind: AuthErrorKind::RequiresLogin,
            });
        }
        return Err(ApiError::Status { code: status });
    }
    if status == 403 {
        let body_text = String::from_utf8_lossy(&body);
        if body_text.trim() == OVERRIDE_DISABLED_BODY {
            return Err(ApiError::Auth {
                kind: AuthErrorKind::OverrideDisabled,
            });
        }
        // §1 uniform 403: session cleared, csrfRejected.
        client.clear_session();
        return Err(ApiError::Auth {
            kind: AuthErrorKind::CsrfRejected,
        });
    }
    if !(200..300).contains(&status) {
        return Err(ApiError::Status { code: status });
    }
    let response: Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let echoed = response
        .get("preset_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !ok || echoed != preset_name {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(())
}

/// §3.6 `model_override` action `clear`: `{ok,"effective_preset":"…"}`.
pub async fn override_clear(client: &A0Client, context_id: &str) -> Result<String, ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "action": "clear", "context_id": context_id });
    let response: Value = client
        .post_json(
            "/api/plugins/_model_config/model_override",
            &payload,
            CAP_2MIB,
        )
        .await?;
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let effective = response
        .get("effective_preset")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !ok || effective.is_empty() {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(effective.to_string())
}

/// §3.6 `model_config_get` response (`ModelCatalog.swift:10-22`):
/// `{"chat_providers":[{"value","label"}],"embedding_providers":[…]}`;
/// max 512 per list; ids unique, non-empty, ≤256 bytes; labels non-empty
/// ≤512 bytes (`ModelCatalog.swift:16-19`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderEntry {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    pub chat_providers: Vec<ProviderEntry>,
    pub embedding_providers: Vec<ProviderEntry>,
}

fn validate_provider_list(list: &[ProviderEntry]) -> Result<(), ApiError> {
    if list.len() > 512 {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    let mut seen = std::collections::HashSet::new();
    for entry in list {
        if entry.value.is_empty()
            || entry.value.len() > 256
            || entry.label.is_empty()
            || entry.label.len() > 512
            || !seen.insert(entry.value.clone())
        {
            return Err(ApiError::Protocol("incompatiblePayload".to_string()));
        }
    }
    Ok(())
}

/// §3.6 `POST /api/plugins/_model_config/model_config_get`: request `{}`.
pub async fn config_get(client: &A0Client) -> Result<ModelConfig, ApiError> {
    client.require_socket_session()?;
    let response: Value = client
        .post_json(
            "/api/plugins/_model_config/model_config_get",
            &json!({}),
            CAP_2MIB,
        )
        .await?;
    let config: ModelConfig = serde_json::from_value(response)
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
    validate_provider_list(&config.chat_providers)?;
    validate_provider_list(&config.embedding_providers)?;
    Ok(config)
}

/// §3.6 `model_search` request (`ModelCatalog.swift:23-32`): provider
/// ≤256 B (lower-cased), api_base ≤4096 B.
#[derive(Debug, Clone)]
pub struct ModelSearchRequest {
    pub provider: String,
    pub query: String,
    /// `chat` or `embedding`.
    pub model_type: &'static str,
    pub api_base: String,
}

/// §3.6 `model_search` response (`ModelCatalog.swift:38-43`): `provider`
/// echoed EXACTLY; max 20000 models; each ≤2048 B; empty model names invalid;
/// optional `error` string.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelSearchResponse {
    pub provider: String,
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// §3.6 `POST /api/plugins/_model_config/model_search`.
pub async fn model_search(
    client: &A0Client,
    request: ModelSearchRequest,
) -> Result<ModelSearchResponse, ApiError> {
    if request.provider.is_empty()
        || request.provider.len() > 256
        || request.api_base.len() > 4096
        || !matches!(request.model_type, "chat" | "embedding")
    {
        return Err(ApiError::Protocol("invalidModelSearchRequest".to_string()));
    }
    client.require_socket_session()?;
    let payload = json!({
        "provider": request.provider,
        "query": request.query,
        "model_type": request.model_type,
        "api_base": request.api_base,
    });
    let response: Value = client
        .post_json(
            "/api/plugins/_model_config/model_search",
            &payload,
            CAP_2MIB,
        )
        .await?;
    let parsed: ModelSearchResponse = serde_json::from_value(response)
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
    if parsed.provider != request.provider {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    if parsed.models.len() > 20_000 || parsed.models.iter().any(|m| m.is_empty() || m.len() > 2048)
    {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(parsed)
}
