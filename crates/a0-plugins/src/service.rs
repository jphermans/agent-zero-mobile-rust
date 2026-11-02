//! Plugins service over the verified §3.4 endpoints.
//!
//! Uncertain-result policy (protocol inventory §6, mirrored from the
//! reference app): mutating installer calls (install/uninstall/update) are
//! sent exactly once. On timeout or a non-decidable response the operation
//! is surfaced as [`PluginsError::UncertainResult`] — never automatically
//! repeated, because a delayed server may still execute the first request.

use a0_api::client::A0Client;
use a0_api::endpoints::plugins::{self, InstallerAction, PluginIndex, PluginRow, ToggleStatus};

use crate::error::{PluginsError, Result};

/// Which installer operations must never be auto-retried: everything
/// except the read-only index fetch.
fn is_destructive(action: &InstallerAction) -> bool {
    !matches!(action, InstallerAction::FetchIndex)
}

/// Local operation label (the API crate keeps its formatter private).
fn operation_name(action: &InstallerAction) -> &'static str {
    match action {
        InstallerAction::FetchIndex => "fetch_index",
        InstallerAction::InstallGit { .. } => "install_git",
        InstallerAction::UpdatePlugin => "update_plugin",
    }
}

/// Classifies errors where the server-side completion state is unknowable:
/// timeouts (the request may still execute) and protocol violations (an
/// unparseable response after a mutation). Both forbid auto-retry.
fn is_uncertain(error: &a0_api::ApiError) -> bool {
    error.is_transient() || matches!(error, a0_api::ApiError::Protocol(_))
}

/// Plugins feature service.
#[derive(Debug, Default)]
pub struct PluginsService;

impl PluginsService {
    /// Lists installed plugins (typed rows).
    pub async fn list(client: &A0Client) -> Result<Vec<PluginRow>> {
        Ok(plugins::list(client).await?)
    }

    /// Fetches the Plugin Hub index (capped at 10000 entries).
    pub async fn hub_index(client: &A0Client) -> Result<PluginIndex> {
        Ok(plugins::fetch_index(client).await?)
    }

    /// Reads the enable/disable state of one plugin.
    pub async fn toggle_status(client: &A0Client, plugin_name: &str) -> Result<ToggleStatus> {
        Ok(plugins::get_toggle_status(client, plugin_name).await?)
    }

    /// Performs an installer action. Destructive actions (install,
    /// uninstall, update) are never auto-retried: transport timeouts and
    /// protocol-indecipherable responses surface as
    /// [`PluginsError::UncertainResult`] for explicit user reconciliation.
    pub async fn installer(client: &A0Client, action: InstallerAction) -> Result<()> {
        let destructive = is_destructive(&action);
        let operation = operation_name(&action);
        match plugins::installer_call(client, action).await {
            Ok(_) => Ok(()),
            Err(e) if destructive && is_uncertain(&e) => Err(PluginsError::UncertainResult {
                operation: operation.to_string(),
            }),
            Err(e) => Err(e.into()),
        }
    }
}
