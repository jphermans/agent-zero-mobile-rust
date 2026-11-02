//! Offline wire tests for `a0-api` — fixture values taken straight from
//! `docs/phase1/protocol-inventory.md` §8 (login §8.3, poll §8.4/§8.1, chat
//! commands §8.9, stop sequence §8.10). HTTP is mocked with wiremock; the
//! loopback `http` origin uses the §1 `.loopbackDevelopment` debug policy.

use a0_api::client::{A0Client, Origin};
use a0_api::endpoints::agentcontrols;
use a0_api::endpoints::core::{self, StateRequest};
use a0_api::error::{ApiError, AuthErrorKind};
use a0_api::upload::{message_async_multipart, message_queue_add_with_attachments, upload_files, UploadFile};
use serde_json::{json, Value};
use wiremock::matchers::{body_json, body_string_contains, header_regex, method, path};
use wiremock::{Mock, MockServer, Respond, ResponseTemplate};

async fn test_client(server: &MockServer) -> A0Client {
    let origin = Origin::new(&server.uri(), true).expect("loopback debug origin");
    A0Client::builder(origin).build().expect("client")
}

/// §8.3 login sequence with the exact wire values:
/// GET /api/csrf_token → 302 /login?next=/api/csrf_token;
/// POST /login → 302 / with Set-Cookie session_test-runtime=synthetic-session;
/// GET /api/csrf_token → 200 bootstrap {ok,token,runtime_id}.
async fn mount_login_flow(server: &MockServer) {
    // wiremock matches overlapping mocks LAST-mounted-first, so the limited
    // probe-302 mock must mount AFTER the unlimited bootstrap-200 mock: the
    // first GET consumes the 302 (up_to_n_times(1)), the second GET falls
    // through to the 200 bootstrap.
    // Authenticated bootstrap: only matches once the session cookie is sent,
    // so the anonymous probe cannot hit it regardless of wiremock match order.
    Mock::given(method("GET"))
        .and(path("/api/csrf_token"))
        .and(header_regex("cookie", "session_test-runtime="))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "token": "synthetic-csrf",
                "runtime_id": "test-runtime",
            })),
        )
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(
            ResponseTemplate::new(302)
                .append_header("Location", "/")
                .append_header("Set-Cookie", "session_test-runtime=synthetic-session; Path=/; HttpOnly"),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/csrf_token"))
        .respond_with(
            ResponseTemplate::new(302).append_header("Location", "/login?next=/api/csrf_token"),
        )
        .up_to_n_times(1)
        .mount(server)
        .await;
}

/// Login against [`mount_login_flow`] and return (client, requests so far).
async fn logged_in_client(server: &MockServer) -> A0Client {
    let client = test_client(server).await;
    client.login("user+name", "a&b= c").await.expect("login ok");
    client
}

#[tokio::test]
async fn login_handshake_shape_s8_3() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = test_client(&server).await;
    client.login("user+name", "a&b= c").await.expect("login ok");

    let ss = client.socket_session().expect("session held");
    assert_eq!(ss.csrf_token, "synthetic-csrf");
    assert_eq!(ss.runtime_id, "test-runtime");
    // §2.4: jar cookie name-sorted, synthetic csrf cookie appended LAST.
    assert_eq!(
        ss.cookie_header,
        "session_test-runtime=synthetic-session; csrf_token_test-runtime=synthetic-csrf"
    );

    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 3, "probe + login POST + bootstrap");

    // POST /login body: RFC 3986 unreserved encoding of the §8.3 credentials.
    let login_post = requests
        .iter()
        .find(|r| r.url.path() == "/login")
        .expect("login POST present");
    assert_eq!(
        String::from_utf8_lossy(&login_post.body),
        "username=user%2Bname&password=a%26b%3D%20c"
    );
    assert_eq!(login_post.method.as_str(), "POST");
    // No X-CSRF-Token during login (absent until a token is held, §1).
    assert!(login_post.headers.get("x-csrf-token").is_none());
    // Origin header on every request (§1).
    assert_eq!(
        login_post.headers.get("origin").and_then(|v| v.to_str().ok()),
        Some(server.uri().as_str())
    );
    // Bootstrap GET must carry the session cookie now.
    let bootstrap = &requests[2];
    assert_eq!(
        bootstrap
            .headers
            .get("cookie")
            .and_then(|v| v.to_str().ok()),
        Some("session_test-runtime=synthetic-session")
    );
}

#[tokio::test]
async fn login_200_html_is_invalid_credentials() {
    let server = MockServer::start().await;
    // Probe requires login, then the login POST returns a 200 HTML error page
    // (§2.1 step 3, `HTTPTests.swift:37-43`).
    Mock::given(method("GET"))
        .and(path("/api/csrf_token"))
        .respond_with(
            ResponseTemplate::new(302).append_header("Location", "/login?next=/api/csrf_token"),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>nope</html>"))
        .mount(&server)
        .await;
    let client = test_client(&server).await;
    let err = client.login("u", "p").await.unwrap_err();
    assert!(matches!(
        err,
        ApiError::Auth { kind: AuthErrorKind::InvalidCredentials }
    ));
}

#[tokio::test]
async fn login_cross_origin_redirect_rejected_without_resending() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/csrf_token"))
        .respond_with(
            ResponseTemplate::new(302).append_header("Location", "/login?next=/api/csrf_token"),
        )
        .mount(&server)
        .await;
    // Cross-origin redirect (§2.1 step 3, `HTTPTests.swift:44-49`).
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(
            ResponseTemplate::new(302).append_header("Location", "https://evil.example.com/steal"),
        )
        .mount(&server)
        .await;
    let client = test_client(&server).await;
    let err = client.login("u", "p").await.unwrap_err();
    assert!(matches!(
        err,
        ApiError::Auth { kind: AuthErrorKind::InvalidCredentials }
    ));
    // Credentials were sent exactly ONCE (§2.1: never resent).
    let requests = server.received_requests().await.expect("requests");
    let login_posts = requests
        .iter()
        .filter(|r| r.url.path() == "/login")
        .count();
    assert_eq!(login_posts, 1);
}

#[tokio::test]
async fn login_disabled_loopback_dev_origin_accepted_s2_1() {
    let server = MockServer::start().await;
    // A 200 bootstrap on an empty jar = login disabled; accepted ONLY for
    // unauthenticated loopback dev origins (§2.1 step 1) — which is exactly
    // what the debug-loopback test origin is.
    Mock::given(method("GET"))
        .and(path("/api/csrf_token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({
                    "ok": true,
                    "token": "dev-csrf",
                    "runtime_id": "dev-runtime",
                }))
                // §2.2: the jar must hold session_<runtime_id> for the
                // bootstrap to validate.
                .append_header("Set-Cookie", "session_dev-runtime=dev-session; Path=/"),
        )
        .mount(&server)
        .await;
    let client = test_client(&server).await;
    client.login("u", "p").await.expect("login-disabled dev origin accepted");
    // No POST /login happened.
    let requests = server.received_requests().await.expect("requests");
    assert!(requests.iter().all(|r| r.url.path() != "/login"));
    let ss = client.socket_session().expect("session");
    assert_eq!(ss.runtime_id, "dev-runtime");
}

#[tokio::test]
async fn poll_request_and_snapshot_s8_4_s8_1() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;

    // Request body EXACTLY per §8.4 fixture; response per §8.1 full snapshot.
    let expected_request = json!({
        "context": Value::Null,
        "log_from": 0,
        "notifications_from": 0,
        "timezone": "UTC",
        "collections_delta": true,
    });
    let snapshot = json!({
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
    });
    Mock::given(method("POST"))
        .and(path("/api/poll"))
        .and(body_json(expected_request))
        .respond_with(ResponseTemplate::new(200).set_body_json(snapshot))
        .mount(&server)
        .await;

    let snap = core::poll(&client, &StateRequest::new()).await.expect("poll");
    assert_eq!(snap.context, "synthetic-chat");
    assert_eq!(snap.logs.len(), 2);
    assert_eq!(snap.logs[0].no, 0);
    assert_eq!(snap.logs[1].content.as_deref(), Some("Working"));
    assert_eq!(snap.log_guid, "synthetic-log");
    assert_eq!(snap.log_version, 2);
    assert_eq!(snap.notifications_version, 1);
    assert!(snap.contexts.is_some());
    assert!(snap.tasks.is_some());

    // Poll headers: X-CSRF-Token + full §2.4 cookie header + Origin.
    let requests = server.received_requests().await.expect("requests");
    let poll_req = requests
        .iter()
        .find(|r| r.url.path() == "/api/poll")
        .expect("poll request present");
    assert_eq!(
        poll_req
            .headers
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok()),
        Some("synthetic-csrf")
    );
    assert_eq!(
        poll_req
            .headers
            .get("cookie")
            .and_then(|v| v.to_str().ok()),
        Some("session_test-runtime=synthetic-session; csrf_token_test-runtime=synthetic-csrf")
    );
}

/// Echo responder: parses the JSON body, echoes the `item_id` field inside
/// `{"ok":true,"item_id":...}` (§3.1 queue-add contract).
struct EchoItemId;

impl Respond for EchoItemId {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        let item_id = body.get("item_id").cloned().unwrap_or(Value::Null);
        ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "item_id": item_id }))
    }
}

/// Echo responder for `POST /api/upload` (§7): extracts the client-computed
/// upload names from the multipart body — file parts named `file` carry
/// `filename="<uploadName>"` — and returns `{"filenames":[...]}` in order.
struct EchoUploadNames;

impl Respond for EchoUploadNames {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let text = String::from_utf8_lossy(&request.body);
        let first_line = text.split("\r\n").next().unwrap_or("");
        let boundary = first_line.strip_prefix("--").unwrap_or("");
        let mut names = Vec::new();
        for part in text.split("--").skip(1) {
            let full = format!("--{part}");
            if !full.contains("name=\"file\"") {
                continue;
            }
            if let Some(pos) = full.find("filename=\"") {
                let rest = &full[pos + "filename=\"".len()..];
                if let Some(end) = rest.find('"') {
                    names.push(rest[..end].to_string());
                }
            }
        }
        let _ = boundary;
        ResponseTemplate::new(200).set_body_json(json!({ "filenames": names }))
    }
}

#[tokio::test]
async fn chat_commands_s8_9() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;

    Mock::given(method("POST"))
        .and(path("/api/chat_create"))
        .and(body_json(json!({ "new_context": "ctx-1", "current_context": "" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "ctxid": "ctx-1" })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/message_async"))
        .and(body_string_contains("\"text\":\"hi\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "context": "ctx-1" })))
        .mount(&server)
        .await;

    core::chat_create(&client, "ctx-1").await.expect("chat_create");
    let message_id = core::message_async(&client, "ctx-1", "hi").await.expect("message_async");
    assert!(!message_id.is_empty());

    // message_async request must carry context/text/message_id (§3.1 JSON).
    let requests = server.received_requests().await.expect("requests");
    let send = requests
        .iter()
        .find(|r| r.url.path() == "/api/message_async")
        .expect("send request");
    let body: Value = serde_json::from_slice(&send.body).unwrap();
    assert_eq!(body["context"], "ctx-1");
    assert_eq!(body["text"], "hi");
    assert_eq!(body["message_id"], json!(message_id));
    assert_eq!(
        send.headers.get("content-type").and_then(|v| v.to_str().ok()),
        Some("application/json")
    );
}

#[tokio::test]
async fn queue_add_remove_s8_9_s8_10() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;

    Mock::given(method("POST"))
        .and(path("/api/message_queue_add"))
        .respond_with(EchoItemId)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/message_queue_remove"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "remaining": 0 })))
        .mount(&server)
        .await;

    let request = core::MessageQueueAddRequest::new("ctx-1", "hi");
    let item_id = core::message_queue_add(&client, &request).await.expect("queue add");
    assert!(!item_id.is_empty());
    let remaining = core::message_queue_remove(&client, "ctx-1").await.expect("queue remove");
    assert_eq!(remaining, 0);

    let requests = server.received_requests().await.expect("requests");
    let add = requests
        .iter()
        .find(|r| r.url.path() == "/api/message_queue_add")
        .expect("queue add request");
    let body: Value = serde_json::from_slice(&add.body).unwrap();
    assert_eq!(body["context"], "ctx-1");
    assert_eq!(body["text"], "hi");
    assert_eq!(body["item_id"], json!(item_id));
    // Bare text variant has NO attachments key (§3.1).
    assert!(body.get("attachments").is_none());
    let remove = requests
        .iter()
        .find(|r| r.url.path() == "/api/message_queue_remove")
        .expect("queue remove request");
    let body: Value = serde_json::from_slice(&remove.body).unwrap();
    assert_eq!(body, json!({ "context": "ctx-1" }));
}

#[tokio::test]
async fn upload_queued_flow_s7() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;

    Mock::given(method("POST"))
        .and(path("/api/upload"))
        .respond_with(EchoUploadNames)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/message_queue_add"))
        .respond_with(EchoItemId)
        .mount(&server)
        .await;

    let files = vec![
        UploadFile {
            bytes: b"PNG-DATA-1".to_vec(),
            mime: "image/png".to_string(),
            basename: "shot one.png".to_string(), // sanitized on the wire
        },
        UploadFile {
            bytes: b"JPEG-DATA-2".to_vec(),
            mime: "image/jpeg".to_string(),
            basename: "photo.jpeg".to_string(),
        },
    ];
    let names = upload_files(&client, &files).await.expect("upload");
    assert_eq!(names.len(), 2);
    assert!(names[0].ends_with("shot_one.png"));
    assert!(names[1].ends_with("photo.jpeg"));
    let item_id = message_queue_add_with_attachments(&client, "ctx-1", "hi", &names)
        .await
        .expect("queue add with attachments");
    assert!(!item_id.is_empty());

    // §7 upload request shape: file parts, no text fields.
    let requests = server.received_requests().await.expect("requests");
    let upload = requests
        .iter()
        .find(|r| r.url.path() == "/api/upload")
        .expect("upload request");
    let ctype = upload
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(ctype.starts_with("multipart/form-data; boundary=A0-"), "boundary prefix A0-");
    let body = String::from_utf8_lossy(&upload.body);
    assert!(body.contains("name=\"file\"; filename=\""));
    assert!(body.contains("Content-Type: image/png"));
    assert!(body.contains("PNG-DATA-1"));
    assert!(body.contains("JPEG-DATA-2"));
    // NO text fields on /api/upload (§7 flow 1).
    assert!(!body.contains("name=\"context\""));

    // Queue-add carries the attachments array of uploadNames (§3.1/§7).
    let add = requests
        .iter()
        .find(|r| r.url.path() == "/api/message_queue_add")
        .expect("queue add");
    let payload: Value = serde_json::from_slice(&add.body).unwrap();
    assert_eq!(payload["attachments"], json!(names));
}

#[tokio::test]
async fn message_async_multipart_s7() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;

    Mock::given(method("POST"))
        .and(path("/api/message_async"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "context": "ctx-1" })))
        .mount(&server)
        .await;

    let files = vec![UploadFile {
        bytes: b"WEBP-DATA".to_vec(),
        mime: "image/webp".to_string(),
        basename: "img.webp".to_string(),
    }];
    message_async_multipart(&client, "ctx-1", "hello", &files)
        .await
        .expect("immediate multipart send");

    let requests = server.received_requests().await.expect("requests");
    let send = requests
        .iter()
        .find(|r| r.url.path() == "/api/message_async")
        .expect("multipart send");
    let body = String::from_utf8_lossy(&send.body);
    // Text fields context/text/message_id present WITHOUT filename (§7).
    assert!(body.contains("name=\"context\"\r\n\r\nctx-1"));
    assert!(body.contains("name=\"text\"\r\n\r\nhello"));
    assert!(body.contains("name=\"message_id\"\r\n\r\n"));
    // File parts named `attachments` with filename + mime (§7).
    assert!(body.contains("name=\"attachments\"; filename=\""));
    assert!(body.contains("Content-Type: image/webp\r\n\r\nWEBP-DATA"));
    // Exactly ONE request (mutation, never retried).
    let sends = requests.iter().filter(|r| r.url.path() == "/api/message_async").count();
    assert_eq!(sends, 1);
}

/// Compact valid snapshot (§8.2 delta style: contexts/tasks null).
fn snapshot_json() -> Value {
    json!({
        "deselect_chat": false,
        "context": "synthetic-chat",
        "contexts": Value::Null,
        "tasks": Value::Null,
        "logs": [{"no": 1, "id": "synthetic-response", "content": "Working"}],
        "log_guid": "synthetic-log",
        "log_version": 2,
        "log_progress": "Working",
        "log_progress_active": true,
        "paused": false,
        "notifications": [],
        "notifications_guid": "synthetic-notifications",
        "notifications_version": 1
    })
}

#[tokio::test]
async fn redirect_not_followed_generic_3xx_is_status_s1() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;

    // Same-origin redirect to a NON-/login path: never followed (§1),
    // surfaces as ApiError::Status.
    Mock::given(method("POST"))
        .and(path("/api/poll"))
        .respond_with(ResponseTemplate::new(302).append_header("Location", "/somewhere-else"))
        .mount(&server)
        .await;

    let err = core::poll(&client, &StateRequest::new()).await.unwrap_err();
    assert!(matches!(err, ApiError::Status { code: 302 }), "got {err:?}");

    // Exactly ONE poll request — the redirect target was never requested.
    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.iter().filter(|r| r.url.path() == "/api/poll").count(), 1);
    assert!(requests.iter().all(|r| r.url.path() != "/somewhere-else"));
    // Session is KEPT on a generic 3xx (only 401/403 clear, §1).
    assert!(client.socket_session().is_some());
}

#[tokio::test]
async fn login_redirect_after_auth_requires_login_and_clears_s1() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;

    // §1 login redirect (same origin, path /login) on an authenticated call:
    // error requiresLogin AND the session is cleared.
    Mock::given(method("POST"))
        .and(path("/api/poll"))
        .respond_with(ResponseTemplate::new(302).append_header("Location", "/login?next=/api/poll"))
        .mount(&server)
        .await;

    let err = core::poll(&client, &StateRequest::new()).await.unwrap_err();
    assert!(matches!(
        err,
        ApiError::Auth { kind: AuthErrorKind::RequiresLogin }
    ));
    assert!(client.socket_session().is_none(), "session cleared after login redirect");
}

#[tokio::test]
async fn get_retry_then_success_s6() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;

    // §6 poll recovery: first attempt 500, retry succeeds. wiremock 0.6
    // prefers the FIRST-mounted matching mock, so the limited 500 must be
    // mounted BEFORE the 200: poll #1 hits the 500, poll #2 falls through.
    Mock::given(method("POST"))
        .and(path("/api/poll"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/poll"))
        .respond_with(ResponseTemplate::new(200).set_body_json(snapshot_json()))
        .mount(&server)
        .await;

    let snap = core::poll(&client, &StateRequest::new()).await.expect("retry succeeds");
    assert_eq!(snap.context, "synthetic-chat");
    assert_eq!(snap.logs.len(), 1);

    let requests = server.received_requests().await.expect("requests");
    let polls = requests.iter().filter(|r| r.url.path() == "/api/poll").count();
    assert_eq!(polls, 2, "one failed attempt + one successful retry");
}

#[tokio::test]
async fn mutation_never_retried_s6() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;

    // Mutations are NEVER auto-retried (§6): a persistent 500 on /api/stop
    // must surface after EXACTLY ONE request.
    Mock::given(method("POST"))
        .and(path("/api/stop"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let err = agentcontrols::stop(&client, "ctx-1").await.unwrap_err();
    assert!(matches!(err, ApiError::Status { code: 500 }));

    let requests = server.received_requests().await.expect("requests");
    let stops = requests.iter().filter(|r| r.url.path() == "/api/stop").count();
    assert_eq!(stops, 1, "mutation sent exactly once");
}

#[tokio::test]
async fn auth_error_classification_s1() {
    // 401 → requiresLogin, session cleared.
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/history_get"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let err = agentcontrols::history_get(&client, "ctx-1").await.unwrap_err();
    assert!(matches!(
        err,
        ApiError::Auth { kind: AuthErrorKind::RequiresLogin }
    ));
    assert!(client.socket_session().is_none(), "401 clears the session");
    drop(client);

    // 403 → csrfRejected, session cleared.
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/pause"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let err = agentcontrols::pause(&client, "ctx-1", true).await.unwrap_err();
    assert!(matches!(
        err,
        ApiError::Auth { kind: AuthErrorKind::CsrfRejected }
    ));
    assert!(client.socket_session().is_none(), "403 clears the session");
    drop(client);

    // 404 → plain Status, session KEPT.
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let client = logged_in_client(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/nudge"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let err = agentcontrols::nudge(&client, "ctx-1").await.unwrap_err();
    assert!(matches!(err, ApiError::Status { code: 404 }));
    assert!(client.socket_session().is_some(), "404 keeps the session");
}
