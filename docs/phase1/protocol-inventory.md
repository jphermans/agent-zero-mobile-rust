# Agent Zero Wire Protocol Inventory — Phase 1

**Purpose:** implementation contract for the Rust `a0-api` and `a0-realtime` crates.
**Reference source (read-only):** `/home/jphermans/workdir/ref/a0-iOS` (Swift). Citations `file:line` are relative to that tree.
**Methodology (leanrea):** every claim is **seen** (cited file:line) or **inferred**; anything not found is **unknown** and listed in section 10. Static reading only; nothing was run against a live server.
**Date:** 2026-11-02.

---

## 1. Conventions

- **Base origin:** HTTPS-only; no userinfo, path, query or fragment. Loopback `http` is accepted only in DEBUG builds with the `.loopbackDevelopment` policy (`HTTPClient.swift:22-50`). Host is lower-cased, path stripped (`HTTPClient.swift:44`).
- **Redirects are never followed:** the URLSession task delegate cancels every redirect (`HTTPClient.swift:76-82`, installed `HTTPClient.swift:94`). 3xx responses are always inspected manually (login-redirect test `HTTPClient.swift:295-300`).
- **Cookies are managed manually:** ephemeral session, `httpCookieStorage = nil`, `httpShouldSetCookies = false`, no credential storage, no URL cache, `reloadIgnoringLocalCacheData` (`HTTPClient.swift:86-93`). Response cookies are parsed per call and kept only when their domain equals the origin host; expired ones are dropped (`HTTPClient.swift:273-278`).
- **Headers on every API request** (`HTTPClient.swift:260-268`) — **seen**:
  - `Accept: application/json` — always.
  - `Origin: <origin>` — always.
  - `Content-Type: <contentType>` — only when the request has a body; default `application/json`.
  - `X-CSRF-Token: <csrf>` — sent once a CSRF token is held (absent during login).
  - `Cookie: <cookieHeader>` — when non-empty (format: section 2.4).
- **Epoch fencing:** every request captures an `authenticationEpoch` UUID and discards the response if `disconnect()` ran meanwhile (`HTTPClient.swift:138-142,269-272`; same for bounded image transfers `HTTPClient.swift:283-287`). — **seen**
- **Response validation** (`validate`, `HTTPClient.swift:301-306`): status must be 2xx, else `httpStatus(code)`; `Content-Type` must contain `application/json`, else `unexpectedResponse`. JSON decode failure maps to `incompatiblePayload` (`HTTPClient.swift:307-310`). — **seen**
- **Uniform auth-error handling on every authenticated endpoint** — **seen** (representative cites: `HTTPClient.swift:211-212,239-240`, `Projects.swift:174-175`, `Plugins.swift:107-109`, `Attachments.swift:96-97`, `AgentControls.swift:63-64`, `ModelPresets.swift:190-195`, `ModelCatalog.swift:47-48`, `ContextUsage.swift:87-88`, `HostConnection.swift:145-147`):
  - HTTP 401 **or** a login redirect (3xx whose `Location` resolves to the same origin with path `/login`, `HTTPClient.swift:295-300`) → clear the session, error `requiresLogin`.
  - HTTP 403 → clear the session, error `csrfRejected` (single exception: section 3.6 `model_override`).
  - HTTP 404 → endpoint-specific: plugins → `unavailable` (`Plugins.swift:109`); connector routes → treated as missing where allowed (`HostConnection.swift:112,147`).

---

## 2. Authentication & session lifecycle

### 2.1 First login (credentials)

Sequence — **seen** (`HTTPClient.swift:144-171`):

1. `GET /api/csrf_token` with an empty cookie jar.
   - HTTP 200 + valid bootstrap: the server has login **disabled**. Accepted only for unauthenticated loopback dev origins, otherwise error `unauthenticatedServer` (`HTTPClient.swift:148-154`).
   - Login redirect or HTTP 401 → continue.
   - Anything else → `unexpectedResponse`.
2. `POST /login` — `Content-Type: application/x-www-form-urlencoded`, body `username=<enc>&password=<enc>` with RFC 3986 unreserved-character percent-encoding (`HTTPClient.swift:159-160,311-314`). Verified wire example `Tests/A0CoreTests/HTTPTests.swift:31`: `username=user%2Bname&password=a%26b%3D%20c`.
3. Success = HTTP 3xx whose `Location` resolves to the **same origin** with path **not** `/login` (`HTTPClient.swift:161-166`). Anything else (including a 200 HTML error page, `HTTPTests.swift:37-43`) → `invalidCredentials`. Cross-origin redirects are rejected without resending credentials (`HTTPTests.swift:44-49`).
4. `GET /api/csrf_token` again (now with the session cookie). A login redirect here → `invalidCredentials` (`HTTPClient.swift:167-169`).
5. Bootstrap validation (`acceptBootstrap`, `HTTPClient.swift:195-207`) — see section 2.2.

### 2.2 Bootstrap response

**seen** (`HTTPClient.swift:197-206`; fixture `Tests/A0CoreTests/HTTPTests.swift:18`):

```json
{"ok": true, "token": "synthetic-csrf", "runtime_id": "test-runtime"}
```

Requirements: `ok:true`; `token` non-empty ASCII `[A-Za-z0-9_-]`; `runtime_id` non-empty ASCII `[A-Za-z0-9_-]`; the cookie jar must contain a cookie named `session_<runtime_id>`; afterwards the jar is pruned to exactly that cookie (`HTTPClient.swift:203-205`).

Session cookie observed on the login 302: `session_test-runtime=synthetic-session; Path=/; HttpOnly` (`HTTPTests.swift:17`). Cookies are honored from `Set-Cookie` headers of any response via `HTTPCookie.cookies(withResponseHeaderFields:)` (`HTTPClient.swift:274-278`). **seen**

### 2.3 Restore, refresh, logout

- **Persisted material** is exactly one cookie: `{version:1, profile, name, value, expires}` (`SavedAuthentication.swift:5-13`). Validation: name starts with `session_`, max 256 chars, charset `[A-Za-z0-9_-]`; value non-empty, max 16384 bytes, printable ASCII excluding `;` and `,`; not expired (`SavedAuthentication.swift:14-21`). Passwords and CSRF tokens are deliberately **not** persisted (`SavedAuthentication.swift:3-4`). — **seen**
- **Restore without credentials** (`HTTPClient.swift:178-194`): anonymous `GET /api/csrf_token` must still require login (HTTP 200 → error `unauthenticatedServer`; 3xx/401 → continue); then the saved cookie is inserted into the jar and the bootstrap is retried; 3xx/401 on the authenticated bootstrap → `requiresLogin`. Any failure disconnects. — **seen**
- **Session refresh without credentials** (`refreshSocketSession`, `HTTPClient.swift:245-255`): re-runs `GET /api/csrf_token` with the existing cookie and re-validates section 2.2. A transient HTTP failure keeps the polling session alive; a bootstrap validation failure disconnects. The realtime coordinator calls this before every reconnect (`App/SpikeModel.swift:457`). — **seen**
- **Logout:** client-side `disconnect()` wipes cookies, CSRF token, runtime id and authenticated state (`HTTPClient.swift:142`). No server-side logout endpoint is called anywhere in the source (absence verified by the endpoint grep in section 3.10). A Rust client treats logout as local state destruction. — **seen**

### 2.4 Cookie header format

**seen** (`HTTPClient.swift:289-294`): all non-expired cookies sorted by name, joined as `name=value; ...`, then the synthetic cookie `csrf_token_<runtime_id>=<csrf>` appended **last**. Verified in `Tests/A0CoreTests/HTTPTests.swift:33-34`: a poll request carries both `session_test-runtime=synthetic-session` and `csrf_token_test-runtime=synthetic-csrf`.

### 2.5 Socket session object

**seen** (`HTTPClient.swift:122-127,256-259`): `SocketSession { origin, cookieHeader, csrfToken, runtimeID }`. Feature methods call `socketSession()` as an authentication guard before mutating endpoints (`Projects.swift:171`, `Plugins.swift:105`, `Attachments.swift:72,94`, `AgentControls.swift:43,61`, `ModelPresets.swift:187`, `ModelCatalog.swift:45`, `ContextUsage.swift:83`, `HostConnection.swift:143`).

---

## 3. HTTP endpoints

All request bodies are JSON unless stated otherwise (Swift `JSONEncoder` output). `StateRequest` always writes `context`, as JSON `null` when absent (`Protocol.swift:76-83`; verified `ProtocolTests.swift:15`). **seen**

### 3.1 Core chat

#### `POST /api/poll` — **seen** (`HTTPClient.swift:208-215`)
- Request body: `StateRequest` JSON — `{"context":string|null,"log_from":int,"notifications_from":int,"timezone":string,"collections_delta":bool}` (`Protocol.swift:61-84`). No `forceFull` key is ever sent — **seen** (`ProtocolTests.swift:11-18` asserts its absence).
- Response: `Snapshot` (section 5.1).
- Auth errors: section 1; a 403 poll failure also disconnects (`HTTPClient.swift:211-212`).

#### `POST /api/chat_create` — **seen** (`HTTPClient.swift:216-222`)
- Request: `{"new_context":"<id>","current_context":""}`.
- Response must decode `{"ok":true,"ctxid":"<id>"}` with `ctxid` equal to the requested id, else `incompatiblePayload`.

#### `POST /api/message_async` (immediate send) — **seen** (`HTTPClient.swift:223-235`)
- JSON request: `{"context":"<ctx>","text":"<text>","message_id":"<client uuid>"}`.
- Response must decode `{"context":"<ctx>"}` with matching context.
- Multipart attachment variant: section 7.

#### `POST /api/message_queue_add` (queued send) — **seen** (`HTTPClient.swift:223-229`; with attachments `Attachments.swift:79-84`)
- JSON request: `{"context":"<ctx>","text":"<text>","item_id":"<client uuid>"}` (bare text) or `{"context","text","item_id","attachments":["<uploadName>",…]}` (after a prior upload, section 7).
- Response must be `{"ok":true,"item_id":"<uuid>"}` with `item_id` equal to the client-generated id.

#### `POST /api/message_queue_remove` — **seen** (`AgentControls.swift:46-53`)
- Request: `{"context":"<ctx>"}` (the stop payload is reused).
- Response must be `{"ok":true,"remaining":0}`.
- Ordering invariant: sent **before** `/api/stop` so a delayed queue worker cannot restart the cancelled chat; a clear failure is captured and surfaced **after** the stop attempt, never retried (`AgentControls.swift:45-57`). **seen**

### 3.2 Agent controls — **seen** (`AgentControls.swift:9-38,42-67`)

| Endpoint | Request body | Required response keys |
|---|---|---|
| `POST /api/stop` | `{"context":"<ctx>"}` | `context` == ctx, `stopped`: bool |
| `POST /api/pause` | `{"context":"<ctx>","paused":bool}` | `pause` == requested bool |
| `POST /api/nudge` | `{"ctxid":"<ctx>"}` (note key) | `ctxid` == ctx |
| `POST /api/history_get` | `{"context":"<ctx>"}` | `history`: string; optional `tokens`: number |
| `POST /api/ctx_window_get` | `{"context":"<ctx>"}` | `content`: string; optional `tokens`: number |

All five responses capped at 2 MiB and guarded by the standard auth handling (`AgentControls.swift:19-21,60-67`). — **seen**

### 3.3 Projects — `POST /api/projects` — **seen** (`Projects.swift:89-178`)

Single action-dispatched endpoint. Response envelope `{"ok":bool,"data":…}`, cap 2 MiB; `ok:false` → `serverRejected` (`Projects.swift:120-125`).

| Action | Request body | `data` payload |
|---|---|---|
| `list` | `{"action":"list"}` | array (≤10000) of `{name,title,description,color}`; unique non-empty names (`Projects.swift:127-136`) |
| `load` | `{"action":"load","name":"<n>"}` | project document; `name` must equal request (`Projects.swift:137-140`) |
| `create` / `update` / `clone` | `{"action":"create\|update\|clone","project":{…}}` — document fields: `name,title,description,instructions,color,git_url,include_agents_md` plus optional `variables,secrets,mcp_servers,file_structure`; `clone` adds `git_token` (HTTPS-only URL without userinfo/query/fragment, `Projects.swift:103-108`) (`Projects.swift:89-119`) | project document |
| `delete` | `{"action":"delete","name":"<n>"}` | string equal to `<n>` (`Projects.swift:146-148`) |
| `activate` | `{"action":"activate","name":"<n>","context_id":"<ctx>"}` | `null` (`Projects.swift:149-151`) |
| `deactivate` | `{"action":"deactivate","context_id":"<ctx>"}` | `null` |
| `file_structure` | `{"action":"file_structure","name":"<n>","settings":{…}?}` | string (`Projects.swift:152-154`) |

- Project name validation: non-empty, trimmed, not `.`/`..`, no `/`, no `\\`, no control chars (`Projects.swift:162-165`). — **seen**
- Default file_structure: `{"enabled":true,"max_depth":5,"max_files":20,"max_folders":20,"max_lines":250,"gitignore":""}` (`Projects.swift:68`). — **seen**

### 3.4 Plugins

#### `POST /api/plugins_list` — **seen** (`Plugins.swift:117-126`)
- Request: `{"filter":{"custom":true,"builtin":true}}`.
- Response: `{"ok":true,"plugins":[…]}`, max 5000 rows; each row requires string `name` `[A-Za-z0-9_-]` max 160 chars (`Plugins.swift:35-40`); observed row fields: `name, display_name, description, version, author, is_custom, always_enabled, has_main_screen, has_config_screen, per_project_config, per_agent_config, toggle_state` (`Plugins.swift:23-33`).

#### `POST /api/plugins` — **seen** (`Plugins.swift:76-100,104-116,127-148`)
Requests always carry `plugin_name`; scope keys `project_name`/`agent_profile` are added when the operation is scoped (`Plugins.swift:41-47,83`).

| Action | Extra keys | Response (in `ok:true` envelope) |
|---|---|---|
| `get_toggle_status` | — | `status:"enabled"\|"disabled"`, `loaded_project_name`, `loaded_agent_profile`, `loaded_path` (`Plugins.swift:127-132`) |
| `get_doc` | `doc:"readme"\|"license"` | `content`: string ≤1 MiB (`Plugins.swift:143-148`) |
| `toggle_plugin` | `enabled:bool`, `clear_overrides:false` | generic ok (`Plugins.swift:81-84`) |
| `delete_config` | `path:"<config path>"` | generic ok (`Plugins.swift:85-87`) |
| `delete_plugin` | — (custom plugins only) | generic ok (`Plugins.swift:88-90`) |

Envelope handling: 404 → `unavailable`; cap 8 MiB; success key `ok`; `ok:false` → `rejected` (`Plugins.swift:104-115`). — **seen**

#### `POST /api/plugins/_plugin_installer/plugin_install` — **seen** (`PluginHub.swift:4,47-56`; `Plugins.swift:91-99,113`)
Installer calls use the success key **`success`** instead of `ok` (`Plugins.swift:113`). — **seen**

| Action | Extra keys | Response |
|---|---|---|
| `fetch_index` | — | `success:true`, `index.plugins` = object map id→entry (≤10000) with fields `title, description, author, version, github, commit / latest_commit, updated / latest_commit_timestamp, tags[], suspended`, plus `installed_plugins`: array of ids (`PluginHub.swift:48-55`) |
| `install_git` | `git_url:"https://…"` (HTTPS-only, no userinfo/query/fragment — `PluginHub.swift:21-25`) | generic success (`Plugins.swift:91-94`) |
| `update_plugin` | — (custom plugins only) | generic success (`Plugins.swift:95-97`) |

### 3.5 Agents & scope options — **seen** (`Plugins.swift:133-142`)
- `POST /api/agents` body `{"action":"list"}` → `{"ok":true,"data":[{"key":"…","label":"…"}],…}` max 5000 rows, unique keys.
- `POST /api/projects` with `{"action":"list_options"}` returns the same shape for projects (same parser, `Plugins.swift:133-142`).
- UI callers: `App/PluginsView.swift:182-183`. — **seen**

### 3.6 Model config plugin

#### `POST /api/plugins/_model_config/model_presets` — **seen** (`ModelPresets.swift:108-138,139-168`)
Response cap 2 MiB; max 1000 presets; unique lower-cased names; `Default` must carry chat, utility and embedding identities (`ModelPresets.swift:140,151-161,173-179`).

| Action | Request | Response |
|---|---|---|
| `get` | `{"action":"get","context_id":"…"?}` | `{ok:true,"presets":[{name,chat?,vision?,utility?,embedding?,…}],"configured_preset"?,"selected_preset"?}` |
| `save` | `{"action":"save","presets":[…],"renames":[{"from","to"}]}` — each preset stripped of top-level `_`-prefixed and `api_key` keys, per-slot likewise (`ModelPresets.swift:63-71`) | `{ok,"presets":[…]}` |
| `reset` | `{"action":"reset"}` | `{ok,"presets":[…]}` |
| `select` | `{"action":"select","name":"…","project_name"?,"agent_profile"?,"context_id"?}` | `{ok,"selected_preset":"…"}` |

Slot keys per preset: `chat, vision, utility, embedding` (`ModelSlot`, `ModelPresets.swift:14-18`); slot objects contain `provider`, `name`, optional `kwargs` etc.; `kwargs` is opaque and never stripped (`ModelPresets.swift:37-46,61-63`). — **seen**

#### `POST /api/plugins/_model_config/model_override` — **seen** (`ModelPresets.swift:130-136,139-148,186-198`)
- `get`: `{"action":"get","context_id":"…"}` → `{"allowed":bool,"configured_preset":"…","effective_preset":"…","override":object|null}`.
- `set_preset`: `{"action":"set_preset","context_id","preset_name"}` → `{ok,"preset_name":"…"}`.
- `clear`: `{"action":"clear","context_id"}` → `{ok,"effective_preset":"…"}`.
- **403 exception:** on `set_preset`, HTTP 403 with body exactly `Per-chat override is disabled` maps to `overrideDisabled` without clearing the session (`ModelPresets.swift:191-195`). — **seen**

#### `POST /api/plugins/_model_config/model_config_get` — **seen** (`ModelCatalog.swift:10-22,35-37,44-51`)
- Request: `{}`.
- Response: `{"chat_providers":[{"value":"…","label":"…"}],"embedding_providers":[…]}`; cap 2 MiB; max 512 per list; ids unique, non-empty, ≤256 bytes; labels non-empty ≤512 bytes (`ModelCatalog.swift:16-19`).

#### `POST /api/plugins/_model_config/model_search` — **seen** (`ModelCatalog.swift:23-32,38-43`)
- Request: `{"provider":"<lowercased>","query":"","model_type":"chat"\|"embedding","api_base":"<url>"}`; provider ≤256 B, api_base ≤4096 B.
- Response: `{"provider":"<echoed exactly>","models":[…],"error":"…"?}`; max 20000 models; each ≤2048 B; empty model names invalid.

### 3.7 Context window usage — `POST /api/plugins/_context_window/context_window` — **seen** (`ContextUsage.swift:35-91`)
- Request: `{"context":"<ctx>"}`.
- Response cap 64 KiB: `{"tokens":int,"context_window":int,"usage":{…}?`` provider_usage":{"input_tokens":n,"cached_tokens":n,"output_tokens":n,"cost":number}}`.
- `usage` optional/null → no breakdown; when present, exactly these six keys must each be a non-negative number and **sum exactly to `tokens`**: `messages, system_tools, skills, mcp_tools, system_prompt, extras` (`ContextUsage.swift:41-57,70-77`). — **seen**

### 3.8 Host connector — `POST /api/plugins/_a0_connector/v1/<route>` — **seen** (`HostConnection.swift:108-151`)
Shared wrapper: socketSession guard, standard 401/403 handling, 404 tolerated only where noted, response cap 64 KiB (`HostConnection.swift:142-151`).

| Route | Request | Response |
|---|---|---|
| `capabilities` | `{}` | `{"protocol":"a0-connector.v1","features":["host_tasks_v1"\|"launcher_gateway"\|"host_setup_v1",…]}`; HTTP 404 ⇒ connector absent (`HostConnection.swift:111-115`) |
| `host_status` | `{"context":"<ctx>"}` | `{"version":1,"context_id":"<ctx>","state":str≤64,"host_label":str≤128?,"target_id":"<64 hex>"?,"generation":"<64 hex>"?,"bound":bool,"binding_current":bool,"capabilities":{"browser":{…},"computer_use":{…},"files":{…},"file_write":{…},"code_execution":{…}}}`; each capability = `{"state":"ready\|off\|unsupported\|needs_attention\|container","ready":bool}` with `ready == (state=="ready")` (`HostConnection.swift:37-56`) |
| `launcher_gateway_status` | `{}` | `{"connected":bool,"multiple_hosts":bool?,"gateway":{"version":1,"kind":"launcher","id":"…","host_label":"…","master_enabled":bool,"scopes":{"browser":bool,"computer_use":bool,"files":bool,"file_write":bool,"code_execution":bool}}?}` (parsed `HostConnection.swift:57-96`) |
| `host_setup` | `{"action":"status"}` | `ComputerSetupSnapshot` — shape **unknown** (struct not read; see section 10); 404 tolerated (`HostConnection.swift:122-124`) |
| `host_task` | `{"context","text","message_id","queued":"true"\|"false","target_id","generation","capability":"browser"\|"computer_use"}` — note `queued` is a **string** here | `{"ok":true,"context":"<ctx>","message_id":"<id>","queued":bool}` (`HostConnection.swift:134-141`) |

`target_id`/`generation` are exactly 64 lowercase-hex characters (`HostConnection.swift:102-105`). — **seen**

### 3.9 Media — `GET /api/image_get?path=<path>` — **seen** (`BrowserScreenshot.swift:58-74`)
- Query: single `path` parameter; client-side path validation before requesting: non-empty, ≤2048 B, no leading `//`, no `://`, no `\\`, no control chars, none of `%?#&`, no `.`/`..` path segments, extension ∈ {png, jpg, jpeg, webp, gif, bmp} (`BrowserScreenshot.swift:50-57`).
- Request headers: `Accept: image/png, image/jpeg, image/webp, image/gif, image/bmp`, `Origin`, full `Cookie` header (including the csrf cookie), `X-CSRF-Token` (`BrowserScreenshot.swift:62-66`).
- Response: HTTP 200; `Content-Type` must be one of those image MIMEs; body non-empty, ≤8 MiB (`BrowserScreenshot.swift:12,69-74`). Fetched via a separate bounded, non-caching, non-redirect transport with a post-transfer session re-check (`HTTPClient.swift:104-118,281-288`; `BrowserScreenshot.swift:77-87`). — **seen**

### 3.10 Endpoint summary

Exhaustive literal grep over `Sources/` and `App/` — **seen** (grep output, all matches):
`/login`, `/api/csrf_token`, `/api/poll`, `/api/chat_create`, `/api/message_async`, `/api/message_queue_add`, `/api/message_queue_remove`, `/api/upload`, `/api/image_get`, `/api/history_get`, `/api/ctx_window_get`, `/api/stop`, `/api/pause`, `/api/nudge`, `/api/projects`, `/api/agents`, `/api/plugins`, `/api/plugins_list`, `/api/plugins/_plugin_installer/plugin_install`, `/api/plugins/_model_config/model_presets`, `/api/plugins/_model_config/model_override`, `/api/plugins/_model_config/model_config_get`, `/api/plugins/_model_config/model_search`, `/api/plugins/_context_window/context_window`, plus the connector routes `/api/plugins/_a0_connector/v1/{capabilities,host_status,launcher_gateway_status,host_setup,host_task}` (prefix seen in `HostConnection.swift:144`).

---

## 4. Realtime contract (Socket.IO)

Reference implementation: `Sources/A0Realtime/Realtime.swift` (SocketIO Swift client; all wire-relevant options are explicit).

- **Engine options** (`Realtime.swift:32-36`) — **seen**: Socket.IO **version 3** (`.version(.three)`), path **`/socket.io/`**, **websocket transport only** (`.forceWebsockets(true)`), **`reconnects(false)`** (no library auto-reconnect), `.forceNew(true)`, extra handshake headers **`Origin: <origin>`** and **`Cookie: <cookieHeader>`** (the full cookie header of section 2.4, including the csrf cookie).
- **Namespace:** `/ws` (`Realtime.swift:37`). — **seen**
- **Connect payload** (`Realtime.swift:67`): `{"csrf_token":"<csrf>","handlers":["ws_webui"]}`; connect timeout 10 s; on timeout the attempt fails with stage `connect-timeout` (`Realtime.swift:67-72`). — **seen**
- **Server→client event `state_push`** (`Realtime.swift:45-58`): first argument is a JSON `StatePush` (section 5.2). — **seen**
- **Client→server `state_request` (ack)** (`Realtime.swift:74-104`): emitted with `emitWithAck`; payload `{"ts":<epoch milliseconds as double>,"data":<StateRequest JSON>,"correlationId":"<UUID>"}`; ack timeout 10 s (`Realtime.swift:84-85`). — **seen**
- **Server ack** = `HandshakeAck` JSON (section 5.3); validated against the exact `correlationId`; accepted `handlerId` values: `ws_webui.WsWebui` **or** `api.ws_webui.WsWebui` (`Protocol.swift:105-113`). — **seen**
- **Handshake sequencing** (`Realtime.swift:39-44,74-104`) — **seen**:
  1. On socket connect the client immediately emits `state_request` with the pending `StateRequest`.
  2. `state_push` events arriving before the ack are buffered in arrival order (`pendingPushes`).
  3. A `state_request` issued while another is in flight is queued and re-sent after the current ack (`Realtime.swift:76,95-99`).
  4. After a valid ack the client emits `handshake(epoch, sequenceBase)` upstream, then replays buffered pushes in order (`Realtime.swift:100-102`).
- **Bounded buffer:** pre-handshake buffer holds at most 31 pushes; a push arriving when `pendingPushes.count >= 31` (guard `count < 32`) fails the whole attempt with stage `payload-or-handshake` (`Realtime.swift:53-56`). — **seen** (a 32-push buffer that rejects at the 32nd)
- **Failure stages** surfaced as events (`Realtime.swift:51,63,70,81,93`): `payload-or-handshake`, `socket-error`, `socket-disconnect`, `connect-timeout`, `encoding`. — **seen**
- **No library auto-reconnect; coordinator-driven** (`Realtime.swift:18-19,33`): every reconnect first obtains a **fresh security context** (`refreshSocketSession()` → new CSRF token + cookie header, `HTTPClient.swift:245-255`; coordinator call at `App/SpikeModel.swift:457`) and a **new connection generation UUID** fences all callbacks of the old attempt (`Realtime.swift:24-31,41,47,62,69,87`). The full coordinator state machine is **inferred** from these anchors plus `RealtimeRecovery.swift`; the cited lines are **seen**.
- **Reconnect backoff:** `RealtimeRetrySchedule` — first attempt after 30 s of monotonic foreground time; interval doubles per attempt (30→60→120) capped at **120 s**; missed deadlines never cause catch-up bursts (`RealtimeRecovery.swift:3-14`). — **seen**
- **Foreground poll fallback:** while realtime is not healthy, `/api/poll` continues; both transports feed the same reducer and must converge to identical state (`ProtocolTests.swift:29-44`). — **seen**
- **Teardown:** `disconnect()` bumps the generation, clears handshake/buffer/queue, removes all handlers, disconnects socket and manager (`Realtime.swift:106-110`). — **seen**

---

## 5. State synchronization model

### 5.1 `Snapshot` — **seen** (`Protocol.swift:39-59`; fixtures in section 8.1/8.2)

Required keys: `deselect_chat` (bool), `context` (string), `logs` (array), `log_guid` (string), `log_version` (int), `log_progress` (any JSON), `log_progress_active` (bool), `paused` (bool), `notifications` (array), `notifications_guid` (string), `notifications_version` (int). Nullable: `contexts`, `tasks` (arrays or `null`).

- `LogEntry` (`Protocol.swift:28-37`): `no` int; optional `id` string, `type` string, `heading`, `content`, `kvps` object, `timestamp` (any; number in fixtures), `agentno` int.
- `notifications` entries are opaque objects; fixture shape `{id, no, message, read}`. `contexts` entries opaque; fixture shape `{id, name, running}`. — **seen**

### 5.2 `state_push` payload — **seen** (`Protocol.swift:86-94`; construction `ProtocolTests.swift:35,65`)

```json
{"data":{"runtime_epoch":"<string>","seq":<int>,"snapshot":{ ...Snapshot... }}}
```

### 5.3 `HandshakeAck` — **seen** (`Protocol.swift:96-113`; fixtures in section 8.6/8.7)

```json
{"correlationId":"<uuid>","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"runtime_epoch":"…","seq_base":0},"error":null}]}
```

Validation (`validated`, `Protocol.swift:105-113`): `correlationId` must equal the client's id; a result with `handlerId` ∈ {`ws_webui.WsWebui`, `api.ws_webui.WsWebui`} must exist with `ok:true`; `data` must be an object with non-empty string `runtime_epoch` and integer `seq_base` ≥ 0; `data.code` must be absent/null; `error` must be null. Any violation → `invalidHandshake`. Application errors hidden inside a successful ack are rejected (`ProtocolTests.swift:84-88`). — **seen**

### 5.4 epoch/sequence semantics (`SyncReducer`) — **seen** (`Sources/A0Core/SyncReducer.swift`)

- `beginHandshake(epoch, sequenceBase)` arms the reducer: `epoch = e`, `sequence = seq_base`, `needsFullSync = true` (`SyncReducer.swift:31-33`).
- A push applies **only** when its `runtime_epoch` matches and `seq == sequence + 1`; on success `sequence = push.seq`; otherwise the result is `needsFullSync` (full resync required) (`SyncReducer.swift:42-50`).
- Snapshot application (`SyncReducer.swift:51-84`) — **seen**:
  - stale generation (context switch / newer attempt) → `ignored`;
  - `snapshot.context` must equal the selected context unless `deselect_chat`;
  - incremental snapshots (`full=false`) are rejected (`needsFullSync`) when the pending state requires a full sync, when `log_guid` or `notifications_guid` changed, or when `log_version`/`notifications_version` regressed;
  - a **full** snapshot must contain non-null `contexts` **and** `tasks` (`SyncReducer.swift:59-61`);
  - logs are keyed by `no`; incremental logs upsert by `no`, then sort ascending (`SyncReducer.swift:65-70`);
  - notifications are versioned collection entries, upserted by `id` (`SyncReducer.swift:72-79`);
  - `deselect_chat` clears the selection and logs (`SyncReducer.swift:62`);
  - guid/version/progress/paused are committed only on success; `needsFullSync` clears (`SyncReducer.swift:80-83`).
- `StateRequest` derivation: `log_from = needsFullSync ? 0 : log_version`, `notifications_from = needsFullSync ? 0 : notifications_version` (`SyncReducer.swift:38-41`). — **seen**
- GUID reset (server restart of a collection) always forces a full snapshot (`ProtocolTests.swift:52-60`). — **seen**
- Realtime handoff safety: a socket-derived candidate is adopted only when the current poll generation matches, GUIDs did not reset, and versions did not regress (`RealtimeRecovery.swift:18-41`). — **seen**

---

## 6. Foreground poll fallback & recovery

- **Poll recovery policy** (`Sources/A0Core/PollRecovery.swift`) applies **only to foreground snapshot reads**, never to commands or login (`PollRecovery.swift:7`). — **seen**
  - Transient errors: URLError codes `notConnectedToInternet`, `networkConnectionLost`, `timedOut`, `cannotConnectToHost`, `cannotFindHost`, `dnsLookupFailed`; HTTP 500/502/503/504 (`PollRecovery.swift:20-29`). — **seen**
  - Schedule: delay `2^attempts` seconds (1, 2, 4, 8 s) multiplied by `1 + jitter*0.25` (jitter clamped 0…1); **max 4 transient retries**; the 5th failure or any non-transient error transitions to **`paused`** (stop polling, keep last state) (`PollRecovery.swift:12-19`). — **seen**
- **Realtime retry schedule:** 30 s → 60 s → 120 s cap (section 4; `RealtimeRecovery.swift:4-14`). — **seen**
- On any sync interruption the coordinator invalidates state via `invalidateForRecovery()` (epoch cleared, sequence 0, `needsFullSync = true`; display kept) so the next successful bootstrap performs a full snapshot (`SyncReducer.swift:35-37`; coordinator calls `App/SpikeModel.swift:491,539,631`). — **seen**
- Command semantics: mutations (sends, controls, plugin/project/model calls) are **never** auto-retried; callers persist intent before invoking (`Projects.swift:169`, `Plugins.swift:149`, `ModelPresets.swift:185`, `Attachments.swift:70`). — **seen**

---

## 7. Uploads & attachments

**Multipart, not base64** — **seen** (`Sources/A0Core/Attachments.swift:47-67`):

- `Content-Type: multipart/form-data; boundary=A0-<UUID>` (`Attachments.swift:54-55`).
- Text parts: `Content-Disposition: form-data; name="<key>"` followed by the raw value; allowed field names: `text`, `context`, `message_id` (`Attachments.swift:53,58-60`).
- File parts: `Content-Disposition: form-data; name="<fileField>"; filename="<uploadName>"` plus `Content-Type: <mime>`, then raw bytes; `fileField` ∈ {`file`, `attachments`} (`Attachments.swift:53,61-64`).
- Limits: max **5 files** per message; **10 MiB per file**; **20 MiB total** per message (`Attachments.swift:22,51-52`). — **seen**
- `uploadName` = `<UUID>-<basename>` where basename maps every char outside ASCII letters/digits/`.-_` to `_`, is truncated to 120 chars, trailing dots stripped, empty → `file` (`Attachments.swift:39-45`). — **seen**

Two flows — **seen** (`Attachments.swift:71-92`):

1. **Queued:** `POST /api/upload` (file parts named `file`, no text fields) → response must decode `{"filenames":["<uploadName>",…]}` **equal to the client-computed upload names, in order** (`Attachments.swift:74-78`); then `POST /api/message_queue_add` JSON with `attachments` array (section 3.1, `Attachments.swift:79-84`).
2. **Immediate:** single `POST /api/message_async` multipart with text fields `context`, `text`, `message_id` and file parts named `attachments` → response `{"context":"<ctx>"}` (`Attachments.swift:85-91`).

Both flows run the standard 401/403 handling (`Attachments.swift:93-99`). — **seen**

No size limit is communicated by the server in the protocol beyond the client-enforced limits above (client-side enforcement is **seen**; a server-side cap is **unknown**, section 10).

---

## 8. Representative fixtures

### 8.1 Full snapshot — **seen** `Tests/A0CoreTests/Fixtures/full.json`

```json
{
  "deselect_chat": false,
  "context": "synthetic-chat",
  "contexts": [{"id": "synthetic-chat", "name": "Synthetic fixture", "running": false}],
  "tasks": [],
  "logs": [
    {"no": 0, "id": "synthetic-user", "type": "user", "heading": "", "content": "Synthetic request", "kvps": {}, "timestamp": 0, "agentno": 0},
    {"no": 1, "id": "synthetic-response", "type": "response", "heading": "", "content": "Working", "kvps": {}, "timestamp": 1, "agentno": 0}
  ],
  "log_guid": "synthetic-log",
  "log_version": 2,
  "log_progress": "Working",
  "log_progress_active": true,
  "paused": false,
  "notifications": [{"id": "synthetic-notice", "no": 0, "message": "Synthetic notice", "read": false}],
  "notifications_guid": "synthetic-notifications",
  "notifications_version": 1
}
```

### 8.2 Delta snapshot — **seen** `Tests/A0CoreTests/Fixtures/delta.json`

Identical key set; differences: `"contexts": null`, `"tasks": null` (collections retained), logs contain only `no:1` with content `Completed synthetic response`, `log_version: 3`, `log_progress: ""`, `log_progress_active: false`, notification `"read": true`, `notifications_version: 2`.

### 8.3 Login sequence with exact wire values — **seen** (`Tests/A0CoreTests/HTTPTests.swift:15-36`)

```text
GET  /api/csrf_token          → 302 Location: /login?next=/api/csrf_token
POST /login                   body: username=user%2Bname&password=a%26b%3D%20c
                              → 302 Location: /   Set-Cookie: session_test-runtime=synthetic-session; Path=/; HttpOnly
GET  /api/csrf_token          → 200 {"ok":true,"token":"synthetic-csrf","runtime_id":"test-runtime"}
```

Subsequent poll request headers verified (`HTTPTests.swift:32-35`): `X-CSRF-Token: synthetic-csrf`; `Cookie` contains both `session_test-runtime=synthetic-session` and `csrf_token_test-runtime=synthetic-csrf`; `Origin: https://server.test`.

### 8.4 Poll request — **seen** (`ProtocolTests.swift:11-18`; `Protocol.swift:76-83`)

```json
{"context": null, "log_from": 0, "notifications_from": 0, "timezone": "UTC", "collections_delta": true}
```

### 8.5 `state_request` emit — **seen** (`Realtime.swift:84-85`)

```json
{"ts": 1730000000000, "data": {"context": "synthetic-chat", "log_from": 0, "notifications_from": 0, "timezone": "Europe/Amsterdam", "collections_delta": true}, "correlationId": "<UUID>"}
```

(`ts` value illustrative; field is an epoch-milliseconds double.)

### 8.6 Valid handshake ack — **seen** (`ProtocolTests.swift:78-83`)

```json
{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"runtime_epoch":"runtime","seq_base":1}}]}
```

### 8.7 Application error inside a successful ack (must be rejected) — **seen** (`ProtocolTests.swift:84-88`)

```json
{"correlationId":"request-1","results":[{"handlerId":"ws_webui.WsWebui","ok":true,"data":{"code":"INVALID_REQUEST"}}]}
```

### 8.8 `state_push` wrapper — **seen** (`ProtocolTests.swift:35,65`)

```json
{"data":{"runtime_epoch":"synthetic-runtime","seq":2,"snapshot":{ ...full.json... }}}
```

### 8.9 Chat commands — **seen** (`HTTPClient.swift:217,224-225`)

```text
POST /api/chat_create        {"new_context":"ctx-1","current_context":""}        → {"ok":true,"ctxid":"ctx-1"}
POST /api/message_async      {"context":"ctx-1","text":"hi","message_id":"m-1"}  → {"context":"ctx-1"}
POST /api/message_queue_add  {"context":"ctx-1","text":"hi","item_id":"m-2"}     → {"ok":true,"item_id":"m-2"}
```

### 8.10 Stop sequence — **seen** (`AgentControls.swift:12,50-53`)

```text
POST /api/message_queue_remove  {"context":"ctx-1"}  → {"ok":true,"remaining":0}
POST /api/stop                  {"context":"ctx-1"}  → {"context":"ctx-1","stopped":true}
```

---

## 9. Size & count limits (all **seen**)

| Item | Limit | Source |
|---|---|---|
| Attachments per file / count / total | 10 MiB / 5 / 20 MiB | `Attachments.swift:22,51-52` |
| Image download (`/api/image_get`) | 8 MiB; image MIMEs only | `BrowserScreenshot.swift:12,69-74` |
| Plugin response envelope | 8 MiB | `Plugins.swift:111` |
| Plugin list rows / doc content | 5000 / 1 MiB | `Plugins.swift:119,146` |
| Projects response / list items | 2 MiB / 10000 | `Projects.swift:121,128` |
| Agent-controls response | 2 MiB | `AgentControls.swift:20` |
| Model presets response / count | 2 MiB / 1000 | `ModelPresets.swift:140,153` |
| Model catalog / provider lists | 2 MiB / 512 per list | `ModelCatalog.swift:16-19` |
| Model search models / name length | 20000 / 2048 B | `ModelCatalog.swift:29-30` |
| Context-window & connector responses | 64 KiB | `ContextUsage.swift:36`; `HostConnection.swift:113,149` |
| HostConnection raw payload | 64 KiB | `HostConnection.swift:38,61` |
| Pre-handshake push buffer | 32 pushes (reject at the 32nd) | `Realtime.swift:54` |
| Socket connect / ack timeout | 10 s each | `Realtime.swift:67,85` |
| HTTP request / resource timeout | 20 s / 30 s | `HTTPClient.swift:91-92` |
| Persisted cookie name / value | 256 chars / 16 KiB | `SavedAuthentication.swift:16-18` |
| Plugin identity / doc path name | 160 chars / 255 chars | `Plugins.swift:37`; `Attachments.swift:24` |
| Image path length / URI length | 2048 B / 4096 B | `BrowserScreenshot.swift:42,51` |
| Project list items | 10000 | `Projects.swift:128` |

---

## 10. Unknowns & not covered

- **`ComputerSetupSnapshot`** (`host_setup` response) is referenced (`HostConnection.swift:124`) but its struct source was not read; wire shape **unknown**.
- Server-side semantics of `collections_delta` (client default `true`; flag name/position **seen**, effect **inferred** as delta-vs-full collection control).
- Server-side meaning of `StateRequest.timezone` beyond the client sending the IANA identifier (`Protocol.swift:65-68`) — **unknown**.
- `log_progress` is typed any-JSON (`Protocol.swift:47`); fixture shows a string; other variants **unknown**.
- No pagination/range parameters observed on `/api/history_get` / `/api/ctx_window_get` — **seen** (absence in `AgentControls.swift`); server may truncate — **unknown**.
- The coordinator state machine in `App/SpikeModel.swift` was sampled only at cited lines (`457,472,491,495-496,539,631`); full orchestration order is **inferred**, not exhaustively read.
- No WebSocket ping/pong or custom heartbeat in the client (`Realtime.swift` contains none) — Engine.IO pings presumably handled by the library; **unknown/unverified**.
- Server-side attachment size caps are **unknown** (only client-enforced limits are seen, section 7).
- `PreviewHTTPTransport.swift` / `PreviewRealtimeClient.swift` (App previews) were not inventoried; they are test doubles, not wire contracts.

---

*End of Phase 1 inventory. Every claim marked **seen** is directly citable to the listed file:line locations in the reference tree. Sections 1–9 are implementation-ready for the Rust `a0-api` and `a0-realtime` crates.*
