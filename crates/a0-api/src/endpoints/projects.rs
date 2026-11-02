//! Projects — inventory §3.3 (`Projects.swift:68-178`): single
//! action-dispatched endpoint, envelope `{"ok":bool,"data":…}`, cap 2 MiB,
//! `ok:false` → `serverRejected` (mapped here to a protocol error).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::A0Client;
use crate::error::ApiError;

const CAP_2MIB: usize = 2 * 1024 * 1024;
const MAX_LIST_ITEMS: usize = 10_000; // §9 `Projects.swift:128`

/// §3.3 project list row: `{name,title,description,color}`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectSummary {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

/// §3.3 project document (`Projects.swift:89-119`). Optional maps are opaque
/// `Value`s (only field NAMES are seen, not their internal shapes).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectDocument {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default, rename = "git_url")]
    pub git_url: Option<String>,
    #[serde(default, rename = "include_agents_md")]
    pub include_agents_md: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_structure: Option<Value>,
}

/// §3.3 default file_structure settings (`Projects.swift:68`):
/// `{"enabled":true,"max_depth":5,"max_files":20,"max_folders":20,
/// "max_lines":250,"gitignore":""}`.
pub fn default_file_structure() -> Value {
    json!({
        "enabled": true,
        "max_depth": 5,
        "max_files": 20,
        "max_folders": 20,
        "max_lines": 250,
        "gitignore": "",
    })
}

/// §3.3 project name validation (`Projects.swift:162-165`): non-empty,
/// trimmed, not `.`/`..`, no `/`, no `\\`, no control chars.
pub fn validate_project_name(name: &str) -> Result<(), ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.chars().any(|c| c.is_control())
    {
        return Err(ApiError::Protocol("invalidProjectName".to_string()));
    }
    Ok(())
}

/// §3.3 HTTPS-only git URL validation for `clone` (`Projects.swift:103-108`):
/// no userinfo/query/fragment.
fn validate_git_url(url_text: &str) -> Result<(), ApiError> {
    let parsed = url::Url::parse(url_text)
        .map_err(|_| ApiError::Protocol("invalidGitUrl".to_string()))?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(ApiError::Protocol("invalidGitUrl".to_string()));
    }
    Ok(())
}

/// §3.3 actions (`Projects.swift:89-155`).
pub enum ProjectsAction {
    List,
    Load { name: String },
    Create { project: ProjectDocument },
    Update { project: ProjectDocument },
    Clone { project: ProjectDocument, git_token: String },
    Delete { name: String },
    Activate { name: String, context_id: String },
    Deactivate { context_id: String },
    FileStructure { name: String, settings: Option<Value> },
}

impl ProjectsAction {
    fn name(&self) -> &'static str {
        match self {
            ProjectsAction::List => "list",
            ProjectsAction::Load { .. } => "load",
            ProjectsAction::Create { .. } => "create",
            ProjectsAction::Update { .. } => "update",
            ProjectsAction::Clone { .. } => "clone",
            ProjectsAction::Delete { .. } => "delete",
            ProjectsAction::Activate { .. } => "activate",
            ProjectsAction::Deactivate { .. } => "deactivate",
            ProjectsAction::FileStructure { .. } => "file_structure",
        }
    }

    fn to_payload(&self) -> Result<Value, ApiError> {
        let mut payload = json!({ "action": self.name() });
        match self {
            ProjectsAction::List => {}
            ProjectsAction::Load { name }
            | ProjectsAction::Delete { name }
            | ProjectsAction::Activate { name, .. }
            | ProjectsAction::FileStructure { name, .. } => {
                validate_project_name(name)?;
                payload["name"] = json!(name);
            }
            ProjectsAction::Create { project } | ProjectsAction::Update { project } => {
                validate_project_name(&project.name)?;
                payload["project"] = serde_json::to_value(project)
                    .map_err(|e| ApiError::Protocol(format!("encode: {e}")))?;
            }
            ProjectsAction::Clone { project, git_token } => {
                validate_project_name(&project.name)?;
                if let Some(git_url) = &project.git_url {
                    validate_git_url(git_url)?;
                } else {
                    return Err(ApiError::Protocol("invalidGitUrl".to_string()));
                }
                payload["project"] = serde_json::to_value(project)
                    .map_err(|e| ApiError::Protocol(format!("encode: {e}")))?;
                payload["git_token"] = json!(git_token);
            }
            ProjectsAction::Deactivate { context_id } => {
                payload["context_id"] = json!(context_id);
            }
            ProjectsAction::Activate { context_id, .. } => {
                payload["context_id"] = json!(context_id);
            }
            ProjectsAction::FileStructure { settings, .. } => {
                if let Some(s) = settings {
                    payload["settings"] = s.clone();
                }
            }
        }
        Ok(payload)
    }
}

/// §3.3 dispatch (`Projects.swift:89-125`): posts the action payload and
/// unwraps the `{ok,data}` envelope; `ok:false` → protocol error
/// `serverRejected`.
async fn dispatch(client: &A0Client, action: ProjectsAction) -> Result<Value, ApiError> {
    client.require_socket_session()?;
    let payload = action.to_payload()?;
    let response: Value = client.post_json("/api/projects", &payload, CAP_2MIB).await?;
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if !ok {
        return Err(ApiError::Protocol("serverRejected".to_string()));
    }
    Ok(response.get("data").cloned().unwrap_or(Value::Null))
}

/// §3.3 `list`: array (≤10000) of `{name,title,description,color}` with
/// unique non-empty names (`Projects.swift:127-136`).
pub async fn list(client: &A0Client) -> Result<Vec<ProjectSummary>, ApiError> {
    let data = dispatch(client, ProjectsAction::List).await?;
    let Some(rows) = data.as_array() else {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    };
    if rows.len() > MAX_LIST_ITEMS {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let project: ProjectSummary = serde_json::from_value(row.clone())
            .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
        if project.name.is_empty() || out.iter().any(|p: &ProjectSummary| p.name == project.name) {
            return Err(ApiError::Protocol("incompatiblePayload".to_string()));
        }
        out.push(project);
    }
    Ok(out)
}

/// §3.3 `load`: the project document; `data.name` must equal the request
/// (`Projects.swift:137-140`).
pub async fn load(client: &A0Client, name: &str) -> Result<ProjectDocument, ApiError> {
    let data = dispatch(client, ProjectsAction::Load { name: name.to_string() }).await?;
    let project: ProjectDocument = serde_json::from_value(data)
        .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
    if project.name != name {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(project)
}

/// §3.3 `create` / `update`: returns the stored project document.
pub async fn save(client: &A0Client, project: ProjectDocument, create: bool) -> Result<ProjectDocument, ApiError> {
    let action = if create {
        ProjectsAction::Create { project }
    } else {
        ProjectsAction::Update { project }
    };
    let data = dispatch(client, action).await?;
    serde_json::from_value(data).map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))
}

/// §3.3 `clone`: HTTPS-only git_url required; `git_token` added to the
/// request (`Projects.swift:103-119`).
pub async fn clone(client: &A0Client, project: ProjectDocument, git_token: &str) -> Result<ProjectDocument, ApiError> {
    let data = dispatch(client, ProjectsAction::Clone {
        project,
        git_token: git_token.to_string(),
    })
    .await?;
    serde_json::from_value(data).map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))
}

/// §3.3 `delete`: `data` must be the string equal to `<n>`
/// (`Projects.swift:146-148`).
pub async fn delete(client: &A0Client, name: &str) -> Result<(), ApiError> {
    let data = dispatch(client, ProjectsAction::Delete { name: name.to_string() }).await?;
    if data.as_str() != Some(name) {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(())
}

/// §3.3 `activate`: `data` must be `null` (`Projects.swift:149-151`).
pub async fn activate(client: &A0Client, name: &str, context_id: &str) -> Result<(), ApiError> {
    let data = dispatch(client, ProjectsAction::Activate {
        name: name.to_string(),
        context_id: context_id.to_string(),
    })
    .await?;
    if !data.is_null() {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(())
}

/// §3.3 `deactivate`: `data` must be `null`.
pub async fn deactivate(client: &A0Client, context_id: &str) -> Result<(), ApiError> {
    let data = dispatch(client, ProjectsAction::Deactivate { context_id: context_id.to_string() }).await?;
    if !data.is_null() {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(())
}

/// §3.3 `file_structure`: `data` must be a string (`Projects.swift:152-154`).
pub async fn file_structure(client: &A0Client, name: &str, settings: Option<Value>) -> Result<String, ApiError> {
    let data = dispatch(client, ProjectsAction::FileStructure {
        name: name.to_string(),
        settings,
    })
    .await?;
    data.as_str()
        .map(|s| s.to_string())
        .ok_or(ApiError::Protocol("incompatiblePayload".to_string()))
}
