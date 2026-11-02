//! Projects service over the verified endpoints (protocol inventory §3.5).
//!
//! Safety rules (project instruction §9): deletion requires an explicit
//! typed confirmation from the caller — the service refuses to call the
//! destructive endpoint without it; activation is bound to one chat.

use a0_api::client::A0Client;
use a0_api::endpoints::projects::{self, ProjectDocument, ProjectSummary};

use crate::error::{ProjectsError, Result};

/// Typed deletion confirmation. Construct deliberately at the UI layer
/// (double-confirm dialog); carries the exact name the user confirmed so a
/// race between dialog and call cannot delete a different project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteConfirmation {
    confirmed_name: String,
}

impl DeleteConfirmation {
    /// Records that the user confirmed deletion of exactly `name`.
    pub fn for_project(name: impl Into<String>) -> Self {
        Self {
            confirmed_name: name.into(),
        }
    }
}

/// Projects feature service.
#[derive(Debug, Default)]
pub struct ProjectsService;

impl ProjectsService {
    /// Lists all projects.
    pub async fn list(client: &A0Client) -> Result<Vec<ProjectSummary>> {
        Ok(projects::list(client).await?)
    }

    /// Loads one project document.
    pub async fn load(client: &A0Client, name: &str) -> Result<ProjectDocument> {
        Self::validate_name(name)?;
        Ok(projects::load(client, name).await?)
    }

    /// Creates or updates a project (`create` chooses endpoint semantics).
    pub async fn save(
        client: &A0Client,
        project: ProjectDocument,
        create: bool,
    ) -> Result<ProjectDocument> {
        Ok(projects::save(client, project, create).await?)
    }

    /// Deletes a project. Requires [`DeleteConfirmation`] for the exact
    /// same name; a mismatch (renamed since the dialog) is refused.
    pub async fn delete(
        client: &A0Client,
        name: &str,
        confirmation: &DeleteConfirmation,
    ) -> Result<()> {
        Self::validate_name(name)?;
        if confirmation.confirmed_name != name {
            return Err(ProjectsError::DeleteNotConfirmed);
        }
        Ok(projects::delete(client, name).await?)
    }

    /// Activates `name` for the given chat context.
    pub async fn activate(client: &A0Client, name: &str, context_id: &str) -> Result<()> {
        Self::validate_name(name)?;
        Ok(projects::activate(client, name, context_id).await?)
    }

    /// Deactivates the project bound to `context_id`.
    pub async fn deactivate(client: &A0Client, context_id: &str) -> Result<()> {
        Ok(projects::deactivate(client, context_id).await?)
    }

    fn validate_name(name: &str) -> Result<()> {
        if name.trim().is_empty() {
            return Err(ProjectsError::InvalidName);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_confirmation_binds_exact_name() {
        let confirmation = DeleteConfirmation::for_project("my-project");
        assert_eq!(confirmation.confirmed_name, "my-project");
    }

    #[test]
    fn empty_name_is_rejected_before_any_network_call() {
        assert!(ProjectsService::validate_name("").is_err());
        assert!(ProjectsService::validate_name("   ").is_err());
        assert!(ProjectsService::validate_name("real").is_ok());
    }
}
