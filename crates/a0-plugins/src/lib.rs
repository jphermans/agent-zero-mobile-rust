//! `a0-plugins` — Agent Zero plugins feature service.
//!
//! Typed wrappers over the verified §3.4 endpoints with the documented
//! uncertain-result policy: destructive installer operations are sent once
//! and never auto-repeated (timeouts surface for explicit reconciliation,
//! project instruction §9).

pub mod error;
pub mod service;

pub use error::{PluginsError, Result};
pub use service::PluginsService;
