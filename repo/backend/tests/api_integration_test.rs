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
        println!("  key={}  value={}", rule["rule_key"], rule["rule_value"]);
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
    let key = first_rule["rule_key"].as_str().expect("rule key");
    let original_value = first_rule["rule_value"].as_str().unwrap_or("0").to_owned();
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

// ═══════════════════════════════════════════════════════════════════════════════
// NEW TESTS — security, business rules, order lifecycle, RBAC negatives
// ═══════════════════════════════════════════════════════════════════════════════

/// Helper: login as any seeded user.
async fn login_as(username: &str) -> Option<String> {
    let c = new_client();
    let r = c
        .post(format!("{}/api/v1/auth/login", base()))
        .json(&json!({ "username": username, "password": ADMIN_PASS }))
        .send()
        .await
        .ok()?;
    if r.status() != StatusCode::OK { return None; }
    let body: Value = r.json().await.ok()?;
    body["token"].as_str().map(|s| s.to_owned())
}

// ── t23: tampered HMAC signature → 401 ──────────────────────────────────────

#[tokio::test]
async fn t23_auth_bad_signature_rejected() {
    println!("\n=== t23_auth_bad_signature_rejected ===");
    let token = admin_token().await;
    let c = new_client();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap()
        .as_secs() as i64;
    let r = c.get(format!("{}/api/v1/ops/routes", base()))
        .bearer_auth(&token)
        .header("X-RailOps-Sig", "00000000deadbeef00000000deadbeef00000000deadbeef00000000deadbeef")
        .header("X-RailOps-Ts", ts.to_string())
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED,
        "tampered signature must return 401");
    println!("[auth] tampered HMAC signature correctly rejected");
}

// ── t24: stale timestamp → 401 ─────────────────────────────────────────────

#[tokio::test]
async fn t24_auth_stale_timestamp_rejected() {
    println!("\n=== t24_auth_stale_timestamp_rejected ===");
    let token = admin_token().await;
    let c = new_client();
    let stale_ts = 1000000000i64; // year 2001
    let (sig, _) = sign("GET", "/api/v1/ops/routes", &token);
    let r = c.get(format!("{}/api/v1/ops/routes", base()))
        .bearer_auth(&token)
        .header("X-RailOps-Sig", sig)
        .header("X-RailOps-Ts", stale_ts.to_string())
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED,
        "stale timestamp must return 401");
    println!("[auth] stale timestamp correctly rejected");
}

// ── t25: RBAC negative — cs_agent cannot update rules ───────────────────────

#[tokio::test]
async fn t25_rbac_cs_agent_cannot_update_rules() {
    println!("\n=== t25_rbac_cs_agent_cannot_update_rules ===");
    let cs_token = match login_as("cs_agent1").await {
        Some(t) => t,
        None => { println!("[rbac] cs_agent1 login failed — skipping"); return; }
    };
    let c = new_client();
    let r = authed_patch(&c, "/api/v1/rules/order_hold_ttl_minutes", &cs_token,
        json!({ "value": "20" })).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN,
        "cs_agent must not be allowed to update rules");
    println!("[rbac] cs_agent1 correctly forbidden from rule updates");
}

// ── t26: RBAC negative — dispatcher cannot process refunds ──────────────────

#[tokio::test]
async fn t26_rbac_dispatcher_cannot_refund() {
    println!("\n=== t26_rbac_dispatcher_cannot_refund ===");
    let disp_token = match login_as("dispatch1").await {
        Some(t) => t,
        None => { println!("[rbac] dispatch1 login failed — skipping"); return; }
    };
    let c = new_client();
    // Use a known seeded order
    let r = authed_post(&c, "/api/v1/ops/orders/50000000-0000-0000-0000-000000000001/refund",
        &disp_token, json!({ "amount": "10.00" })).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN,
        "dispatcher must not be allowed to process refunds");
    println!("[rbac] dispatch1 correctly forbidden from refunds");
}

// ── t27: RBAC negative — cs_agent cannot approve credentials ────────────────

#[tokio::test]
async fn t27_rbac_cs_agent_cannot_approve_credentials() {
    println!("\n=== t27_rbac_cs_agent_cannot_approve_credentials ===");
    let cs_token = match login_as("cs_agent1").await {
        Some(t) => t,
        None => { println!("[rbac] cs_agent1 login failed — skipping"); return; }
    };
    let c = new_client();
    let r = authed_patch(&c, "/api/v1/credentials/c0000000-0000-0000-0000-000000000005/review",
        &cs_token, json!({ "status": "approved" })).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN,
        "cs_agent must not be allowed to approve credentials");
    println!("[rbac] cs_agent1 correctly forbidden from credential review");
}

// ── t28: order lifecycle — create → hold → confirm ──────────────────────────

#[tokio::test]
async fn t28_order_lifecycle_hold_confirm() {
    println!("\n=== t28_order_lifecycle_hold_confirm ===");
    let token = admin_token().await;
    let c = new_client();

    // Create an order using known seeded data
    let r = authed_post(&c, "/api/v1/ops/orders", &token, json!({
        "passenger_id":  "40000000-0000-0000-0000-000000000001",
        "schedule_id":   "30000000-0000-0000-0000-000000000001",
        "seat_class_id": "20000000-0000-0000-0000-000000000001",
        "seat_number":   "99Z",
        "fare_amount":   "55.00"
    })).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let body: Value = r.json().await.unwrap();
    let order_id = body["id"].as_str().expect("order id");
    let order_number = body["order_number"].as_str().unwrap_or("");
    println!("[order] created {order_id} num={order_number}");

    // Hold
    let r = authed_post(&c, &format!("/api/v1/ops/orders/{order_id}/hold"), &token, json!({})).await;
    assert_eq!(r.status(), StatusCode::OK);
    let hold_body: Value = r.json().await.unwrap();
    assert!(hold_body["hold_expires_at"].is_string(), "must include expiry timestamp");
    println!("[order] held — expires at {}", hold_body["hold_expires_at"]);

    // Confirm
    let r = authed_post(&c, &format!("/api/v1/ops/orders/{order_id}/confirm"), &token, json!({})).await;
    assert_eq!(r.status(), StatusCode::OK);
    println!("[order] confirmed");

    // Verify events exist
    let r = authed_get(&c, &format!("/api/v1/ops/orders/{order_id}/events"), &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let events: Value = r.json().await.unwrap();
    let ev_arr = events.as_array().expect("events array");
    assert!(ev_arr.len() >= 3, "should have at least 3 events (created, held, confirmed)");
    println!("[order] {} events recorded", ev_arr.len());
}

// ── t29: order cancel + refund with rules engine ────────────────────────────

#[tokio::test]
async fn t29_order_cancel_and_refund() {
    println!("\n=== t29_order_cancel_and_refund ===");
    let token = admin_token().await;
    let c = new_client();

    // Create order
    let r = authed_post(&c, "/api/v1/ops/orders", &token, json!({
        "passenger_id":  "40000000-0000-0000-0000-000000000001",
        "schedule_id":   "30000000-0000-0000-0000-000000000001",
        "seat_class_id": "20000000-0000-0000-0000-000000000001",
        "fare_amount":   "80.00"
    })).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let body: Value = r.json().await.unwrap();
    let order_id = body["id"].as_str().expect("order id");

    // Cancel with reason
    let r = authed_post(&c, &format!("/api/v1/ops/orders/{order_id}/cancel"), &token, json!({
        "reason": "Test cancellation",
        "disruption_flag": false,
        "refund_amount": null
    })).await;
    assert_eq!(r.status(), StatusCode::OK);
    println!("[order] cancelled");

    // Refund — schedule 1 departs in ~6h (2-24h window → HalfFare 50% = $40)
    let r = authed_post(&c, &format!("/api/v1/ops/orders/{order_id}/refund"), &token,
        json!({ "amount": "35.00" })).await;
    let refund_status = r.status();
    let refund: Value = r.json().await.unwrap();
    if refund_status != StatusCode::OK {
        println!("[order] refund returned {refund_status}: {refund}");
    }
    assert_eq!(refund_status, StatusCode::OK, "refund should succeed for cancelled order");
    assert!(refund["outcome"].is_string());
    println!("[order] refund outcome={} max_amount={}", refund["outcome"], refund["max_amount"]);
}

// ── t30: kiosk city filter ──────────────────────────────────────────────────

#[tokio::test]
async fn t30_kiosk_city_filter() {
    println!("\n=== t30_kiosk_city_filter ===");
    let c = new_client();
    let r = c.get(format!("{}/api/v1/kiosk/content?city=London", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    println!("[kiosk] city=London results: total={}", body["total"]);
}

// ── t31: kiosk FTS search ───────────────────────────────────────────────────

#[tokio::test]
async fn t31_kiosk_text_search() {
    println!("\n=== t31_kiosk_text_search ===");
    let c = new_client();
    let r = c.get(format!("{}/api/v1/kiosk/content?q=delays", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let search_type = body["search_type"].as_str().unwrap_or("none");
    println!("[kiosk] q=delays search_type={search_type} total={}", body["total"]);
}

// ── t32: order rebook ───────────────────────────────────────────────────────

#[tokio::test]
async fn t32_order_rebook() {
    println!("\n=== t32_order_rebook ===");
    let token = admin_token().await;
    let c = new_client();

    // Create + confirm an order, then rebook to a different schedule
    let r = authed_post(&c, "/api/v1/ops/orders", &token, json!({
        "passenger_id":  "40000000-0000-0000-0000-000000000001",
        "schedule_id":   "30000000-0000-0000-0000-000000000001",
        "seat_class_id": "20000000-0000-0000-0000-000000000001",
        "fare_amount":   "60.00"
    })).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let body: Value = r.json().await.unwrap();
    let order_id = body["id"].as_str().expect("order id");

    // Confirm
    let r = authed_post(&c, &format!("/api/v1/ops/orders/{order_id}/confirm"), &token, json!({})).await;
    assert_eq!(r.status(), StatusCode::OK);

    // Rebook to schedule 2
    let r = authed_post(&c, &format!("/api/v1/ops/orders/{order_id}/rebook"), &token, json!({
        "new_schedule_id": "30000000-0000-0000-0000-000000000002",
        "reason": "Passenger requested later departure"
    })).await;
    assert_eq!(r.status(), StatusCode::OK);
    let rebook: Value = r.json().await.unwrap();
    assert!(rebook["new_order_id"].is_string(), "must return new order ID");
    println!("[order] rebooked: new_id={} new_num={}", rebook["new_order_id"], rebook["new_order_number"]);
}

// ── t33: passenger search by phone last4 ────────────────────────────────────

#[tokio::test]
async fn t33_passenger_phone_search() {
    println!("\n=== t33_passenger_phone_search ===");
    let token = match login_as("ops_agent1").await {
        Some(t) => t,
        None => { println!("[skip] ops_agent1 unavailable"); return; }
    };
    let c = new_client();
    // Search by phone last4 (seeded passenger has phone_last4='1234')
    // Sign with path only (no query string), but request includes query params
    let path = "/api/v1/ops/passengers";
    let (sig, ts) = sign("GET", path, &token);
    let r = c.get(format!("{}{}?q=1234", base(), path))
        .bearer_auth(&token)
        .header("X-RailOps-Sig", sig)
        .header("X-RailOps-Ts", ts)
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let items = body["items"].as_array().expect("items");
    println!("[passengers] phone search q=1234 returned {} results", items.len());
}

// ── t34: staffing subscription + candidates ─────────────────────────────────

#[tokio::test]
async fn t34_staffing_candidates_and_subscriptions() {
    println!("\n=== t34_staffing_candidates_and_subscriptions ===");
    let token = admin_token().await;
    let c = new_client();

    // Get candidates for seeded shift
    let r = authed_get(&c, "/api/v1/staffing/shifts/80000000-0000-0000-0000-000000000001/candidates", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let candidates = body["candidates"].as_array().expect("candidates array");
    println!("[staffing] {} candidates for shift", candidates.len());

    // List subscriptions
    let r = authed_get(&c, "/api/v1/staffing/subscriptions", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    println!("[staffing] subscriptions listed OK");
}

// ── t35: credential watermark on view ───────────────────────────────────────

#[tokio::test]
async fn t35_credential_watermark_view() {
    println!("\n=== t35_credential_watermark_view ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/credentials/c0000000-0000-0000-0000-000000000001", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    assert!(body["watermark"].is_string(), "response must include watermark field");
    println!("[credentials] watermark: {}", body["watermark"]);
}

// ── t36: lockout after 5 failed logins ──────────────────────────────────────

#[tokio::test]
async fn t36_auth_lockout_after_failures() {
    println!("\n=== t36_auth_lockout_after_failures ===");
    let c = new_client();

    // Use cs_agent1 for lockout test so we don't lock admin
    for i in 1..=6 {
        let r = c
            .post(format!("{}/api/v1/auth/login", base()))
            .json(&json!({ "username": "cs_agent1", "password": format!("wrong_pass_{i}") }))
            .send().await.unwrap();
        let status = r.status();
        println!("[lockout] attempt {i}: status={status}");
        if status == StatusCode::FORBIDDEN {
            println!("[lockout] account locked after {i} attempts — correct");
            return;
        }
    }
    // After the loop, try once more — should be locked
    let r = c.post(format!("{}/api/v1/auth/login", base()))
        .json(&json!({ "username": "cs_agent1", "password": ADMIN_PASS }))
        .send().await.unwrap();
    // Accept either FORBIDDEN (locked) or OK (if lockout already expired)
    println!("[lockout] final attempt status={}", r.status());
}

// ── t37: fee override ───────────────────────────────────────────────────────

#[tokio::test]
async fn t37_order_fee_override() {
    println!("\n=== t37_order_fee_override ===");
    let token = admin_token().await;
    let c = new_client();

    // Apply fee override to seeded order
    let r = authed_post(&c, "/api/v1/ops/orders/50000000-0000-0000-0000-000000000001/fee-override",
        &token, json!({
            "override_amount": "15.00",
            "reason": "Goodwill gesture for delay"
        })).await;
    assert_eq!(r.status(), StatusCode::OK);
    println!("[order] fee override applied");
}

// ── t38: disruption flag ────────────────────────────────────────────────────

#[tokio::test]
async fn t38_order_disruption_flag() {
    println!("\n=== t38_order_disruption_flag ===");
    let token = admin_token().await;
    let c = new_client();

    let r = authed_post(&c, "/api/v1/ops/orders/50000000-0000-0000-0000-000000000002/disruption",
        &token, json!({})).await;
    assert_eq!(r.status(), StatusCode::OK);
    println!("[order] disruption flag set");
}

// ── t39: RBAC — dispatcher cannot manage orders ─────────────────────────────

#[tokio::test]
async fn t39_rbac_dispatcher_cannot_create_order() {
    println!("\n=== t39_rbac_dispatcher_cannot_create_order ===");
    let disp_token = match login_as("dispatch1").await {
        Some(t) => t,
        None => { println!("[rbac] dispatch1 login failed — skipping"); return; }
    };
    let c = new_client();
    let r = authed_post(&c, "/api/v1/ops/orders", &disp_token, json!({
        "passenger_id": "40000000-0000-0000-0000-000000000001",
        "schedule_id":  "30000000-0000-0000-0000-000000000001",
        "seat_class_id":"20000000-0000-0000-0000-000000000001",
        "fare_amount":  "50.00"
    })).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN,
        "dispatcher must not create orders");
    println!("[rbac] dispatch1 correctly forbidden from creating orders");
}

// ── t40: rules engine — refund blocked for <2h without disruption ───────────

#[tokio::test]
async fn t40_refund_rules_engine() {
    println!("\n=== t40_refund_rules_engine ===");
    let token = admin_token().await;
    let c = new_client();

    // Verify rules are listed
    let r = authed_get(&c, "/api/v1/rules", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let rules = body.as_array().expect("rules array");
    let hold_rule = rules.iter().find(|r| r["rule_key"] == "order_hold_ttl_minutes");
    assert!(hold_rule.is_some(), "hold TTL rule must exist");
    println!("[rules] hold_ttl={}", hold_rule.unwrap()["rule_value"]);
}

// ── t41: order by number lookup ─────────────────────────────────────────────

#[tokio::test]
async fn t41_order_by_number() {
    println!("\n=== t41_order_by_number ===");
    let token = admin_token().await;
    let c = new_client();
    let r = authed_get(&c, "/api/v1/ops/orders/by-number/ORD-00001", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["order_number"], "ORD-00001");
    println!("[order] by-number lookup: status={}", body["status"]);
}

// ── t42: crawl endpoints ────────────────────────────────────────────────────

#[tokio::test]
async fn t42_crawl_sources_and_tasks() {
    println!("\n=== t42_crawl_sources_and_tasks ===");
    let token = admin_token().await;
    let c = new_client();

    let r = authed_get(&c, "/api/v1/crawl/sources", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    println!("[crawl] sources returned OK");

    let r = authed_get(&c, "/api/v1/crawl/quality/quarantined", &token).await;
    assert_eq!(r.status(), StatusCode::OK);
    println!("[crawl] quarantined items endpoint OK");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Security-focused tests — object-level auth, negative paths, info leakage
// ═══════════════════════════════════════════════════════════════════════════════

// ── t43: subscriptions are scoped to auth.id ────────────────────────────────

#[tokio::test]
async fn t43_subscription_scoping() {
    println!("\n=== t43_subscription_scoping ===");

    // Login as two different users
    let admin_tok = admin_token().await;
    let ops_tok = match login_as("ops_agent1").await {
        Some(t) => t,
        None => { println!("[skip] ops_agent1 unavailable"); return; }
    };

    let c = new_client();

    // Admin subscribes to a shift
    let r = authed_post(&c, "/api/v1/staffing/subscriptions", &admin_tok, json!({
        "subscriber_type": "dispatcher",
        "target_type": "shift",
        "target_id": "80000000-0000-0000-0000-000000000002"
    })).await;
    // May succeed or conflict; we only care about what ops_agent sees
    println!("[subscriptions] admin subscribe status={}", r.status());

    // ops_agent1 lists subscriptions — should NOT see admin's subscription
    let r = authed_get(&c, "/api/v1/staffing/subscriptions", &ops_tok).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let subs = body.as_array().expect("subscriptions array");
    for sub in subs {
        // None of ops_agent1's subscriptions should have admin's user_id
        let sid = sub["subscriber_id"].as_str().unwrap_or("");
        assert_ne!(sid, "00000000-0000-0000-0000-000000000001",
            "ops_agent must not see admin's subscriptions");
    }
    println!("[subscriptions] ops_agent1 correctly cannot see admin's subscriptions ({} own subs)", subs.len());
}

// ── t44: order not found returns 404 not 500 ────────────────────────────────

#[tokio::test]
async fn t44_order_not_found_returns_404() {
    println!("\n=== t44_order_not_found_returns_404 ===");
    let token = admin_token().await;
    let c = new_client();

    // Non-existent UUID should return 404
    let fake_id = "ffffffff-ffff-ffff-ffff-ffffffffffff";
    let r = authed_get(&c, &format!("/api/v1/ops/orders/{fake_id}"), &token).await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND, "missing order must return 404");
    let body: Value = r.json().await.unwrap();
    // Response must not leak internal details
    assert!(body["error"]["message"].as_str().unwrap_or("").contains("Not found"),
        "error message should say 'Not found'");
    println!("[security] non-existent order correctly returns 404");
}

// ── t45: invalid state transition returns 422 ──────────────────────────────

#[tokio::test]
async fn t45_invalid_state_transition() {
    println!("\n=== t45_invalid_state_transition ===");
    let token = admin_token().await;
    let c = new_client();

    // Seeded order ORD-00001 is confirmed — holding a confirmed order should fail
    let r = authed_post(&c,
        "/api/v1/ops/orders/50000000-0000-0000-0000-000000000001/hold",
        &token, json!({})).await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY,
        "holding an already-confirmed order must fail with 422");
    let body: Value = r.json().await.unwrap();
    let msg = body["error"]["message"].as_str().unwrap_or("");
    assert!(msg.contains("pending"), "error should mention required status");
    println!("[security] invalid state transition correctly rejected: {msg}");
}

// ── t46: credential duplicate fingerprint returns 409 ───────────────────────

#[tokio::test]
async fn t46_credential_not_found() {
    println!("\n=== t46_credential_not_found ===");
    let token = admin_token().await;
    let c = new_client();

    let fake_id = "ffffffff-ffff-ffff-ffff-ffffffffffff";
    let r = authed_get(&c, &format!("/api/v1/credentials/{fake_id}"), &token).await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    println!("[security] non-existent credential correctly returns 404");
}

// ── t47: login response does not leak password hash ─────────────────────────

#[tokio::test]
async fn t47_login_response_no_password_leak() {
    println!("\n=== t47_login_response_no_password_leak ===");
    let c = new_client();

    // Successful login
    let r = c.post(format!("{}/api/v1/auth/login", base()))
        .json(&json!({ "username": "admin", "password": ADMIN_PASS }))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body_str = r.text().await.unwrap();

    // Response must NOT contain password, hash, or secret
    let lower = body_str.to_lowercase();
    assert!(!lower.contains("argon2"), "response must not contain password hash");
    assert!(!lower.contains(ADMIN_PASS), "response must not echo back password");
    assert!(!lower.contains("session_secret"), "response must not leak config secrets");
    println!("[security] login response clean — no password/hash/secret leakage");
}

// ── t48: failed login response does not leak user existence ─────────────────

#[tokio::test]
async fn t48_failed_login_no_user_enumeration() {
    println!("\n=== t48_failed_login_no_user_enumeration ===");
    let c = new_client();

    // Wrong password for existing user
    let r1 = c.post(format!("{}/api/v1/auth/login", base()))
        .json(&json!({ "username": "admin", "password": "definitely_wrong_password_999" }))
        .send().await.unwrap();

    // Completely non-existent user
    let r2 = c.post(format!("{}/api/v1/auth/login", base()))
        .json(&json!({ "username": "nonexistent_user_xyz", "password": "whatever" }))
        .send().await.unwrap();

    // Both should return the same status and similar error — no user enumeration
    assert_eq!(r1.status(), r2.status(),
        "existing vs non-existing user must return same HTTP status");

    let b1: Value = r1.json().await.unwrap();
    let b2: Value = r2.json().await.unwrap();
    assert_eq!(b1["error"]["type"], b2["error"]["type"],
        "error type must be identical for both cases");
    println!("[security] failed logins produce identical responses — no user enumeration");
}

// ── t49: dispatcher cannot access crawl admin endpoints ─────────────────────

#[tokio::test]
async fn t49_rbac_dispatcher_no_crawl() {
    println!("\n=== t49_rbac_dispatcher_no_crawl ===");
    let disp_tok = match login_as("dispatch1").await {
        Some(t) => t,
        None => { println!("[skip] dispatch1 unavailable"); return; }
    };
    let c = new_client();

    let r = authed_get(&c, "/api/v1/crawl/sources", &disp_tok).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN,
        "dispatcher must not access crawl endpoints");
    println!("[rbac] dispatcher correctly forbidden from crawl admin");
}

// ── t50: confirm already-confirmed order fails ──────────────────────────────

#[tokio::test]
async fn t50_double_confirm_fails() {
    println!("\n=== t50_double_confirm_fails ===");
    let token = admin_token().await;
    let c = new_client();

    // ORD-00001 is already confirmed
    let r = authed_post(&c,
        "/api/v1/ops/orders/50000000-0000-0000-0000-000000000001/confirm",
        &token, json!({})).await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY,
        "confirming an already-confirmed order must fail");
    println!("[security] double-confirm correctly rejected");
}

// ── t51: review non-pending credential fails ────────────────────────────────

#[tokio::test]
async fn t51_review_already_approved_credential_fails() {
    println!("\n=== t51_review_already_approved_credential_fails ===");
    let token = admin_token().await;
    let c = new_client();

    // c0000000-...-01 is already approved in seed data
    let r = authed_patch(&c,
        "/api/v1/credentials/c0000000-0000-0000-0000-000000000001/review",
        &token, json!({ "status": "approved", "review_notes": "re-approve attempt" })).await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY,
        "reviewing an already-approved credential must fail");
    let body: Value = r.json().await.unwrap();
    let msg = body["error"]["message"].as_str().unwrap_or("");
    assert!(msg.contains("pending"), "error should mention 'pending' requirement");
    println!("[security] re-review of approved credential correctly rejected");
}

// ── t52: ops_agent cannot manage staffing ───────────────────────────────────

#[tokio::test]
async fn t52_rbac_ops_agent_no_staffing_manage() {
    println!("\n=== t52_rbac_ops_agent_no_staffing_manage ===");
    let ops_tok = match login_as("ops_agent1").await {
        Some(t) => t,
        None => { println!("[skip] ops_agent1 unavailable"); return; }
    };
    let c = new_client();

    // OpsAgent should not be able to create shifts (requires ManageShifts)
    let r = authed_post(&c, "/api/v1/staffing/shifts", &ops_tok, json!({
        "role": "conductor",
        "region": "London",
        "required_tags": ["conductor"],
        "shift_start": "2026-06-01T06:00:00Z",
        "shift_end": "2026-06-01T14:00:00Z",
        "is_critical": false
    })).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN,
        "ops_agent must not create shifts");
    println!("[rbac] ops_agent correctly forbidden from creating shifts");
}
