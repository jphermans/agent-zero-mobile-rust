# Agent Zero Mobile — Rust Cross-Platform Port
## Phase 1: Feature Parity Matrix

**Status:** Draft for review · **Date:** 2026-11-02
**Reference:** `ref/a0-iOS` (Swift/SwiftUI, v0.1.0-beta.5): README.md, PRODUCT.md, DESIGN.md, docs/{PROJECTS, PLUGINS, MODEL_PRESETS, GENERATIVE-UI, VOICE, SESSION-RESTORATION, LIVE-VIEWER, CHAT-MEDIA, WORKSPACE}.md, `App/` SwiftUI sources, `Sources/` core modules.
**Target:** iOS/iPadOS/Android companion for an existing authenticated Agent Zero server, in Rust with a shared core and thin platform shells.

**Inherited scope boundary:** agent execution stays on the server; no full-WebUI parity is claimed; drafts, receipts and staged attachments stay on-device; transcripts are not persisted; optional passwords live only in device secure storage; uncertain sends are never replayed automatically.

---

## 0. How to read this matrix

- **Complexity:** `S` ≤ ~3 dev-days, `M` ≤ ~2 dev-weeks, `L` > 2 dev-weeks (shared-core leverage included; UI shells are extra).
- **Rust module** maps to the planned workspace crates (Section 1). Where the reference has a dedicated Swift module, the port target is named.
- **iOS dependency** = iOS-only platform service the reference relies on. **Android equivalent** = the substitute to use.

## 1. Target workspace crate map

| Crate | Responsibility (port of reference) |
| --- | --- |
| `a0-core` | Protocol DTOs, sync reducer, draft/delivery state machine, control journal, QR validation, transcript grouping, send modes, subagent relationships, A2UI document validation, context-usage model |
| `a0-api` | Authenticated HTTPS client: CSRF bootstrap, health, poll, sends (`message_async`), uploads, `message_queue_add`, projects/plugins/presets/context/image endpoints, bounded media downloads |
| `a0-realtime` | Socket.IO-over-WebSocket client, ack validation, bounded handshake buffering, realtime↔poll promotion |
| `a0-auth` | Login, cookie/CSRF session, session restore, account-generation fencing, cookie export for WebView handoff |
| `a0-storage` | Profile store, session archives (drafts, receipts, pending preset names), display preferences, attachment staging files |
| `a0-chat` | Conversation/composer domain: markdown-subset pipeline, activity grouping, follow state, voice draft insertion, preset selection scope, screenshot-card model |
| `a0-projects` | Project documents, typed operations, mutation receipts, delete confirmation model |
| `a0-plugins` | Installed/Hub catalogs, lifecycle commands, thumbnail references, screen routes |
| `a0-workspace` | Embedded-WebUI adapter JS (port of `PluginWebAdapter.js` / `WorkspaceWebAdapter.js`), same-origin navigation/fetch policy, file exports |
| `a0-ui` | Platform-shared view models, design tokens, A2UI widget catalog incl. charts, rich-reply surfaces |
| `a0-platform` | Trait seams per OS: secure storage, camera/QR, speech/TTS, WebView host, pickers, share, clipboard, audio focus, backup-exclusion flags |

## 2. Feature parity matrix

### 2.1 Onboarding, connection & resilience

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Manual origin connect | User enters bare HTTPS origin + credentials; protected CSRF bootstrap; HTTP client never auto-follows redirects or retries mutations | ATS default HTTPS-only | Network security config (cleartext disabled), no-redirect HTTP client policy | `a0-api`, `a0-auth`, `a0-ui` | M |
| QR onboarding | Camera scan; payload must be plain HTTPS origin ≤2048 UTF-8 bytes, ASCII/punycode host; credentials/paths/queries/fragments/non-HTTPS rejected; destination reviewed before sign-in | AVFoundation camera + metadata detector | CameraX + ML Kit Barcode Scanning | `a0-core` (QRImport port), `a0-platform` (camera trait), `a0-ui` | M |
| Saved server profiles | Multiple origins/usernames; Disconnect keeps server+drafts, Forget password clears credential, Remove server deletes entry+credential after confirm | Keychain + local files | EncryptedSharedPreferences / DataStore backed by Android Keystore | `a0-storage`, `a0-auth`, `a0-ui` | M |
| Secure credential storage | Optional password in device-only, when-unlocked secure store; never plaintext fallback; failures visible | Keychain (`ThisDeviceOnly`, `WhenUnlocked`) | Android Keystore + EncryptedSharedPreferences (or Tink) | `a0-platform` trait + per-OS impls, `a0-storage` | M |
| Session restore | Cookie-only secure item bound to username+exact origin; protected bootstrap verify, fresh CSRF, profile-isolated drafts reloaded; cold launch opens new-chat draft; expired → sign-in; no login/mutation replay | Separate Keychain item | Keystore-encrypted DataStore entry | `a0-auth`, `a0-storage`, `a0-ui` (splash/route) | M |
| Connection health indicator | Status dot in composer (green realtime / blue polling / amber sync / red attention / gray none); tap opens accessible explanation + agent-paused info + Retry sync after exhaustion | SF Symbols, VoiceOver | Material icons, TalkBack contentDescription | `a0-core` (state), `a0-chat`, `a0-ui` | S |
| Degraded polling state | Foreground `/api/poll` fallback; 4 retries with bounded backoff; re-check realtime at 30/60/120 s (cap 120 s); sends pause until full snapshot; Retry sync explicit; auth/cert errors require attention | None specific | Lifecycle observers; no WorkManager auto-retry | `a0-realtime`, `a0-core` (PollRecovery port) | M |
| Realtime transport | Socket.IO over WebSocket; `/ws` validates Origin + runtime CSRF; ack validation; independent foreground polling shares one reducer | URLSession + socket.io-client-swift | Rust Socket.IO client used on both platforms (no native dependency) | `a0-realtime`, `a0-core` (SyncReducer) | L |

### 2.2 Chat list & conversation view

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Chat list & search | Two-line rows, chevrons; native search filters; Workspace drawer with search, Projects filter, Tasks section; project dots; running-waveform for server-reported live chats | NavigationStack, SF Symbols | Compose LazyColumn, Material icons | `a0-chat`, `a0-ui` | M |
| Streaming conversation | Live updates via shared reducer fed by socket or polling; identical visible state from both transports | SwiftUI Observation | Compose state flows | `a0-core`, `a0-chat`, `a0-ui` | M |
| Streaming markdown subset | Native headings, emphasis, lists, quotes, fenced code, simple tables, dividers; explicit supported subset, not full GFM; links confirmed before opening; ordinary images never fetched | AttributedString + custom MarkdownDocument | Markwon or custom AnnotatedString renderer | `a0-chat` (pulldown-cmark-based pipeline), `a0-ui` | L |
| Code blocks + copy | Horizontal scrolling, monospaced, per-block copy with Copied checkmark feedback | UIPasteboard | ClipboardManager | `a0-ui` | S |
| Collapsible long messages | >5-line preview + Show more/less; per-message keep-open preference; user echoes collapse only app-owned rich-reply suffix | DisclosureGroup | Compose expandable layouts | `a0-chat`, `a0-ui` | S |
| Tool activity grouping | Consecutive entries share one Agent activity panel: step count, latest non-utility step preview, timeline rail, per-step Details/Raw event/Copy; `icon://` → symbol mapping; known tool names → readable actions; warnings/errors separate | SF Symbols + icon mapping | Material Symbols mapping table | `a0-chat` (TranscriptGroup/LogPresentation), `a0-ui` | L |
| Browser screenshot cards | `browser_snapshot`/legacy Screenshot URI opts in; authenticated `/api/image_get` only: no redirects, 8 MiB cap, bounded pixels, off-main decode, fenced to account/context/epoch; popover preview ≤440 pt | Custom authenticated loader | Coil/custom loader with auth interceptor + bounds | `a0-chat`, `a0-api`, `a0-ui` | M |
| Scroll-follow / Latest | Auto-follow newest until user scrolls into history or expands content; explicit Latest resumes with short animation; suppressed under Reduce Motion | ScrollViewReader | LazyListState + scrollToItem | `a0-core` (TranscriptFollowState), `a0-ui` | S |
| Agents inspector | Read-only grouping of recorded activity by agent number; latest tool label + summary per agent; single-open disclosure | None specific | None specific | `a0-chat`, `a0-ui` | S |
| Subagent relationships | Server-reported subordinate metadata between visible contexts only; missing/deleted/ambiguous/cyclic discarded; Subagents control with New count; Return to parent; arrival never changes selected chat | None specific | None specific | `a0-core` (SubagentRelationships), `a0-chat`, `a0-ui` | M |

### 2.3 Composer & messaging

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Per-chat drafts | Protected, profile-isolated archives; atomic writes; excluded from backup; silent save with visible failure + Retry saving; retained across disconnect/changing chats | File protection `.complete` + backup exclusion | Encrypted DataStore/files + backup-exclude rules XML | `a0-storage`, `a0-chat` | M |
| Attachments staging | Tools → photos/files; removable tray (name+size); 5 files, 10 MiB each, 20 MiB total; local until Send; Queue-mode uploads via `upload` + `message_queue_add`, immediate via `message_async` parts | PhotosUI + UIDocumentPicker | Photo Picker + SAF (ACTION_OPEN_DOCUMENT) | `a0-chat`, `a0-api`, `a0-platform` (picker trait) | M |
| Queue/Steer send modes | Default Queue joins server queue while busy; Steer submits immediately; Stop agent + clear queue option keeps draft; mode affects future sends only | None | None | `a0-core` (SendMode), `a0-chat`, `a0-ui` | S |
| Pause / Resume / Nudge / History / Context tools | Compact tools popover; agent controls journaled, never auto-replayed; Nudge explains interruption effect; History & Context are selectable-text reading views with token counts | Popover + SF Symbols | DropdownMenu/popup + Material icons | `a0-chat`, `a0-api` (agent controls), `a0-core` (ControlJournal) | M |
| Context ring / breakdown | Server-reported tokens/window from context_window plugin: category rows (Messages, System tools, Skills, MCP tools, System prompt, Extras), free space; provider input/output/cache separate; missing values stay missing; stale reports labeled; refresh debounced + stale-rejected | None | None | `a0-chat` (ContextUsage port), `a0-api`, `a0-ui` | M |
| Model preset selector + chat override | Picker shows effective Main/Utility/Embedding(+Vision); `set_preset` for current chat; Use inherited clears override; new-draft choice stored with draft, applied on first Send before text; pending preset name travels with delivery | None | None | `a0-chat`, `a0-api`, `a0-core`, `a0-storage` | M |
| Delivery states / retry without auto-resend | Sending → accepted/queued/failed/uncertain receipts; intent persisted before send; uncertain blocks replay across relaunch; explicit Retry sync only after inspection; read-only refresh never clears uncertain intent | None | WorkManager must NOT be used (would auto-retry) | `a0-core` (delivery state machine), `a0-storage` (journal) | M |
| Working strip + Send/Stop | One trailing Send/Stop control: empty draft + running/paused/queued shows Stop; progress ring while running; server-derived working/synchronizing text, never inferred from open socket | None | None | `a0-chat`, `a0-ui` | S |

### 2.4 Projects

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Project list / filter / active dot | Search title+description, server colors as outlined dots, active checkmark; All-projects filter in chat navigation; invalid selections clear | None | None | `a0-projects`, `a0-chat`, `a0-ui` | S |
| Create / clone / edit settings | Forms: title, folder name, description, color (swatch grid + hex), instructions, AGENTS.md inclusion; existing projects add file-structure, variables, MCP JSON, opt-in masked secrets; unknown fields preserved; credential-free Git URLs; token request-only | None | None | `a0-projects`, `a0-api` | M |
| Assign / remove chat; project chats | New chat records generated context ID before create+assign; partial failure offers Check chat, never replays either step | None | None | `a0-projects`, `a0-core` (journal) | S |
| Delete with typed confirmation | Own sheet: whole server-side folder, files/settings and every chat assignment removed; requires exact folder-name text before enabling | None | None | `a0-projects`, `a0-ui` | S |
| Mutation receipts | All project mutations share profile-isolated receipts; intent persisted before send; uncertain outcomes retained until explicitly checked | None | None | `a0-core`, `a0-projects` | S |

### 2.5 Model presets editing

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Shared preset editor | Staged add/edit/rename/remove until explicit Save shared presets; Default protected; removal explains fallback to Default; stale-editor baseline check before save/reset (client-side, not CAS) | None | None | `a0-api`, `a0-core` (ModelPresets), `a0-ui` | M |
| Provider/model catalogs & suggestions | `model_config_get` provider catalogs; `model_search` suggestions filtered locally ≤300 rows; custom-ID entry form; provider switch clears base+kwargs, keeps model name; unknown IDs never silently replaced; no credentials in picker | None | None | `a0-api`, `a0-core` (ModelCatalog), `a0-ui` | M |
| Inheritance projection | Sparse docs; slots merge display from Default without inheriting kwargs; separate Vision never inherited; display projection never overwrites raw document | None | None | `a0-core` | S |
| Provider credentials / scoped defaults | Remain in WebUI via confirmed handoff (no native key entry) | None | None | `a0-ui` (handoff), `a0-workspace` | S |

### 2.6 Plugins

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Installed / Hub / details navigation | Custom, Built-in and Plugin Hub collections with search; details cards; update indicators from commit/timestamp; hub suspension blocks install/update | None | None | `a0-plugins`, `a0-ui` | M |
| Lifecycle: install / update / delete / activate | Validated commands through shared journal, submitted once; timeout/failed-ack = uncertain; global enable switch (always-on locked, built-ins non-deletable); override removal submits server-returned path only | None | None | `a0-plugins`, `a0-core` (journal), `a0-api` | M |
| Plugin thumbnails | Server artwork: validated `/plugins/<id>/webui/thumbnail.<ext>` path, authenticated, no redirects, 4 MiB cap; Hub public artwork credential-free with host checks; downsample ≤384 px; fenced to connection+foreground | ImageIO downsample | BitmapFactory with inSampleSize bounds | `a0-plugins` (PluginThumbnail), `a0-api`, `a0-ui` | M |
| Embedded plugin WebUI screens | Ephemeral nonpersistent WebView with validated session cookie; entry `/index.html` (never `/`); exact-origin navigation; fetch interception rejects redirects, throws on 401/403, no retry; screen accepts only bounded status strings; 60 s startup deadline; native chat change closes screen; backgrounding destroys web state | WKWebView + user scripts | WebView + injected JS fetch hook, CookieManager isolation; shouldOverrideUrlLoading origin policy | `a0-workspace` (adapter JS port), `a0-platform` (webview trait), `a0-auth` (cookie) | L |
| Plugin settings screens | Settings open inside app against server WebUI with current scope + chat identity as structured JS args; custom controls/modals stay WebUI-owned | Same as above | Same as above | `a0-workspace`, `a0-plugins` | M |
| Dev Tunnels header | `X-Tunnel-Skip-AntiPhishing-Page: true` on app/same-origin requests only for authenticated `*.devtunnels.ms`; never external requests | None | None | `a0-workspace`, `a0-api` | S |
| Third-party install disclosure | Hub install shows explicit code-execution disclosure; Plugin Scanner surfaced when server provides it | None | None | `a0-plugins`, `a0-ui` | S |

### 2.7 Workspace WebUI handoff

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Workspace canvas + siderail | Draggable A0 tab + drawer entry; authenticated presentation of server right canvas: File Browser, Browser, Desktop, Editor, plugin-registered surfaces from server registry (never a local list); tiles with title/icon; full-width panels; Back to tools | WKWebView (shared ephemeral session) | WebView sharing the plugin-screen session | `a0-workspace`, `a0-platform`, `a0-ui` (tab) | L |
| Same-origin controls | Exact-origin navigation incl. scheme/host/port; login navigation rejected; external HTTPS links separated; intercepted fetch follows plugin-screen rules; server-side tool actions keep WebUI semantics without native receipts (user-approved exception) | WKNavigationDelegate | WebViewClient + JS bridge guards | `a0-workspace` | M |
| File exports | Same-origin downloads + blobs via download delegate; sanitized unique temp dirs; protected storage + backup exclusion; share sheet; backgrounding cancels; bytes never enter JS bridge or logs | WKDownloadDelegate + share sheet | JS-bridge blob fetch → SAF create document + share intent | `a0-workspace`, `a0-platform` (share trait) | M |
| Open full WebUI | Confirmed handoff of HTTPS origin to external browser; no automatic cookie sharing assumed | UIApplication.open | Custom Tabs / ACTION_VIEW | `a0-platform`, `a0-ui` | S |

### 2.8 A2UI rich replies (bounded catalog)

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Recognition & validation | Response-only opt-in via fenced `a2ui` block or `LogEntry.kvps.a2ui`; bounded doc: 64 KiB, 32 messages, 64 components, 12 layout levels, 128 nodes, 32 options; actions ≤16 KiB; cycles/duplicate IDs/version/catalog/property/slider checks; no HTML/JS/themes/functions/remote media | None | None | `a0-core` (validation port), `a0-chat` (surface session) | L |
| Rich replies send decoration | Explicit Send appends exact client-owned catalog-instructions suffix; queue/immediate paths identical; toggle off → plain requests; user text preserved separately with disclosure | None | None | `a0-chat`, `a0-api` | S |
| Basic catalog widgets | Text, Row, Column, Card, Divider, Button, TextField (persistent labels), CheckBox, ChoicePicker, Slider; absolute JSON-pointer bindings; complete surface state per reply (no deltas); repeated snapshots keep local inputs | SwiftUI | Compose Material3 | `a0-ui` | L |
| Expanded catalog | Forecast, Metric, DataTable (≤6 cols × 50 rows), Timeline (≤30), Checklist (≤20), Dashboard (adaptive grid), ImageCarousel (≤10 images, captions, source links, paging) | Native grids | Compose LazyGrid | `a0-ui`, `a0-chat` | L |
| Charts (14 kinds) | line, area, bar, horizontalBar, groupedBar, stackedBar, stackedArea, pie, donut, scatter, bubble, histogram, range, heatmap; bounds 128 pts/32 cats/8 series; finite values <1e12; zero baselines; values disclosure + accessibility; client invents nothing | Swift Charts | No first-party equivalent — custom Compose Canvas or Vico | `a0-ui` (shared custom chart renderer) | L |
| Reviewed action flow | Button opens Review response with labeled values; Cancel no-op; Add to draft appends `a2ui-action` envelope without overwriting text or sending; Send stays explicit | None | None | `a0-chat`, `a0-ui` | M |
| Image carousel downloads | Separate ephemeral cookie-free client: public-HTTPS host check + DNS preflight, no redirects, raster MIME allowlist, 4 MiB cap, ≤1200 px thumbnails, bounded memory cache; Retry keeps source link | URLSession + ImageIO | Manual DNS resolve + pinned HTTP client (no auto-redirect) + BitmapFactory bounds | `a0-api`, `a0-ui` | M |
| Reply media preview | ≤4 unique direct HTTPS mp3/m4a/aac/wav/mp4/mov links detected in prose (≤128 KiB reply, 2 KiB URL), no network on discovery; explicit A2UI takes precedence | None | None | `a0-chat` | S |
| Audio/video players | Load → credential-free bounded download (audio 32 MiB, video 100 MiB, 2 concurrent, 90 s timeout, 64 KiB chunks to protected temp); explicit Play; inline playback; Unload releases; background/chat-change cancels; no autoplay/PiP | AVFoundation/AVKit + captions | Media3/ExoPlayer with size caps + track selection | `a0-ui` (players), `a0-api` (downloads), `a0-platform` (audio focus) | L |
| Jev candidates (optional) | Masked TYPESAFE_API_KEY in device-only store; opt-in `a2ui-candidates` envelope on Send; one isolated POST pinned jev-1.13.0 projecting intent/IDs/type summaries only; 5 s timeout, 8 KiB req / 64 KiB resp caps; attempt journal 30 days / 2048 records; duplicate/interrupted attempts never replay | Dedicated Keychain service | Keystore-backed store | `a0-storage` (key), `a0-api` (client), `a0-core` (journal), `a0-chat`, `a0-ui` | L |
| Producer guidance screen | Copyable instructions + executable example; copying never sends or modifies the server | None | None | `a0-chat`, `a0-ui` | S |

### 2.9 Voice

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Inline dictation | Composer mic → live on-device recognition (required per request, no cloud fallback) into existing draft; partial results replace only own insertion; manual edits end capture; foreground-limited; backgrounding/chat-change/interruption stops | Speech framework (SFSpeechRecognizer + AVAudioSession) | SpeechRecognizer (EXTRA_PREFER_OFFLINE, device-dependent) or bundled on-device model + AudioRecord; RecognitionListener partials | `a0-platform` (speech trait), `a0-chat` (VoiceDraftBuffer/Insertion port), `a0-ui` | L |
| Continuous listening | Remembered pref, default off, never auto-starts; 45 s segment rollover, 5 min cap, 12000-char transcript bound; disabled while voice active | Speech | Recognition restart segmentation (mind OS timeout limits) | `a0-chat`, `a0-storage` (pref), `a0-platform` | M |
| Read reply (TTS, optional) | System synthesizer reads latest assistant prose excluding code; menu-triggered only; stops on background/leave | AVSpeechSynthesizer | android.speech.tts.TextToSpeech | `a0-platform`, `a0-chat` | M |
| Draft polish (optional, flagged) | On-device LLM cleanup ≤1500 chars with Original/Suggested review; Keep original or explicit Use; change invalidates proposal; never sends | FoundationModels (iOS 26 only) | No direct equivalent — AICore/Gemini Nano where available; else omit | `a0-platform` (gen-ai trait), `a0-chat` | S |
| Audio ownership | Weak synchronous foreground handoff among dictation/TTS/inline media; starting one stops others; only current owner deactivates session | AVAudioSession | AudioFocusRequest + AudioAttributes | `a0-platform`, `a0-chat` | M |
| Permission bridge | Sendable nonisolated boundary for recognition permission callbacks (crash-fix pattern) | Speech/TCC | Standard runtime permission API (simpler) | `a0-platform` | S |

### 2.10 Live viewer & agent activity

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Live viewer pane | One Browser/Computer pane per conversation; host_viewer_v1 advertisement; bounded JPEG captures ≤1600×1200 / 1.5 MB / 650 ms cadence; history selector; expand full-screen (iPad: resizable side pane); watching cannot send input | WKWebView network-disabled | WebView with network disabled; render into offscreen container | `a0-chat` (LiveViewer model port), `a0-api` (hostViewer), `a0-ui` | L |
| Take over / Return | Centered tab drains agent dispatch before enabling touch/keyboard; Return drains input, captures final state, releases gate; keyboard toggle + inspect mode; text cleared after submit; never restarts completed/stopped agent | None | None | `a0-core` (fencing/journal), `a0-chat`, `a0-api` | L |
| Capture/lease recovery | Mutations journaled before transmission; background/context change stops capture + clears pixels; lease expiry revokes input, keeps hold; second viewer cannot steal lease; Recover/Return explicit | None | None | `a0-core`, `a0-chat` | M |
| Subagent chat navigation | Verified child chats open via existing selection, retaining each draft; same-chat recorded steps stay separate; incomplete sync disables navigation | None | None | `a0-core`, `a0-chat`, `a0-ui` | S |

### 2.11 Settings, system & cross-cutting

| Feature | Reference behavior (iOS) | iOS dependency | Android equivalent | Rust module(s) | Cplx |
| --- | --- | --- | --- | --- | --- |
| Appearance light/dark/system | Adaptive neutral tokens (Canvas/Panel/Tint pairs); System default with Light/Dark override; applied immediately | UITraitCollection | Compose Material3 color scheme + isSystemInDarkTheme | `a0-ui` (tokens), `a0-storage` (pref) | S |
| Server theme matching | Optional match to server Selectable Theme plugin palettes incl. linear gradients; independent of Light/Dark/System | None | None | `a0-core` (ServerTheme), `a0-chat`, `a0-ui` | M |
| Dynamic Type / font scaling | Semantic text styles throughout; accessibility sizes stack labels, single-column dashboards, intact numeric groups | Dynamic Type | sp scaling + fontScale (test at 200%) | `a0-ui` | M |
| Reading preferences | Collapse long messages, Group agent activity, Rich replies toggle, Continuous listening — local, credential-free, immediate | UserDefaults | DataStore | `a0-storage`, `a0-ui` | S |
| Privacy & storage screen | Explains local retention: drafts+receipts on-device, transcripts never persisted, no passwords/cookies/CSRF in archives; backup exclusion; synthetic preview memory-only | File protection + backup flags | Backup-exclude rules + encrypted storage | `a0-storage`, `a0-ui` | S |
| Launch splash + cold-launch route | Branded cover only during real profile/session startup; authenticated cold launch → new-chat draft, drawer closed; background/foreground keeps current route | None | Splash + lifecycle handling | `a0-auth`, `a0-storage`, `a0-ui` | S |
| Workspace drawer | Bounded leading drawer: search, Projects, conversations, Tasks, Settings; backdrop/escape/swipe dismiss; opens dismissed keyboard, preserves drafts; hides underlying screen from accessibility | SwiftUI | ModalNavigationDrawer | `a0-ui` | S |
| Accessibility | VoiceOver/TalkBack labels, ≥44 pt targets, Reduce Motion honored, status announcements, drawer focus order | VoiceOver, Reduce Motion | TalkBack contentDescription, 48 dp targets, animator scale | `a0-ui`, `a0-platform` | M |
| Tablet / adaptive layouts | iPhone + adaptive iPad layouts; popovers retained on compact devices; split-view navigation | UIKit size classes | Window size classes + canonical list-detail | `a0-ui` | M |
| Project-aware theming details | Server project colors as dots/swatches with outlines; color never sole identifier; alpha-preserving hex | None | None | `a0-projects`, `a0-ui` | S |

## 3. Complexity summary

| Complexity | Count | Notes |
| --- | --- | --- |
| S | 24 | Pure-core ports, simple UI actions, platform one-liners |
| M | 33 | Forms, flows with receipts, authenticated loaders, platform seams |
| L | 16 | Markdown pipeline, activity grouping, WebUI embed, A2UI catalog+charts, voice, live viewer, realtime |

## 4. Platform dependency translation table

| iOS | Android | Shared Rust seam |
| --- | --- | --- |
| Keychain (device-only, WhenUnlocked) | Keystore + EncryptedSharedPreferences / DataStore | `a0-platform::secure_store` |
| SF Symbols | Material Symbols (needs name-mapping table per icon) | `a0-ui::icons` |
| Dynamic Type | sp scaling + fontScale | `a0-ui::typography` |
| Speech framework (on-device required) | SpeechRecognizer offline / bundled model | `a0-platform::speech` |
| AVSpeechSynthesizer | TextToSpeech | `a0-platform::tts` |
| AVAudioSession ownership | AudioFocusRequest | `a0-platform::audio_focus` |
| WKWebView nonpersistent + user scripts | WebView + injected JS + CookieManager isolation | `a0-platform::webview` |
| PhotosUI / UIDocumentPicker | Photo Picker / SAF | `a0-platform::pickers` |
| Share sheet (UIActivityViewController) | Share intent + SAF create | `a0-platform::share` |
| UIPasteboard | ClipboardManager | `a0-platform::clipboard` |
| Swift Charts | Custom Compose Canvas (or Vico) | `a0-ui::charts` (shared) |
| FoundationModels | AICore / Gemini Nano (optional; feature-flag) | `a0-platform::gen_ai` |
| File protection `.complete` + backup exclusion | Encrypted storage + backup-exclude rules | `a0-platform::fs_policy` |

## 5. Phased implementation roadmap (Phases 2–8)

Phase 1 (this document) is complete analysis. Each phase below ends with a verifiable deliverable, mirroring the reference repo's TDD-slice discipline.

### Phase 2 — Workspace & protocol foundation
- Scaffold Cargo workspace with the 11 crates; CI (fmt, clippy, tests), MSRV pin, fixture-server harness.
- Port to `a0-core`: DTOs, envelope validation, `SyncReducer`, delivery state machine, `ControlJournal` traits, SendMode, QR-origin validation.
- Build `a0-api`: HTTPS client with no-redirect / no-auto-retry policy, CSRF bootstrap, health, poll, `message_async`, upload, `message_queue_add`.
- Build `a0-auth`: login, session cookie + CSRF handling, account-generation fencing.
- **Deliverable:** headless integration suite against the reference fixture proving login → poll → send → ack → delivery receipt, plus a protocol-contract port note (state_request zeroed cursors, `state_push` envelope semantics, `ws_webui` activation checks).

### Phase 3 — Storage & session continuity
- `a0-storage`: profile store, session archives (drafts, receipts, pending preset names), preferences, attachment staging; atomic writes; sensitive-field bans enforced by type design.
- `a0-platform::secure_store` + iOS Keychain and Android Keystore implementations; backup-exclusion policy per OS.
- `a0-auth`: session restore (protected bootstrap → cookie re-verify → fresh CSRF), expired/rejected → sign-in, disconnect clears session item.
- **Deliverable:** cold-launch restore tests, session-isolation tests (username+origin binding), background password-clearing parity, documented on-device encryption choices.

### Phase 4 — Realtime & connection resilience
- `a0-realtime`: Socket.IO-over-WebSocket client (rust-socketio or custom), ack validation, bounded handshake buffering, Origin+CSRF `/ws` requirements.
- Degraded polling: foreground fallback, 4-retry bounded backoff, realtime re-check ladder (30/60/120 s), stale-state rejection, send-gating until full snapshot.
- Connection health state machine + accessible indicator model.
- **Deliverable:** dual-transport parity suite (identical reducer output from socket and poll), recovery/timeout test pack, health-indicator spec.

### Phase 5 — Chat & composer shell (first usable release)
- `a0-ui`/`a0-chat`: chat list + search, drawer, conversation view, streaming markdown subset, code copy, collapse, activity grouping with timeline, follow/Latest, screenshot cards.
- Composer: per-chat drafts, attachment staging with limits, Queue/Steer, Pause/Resume/Nudge/History/Context popover, preset selector with override scoping, context ring, delivery states + explicit retry, connection dot.
- `a0-platform`: pickers, clipboard, share baseline.
- **Deliverable:** end-to-end fixture-driven acceptance pack on iOS + Android matching the reference's first-usable-release checklist (scan/sign-in → chat → send text/file → progress → pause/stop → accurate return after lock).

### Phase 6 — Projects & model presets
- `a0-projects`: list/filter, create/clone/edit with field preservation, assign/remove, delete with typed confirmation, journal-backed receipts, two-step new-chat+assign with Check chat.
- Presets: shared editor (staged changes, stale-baseline check, protected Default), picker with current-chat overrides, provider/model catalogs + suggestions, first-send preset application ordering.
- Context-window endpoint integration with truthful-missing-value rules.
- **Deliverable:** project + preset contract test suite; uncertain-outcome and conflict tests; context-usage popover parity.

### Phase 7 — Plugins, workspace handoff, A2UI
- `a0-plugins`: collections, lifecycle commands with receipts, thumbnails (server + hub policy); third-party install disclosure.
- `a0-workspace`: adapter JS port (fetch interception, exact-origin navigation, screen-status bridge, startup deadline), embedded plugin/settings screens, workspace canvas + siderail, same-origin exports + share.
- A2UI: validation in `a0-core`, surface session in `a0-chat`, widget catalog + 14-kind chart renderer in `a0-ui`, image/media bounded downloads in `a0-api`, reviewed-action flow.
- Optional Jev candidates behind a settings flag (key store, isolated client, attempt journal).
- **Deliverable:** plugin fixture parity tests, A2UI conformance suite against `GenerativeGuide` examples, WebUI-handoff security review (origin/cookie/CSRF/download policies).

### Phase 8 — Voice, live viewer, polish & release
- Voice: dictation with on-device requirement + insertion scoping, continuous listening, optional TTS read-reply, audio-ownership handoff, optional flagged draft polish.
- Live viewer: capture pipeline + history, takeover/return with journaling and lease rules, agents inspector, subagent relationships.
- Settings: appearance + server-theme matching, reading prefs, privacy & storage screen; accessibility pass (TalkBack/VoiceOver labels, font scaling, Reduce Motion); tablet adaptive layouts.
- Release hardening: privacy disclosures per platform, security review, packaging/signing, store metadata, device acceptance protocol mirroring the reference ACCEPTANCE.md boundaries.
- **Deliverable:** dual-platform beta candidates with recorded device-acceptance results and a known-limits document.

## 6. Risks & open questions

| Risk / question | Impact | Mitigation |
| --- | --- | --- |
| Android has no first-party chart library | 14 chart kinds need a custom renderer (largest UI cost) | Build one shared `a0-ui::charts` Canvas renderer; validate against the reference's chart bounds table |
| On-device speech quality/availability varies by Android OEM | Dictation parity gaps | Feature-detect offline support; visible fallback explanation (reference already requires visible unsupported state) |
| Android WebView fetch interception is more invasive than WKWebView user scripts | Plugin/WebUI embed fidelity | Shared adapter JS with per-platform injection harness; port `PluginWebAdapterTests.mjs` contract tests first |
| Rust Socket.IO client maturity vs pinned socket.io-client-swift 16.1.1 behavior | Realtime edge cases (acks, CSRF handshake) | Budget custom Engine.IO work in Phase 4; golden-fixture replay tests from reference TDD evidence |
| FoundationModels cleanup is iOS-26-only | Android has no equivalent | Ship as optional flagged feature; omit where unavailable (reference pattern: availability-gated) |
| Jev pinned model version (jev-1.13.0) may drift | Candidate-selection contract changes | Version pin in config; tolerable failure = Markdown fallback (already spec'd) |
| WebUI handoff exception (no native receipts for embedded tool actions) | User-approved trust boundary must be preserved verbatim | Carry the approved-exception text into port docs; native lifecycle commands remain journaled |

## 7. Source coverage note

Every feature row above traces to the reference docs and sources listed in the header. Acceptance claims in the reference (physical-device evidence, live-server boundaries) are inherited as *boundaries*, not as completed-port proof; each phase deliverable re-establishes its own evidence.
