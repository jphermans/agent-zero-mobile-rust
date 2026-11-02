# Agent Zero Mobile — Rust-first Cross-Platform Architecture (Phase 1)

- **Status:** Draft for review
- **Date:** 2026-11-02 (all sources accessed 2026-11-02 unless dated otherwise)
- **Scope:** iOS + Android app over the Agent Zero backend. UI framework decision, Rust workspace architecture, dependency evaluation, concurrency model, security architecture, build/CI integration.
- **Method:** Capabilities verified via live web search on 2026-11-02; citations are inline with access dates. Items that could **not** be verified are flagged inline and consolidated in §7.2 — they must be re-checked at implementation pin time.

---

## 1. UI Framework Decision

### 1.1 Hard requirements

| Requirement | Priority |
|---|---|
| Accessibility: VoiceOver (iOS) + TalkBack (Android) across **all** flows | **CRITICAL** |
| Dynamic Type / system font scaling | **CRITICAL** (accessibility) |
| Adaptive layouts: compact <600dp, medium 600–839dp, expanded >=840dp | Must |
| Keyboard inset handling (chat input) | Must |
| Foldables (posture/hinge-aware relayout) | Must |
| Native-feel chat UX: text selection, IME, share sheet, notifications, speech | Must |
| Offline hot-reload for development | Nice-to-have |

### 1.2 Slint — verified 2026 state

| Area | 2026 state | Evidence (accessed 2026-11-02) |
|---|---|---|
| Android | Supported: app is a library crate using the `android-activity` crate; `slint::android` module, `android_main` entry; features `backend-android-activity-05/06`. Android support shipped in Slint 1.5 (2024-03-14). | https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/android/ ; https://docs.rs/slint/latest/slint/android/index.html ; https://slint.dev/blog/slint-1.5-released (2024-03-14) |
| iOS | Initial support, tech-preview quality: Rust-only cross-compilation to iPhone/iPad/simulators via Winit backend + Skia renderer. Landed in Slint 1.12. NLnet-funded effort. | https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/ios/ ; https://slint.dev/blog/slint-1.12-released (release date not captured; accessed 2026-11-02) ; https://nlnet.nl/project/SlintiOS/ ; https://github.com/slint-ui/slint/issues/47 |
| Touch handling | Multi-touch via winit (iOS) / android-activity (Android) backends; production maturity on iOS < 1 year. | Mobile guides above |
| Safe-area / keyboard insets | **No first-class safe-area or keyboard-inset API documented in the current mobile guides**; must be handled via manual platform bridging. (Gap.) | Mobile guides above |
| Accessibility | AccessKit-based mapping through the **winit backend** (desktop-oriented) and Qt backend; **mobile screen-reader (VoiceOver/TalkBack) support is tracked in open issue #32 — not production-ready**. | https://github.com/slint-ui/slint/issues/32 (open) ; https://deepwiki.com/slint-ui/slint/13.2-accessibility ; https://github.com/AccessKit/accesskit |
| Text rendering / Unicode | Own text stack over Skia glyphs; adequate Unicode coverage; complex-script/bidi quality below platform text engines for edge locales (verify per target locale). | Slint docs |
| WebView embedding | No supported in-app WebView item for the mobile backends found in current docs — treat as **unavailable**. [unverified-negative] | Mobile guides above |
| Binary size | Compiled .slint DSL + Skia renderer: order-of-magnitude +8–15 MB per platform. [estimate, unverified] | — |
| Licensing | Dual model: GPL and/or royalty-free license, commercial options for embedded/non-GPL distribution. **Exact current terms not verified this session — confirm at slint.dev before adoption.** [unverified] | https://slint.dev |
| Offline hot-reload | Slint Live-Preview / slint-viewer for .slint files during development (desktop-oriented workflow; no on-device hot reload). | https://slint.dev/blog/ |
| Mobile examples | Official Android + iOS guide pages with setup instructions exist and are current (accessed 2026-11-02). | URLs above |

**Plain statement:** For an accessibility-critical mobile app, Slint's 2026 mobile screen-reader story is an open work item (issue #32), iOS support is a ~1-year-old tech preview, and safe-area/keyboard-inset APIs are not first-class. Using Slint would require a **native shell approach** (Swift/Compose containers owning insets, navigation and all a11y-critical screens) — i.e., most of the native UI work would still be needed.

### 1.3 Dioxus — verified 2026 state

| Area | 2026 state | Evidence (accessed 2026-11-02) |
|---|---|---|
| Mobile rendering | Default mobile target = **system WebView** (HTML/CSS UI); native renderer `dioxus-native`/Blitz on WGPU/Vello(+Skia via anyrender) is documented as **beta**: "usable for making apps if you are an early adopter... many bugs and missing features". | https://dioxuslabs.com/learn/0.7/beyond/project_structure/ ; https://lib.rs/crates/blitz-shell ; https://github.com/DioxusLabs/blitz ; https://deepwiki.com/dioxuslabs/blitz/2.3.2-rendering-backends |
| Native components | None — Blitz draws HTML/CSS itself; platform widgets are not used. | Blitz repo/docs above |
| Accessibility | WebView mode: platform web-content a11y (decent). Native/Blitz mode: experimental a11y (`dioxus-native-dom`). | https://lib.rs/crates/dioxus-native-dom |
| Performance | WebView mode carries DOM/JS-bridge overhead for chat-volume UIs; Blitz promising but beta. | Blitz repo |
| Dynamic type / font scaling | Not automatic; manual CSS work. | — |

**Verdict:** best fit for web-background teams. For a native-feeling, a11y-critical chat app both modes compromise a critical axis (performance or a11y maturity).

### 1.4 Native SwiftUI + Jetpack Compose frontends over a UniFFI Rust core

- Accessibility primitives are platform-first-class: VoiceOver/TalkBack traits, focus order, Dynamic Type, reduced motion — required by the critical criterion, zero extra framework work.
- Adaptive layout is first-class: SwiftUI size classes; Jetpack Compose `WindowSizeClass` maps exactly onto the 600/840dp bands; foldables via Jetpack WindowManager posture; keyboard insets via native inset APIs.
- Text rendering uses the platform text engines (bidi, complex scripts, emoji, selection) — important for chat + markdown.
- Maintainability trade-off: two declarative UIs. Mitigated by keeping them thin (views + view-model mapping only) with all domain logic in the shared Rust core; behavior parity enforced by shared core tests.
- Packaging uses standard toolchains (Xcode + Swift package; Gradle + AAR).

### 1.5 Decision table

| Criterion (weight) | Slint 1.12+ | Dioxus 0.7 (WebView / Blitz) | Native over UniFFI |
|---|---|---|---|
| Accessibility (critical) | **Weak on mobile** (issue #32 open) | Medium (web a11y) / experimental native | **Best** |
| Mobile maturity | iOS tech-preview (1.12); Android since 1.5 | WebView mature; Blitz beta | **Mature platform SDKs** |
| Adaptive layout + foldables | Manual | CSS + manual | **First-class APIs** |
| Keyboard insets / Dynamic Type | Manual / gaps | Manual CSS | **Native** |
| Text/Unicode | Custom stack (Skia) | WebView text stack | **Platform text engine** |
| Single UI codebase | **Yes** | **Yes** | No — two thin UIs |
| Licensing | GPL / royalty-free dual [confirm terms] | MIT/Apache | N/A (platform SDKs) |
| Packaging (Xcode/Gradle) | Custom glue | cargo-mobile2 / custom | **Standard toolchains** |
| Offline hot-reload | Live-Preview (desktop-side) | Dioxus hot-reload (good DX) | Xcode Previews / Compose Preview |

### 1.6 Recommendation

**Adopt native SwiftUI (iOS) + Jetpack Compose (Android) frontends over a UniFFI-exposed Rust core.**

Rationale: accessibility is a hard, critical requirement. In the verified 2026 state, Slint fails that axis on mobile (open screen-reader issue #32, iOS tech-preview, no first-class inset APIs) and Dioxus compromises either a11y maturity (Blitz beta) or native feel/performance (WebView). The accepted trade-off is two deliberately thin UI layers; all domain logic, state, drafts, delivery, storage and realtime live in Rust so cross-platform behavior parity is enforced in one place with shared tests.

Re-evaluation triggers: Slint mobile screen-reader support landing (issue #32 closed, mobile AccessKit backend) and iOS leaving tech-preview — Slint could then power secondary screens or a future embedded/desktop target.

### 1.7 Adaptive layout mapping (per platform)

| Requirement | iOS (SwiftUI) | Android (Compose) |
|---|---|---|
| compact <600dp | `horizontalSizeClass == .compact` | WindowSizeClass width <600dp |
| medium 600–839dp | regular class + app-defined band | 600–839dp band |
| expanded >=840dp | regular class + app-defined band | >=840dp band |
| Keyboard insets | keyboard layout guides / `safeAreaInsets` | `imePadding()`, WindowInsets(ime) |
| Foldables | size-class changes on posture | Jetpack WindowManager `WindowLayoutInfo` |
| Dynamic type / font scale | Dynamic Type + `@ScaledMetric` | `sp` units + `fontScale` |

---
## 2. Recommended Crate Architecture (Cargo workspace)

### 2.1 Workspace layout

```
agent-zero-mobile/
├── Cargo.toml                  # [workspace] resolver = "2", shared [workspace.dependencies]
├── rust-toolchain.toml         # pins stable channel; MSRV policy §3.3
├── crates/
│   ├── a0-core/                # domain types, JSONValue, state reducer, drafts, delivery FSM
│   ├── a0-api/                 # HTTP client (reqwest + rustls)
│   ├── a0-auth/                # session, cookie store, CSRF, keystore integration
│   ├── a0-realtime/            # Socket.IO v3 / engine.io EIO=4 client
│   ├── a0-storage/             # SQLite (rusqlite), migrations, encrypted DB decision
│   ├── a0-chat/                # chat feature module (depends: core, api, realtime, storage)
│   ├── a0-projects/            # projects/tasks feature module
│   ├── a0-plugins/             # plugin catalog / settings model
│   ├── a0-workspace/           # workspace/file browser feature module
│   ├── a0-ui/                  # shared UI state models + view-model mapping (no rendering)
│   ├── a0-platform/            # UniFFI bindings + platform bridges (iOS Swift, Android Kotlin)
│   └── a0-platform-bindgen/    # uniffi-bindgen runner (bin) for CI binding generation
├── ios/RustBridge/             # Xcode project: liba0_platform.a + generated Swift + bridging header
└── android/A0Rust/             # Gradle module: liba0_platform.so (per ABI) + generated Kotlin
```

Dependency direction (enforced by CI `cargo-dependencies` check or `cargo-deny`):
`a0-platform → a0-ui → feature modules (a0-chat, a0-projects, a0-plugins, a0-workspace) → a0-core / a0-api / a0-auth / a0-realtime / a0-storage`. Nothing points upward; a0-core depends on nothing but serde/thiserror.

### 2.2 Crate responsibilities

| Crate | Responsibility | Key rules |
|---|---|---|
| **a0-core** | Domain types (`ChatId`, `MessageId`, `ProjectId`), `JSONValue` enum (thin serde_json wrapper), pure state reducer `(State, Action) -> (State, Effects)`, draft composition model, delivery state machine (`Queued → Sending → Sent/Acked → Failed(cap reason)`), clock injection for tests | No I/O types. Fully `no_std`-compatible except `std` collections — keeps reducer testable on any target |
| **a0-api** | HTTP client: `reqwest` with `rustls`, **`default-features = false`**, `redirect(Policy::none())`, **no automatic retry for mutations** (idempotency keys instead; retries only for idempotent GETs with backoff+jitter) | Single `Client` instance; timeouts: connect 5 s, total 30 s (mutation total 60 s) |
| **a0-auth** | Session lifecycle, cookie jar (`reqwest::cookie::Jar` persisted via a0-storage), CSRF token fetch/renewal, secret storage behind a `SecretStore` trait: keyring v3 ecosystem on iOS; **Android via UniFFI bridge to Kotlin Keystore wrapper** (keyring Android crate maturity flagged §5.3) | Secrets never in SQLite plaintext; never logged |
| **a0-realtime** | Socket.IO **v3** client: engine.io **EIO=4** WebSocket-only handshake/polling upgrade to WebSocket, namespace `/ws`, custom auth payload on `connect`, `state_push` event handler feeding reducer Actions. **Custom minimal client** (rationale §3.2) | Reconnect with exponential backoff + jitter; resync via `state_push` after reconnect |
| **a0-storage** | rusqlite (bundled) + `bundled-sqlcipher` feature; embedded migrations; one writer connection + read pool; WAL mode; FTS5 for chat search | All access behind repository traits; no query leaves the crate |
| **a0-chat / a0-projects / a0-plugins / a0-workspace** | Feature modules: orchestrate core/api/storage per domain; expose use-cases to a0-ui | No UI imports below a0-ui |
| **a0-ui** | Platform-agnostic view-models: observable snapshots, list diffs, i18n strings, formatters. **No rendering** — consumed by SwiftUI/Compose layers | Pure Rust; snapshot tests |
| **a0-platform** | UniFFI (proc-macro API) surface: `A0App` facade, `SecretStore`/`Speech`/`Share`/`Notifications` bridge traits implemented natively, `tokio` runtime handle owned here | Sync API at the FFI boundary (no async across FFI); callbacks/events marshalled to main-thread queues |
| **a0-platform-bindgen** | `uniffi-bindgen` binary for `generate swift/library-mode` and `generate kotlin` in CI | Reproducible binding output checked into VCS |

---

## 3. Dependency Evaluation

### 3.1 Dependency table (versions verified via web search 2026-11-02)

| Crate | Version (2026-11) | Features/decision | Evidence |
|---|---|---|---|
| `tokio` | **1.54.1** (MSRV 1.85) | `rt-multi-thread`, `macros`, `sync`, `time`, `signal` | https://docs.rs/crate/tokio/latest ; https://github.com/tokio-rs/tokio/releases (accessed 2026-11-02) |
| `reqwest` | **0.13.x** — rustls is now the **default** TLS backend in 0.13 | `default-features = false`, `rustls-tls`, `json`, `cookies`, `multipart`, `socks`; `redirect(Policy::none())`; no retry layer for mutations | https://seanmonstar.com/blog/reqwest-v013-rustls-default/ ; https://docs.rs/reqwest/latest/reqwest/tls/ (accessed 2026-11-02) |
| `serde` / `serde_json` | serde `1.0.x` / serde_json `1.0.x` (latest stable line; exact patch pinned in workspace.lock at impl time) | `derive` | crates.io stable line, standard pinning |
| Socket.IO client | **Custom EIO=3 client in a0-realtime** (handshake + `websocket`-upgrade transport, namespace `/ws`, auth payload, `state_push`) | **Do not use `rust-socketio` as-is**: its current line targets engine.io **EIO=4 (Socket.IO v4)** — incompatible with the backend's Socket.IO v3. Exact current crate version/protocol flags **could not be verified this session** (see §7.2); verify before considering a fork | Protocol references: https://socket.io/docs/v3/engine-io-protocol/ (v3/EIO=3); decision rationale §3.2 |
| `rusqlite` | **0.3x line** (latest stable; pin at impl time) | `bundled`, `bundled-sqlcipher`, `functions`, `backup`, `serde_json` | https://docs.rs/rusqlite/latest/rusqlite/ ; https://github.com/rusqlite/rusqlite/issues/219 ; `bundled-sqlcipher` builds SQLCipher from source (accessed 2026-11-02) |
| `sqlx` | 0.9 line — **rejected for mobile core**: compile-time checked queries need offline `.sqlx` artifacts + async pool; we need synchronous, embedded, encrypted SQLite | evaluated, rejected | https://docs.rs/sqlx/latest/sqlx/sqlite/ ; https://deepwiki.com/transact-rs/sqlx/4.2-offline-mode-and-sqlx-prepare (accessed 2026-11-02) |
| `keyring` ecosystem | **keyring v3 wrapper** over per-platform store crates: `apple-native-keyring-store` (iOS Keychain: `keychain-service` features) ✅; Android: `android-native-keyring-store` exists (Keystore + SharedPreferences via JNI) — **maturity assessed as insufficient for a security-critical path; prefer Kotlin bridge (§5.3)** | via keyring-core traits | https://github.com/open-source-cooperative/keyring-rs ; https://docs.rs/android-native-keyring-store ; https://deepwiki.com/open-source-cooperative/keyring-rs/5.1-apple-platforms-(macos-and-ios) (accessed 2026-11-02) |
| `uniffi` | **0.31.x** (`uniffi` + `uniffi-build` + `scaffolding-ffi`) | proc-macro API; Swift via `uniffi-bindgen-swift` library mode; Kotlin via `uniffi-bindgen generate kotlin` | https://docs.rs/crate/uniffi_bindgen/latest ; https://mozilla.github.io/uniffi-rs/next/swift/uniffi-bindgen-swift.html (accessed 2026-11-02) |
| `tracing` / `tracing-subscriber` | 0.1.x / 0.3.x latest stable | subscriber with `fmt`, `env-filter`; custom redacting `Layer` (§5.5); export logs to file + in-app viewer | crates.io stable lines |
| `image` | 0.25.x latest stable | `default-features = false`, `png`, `jpeg`, `webp`, `gif`; **bounded decode**: `ImageReader::with_guessed_format` + `limits()` (max 4096×4096, 64 MB alloc) before any decode | https://docs.rs/image (line; accessed 2026-11-02) |
| Markdown rendering | **`pulldown-cmark` 0.13.6** (events → custom lightweight renderer for chat bubbles) + `syntect` **default-features = false** (`default-onload`) only in a0-chat code blocks, with syntax-set loaded lazily. `comrak` (GFM) rejected: heavier, extension set beyond need; GFM tables handled by pulldown-cmark option | pull-parser = streaming, mobile-friendly | https://docs.rs/crate/pulldown-cmark/latest ; https://github.com/pulldown-cmark/pulldown-cmark ; https://github.com/kivikakk/comrak (accessed 2026-11-02) |
| `tauri` / `wry` | Tauri **2.x stable incl. iOS/Android**; **feasible but rejected as UI substrate** (would drag in a webview-frontend stack we are not using). Noted as feasible escape hatch only | evaluated, rejected | https://v2.tauri.app/ ; https://v2.tauri.app/release/ (accessed 2026-11-02) |
| `slint` | 1.12+ (iOS tech-preview line; Android stable since 1.5) | **Not selected** (§1.6); documented for re-evaluation triggers | https://slint.dev/blog/slint-1.12-released ; https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/android/ |
| `parking_lot` | 0.12.x | sync primitives (std fallback kept minimal) | crates.io stable line |
| `thiserror` | 2.x | error enums per crate | crates.io stable line |
| `futures` | 0.3.x | `StreamExt`/stream combinators over reqwest/tokio streams | crates.io stable line |
| `rustls` + `rustls-platform-verifier` | rustls 0.23 line + rustls-platform-verifier 0.5.x | platform root store (§5.1) | https://seanmonstar.com/blog/reqwest-v013-rustls-default/ (backend context); verifier crate docs at pin time [version unverified] |

### 3.2 rust-socketio vs custom EIO=3 client — decision

- The backend runs Socket.IO **v3** (engine.io protocol revision 3, `EIO=3`, `support: ["polling", "websocket"]`). The maintained `rust-socketio` crate implements the current protocol line (engine.io v4 / Socket.IO v4). A v3 backend and v4 client do not handshake.
- The app uses exactly one namespace (`/ws`), one auth payload, and one high-value event stream (`state_push`). The required subset of EIO=3 is small: open handshake (`sid`, `upgrades`, `pingInterval`, `pingTimeout`), `probe` upgrade to websocket, `2::`/`42["event",data]` framing, `40/ws` namespace connect with auth object, periodic `2` ping/`3` pong.
- **Decision:** implement a minimal, well-tested engine.io v3 client in `a0-realtime` (~600–900 LOC) with the full state machine unit-tested against a recorded packet corpus plus a live fixture server. Keep `rust-socketio` behind a feature flag only as a future option if the backend migrates to Socket.IO v4 — at that point the crate becomes viable; **verify its current version and protocol coverage at that time** (could not be verified this session).

### 3.3 MSRV policy

- **Workspace MSRV = 1.85** (matches tokio 1.54's MSRV — the binding constraint in the verified set). Set `rust-version = "1.85"` in every crate manifest; CI runs the oldest supported toolchain plus current stable.
- Rolling policy: MSRV may rise only (a) when a required dependency raises its own, and (b) the new toolchain has been stable ≥ 6 months (tokio's own rule, adopted workspace-wide). MSRV bumps are minor-version changes in our crates and called out in release notes.
- `rust-toolchain.toml` pins stable for development; CI matrix: `stable` + `1.85.0` floor.

---
## 4. Concurrency Model

### 4.1 Tokio runtime placement per platform

- **One** `tokio::runtime::Runtime` (multi-thread, worker count = `std::thread::available_parallelism().min(4)`) is constructed **inside a0-platform** at app init and owned behind `OnceLock<Runtime>`; every FFI entry point that needs async work uses `runtime.block_on(...)` with a **timeout** and returns a snapshot, never a future, across the UniFFI boundary.
- iOS: the runtime runs on its own background threads; **SwiftUI main thread never touches tokio**. UI updates marshal back via callbacks scheduled onto the iOS main queue (`dispatch::Queue::main`) from a dedicated event forwarder task.
- Android: same model; UI updates marshal onto the Android **main looper** via JNI `Handler(Looper.getMainLooper())` posted from the event forwarder task; the JNI environment is attached per-thread for any native callbacks.
- The Slint caveat (single-threaded event loop) does **not** apply under the native-UI decision, but the same rule is kept as an invariant: **UI framework objects are only touched on the platform main thread**; Rust state is shared lock-free toward UI via immutable snapshots.

### 4.2 Channel strategy

| Use case | Channel | Notes |
|---|---|---|
| Realtime events (socket frames → reducer) | `tokio::sync::mpsc` (bounded 1024) | single producer (a0-realtime read loop) → reducer task; backpressure by await |
| State snapshots to UI | `tokio::sync::broadcast` (cap 64, `lagged` tolerated) | every UI platform subscribes; snapshot is cheap `Arc` clone |
| Outgoing mutations / chat sends | `tokio::sync::mpsc` (bounded 256) per stream | delivery FSM owns ordering + idempotency keys |
| Streaming LLM responses | `futures::Stream` piped into `mpsc` chunks | consumer assembles message draft |
| One-shot work (uploads, file ops) | `tokio::task::spawn` + `JoinHandle` with `tokio::time::timeout` | no unbounded spawning: semaphore (max 4 concurrent) |

Rule: **no unbounded channels anywhere**; every channel documents its overflow policy (await-backpressure vs. lagged-skip).

### 4.3 Structured cancellation for streaming

- Every long-running task is spawned under a `tokio_util::sync::CancellationToken` tree: one root token per chat session, child tokens per stream/mutation.
- Cancellation hierarchy: app-level shutdown → navigation-leaves-screen → user-cancels-stream. Cancellation is **cooperative**: socket read loop, HTTP body reads, and DB writer all select on the token.
- Streaming abort path: `CancellationToken.cancel()` → in-flight HTTP stream dropped → partial content kept with delivery state `Failed(cancelled)` and persisted draft — never silently discarded.
- No `abort()` on JoinHandles in production paths (leaves state half-mutated); graceful cancel-first, hard-abort only in tests.

### 4.4 Background/foreground lifecycle hooks

| Platform | Foreground | Background | Hook points in Rust |
|---|---|---|---|
| iOS | `applicationWillEnterForeground` / `sceneWillEnterForeground` → Swift calls `A0App.resume()` | `sceneDidEnterBackground` → `A0App.suspend()`; socket poll paused; iOS gives ~30 s via `beginBackgroundTask` for DB flush | `resume()` resumes realtime loop + drains broadcast backlog; `suspend()` cancels UI-only work, flushes drafts, checkpoints WAL |
| Android | `Activity.onStart` → `A0App.resume()` via UniFFI | `Activity.onStop` → `A0App.suspend()`; foreground-service only while a send/ stream is in flight (notification required) | same Rust hooks; `onTrimMemory` → clears image cache |
| Both | push notifications re-kick socket (`state_push` resync on wake) | token refresh on wake if stale > session TTL | all logic centralized in a0-realtime/a0-core; platform layers only call `resume/suspend` |

Invariant: the reducer is the single writer of app state, so suspend/resume cannot produce divergent UI state — UIs re-render from the latest broadcast snapshot after resume.

---

## 5. Security Architecture

### 5.1 TLS strategy

- **Decision: rustls with platform root certificates** via `rustls-platform-verifier` (iOS Security.framework trust store; Android system trust store). reqwest 0.13 already defaults to rustls (verified §3.1), so the workspace adds `default-features = false` + `rustls-tls` and wires the platform verifier explicitly.
- Rationale: pure-Rust TLS keeps memory safety and symbol size predictable for mobile static linking; platform roots keep enterprise/CA behavior identical to native apps (user-installed CAs, managed profiles).
- **Certificate pinning:** optional, behind a `PinnedVerifier` implementation of the same verifier trait (SPKI-hash pin list shipped in app config). Enabled only for the A0 backend origin, with a kill-switch remote config; leaf rotation handled by pinning the intermediate/SPKI, not the leaf.
- Native TLS (Security.framework / BoringSSL via platform) rejected: loses pure-Rust audit surface; `native-tls` divergence between platforms was the pre-0.13 default problem.

### 5.2 Threat notes
- No cross-origin redirects (server-controlled open-redirect could leak Authorization headers): `redirect(Policy::none())` — also a correctness requirement for the API client.\- Mutations carry idempotency keys; the client never auto-retries non-idempotent calls, eliminating duplicate-send on flaky mobile networks (a security-adjacent reliability guarantee).

### 5.3 Secret storage — keyring vs platform bridges

| Option | iOS | Android | Verdict |
|---|---|---|---|
| keyring v3 ecosystem crates | `apple-native-keyring-store` wraps Keychain Services (verified) | `android-native-keyring-store` wraps Keystore + SharedPreferences via JNI (exists, verified) | **iOS: use it.** Android: **bridge instead** |
| UniFFI platform bridge | Swift Keychain wrapper | Kotlin `AndroidKeystore` + `EncryptedSharedPreferences`/`KeyGenParameterSpec` (hardware-backed) | **Android: use bridge** — direct Keystore parameter control (StrongBox, user-authentication binding), fewer abstraction layers over a security-critical path; `android-native-keyring-store` maturity judged insufficient for secrets on a security-critical path |

Outcome: `SecretStore` trait in a0-auth with two impls: `KeychainSecretStore` (keyring ecosystem, iOS) and `KeystoreBridge` (UniFFI-implemented in Kotlin, Android). Tokens live only in secure storage; memory copies are zeroized on drop where feasible.

### 5.4 SQLite encryption decision

- **Decision: SQLCipher via `rusqlite` `bundled-sqlcipher`** (builds SQLCipher from source with the crate; verified §3.1). AES-256 full-database encryption; key = random 32-byte DB key wrapped by SecretStore (Keystore/Keychain), never derived from a password for this app.
- Why not OS-level-only (iOS Data Protection + Android Keystore-encrypted layer): iOS Data Protection is device-offline-based only (no at-rest crypto against a compromised process sandbox copy), and an Android "encrypted SQL layer" hand-rolled over Keystore means implementing what SQLCipher already hardens (page-level MAC, KDF, salt rotation). SQLCipher also yields identical ciphertext behavior on both platforms, simplifying backup/migration tests.
- Cost accepted: bundled SQLCipher adds build complexity (C build in CI for both platforms — handled in §6) and ~1–2 MB binary size. Alternatives rejected: `rusqlcipher` (unmaintained wrapper vs. first-party feature), OS-only protection (weaker at-rest story).

### 5.5 Log redaction strategy

- Central `tracing` redacting `Layer` wraps all event fields: deny-list of keys (`authorization`, `cookie`, `token`, `password`, `secret`, `csrf`, `api_key`) → replaced with `«redacted»`; message-level scrubbers strip Bearer schemes and JWT-shaped strings (`eyJ…` triple-segment regex) before emit.
- Structured logs carry correlation IDs (`chat_id`, `delivery_id`) but **never** message bodies; bodies only at `DEBUG` in debug builds gated by `#[cfg(debug_assertions)]` + runtime flag.
- Log sink: rotating file in app-sandbox (excluded from iCloud/Android Auto Backup via backup rules) + in-app viewer reading the same file. tracing-subscriber `env-filter` drives verbosity; production default `info` for a0 crates, `warn` for deps.

---

## 6. Build Integration & CI

### 6.1 iOS: staticlib + Swift bridging

- Artifact: `liba0_platform.a` (staticlib) for `aarch64-apple-ios` + `aarch64-apple-ios-sim` + `x86_64-apple-ios` (simulator slice); `cargo lipo`-style thin fat binary or an XCFramework assembled by script.
- Bindings: `uniffi-bindgen-swift` in **library mode** (verified §3.1) generates `A0Platform.swift` + modulemap; a bridging header exposes the C FFI surface to the app target.
- Xcode integration: a Run Script build phase invoking `cargo build --release --target $TARGET` (with `SDKROOT`/`IPHONEOS_DEPLOYMENT_TARGET` env passthrough), then copying the `.a` + Swift into source trees. **Tooling note:** `cargo-xcodebuildgen` could not be verified to exist/maintain this session (§7.2) — default to the **manual Run Script phase**; re-evaluate tooling at impl time.
- Swift Package Manager wrapper (`RustBridge` binaryTarget on the XCFramework) is the preferred consumption shape for the app project.

### 6.2 Android: cdylib + Gradle

- Artifact: `liba0_platform.so` (cdylib) per ABI: `aarch64-linux-android` (primary), `armv7-linux-androideabi`, `x86_64-linux-android` (emulator).
- Bindings: `uniffi-bindgen generate kotlin --library ...` checked into `android/A0Rust/src/main/java/...`.
- Gradle integration: `sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")` with per-ABI dirs populated by `cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release`; `abiFilters` limits release APK splits; 16-KB page alignment flag set for NDK r28+ compatibility.
- JNI note: Kotlin-side callbacks (Keystore bridge, speech, notifications) are registered objects implementing UniFFI callback interfaces; all JNI access on the thread that owns the call, main-looper marshalling for UI events.

### 6.3 CI skeleton (GitHub Actions)

```yaml
# .github/workflows/mobile.yml (skeleton)
name: mobile
jobs:
  checks:            # ubuntu: fmt, clippy -D warnings, test (host), cargo-deny, MSRV job (1.85)
    runs-on: ubuntu-latest
    steps: [checkout, dtolnay/rust-toolchain@stable, swatinem/rust-cache, cargo fmt --check,
            cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace,
            cargo deny check, cargo +1.85.0 check --workspace]
  ios-build:         # macOS runner: device + simulator staticlibs, bindings, xcodebuild build-test
    runs-on: macos-15
    steps: [checkout, dtolnay/rust-toolchain@stable with targets aarch64-apple-ios,
            aarch64-apple-ios-sim, rust-cache, cargo build --release --target aarch64-apple-ios -p a0-platform,
            run a0-platform-bindgen swift, xcodebuild -scheme RustBridge -destination 'generic/platform=iOS']
  android-build:     # ubuntu + NDK: cdylib per ABI, bindings, gradle assemble
    runs-on: ubuntu-latest
    steps: [checkout, dtolnay/rust-toolchain@stable with android targets, android ndk setup (NDK r28),
            cargo ndk build, run a0-platform-bindgen kotlin, gradle :A0Rust:assembleRelease]
```
- Cache: `Swatinem/rust-cache` keyed per job+target. SQLCipher native build is the slow path — cached via `target/` and, optionally, a prebuilt C cache.
- Release signing, store upload, and device-farm test lanes are out of Phase 1 scope.

---

## 7. Evidence & Open Items

### 7.1 Verification method
All capability claims were checked with live web search on 2026-11-02; primary URLs are cited inline per section (docs.slint.dev, docs.rs, github.com releases, slint.dev blog, keyring-rs wiki/DeepWiki, mozilla uniffi-rs docs, seanmonstar blog, v2.tauri.app, socket.io docs).

### 7.2 Items not verifiable this session (re-check at implementation pin time)

| Item | Why open | Action at impl time |
|---|---|---|
| `rust-socketio` exact current version + protocol config | searches returned no crate-version page | confirm on crates.io/docs.rs; decision stands (custom EIO=3) unless backend upgrades to v4 |
| Slint licensing exact current terms | pricing/licensing page not captured | read slint.dev licensing before any Slint adoption |
| `cargo-xcodebuildgen` existence/maintenance | no hits | use manual Run Script phase (default path in §6.1) |
| `rustls-platform-verifier` exact version | line not captured | pin from crates.io |
| Slint binary size on mobile | estimate only (§1.2) | measure if Slint is ever re-evaluated |
| `image`/`rusqlite`/`serde` exact patch versions | stable lines only | pin in workspace `Cargo.lock` |
| keyring Android store crate security review | exists but maturity unassessed in depth | security review before any change of the §5.3 verdict |
