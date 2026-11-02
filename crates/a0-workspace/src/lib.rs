//! `a0-workspace` — Agent Zero Workspace integration.
//!
//! The workspace tools (File Browser, Browser, Desktop, Editor, plugin
//! panels) are served by the Agent Zero WebUI; the app embeds them through
//! a platform WebView with the session's cookies. This crate owns the
//! security gate: [`handoff::validate_handoff`] decides which URLs may be
//! handed off (strict same-origin, https, no credentials in URLs). Cookie
//! injection, navigation policy and cleanup live in a0-platform bridges.

pub mod error;
pub mod handoff;

pub use error::{Result, WorkspaceError};
pub use handoff::{validate_handoff, HandoffTarget};
