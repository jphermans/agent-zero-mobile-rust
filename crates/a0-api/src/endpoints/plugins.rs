//! Plugins — inventory §3.4 (`Plugins.swift:23-148`, `PluginHub.swift:4-56`)
//! plus the context-window usage endpoint §3.7 (`ContextUsage.swift:35-91`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::A0Client;
use crate::error::ApiError;

const CAP_PLUGIN_MIB: usize = 8 * 1024 * 1024; // §9 `Plugins.swift:111`

fn require_ok(response: Value, success_key: &str) -> Result<Value, ApiError> {
    let ok = response
        .get(success_key)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !ok {
        // §3.4: `ok:false` → `rejected`; installer uses the `success` key.
        return Err(ApiError::Protocol("rejected".to_string()));
    }
    Ok(response)
}

/// §3.4 observed plugin-list row (`Plugins.swift:23-33`). Unspecified fields
/// are tolerated via the `extra` catch-all (serde ignores unknown keys by
/// default, kept explicit for documentation).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginRow {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default, rename = "is_custom")]
    pub is_custom: Option<bool>,
    #[serde(default)]
    pub always_enabled: Option<bool>,
    #[serde(default, rename = "has_main_screen")]
    pub has_main_screen: Option<bool>,
    #[serde(default, rename = "has_config_screen")]
    pub has_config_screen: Option<bool>,
    #[serde(default, rename = "per_project_config")]
    pub per_project_config: Option<bool>,
    #[serde(default, rename = "per_agent_config")]
    pub per_agent_config: Option<bool>,
    #[serde(default, rename = "toggle_state")]
    pub toggle_state: Option<String>,
}

fn valid_plugin_name(name: &str) -> bool {
    // §3.4 (`Plugins.swift:35-40`): `[A-Za-z0-9_-]`, max 160 chars.
    !name.is_empty()
        && name.len() <= 160
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// §3.4 `POST /api/plugins_list` (`Plugins.swift:117-126`): request
/// `{"filter":{"custom":true,"builtin":true}}`; response
/// `{"ok":true,"plugins":[…]}` with max 5000 rows.
pub async fn list(client: &A0Client) -> Result<Vec<PluginRow>, ApiError> {
    let payload = json!({ "filter": { "custom": true, "builtin": true } });
    let response: Value = client
        .post_json("/api/plugins_list", &payload, CAP_PLUGIN_MIB)
        .await?;
    let response = require_ok(response, "ok")?;
    let Some(rows) = response.get("plugins").and_then(Value::as_array) else {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    };
    if rows.len() > 5000 {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let plugin: PluginRow = serde_json::from_value(row.clone())
            .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
        if !valid_plugin_name(&plugin.name) {
            return Err(ApiError::Protocol("incompatiblePayload".to_string()));
        }
        out.push(plugin);
    }
    Ok(out)
}

/// §3.4 `POST /api/plugins` actions (`Plugins.swift:76-100`).
pub enum PluginsAction {
    GetToggleStatus,
    GetDoc {
        /// `readme` or `license`.
        doc: &'static str,
    },
    TogglePlugin {
        enabled: bool,
    },
    DeleteConfig {
        path: String,
    },
    DeletePlugin,
}

impl PluginsAction {
    fn name(&self) -> &'static str {
        match self {
            PluginsAction::GetToggleStatus => "get_toggle_status",
            PluginsAction::GetDoc { .. } => "get_doc",
            PluginsAction::TogglePlugin { .. } => "toggle_plugin",
            PluginsAction::DeleteConfig { .. } => "delete_config",
            PluginsAction::DeletePlugin => "delete_plugin",
        }
    }

    fn extra(&self, payload: &mut Value) {
        match self {
            PluginsAction::GetToggleStatus => {}
            PluginsAction::GetDoc { doc } => payload["doc"] = json!(doc),
            PluginsAction::TogglePlugin { enabled } => {
                payload["enabled"] = json!(enabled);
                payload["clear_overrides"] = json!(false);
            }
            PluginsAction::DeleteConfig { path } => payload["path"] = json!(path),
            PluginsAction::DeletePlugin => {}
        }
    }
}

/// §3.4 `POST /api/plugins` (`Plugins.swift:76-100,104-116`): 404 →
/// `unavailable` (mapped to a protocol error by callers via the status),
/// cap 8 MiB, success key `ok`, `ok:false` → `rejected`. Scoped calls add
/// `project_name`/`agent_profile` (§3.4, `Plugins.swift:41-47,83`).
pub async fn plugins_call(
    client: &A0Client,
    plugin_name: &str,
    action: PluginsAction,
    scope: Option<(&str, &str)>,
) -> Result<Value, ApiError> {
    client.require_socket_session()?;
    if !valid_plugin_name(plugin_name) {
        return Err(ApiError::Protocol("invalidPluginName".to_string()));
    }
    let mut payload = json!({ "plugin_name": plugin_name });
    payload["action"] = json!(action.name());
    action.extra(&mut payload);
    if let Some((project_name, agent_profile)) = scope {
        payload["project_name"] = json!(project_name);
        payload["agent_profile"] = json!(agent_profile);
    }
    let response: Value = client
        .post_json("/api/plugins", &payload, CAP_PLUGIN_MIB)
        .await?;
    require_ok(response, "ok")
}

/// §3.4 toggle-status response (`Plugins.swift:127-132`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToggleStatus {
    /// `enabled` or `disabled`.
    pub status: String,
    #[serde(rename = "loaded_project_name")]
    pub loaded_project_name: Value,
    #[serde(rename = "loaded_agent_profile")]
    pub loaded_agent_profile: Value,
    #[serde(rename = "loaded_path")]
    pub loaded_path: Value,
}

/// §3.4 `get_toggle_status` convenience wrapper.
pub async fn get_toggle_status(
    client: &A0Client,
    plugin_name: &str,
) -> Result<ToggleStatus, ApiError> {
    let response = plugins_call(client, plugin_name, PluginsAction::GetToggleStatus, None).await?;
    serde_json::from_value(response)
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))
}

/// §3.4 `get_doc`: `content` string ≤1 MiB (`Plugins.swift:143-148`).
pub async fn get_doc(
    client: &A0Client,
    plugin_name: &str,
    doc: &'static str,
) -> Result<String, ApiError> {
    let response = plugins_call(client, plugin_name, PluginsAction::GetDoc { doc }, None).await?;
    let content = response
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("");
    if content.len() > 1024 * 1024 {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(content.to_string())
}

/// §3.4 installer route + actions (`PluginHub.swift:4,47-56`); success key is
/// **`success`** instead of `ok` (`Plugins.swift:113`).
pub enum InstallerAction {
    FetchIndex,
    InstallGit {
        /// HTTPS-only URL, no userinfo/query/fragment (`PluginHub.swift:21-25`).
        git_url: String,
    },
    UpdatePlugin,
}

impl InstallerAction {
    fn name(&self) -> &'static str {
        match self {
            InstallerAction::FetchIndex => "fetch_index",
            InstallerAction::InstallGit { .. } => "install_git",
            InstallerAction::UpdatePlugin => "update_plugin",
        }
    }

    fn extra(&self, payload: &mut Value) -> Result<(), ApiError> {
        match self {
            InstallerAction::FetchIndex | InstallerAction::UpdatePlugin => Ok(()),
            InstallerAction::InstallGit { git_url } => {
                let url = url::Url::parse(git_url)
                    .map_err(|_| ApiError::Protocol("invalidGitUrl".to_string()))?;
                if url.scheme() != "https"
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                {
                    return Err(ApiError::Protocol("invalidGitUrl".to_string()));
                }
                payload["git_url"] = json!(git_url);
                Ok(())
            }
        }
    }
}

/// §3.4 `POST /api/plugins/_plugin_installer/plugin_install`.
pub async fn installer_call(client: &A0Client, action: InstallerAction) -> Result<Value, ApiError> {
    client.require_socket_session()?;
    let mut payload = json!({});
    payload["action"] = json!(action.name());
    action.extra(&mut payload)?;
    let response: Value = client
        .post_json(
            "/api/plugins/_plugin_installer/plugin_install",
            &payload,
            CAP_PLUGIN_MIB,
        )
        .await?;
    require_ok(response, "success")
}

/// §3.4 `fetch_index` typed view (`PluginHub.swift:48-55`): `index.plugins`
/// is an id→entry object map (≤10000) with opaque entries plus
/// `installed_plugins`: array of ids. Entry fields are documented as opaque
/// `Value`s because only field NAMES are seen, not enforced types.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginIndex {
    /// Raw `index.plugins` map (id → entry with
    /// `title, description, author, version, github, commit / latest_commit,
    /// updated / latest_commit_timestamp, tags[], suspended`).
    pub index: Value,
    #[serde(rename = "installed_plugins")]
    pub installed_plugins: Vec<String>,
}

/// §3.4 `fetch_index` convenience wrapper (map capped at 10000 entries).
pub async fn fetch_index(client: &A0Client) -> Result<PluginIndex, ApiError> {
    let response = installer_call(client, InstallerAction::FetchIndex).await?;
    let index: PluginIndex = serde_json::from_value(response)
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
    // §3.4: `index.plugins` is the id→entry map, capped at 10000 entries.
    if index
        .index
        .get("plugins")
        .and_then(Value::as_object)
        .map(|o| o.len())
        .unwrap_or(usize::MAX)
        > 10_000
    {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(index)
}

// ---- §3.7 context window usage ------------------------------------------

/// §3.7 response (`ContextUsage.swift:35-91`): `usage` optional/null; when
/// present, exactly the six breakdown keys exist, each a non-negative number
/// summing EXACTLY to `tokens` (`ContextUsage.swift:41-57,70-77`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextWindowUsage {
    pub tokens: i64,
    #[serde(rename = "context_window")]
    pub context_window: i64,
    #[serde(default)]
    pub usage: Option<Value>,
    #[serde(default, rename = "provider_usage")]
    pub provider_usage: Option<ProviderUsage>,
}

/// §3.7 `provider_usage` object.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderUsage {
    #[serde(rename = "input_tokens")]
    pub input_tokens: i64,
    #[serde(rename = "cached_tokens")]
    pub cached_tokens: i64,
    #[serde(rename = "output_tokens")]
    pub output_tokens: i64,
    pub cost: f64,
}

fn validate_usage_breakdown(usage: &Value, tokens: i64) -> Result<(), ApiError> {
    const KEYS: [&str; 6] = [
        "messages",
        "system_tools",
        "skills",
        "mcp_tools",
        "system_prompt",
        "extras",
    ];
    let Some(map) = usage.as_object() else {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    };
    if map.len() != KEYS.len() {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    let mut sum = 0i64;
    for key in KEYS {
        let v = map
            .get(key)
            .and_then(Value::as_f64)
            .filter(|f| *f >= 0.0)
            .ok_or(ApiError::Protocol("incompatiblePayload".to_string()))?;
        sum += v as i64;
    }
    if sum != tokens {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(())
}

/// §3.7 `POST /api/plugins/_context_window/context_window`: request
/// `{"context":"<ctx>"}`; response cap 64 KiB.
pub async fn context_window(
    client: &A0Client,
    context: &str,
) -> Result<ContextWindowUsage, ApiError> {
    client.require_socket_session()?;
    let payload = json!({ "context": context });
    let response: Value = client
        .post_json(
            "/api/plugins/_context_window/context_window",
            &payload,
            64 * 1024,
        )
        .await?;
    let usage: ContextWindowUsage = serde_json::from_value(response)
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
    if let Some(breakdown) = &usage.usage {
        if !breakdown.is_null() {
            validate_usage_breakdown(breakdown, usage.tokens)?;
        }
    }
    Ok(usage)
}
