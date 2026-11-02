//! `a0-projects` — Agent Zero projects feature service.
//!
//! Typed wrappers over the verified §3.5 endpoints with UI-safety rules:
//! deletion requires an explicit typed confirmation bound to the exact
//! project name (project instruction §9 "safe project deletion with
//! confirmation"), names are validated before any network call.

pub mod error;
pub mod service;

pub use error::{ProjectsError, Result};
pub use service::{DeleteConfirmation, ProjectsService};
