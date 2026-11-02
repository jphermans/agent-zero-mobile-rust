//! `a0-platform` — native bridge scaffolding for iOS and Android.
//!
//! The full UniFFI bindings are generated on the platform build machines
//! (Xcode / Android Studio with the NDK — see platforms/ios and
//! platforms/android plus scripts/); this crate compiles everywhere and
//! defines the shared types those bindings expose, so the Rust core API is
//! stable before codegen runs.
//!
//! Binding surface (docs/phase1/architecture.md §6):
//! - session: login/restore/refresh/logout over [`a0_auth`], SecretStore
//!   backed by iOS Keychain / Android Keystore,
//! - realtime: [`a0_realtime`] connect/request_state/disconnect with the
//!   event stream bridged to platform observers,
//! - storage: [`a0_storage`] drafts + message cache per profile,
//! - ui model: [`a0_ui`] window classes + theme tokens for the shells.

/// Build information for the diagnostics screen.
pub fn build_info() -> &'static str {
    concat!(
        "agent-zero-mobile-rust ",
        env!("CARGO_PKG_VERSION"),
        " (shared core)",
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn build_info_carries_workspace_version() {
        let info = super::build_info();
        assert!(info.starts_with("agent-zero-mobile-rust 0.1.0"), "{info}");
    }
}
