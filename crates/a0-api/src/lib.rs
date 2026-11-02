//! `a0-api` — Agent Zero HTTP wire-protocol client (Phase 1).
//!
//! Binding wire contract: `docs/phase1/protocol-inventory.md` ("the
//! inventory"; section references like §3.1 point at that document, `file:line`
//! citations point at the read-only Swift reference tree). Contract rules
//! enforced here:
//!
//! * Redirects are NEVER followed (§1); a generic 3xx surfaces as
//!   [`ApiError::Status`], a login redirect (same origin, path `/login`) as
//!   [`ApiError::Auth`].
//! * Mutations are never auto-retried (§6); idempotent GETs and the
//!   foreground poll retry at most 4 times on transient transport/5xx
//!   failures with bounded backoff+jitter deterministically seeded from a
//!   caller-supplied `u64`.
//! * Cookies are managed manually and serialized per §2.4: jar cookies
//!   sorted by name with the synthetic `csrf_token_<runtime_id>` cookie
//!   appended LAST.
//! * Every request carries `Accept`, `Origin`, a body `Content-Type`,
//!   `X-CSRF-Token` (once a token is held) and `Cookie` headers (§1,
//!   `HTTPClient.swift:260-268`).
//! * Response bodies are size-capped per §9; non-JSON content types map to
//!   `unexpectedResponse` and decode failures to `incompatiblePayload`
//!   (both [`ApiError::Protocol`]).
//!
//! NOTE on key casing: the task brief said "serde camelCase", but the
//! binding wire keys in the inventory are snake_case (`log_from`,
//! `item_id`, `runtime_id`, …); the wire contract wins — every struct uses
//! the exact keys of the inventory.
//!
//! TLS uses reqwest's rustls backend (reqwest 0.13 default; architecture
//! §5.1). The platform verifier — or a full rustls config override for
//! tests — is wired through [`A0ClientBuilder::tls_adjuster`].

#![forbid(unsafe_code)]

pub mod client;
pub mod endpoints;
pub mod error;
pub mod upload;
pub(crate) mod cookies;
pub(crate) mod util;

pub use client::{A0Client, A0ClientBuilder, Origin, SocketSession};
pub use error::{ApiError, AuthErrorKind, TransportKind};
pub use upload::UploadFile;
