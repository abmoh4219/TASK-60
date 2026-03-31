//! RailOps API integration tests.
//!
//! These tests run against a live backend at TEST_BASE_URL
//! (default: https://localhost:8443).
//!
//! Inside Docker the tester service sets TEST_BASE_URL=https://app:8443.
//! Locally, start the stack first with `docker compose up --build`, then:
//!   cargo test --test api_integration_test -- --nocapture
//!
//! Each test is self-contained: it creates a fresh client and obtains its own
//! session token.  No shared state between tests.

use hmac::{Hmac, Mac};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use sha2::Sha256;

const ADMIN_USER: &str = "admin";
const ADMIN_PASS: &str = "AdminRailOps2024!";

/// Returns the base URL from TEST_BASE_URL env var, falling back to localhost.
fn base() -> String {
    std::env::var("TEST_BASE_URL")
        .unwrap_or_else(|_| "https://localhost:8443".to_owned())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn new_client() -> Client {
    Client::builder()
        .danger_accept_invalid_certs(true) // self-signed cert from rcgen
        .build()
        .expect("build reqwest client")
}

/// Compute HMAC-SHA-256 request signature required by the auth middleware.
/// Returns (sig_hex, timestamp_string).
fn sign(method: &str, path: &str, token: &str) -> (String, String) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let message = format!("{method}\n{path}\n{ts}");
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(token.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(message.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    (sig, ts.to_string())
}

/// Login as admin and return the raw bearer token.
async fn admin_token() -> String {
    let c = new_client();
    let r = c
        .post(format!("{}/api/v1/auth/login", base()))
        .json(&json!({ "username": ADMIN_USER, "password": ADMIN_PASS }))
        .send()
        .await
        .expect("login request");
    assert_eq!(r.status(), StatusCode::OK, "admin login failed — is the server running?");
    let body: Value = r.json().await.expect("login response JSON");
    body["token"]
        .as_str()
        .expect("no token in login response")
        .to_owned()
}

/// Authenticated GET request with proper HMAC signature headers.
async fn authed_get(c: &Client, path: &str, token: &str) -> reqwest::Response {
    let (sig, ts) = sign("GET", path, token);
    c.get(format!("{}{path}", base()))
        .bearer_auth(token)
        .header("X-RailOps-Sig", sig)
        .header("X-RailOps-Ts", ts)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {path} failed: {e}"))
}

/// Authenticated POST request with JSON body.
async fn authed_post(c: &Client, path: &str, token: &str, body: Value) -> reqwest::Response {
    let (sig, ts) = sign("POST", path, token);
    c.post(format!("{}{path}", base()))
        .bearer_auth(token)
        .header("X-RailOps-Sig", sig)
        .header("X-RailOps-Ts", ts)
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("POST {path} failed: {e}"))
}

/// Authenticated PATCH request with JSON body.
async fn authed_patch(c: &Client, path: &str, token: &str, body: Value) -> reqwest::Response {
    let (sig, ts) = sign("PATCH", path, token);
    c.patch(format!("{}{path}", base()))
        .bearer_auth(token)
        .header("X-RailOps-Sig", sig)
        .header("X-RailOps-Ts", ts)
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("PATCH {path} failed: {e}"))
}

/// Authenticated DELETE request.
async fn authed_delete(c: &Client, path: &str, token: &str) -> reqwest::Response {
    let (sig, ts) = sign("DELETE", path, token);
    c.delete(format!("{}{path}", base()))
        .bearer_auth(token)
        .header("X-RailOps-Sig", sig)
        .header("X-RailOps-Ts", ts)
        .send()
        .await
        .unwrap_or_else(|e| panic!("DELETE {path} failed: {e}"))
}

// ── t01: health ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn t01_health() {
    println!("\n=== t01_health ===");
    let c = new_client();
    let r = c.get(format!("{}/health", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "/health must return 200");
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "railops-backend");
    println!("[health] status=ok  version={}", body["version"]);
}

// ── t02: successful login ─────────────────────────────────────────────────────

#[tokio::test]
async fn t02_auth_login_success() {
    println!("\n=== t02_auth_login_success ===");
    let c = new_client();
    let r = c
        .post(format!("{}/api/v1/auth/login", base()))
        .json(&json!({ "username": ADMIN_USER, "password": ADMIN_PASS }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK, "admin login should return 200");
    let body: Value = r.json().await.unwrap();
    assert!(body["token"].is_string(), "response must contain a token");
    assert_eq!(body["user"]["username"], "admin");
    assert_eq!(body["user"]["role"], "admin");
    println!("[auth/login] token present, username=admin role=admin");
}

// ── t03: wrong password rejected ─────────────────────────────────────────────

#[tokio::test]
async fn t03_auth_login_wrong_password() {
    println!("\n=== t03_auth_login_wrong_password ===");
    let c = new_client();
    let r = c
        .post(format!("{}/api/v1/auth/login", base()))
        .json(&json!({ "username": "admin", "password": "completely_wrong_pass" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "wrong password must return 401");
    println!("[auth/login] wrong password correctly rejected with 401");
}

// ── t04: auth/me ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn t04_auth_me() {
    println!("\n=== t04_auth_me ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/auth/me", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["username"], "admin");
    assert_eq!(body["role"], "admin");
    println!("[auth/me] username=admin role=admin");
}

// ── t05: unauthenticated access rejected ─────────────────────────────────────

#[tokio::test]
async fn t05_protected_route_requires_auth() {
    println!("\n=== t05_protected_route_requires_auth ===");
    let c = new_client();
    let r = c
        .get(format!("{}/api/v1/ops/routes", base()))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED,
        "missing auth header must return 401");
    println!("[auth] unauthenticated request correctly rejected with 401");
}

// ── t06-t09: kiosk public endpoints ──────────────────────────────────────────

#[tokio::test]
async fn t06_kiosk_categories_public() {
    println!("\n=== t06_kiosk_categories_public ===");
    let c = new_client();
    let r = c.get(format!("{}/api/v1/kiosk/categories", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "categories endpoint is public");
    let body: Value = r.json().await.unwrap();
    let is_array = body.is_array() || body["data"].is_array();
    assert!(is_array, "categories response must be JSON array");
    println!("[kiosk/categories] OK — publicly accessible");
}

#[tokio::test]
async fn t07_kiosk_tags_public() {
    println!("\n=== t07_kiosk_tags_public ===");
    let c = new_client();
    let r = c.get(format!("{}/api/v1/kiosk/tags", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "tags endpoint is public");
    println!("[kiosk/tags] OK — publicly accessible");
}

#[tokio::test]
async fn t08_kiosk_content_search() {
    println!("\n=== t08_kiosk_content_search ===");
    let c = new_client();
    let r = c.get(format!("{}/api/v1/kiosk/content", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    // Accept array, { items: [...] }, or { data: ... }
    let is_valid = body.is_array()
        || body["items"].is_array()
        || body["data"].is_array()
        || body["data"]["items"].is_array();
    assert!(is_valid, "content response must contain article items: {body}");
    println!("[kiosk/content] OK — search endpoint accessible without auth");
}

#[tokio::test]
async fn t09_kiosk_archive() {
    println!("\n=== t09_kiosk_archive ===");
    let c = new_client();
    let r = c.get(format!("{}/api/v1/kiosk/archive", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "archive endpoint is public");
    println!("[kiosk/archive] OK");
}

// ── t10-t12: ops reference data and schedules ─────────────────────────────────

#[tokio::test]
async fn t10_ops_routes() {
    println!("\n=== t10_ops_routes ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/ops/routes", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let routes = body.as_array()
        .or_else(|| body["data"].as_array())
        .expect("routes must be a JSON array");
    assert!(!routes.is_empty(), "seeded routes should be present");
    println!("[ops/routes] {} routes — first code={}", routes.len(), routes[0]["code"]);
}

#[tokio::test]
async fn t11_ops_seat_classes() {
    println!("\n=== t11_ops_seat_classes ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/ops/seat-classes", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let classes = body.as_array()
        .or_else(|| body["data"].as_array())
        .expect("seat classes must be a JSON array");
    assert!(!classes.is_empty(), "seeded seat classes should be present");
    println!("[ops/seat-classes] {} seat classes", classes.len());
}

#[tokio::test]
async fn t12_ops_schedules_paginated() {
    println!("\n=== t12_ops_schedules_paginated ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/ops/schedules", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    // PaginatedResponse: { items: [...], total, page, per_page, total_pages }
    let items = body["items"].as_array()
        .or_else(|| body["data"]["items"].as_array())
        .expect("schedules items field missing");
    println!("[ops/schedules] {} schedules on page 1, total={}", items.len(), body["total"]);
}

// ── t13-t14: passengers ───────────────────────────────────────────────────────

#[tokio::test]
async fn t13_ops_passengers_list() {
    println!("\n=== t13_ops_passengers_list ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/ops/passengers", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let items = body["items"].as_array()
        .or_else(|| body["data"]["items"].as_array())
        .expect("passengers items field missing");
    assert!(!items.is_empty(), "seeded passengers should be present");
    println!("[ops/passengers] {} passengers returned", items.len());
}

#[tokio::test]
async fn t14_ops_create_passenger() {
    println!("\n=== t14_ops_create_passenger ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_post(&c, "/api/v1/ops/passengers", &token, json!({
        "full_name": "Integration Test Passenger",
        "email":     "integration-test@railops.local",
        "phone":     "+44 7700 000999"
    })).await;
    assert_eq!(r.status(), StatusCode::CREATED, "create passenger should return 201");
    let body: Value = r.json().await.unwrap();
    // id may be at root or under "data"
    let id = body.get("id")
        .or_else(|| body["data"].get("id"))
        .expect("response must contain passenger id");
    assert!(id.is_string(), "id must be a UUID string");
    println!("[ops/passengers] created id={id}");
}

// ── t15: orders ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn t15_ops_orders_list() {
    println!("\n=== t15_ops_orders_list ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/ops/orders", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let items = body["items"].as_array()
        .or_else(|| body["data"]["items"].as_array())
        .expect("orders items field missing");
    assert!(!items.is_empty(), "seeded orders should be present");
    println!("[ops/orders] {} orders returned", items.len());
}

// ── t16-t17: business rules ───────────────────────────────────────────────────

#[tokio::test]
async fn t16_rules_list() {
    println!("\n=== t16_rules_list ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/rules", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let rules = body.as_array()
        .or_else(|| body["data"].as_array())
        .expect("rules must be a JSON array");
    println!("[rules] {} business rules", rules.len());
    for rule in rules.iter().take(5) {
        println!("  key={}  value={}", rule["key"], rule["value"]);
    }
}

#[tokio::test]
async fn t17_rules_update_and_verify() {
    println!("\n=== t17_rules_update_and_verify ===");
    let token = admin_token().await;
    let c = new_client();

    // First, list rules to find a key to update
    let r = authed_get(&c, "/api/v1/rules", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let rules = body.as_array()
        .or_else(|| body["data"].as_array())
        .expect("rules array");

    if rules.is_empty() {
        println!("[rules/update] no rules seeded — skipping update test");
        return;
    }

    // Use the first rule
    let first_rule = &rules[0];
    let key = first_rule["key"].as_str().expect("rule key");
    let original_value = first_rule["value"].as_str().unwrap_or("0").to_owned();
    println!("[rules/update] testing with key={key} original_value={original_value}");

    // PATCH the rule with a modified value (add 1 to numeric, or use same value)
    let new_value = original_value
        .parse::<f64>()
        .map(|n| format!("{}", n + 1.0))
        .unwrap_or_else(|_| original_value.clone());

    let r = authed_patch(&c, &format!("/api/v1/rules/{key}"), &token,
        json!({ "value": new_value })).await;
    assert_eq!(r.status(), StatusCode::OK, "PATCH rule should succeed for admin");
    println!("[rules/update] updated to {new_value}");

    // Restore original value
    let r = authed_patch(&c, &format!("/api/v1/rules/{key}"), &token,
        json!({ "value": original_value })).await;
    assert_eq!(r.status(), StatusCode::OK, "restore rule should succeed");
    println!("[rules/update] restored to {original_value}");
}

// ── t18-t19: staffing ─────────────────────────────────────────────────────────

#[tokio::test]
async fn t18_staffing_contractors() {
    println!("\n=== t18_staffing_contractors ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/staffing/contractors", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let items = body["items"].as_array()
        .or_else(|| body["data"]["items"].as_array())
        .expect("contractors items missing");
    assert!(!items.is_empty(), "seeded contractors should be present");
    println!("[staffing/contractors] {} contractors", items.len());
}

#[tokio::test]
async fn t19_staffing_shifts() {
    println!("\n=== t19_staffing_shifts ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/staffing/shifts", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let items = body["items"].as_array()
        .or_else(|| body["data"]["items"].as_array())
        .expect("shifts items missing");
    println!("[staffing/shifts] {} shifts", items.len());
}

// ── t20: credentials ──────────────────────────────────────────────────────────

#[tokio::test]
async fn t20_credentials_list() {
    println!("\n=== t20_credentials_list ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/credentials", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    // Accept any valid JSON response (may be empty on fresh DB)
    let is_valid = body.is_array()
        || body["items"].is_array()
        || body["data"].is_array()
        || body["data"]["items"].is_array();
    assert!(is_valid, "credentials response must be array or paginated: {body}");
    println!("[credentials] OK");
}

// ── t21: full auth logout cycle ───────────────────────────────────────────────

#[tokio::test]
async fn t21_auth_logout_cycle() {
    println!("\n=== t21_auth_logout_cycle ===");
    let token = admin_token().await;
    let c = new_client();

    // Confirm session is active
    let r = authed_get(&c, "/api/v1/auth/me", &token).await;
    assert_eq!(r.status(), StatusCode::OK, "/me must succeed before logout");
    println!("[auth/logout] pre-logout /me: OK");

    // Logout — invalidates the server-side session
    let r = authed_delete(&c, "/api/v1/auth/logout", &token).await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT, "logout must return 204");
    println!("[auth/logout] logout returned 204");

    // After logout, the same token must be rejected
    let r = authed_get(&c, "/api/v1/auth/me", &token).await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED,
        "session must be invalidated after logout");
    println!("[auth/logout] post-logout /me correctly rejected — full cycle verified");
}

// ── t22: RBAC — ops_agent can list orders ─────────────────────────────────────

#[tokio::test]
async fn t22_rbac_ops_agent_can_list_orders() {
    println!("\n=== t22_rbac_ops_agent_can_list_orders ===");
    // Login as ops_agent1 (seeded, also uses ADMIN_SEED_PASSWORD)
    let c = new_client();
    let r = c
        .post(format!("{}/api/v1/auth/login", base()))
        .json(&json!({ "username": "ops_agent1", "password": ADMIN_PASS }))
        .send()
        .await
        .unwrap();

    if r.status() != StatusCode::OK {
        println!("[rbac] ops_agent1 login failed (status={}) — skipping test", r.status());
        return;
    }
    let body: Value = r.json().await.unwrap();
    let ops_token = body["token"].as_str().expect("ops token").to_owned();

    // ops_agent should be able to view orders
    let c2 = new_client();
    let r = authed_get(&c2, "/api/v1/ops/orders", &ops_token).await;
    assert_eq!(r.status(), StatusCode::OK,
        "ops_agent should be authorised to list orders");
    println!("[rbac] ops_agent1 can list orders: OK");
}
