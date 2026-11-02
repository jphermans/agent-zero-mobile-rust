//! Offline auth-flow tests for `a0-auth` — §8.3 login sequence with exact
//! wire values, §2.3 restore/refresh/logout, §2.4 cookie-header ordering and
//! the §2.3 persisted-cookie validation rules.

use a0_auth::error::AuthError;
use a0_auth::http::{parse_bootstrap, AuthOrigin};
use a0_auth::login::{AuthFlow, LoginCredentials};
use a0_auth::session::{csrf_cookie_name, session_cookie_name};
use a0_auth::store::{InMemorySecretStore, SavedCookie, SecretStore, ServerProfileId};
use serde_json::{json, Value};
use wiremock::matchers::{header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROFILE: &str = "home-server";

fn profile() -> ServerProfileId {
    ServerProfileId(PROFILE.to_string())
}

async fn test_flow(server: &MockServer) -> AuthFlow {
    let origin = AuthOrigin::new(&server.uri(), true).expect("loopback debug origin");
    AuthFlow::new(origin, profile()).expect("auth flow")
}

/// §8.3 mount: probe 302 → POST /login 302 (+ Set-Cookie) → bootstrap 200.
/// Same LAST-first mounting trick as the a0-api suite: the limited 302 probe
/// mounts after the unlimited bootstrap mock.
async fn mount_login_flow(server: &MockServer) {
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

#[tokio::test]
async fn login_s2_1_with_exact_s8_3_wire_values() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let flow = test_flow(&server).await;
    let store = InMemorySecretStore::new();

    let session = flow
        .login(
            LoginCredentials::new("user+name", "a&b= c"),
            false,
            &store,
        )
        .await
        .expect("login ok");

    // Session material (§2.5 + username from the credential login).
    assert_eq!(session.csrf_token, "synthetic-csrf");
    assert_eq!(session.runtime_id, "test-runtime");
    assert_eq!(session.username.as_deref(), Some("user+name"));
    assert_eq!(session.cookies, vec![(
        "session_test-runtime".to_string(),
        "synthetic-session".to_string()
    )]);
    // §2.4 order: jar cookie first, synthetic csrf cookie LAST.
    assert_eq!(
        session.cookie_header(),
        "session_test-runtime=synthetic-session; csrf_token_test-runtime=synthetic-csrf"
    );

    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 3);
    let login_post = requests.iter().find(|r| r.url.path() == "/login").expect("login POST");
    assert_eq!(
        String::from_utf8_lossy(&login_post.body),
        "username=user%2Bname&password=a%26b%3D%20c"
    );
    assert_eq!(
        login_post
            .headers
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/x-www-form-urlencoded")
    );
    assert!(login_post.headers.get("x-csrf-token").is_none());

    // §2.3: exactly one cookie persisted.
    let saved = store.get(&profile()).await.expect("store get");
    let saved = saved.expect("cookie persisted");
    assert_eq!(saved.name, "session_test-runtime");
    assert_eq!(saved.value.as_str(), "synthetic-session");
    assert_eq!(saved.version, 1);
}

#[tokio::test]
async fn restore_s2_3_without_credentials() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let flow = test_flow(&server).await;
    let store = InMemorySecretStore::new();
    flow.login(LoginCredentials::new("user+name", "pw"), false, &store)
        .await
        .expect("initial login");

    // Fresh flow (empty in-memory jar) restores from the store ONLY.
    let server2 = MockServer::start().await;
    // §2.3 restore: anonymous probe must REQUIRE login, then the
    // authenticated bootstrap succeeds with the inserted cookie.
    Mock::given(method("GET"))
        .and(path("/api/csrf_token"))
        .and(header_regex("cookie", "session_test-runtime="))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "token": "restored-csrf",
                "runtime_id": "test-runtime",
            })),
        )
        .mount(&server2)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/csrf_token"))
        .respond_with(
            ResponseTemplate::new(302).append_header("Location", "/login?next=/api/csrf_token"),
        )
        .up_to_n_times(1)
        .mount(&server2)
        .await;
    let flow2 = test_flow(&server2).await;
    let session = flow2.restore(&store).await.expect("restore ok");

    assert_eq!(session.csrf_token, "restored-csrf");
    assert_eq!(session.username, None, "username is never reconstructed (§2.3)");
    assert_eq!(
        session.cookie_header(),
        "session_test-runtime=synthetic-session; csrf_token_test-runtime=restored-csrf"
    );

    // A restore on a server that does NOT require login fails with
    // unauthenticatedServer (§2.3: 200 on the anonymous probe = login
    // disabled, not restorable).
    let server3 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/csrf_token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "token": "x",
                "runtime_id": "y",
            })),
        )
        .mount(&server3)
        .await;
    let flow3 = test_flow(&server3).await;
    let err = flow3.restore(&store).await.unwrap_err();
    assert!(matches!(err, AuthError::UnauthenticatedServer));
}

#[tokio::test]
async fn refresh_s2_3_revalidates_bootstrap() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let flow = test_flow(&server).await;
    let store = InMemorySecretStore::new();
    flow.login(LoginCredentials::new("u", "p"), false, &store)
        .await
        .expect("login");

    // Refresh re-runs the bootstrap GET (probe 302 already consumed).
    let refreshed = flow.refresh().await.expect("refresh ok");
    assert_eq!(refreshed.csrf_token, "synthetic-csrf");
    let requests = server.received_requests().await.expect("requests");
    let gets = requests.iter().filter(|r| r.url.path() == "/api/csrf_token").count();
    assert_eq!(gets, 3, "probe + post-login bootstrap + refresh");
}

#[tokio::test]
async fn logout_s2_3_destroys_local_state_only() {
    let server = MockServer::start().await;
    mount_login_flow(&server).await;
    let flow = test_flow(&server).await;
    let store = InMemorySecretStore::new();
    flow.login(LoginCredentials::new("u", "p"), false, &store)
        .await
        .expect("login");
    assert!(flow.http.held_session().is_some());

    flow.logout(&store).await.expect("logout");
    assert!(flow.http.held_session().is_none(), "session destroyed");
    assert!(flow.http.cookies().is_empty(), "jar wiped");
    assert!(flow.http.cookie_header().is_empty());
    assert!(store.get(&profile()).await.expect("get").is_none(), "persisted cookie deleted");
    // No server call happened for the logout (§2.3: local destruction only).
    let requests = server.received_requests().await.expect("requests");
    assert!(requests.iter().all(|r| r.url.path() != "/logout"));
}

#[tokio::test]
async fn cookie_name_helpers_s2_2_s2_4() {
    assert_eq!(session_cookie_name("test-runtime"), "session_test-runtime");
    assert_eq!(csrf_cookie_name("test-runtime"), "csrf_token_test-runtime");
}

#[tokio::test]
async fn bootstrap_validation_s2_2() {
    let good = json!({"ok": true, "token": "synthetic-csrf", "runtime_id": "test-runtime"});
    assert!(parse_bootstrap(&good).is_ok());
    let bad_ok = json!({"ok": false, "token": "t", "runtime_id": "r"});
    assert!(parse_bootstrap(&bad_ok).is_err());
    let bad_charset = json!({"ok": true, "token": "bad token!", "runtime_id": "r"});
    assert!(parse_bootstrap(&bad_charset).is_err());
    let bad_empty = json!({"ok": true, "token": "", "runtime_id": "r"});
    assert!(parse_bootstrap(&bad_empty).is_err());
    let not_json: Value = json!([1, 2, 3]);
    assert!(parse_bootstrap(&not_json).is_err());
}

#[test]
fn saved_cookie_validation_s2_3() {
    let profile = profile();
    // Name rules: session_ prefix, charset, max 256.
    assert!(SavedCookie::new(profile.clone(), "session_ok", "v", None).is_ok());
    assert!(SavedCookie::new(profile.clone(), "nosession", "v", None).is_err());
    assert!(SavedCookie::new(profile.clone(), "session_has space", "v", None).is_err());
    assert!(SavedCookie::new(profile.clone(), &format!("session_{}", "x".repeat(300)), "v", None).is_err());
    // Value rules: non-empty, ≤16384, printable ASCII w/o ';' and ','.
    assert!(SavedCookie::new(profile.clone(), "session_ok", "", None).is_err());
    assert!(SavedCookie::new(profile.clone(), "session_ok", "has;semi", None).is_err());
    assert!(SavedCookie::new(profile.clone(), "session_ok", "has,comma", None).is_err());
    assert!(SavedCookie::new(profile.clone(), "session_ok", "has space", None).is_err());
    let big = "x".repeat(16_385);
    assert!(SavedCookie::new(profile.clone(), "session_ok", big, None).is_err());
    // Expired material is rejected at store time (§2.3: not expired).
    assert!(SavedCookie::new(profile.clone(), "session_ok", "v", Some(1)).is_err());

    // JSON round-trip (§2.3 record {version,profile,name,value,expires}).
    let cookie = SavedCookie::new(profile.clone(), "session_test-runtime", "synthetic-session", None)
        .expect("valid");
    let json = cookie.to_json();
    let parsed = SavedCookie::from_json(&json).expect("round trip");
    assert_eq!(parsed.name, "session_test-runtime");
    assert_eq!(parsed.value.as_str(), "synthetic-session");
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.profile.0, PROFILE);
}

#[tokio::test]
async fn secret_store_put_replaces_single_slot_s2_3() {
    let store = InMemorySecretStore::new();
    let profile = profile();
    store
        .put(SavedCookie::new(profile.clone(), "session_old", "v1", None).unwrap())
        .await
        .unwrap();
    store
        .put(SavedCookie::new(profile.clone(), "session_new", "v2", None).unwrap())
        .await
        .unwrap();
    // Exactly ONE record per profile: the new one replaced the old.
    let saved = store.get(&profile).await.unwrap().expect("present");
    assert_eq!(saved.name, "session_new");
    store.delete(&profile).await.unwrap();
    assert!(store.get(&profile).await.unwrap().is_none());
}

#[test]
fn origin_rules_s1() {
    // HTTPS required for non-loopback; debug loopback http allowed explicitly.
    assert!(AuthOrigin::new("https://server.test", false).is_ok());
    assert!(AuthOrigin::new("http://server.test", false).is_err());
    assert!(AuthOrigin::new("http://127.0.0.1:8080", true).is_ok());
    assert!(AuthOrigin::new("http://localhost", true).is_ok());
    assert!(AuthOrigin::new("http://server.test", true).is_err()); // not loopback
    assert!(AuthOrigin::new("https://user:pw@server.test", false).is_err());
    // §1: a path is STRIPPED from the origin (HTTPClient.swift:44), not rejected.
    let with_path = AuthOrigin::new("https://server.test/some/path", false).expect("path stripped");
    assert_eq!(with_path.to_string(), "https://server.test");
    assert!(AuthOrigin::new("ftp://server.test", false).is_err());
}
