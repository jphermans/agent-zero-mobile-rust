//! Typed endpoint modules — one per §3 domain. Every public function takes
//! `&A0Client` and documents its inventory citation. Request bodies are
//! typed builder-style structs; responses are typed structs matching the
//! exact wire keys of the inventory (snake_case, see lib.rs note).

pub mod agentcontrols;
pub mod agents;
pub mod connector;
pub mod core;
pub mod image;
pub mod models;
pub mod plugins;
pub mod projects;
