//! `a0-platform` — native bridge scaffolding for iOS and Android.
//!
//! The crate compiles everywhere and defines the shared surface the platform
//! shells consume through UniFFI 0.31 (library mode; see `uniffi.toml` and
//! scripts/build-ios.sh / scripts/build-android.sh):
//!
//! Binding surface (docs/phase1/architecture.md §6):
//! - session: login/restore/refresh/logout over a0-auth, SecretStore
//!   backed by iOS Keychain / Android Keystore,
//! - realtime: a0-realtime connect/request_state/disconnect with the
//!   event stream bridged to platform observers,
//! - storage: a0-storage drafts + message cache per profile,
//! - ui model: a0-ui window classes + theme tokens for the shells.

/// Build information for the diagnostics screen.
pub fn build_info() -> &'static str {
    concat!(
        "agent-zero-mobile-rust ",
        env!("CARGO_PKG_VERSION"),
        " (shared core)",
    )
}

/// UniFFI-exported diagnostics entry point (Kotlin package `um.a0.platform`,
/// Swift module `A0Platform`). The session/realtime/storage surfaces are
/// exported here as the platform shells integrate them.
#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn a0_build_info() -> String {
    build_info().to_string()
}

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!("a0_platform");

#[cfg(test)]
mod tests {
    #[test]
    fn build_info_carries_workspace_version() {
        let info = super::build_info();
        assert!(info.starts_with("agent-zero-mobile-rust 0.1.0"), "{info}");
    }
}
