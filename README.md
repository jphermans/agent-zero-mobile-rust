# Agent Zero Mobile — Rust Edition

A cross-platform mobile companion for self-hosted [Agent Zero](https://github.com/agent0ai/agent-zero) servers, built on a shared Rust core with native iOS (SwiftUI) and Android (Jetpack Compose) frontends.

- **Version:** 0.1.0
- **Status:** Rust core implemented and tested (see test results below); platform projects scaffolded for direct build on a local machine.
- **License:** MIT (see [LICENSE](LICENSE))
- **Reference app:** [TerminallyLazy/a0-iOS](https://github.com/TerminallyLazy/a0-iOS) — analyzed, not copied; all wire behavior re-verified against its Swift sources.

## Test status

Verified on the development host (Linux, rustc stable + zig-cc linker):

```
cargo test --workspace
=> 26 test binaries OK, 122 tests passed, 0 failures
```

No Agent Zero server was contacted during development or testing: every HTTP and Socket.IO behavior is covered by offline wire fixtures (wiremock) transcribed from the phase-1 protocol inventory.

## Workspace layout

| Crate | Responsibility |
| --- | --- |
| `a0-core` | Domain types, wire DTOs, pure sync reducer, delivery FSM, drafts |
| `a0-api` | HTTP client over reqwest/rustls; every verified `/api/*` endpoint; §6 retry policy (reads only, mutations never) |
| `a0-auth` | Login/bootstrap/restore/refresh, session + CSRF handling, SecretStore trait |
| `a0-realtime` | Socket.IO v3 / engine.io **EIO=4** WebSocket-only client, namespace `/ws`, `state_push` sync |
| `a0-storage` | Per-profile SQLite persistence (drafts, attachments, message cache); optional SQLCipher feature |
| `a0-chat` | Markdown-to-native-blocks rendering, composer send flows with delivery tracking |
| `a0-projects` | Projects service with typed delete-confirmation gate |
| `a0-plugins` | Plugin/hub service with no-auto-retry uncertain-result policy |
| `a0-workspace` | WebView handoff URL validation (strict same-origin, https) |
| `a0-ui` | Adaptive window-size classes (600/840 dp), navigation decisions, theme tokens |
| `a0-platform` | Shared types for the UniFFI bindings consumed by both shells |

Dependency direction is acyclic: `a0-platform -> a0-ui -> feature crates -> {a0-core, a0-api, a0-auth, a0-realtime, a0-storage}`. `a0-core` depends only on serde.

## Key protocol facts (verified, not assumed)

- The Agent Zero backend speaks **Socket.IO v3 over engine.io EIO=4** — confirmed from the reference client's own version mapping (`.version(.three)` → `&EIO=4`), not from docs. In EIO=4 the **server** sends pings (`2`) and the client answers pongs (`3`).
- Transport is WebSocket-only (`.forceWebsockets(true)` in the reference); degraded polling uses the HTTP `/api/poll` path, not engine.io long-polling.
- Namespace `/ws`, connect payload `{"csrf_token": ..., "handlers": ["ws_webui"]}`, `state_request` acknowledged with a correlation id and validated via `HandshakeAck::validated` (runtime epoch + seq base).
- Mutations are **never** auto-retried; destructive plugin operations surface `UncertainResult` on timeout for explicit user reconciliation.

## Platform projects

```
platforms/
├── ios/       # AgentZero.xcodeproj (XcodeGen), RustBridge, SwiftUI sources
└── android/   # Gradle project, A0Rust native module, Compose sources
```

Both shells consume the same Rust core through UniFFI-generated bindings (built by `scripts/` during the platform builds) and implement the adaptive layout rules from `a0-ui` (compact < 600 dp, medium 600–839 dp, expanded >= 840 dp; list+detail side by side only when expanded).

### Building on a local PC

Requirements:

- Rust stable via [rustup](https://rustup.rs)
- iOS: Xcode 15+, XcodeGen (`brew install xcodegen`), Rust targets `aarch64-apple-ios` and `aarch64-apple-ios-sim`
- Android: Android Studio with NDK, Rust targets `aarch64-linux-android` (arm64) plus emulator ABIs, cargo-ndk
- Optional: `cargo install uniffi-bindgen` for regenerating bindings

Steps:

```sh
# 1. Core + tests (any machine)
cargo test --workspace

# 2. iOS project
cd platforms/ios && xcodegen generate && open AgentZero.xcodeproj

# 3. Android project
cd platforms/android && ./gradlew :app:assembleDebug
```

CI workflows for all three targets live in [.github/workflows](.github/workflows) (`rust.yml`, `ios.yml`, `android.yml`).

## Documentation

- [docs/phase1/protocol-inventory.md](docs/phase1/protocol-inventory.md) — every endpoint, the auth/CSRF flow, Socket.IO contract, payload fixtures, with file:line citations into the reference sources
- [docs/phase1/architecture.md](docs/phase1/architecture.md) — crate architecture, dependency rules, security model, build integration
- [docs/phase1/feature-parity.md](docs/phase1/feature-parity.md) — 73-feature parity matrix mapped to crates with complexity ratings and the phase roadmap

## Security notes

- Credentials live in iOS Keychain / Android Keystore via the SecretStore abstraction; cookies are zeroized in memory; only one session cookie is persisted per profile (reference parity).
- TLS via rustls with webpki roots; redirects are never followed automatically; CSRF token required on mutating endpoints.
- Workspace WebViews are same-origin gated; no tokens or credentials are ever placed in URLs.
- Without the (optional) `sqlcipher` feature, `a0-storage` relies on platform file protection (iOS Complete protection, Android FBE); enable the feature where the build machine provides perl+make for vendored OpenSSL.

## Known gaps

See the roadmap in [docs/phase1/feature-parity.md](docs/phase1/feature-parity.md). The Rust core and platform scaffolding are complete; voice, A2UI rich-reply rendering, and the workspace WebView surfaces are platform-shell work tracked in phases 5–7 and are **not** claimed as done.
