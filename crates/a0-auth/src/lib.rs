//! `a0-auth` — Agent Zero authentication & session lifecycle (Phase 1).
//!
//! Binding contract: `docs/phase1/protocol-inventory.md` §1/§2 (login §2.1,
//! bootstrap §2.2, restore/refresh/logout §2.3, cookie header §2.4) and the
//! secret-storage decision of architecture §5.3 (SecretStore trait).
//!
//! Design notes:
//! * This crate is standalone: it carries its own minimal HTTP layer with the
//!   §1 rules (redirects never followed, manual cookie management, headers
//!   `Accept`/`Origin`/`Content-Type`/`X-CSRF-Token`/`Cookie`). It does NOT
//!   depend on `a0-api` (task dependency list excludes it; sharing would also
//!   create a dev-dependency cycle around the wiremock test suites).
//! * Persisted material is EXACTLY one cookie per profile: `{version:1,
//!   profile, name, value, expires}` (§2.3, `SavedAuthentication.swift:5-13`).
//!   Passwords and CSRF tokens are deliberately NOT persisted
//!   (`SavedAuthentication.swift:3-4`).
//! * Cookie values are held in [`zeroize::Zeroizing`] buffers and wiped on
//!   logout/drop (architecture §5.3: memory copies zeroized where feasible).
//! * [`SecretStore`] is an async key/value trait keyed by [`ServerProfileId`]
//!   with `get`/`put`/`delete`; [`InMemorySecretStore`] is the test/reference
//!   impl. Platform impls (iOS Keychain, Android Keystore bridge) plug in
//!   behind the same trait (architecture §5.3 outcome).

#![forbid(unsafe_code)]

pub mod error;
pub mod http;
pub mod login;
pub mod session;
pub mod store;

pub use error::AuthError;
pub use session::{csrf_cookie_name, session_cookie_name, Session};
pub use store::{InMemorySecretStore, SavedCookie, SecretStore, ServerProfileId};
