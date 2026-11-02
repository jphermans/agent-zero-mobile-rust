//! `a0-storage` — encrypted local persistence for the Agent Zero mobile port.
//!
//! One SQLCipher database per server profile (architecture §5.3): drafts,
//! staged attachment metadata, the persisted auth cookie slot (a0-auth §2.3
//! parity), message cache for instant chat-list rendering, and server
//! profiles. Key material arrives through [`key::KeyProvider`] (platform
//! keystore on device) and is never logged or persisted by this crate.
//!
//! ## At-rest encryption modes
//!
//! - Default build: plain bundled SQLite; confidentiality relies on
//!   platform file protection (iOS Complete file protection, Android
//!   file-based encryption; project instruction §13 platform-backed rule).
//! - `--features sqlcipher`: SQLCipher file encryption with a
//!   [`KeyProvider`]-supplied key (requires perl + make at build time for
//!   the vendored OpenSSL). The mobile platform projects enable this where
//!   the toolchain supports it; see docs/phase1/architecture.md §5.3.
//!
//! Sync rusqlite is deliberate: calls are short, UI hosts dispatch them on a
//! background executor, and an async wrapper would only obscure the single
//! writer invariant (a0-core reducer owns app state; this layer only caches).

pub mod database;
pub mod error;
pub mod key;
pub mod schema;

pub use database::Database;
pub use error::{Result, StorageError};
pub use key::{KeyProvider, StaticKeyProvider, UnavailableKeyProvider};
pub use schema::SCHEMA_VERSION;
