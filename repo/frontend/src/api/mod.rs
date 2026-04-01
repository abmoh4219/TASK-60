//! Typed API client for the RailOps backend.
//!
//! Every request (except /auth/login) is signed:
//!   message   = "METHOD\nPATH\nUNIX_TIMESTAMP_SECS"
//!   key       = raw Bearer token
//!   signature = HMAC-SHA-256(key, message) — hex-encoded
//!
//! Headers added automatically:
//!   Authorization: Bearer <token>
//!   X-RailOps-Ts:  <unix_epoch_secs>
//!   X-RailOps-Sig: <hmac_hex>

pub mod credentials;
pub mod kiosk;
pub mod ops;
pub mod rules;
pub mod staffing;

use gloo_net::http::{Request, Response};
use hmac::{Hmac, Mac};
use serde::{de::DeserializeOwned, Serialize};
use sha2::Sha256;

use crate::auth::CurrentUser;

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status:  u16,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.message)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

// ── Request / response shapes mirroring the backend ──────────────────────────

#[derive(serde::Serialize)]
pub struct LoginPayload<'a> {
    pub username: &'a str,
    pub password: &'a str,
}

#[derive(serde::Deserialize)]
pub struct LoginResponse {
    pub token:      String,
    pub user:       CurrentUser,
    pub expires_at: String,
}

// ── Public API functions ──────────────────────────────────────────────────────

/// Authenticate and receive a session token.
pub async fn login(username: &str, password: &str) -> ApiResult<LoginResponse> {
    let body = LoginPayload { username, password };
    // Login is unsigned (no token yet).
    post_unsigned("/api/v1/auth/login", &body).await
}

/// Invalidate the current session.
pub async fn logout(token: &str) -> ApiResult<()> {
    signed_request("DELETE", "/api/v1/auth/logout", token, None::<&()>).await?;
    Ok(())
}

/// Fetch the authenticated user's info (used to verify a stored token on load).
pub async fn me(token: &str) -> ApiResult<CurrentUser> {
    let resp = signed_request("GET", "/api/v1/auth/me", token, None::<&()>).await?;
    parse_json(resp).await
}

// ── Internal helpers ──────────────────────────────────────────────────────────

async fn post_unsigned<B: Serialize, R: DeserializeOwned>(
    path: &str,
    body: &B,
) -> ApiResult<R> {
    let body_str = serde_json::to_string(body).map_err(|e| ApiError {
        status:  0,
        message: e.to_string(),
    })?;

    let resp = Request::post(path)
        .header("Content-Type", "application/json")
        .body(body_str)
        .map_err(|e| ApiError { status: 0, message: e.to_string() })?
        .send()
        .await
        .map_err(|e| ApiError { status: 0, message: e.to_string() })?;

    parse_json(resp).await
}

pub(crate) async fn signed_request<B: Serialize>(
    method:  &str,
    path:    &str,
    token:   &str,
    body:    Option<&B>,
) -> ApiResult<Response> {
    let ts      = unix_now();
    let message = format!("{method}\n{path}\n{ts}");
    let sig     = hmac_sign(token, &message);

    let mut builder = match method {
        "GET"    => Request::get(path),
        "POST"   => Request::post(path),
        "PUT"    => Request::put(path),
        "PATCH"  => Request::patch(path),
        "DELETE" => Request::delete(path),
        m        => return Err(ApiError { status: 0, message: format!("Unknown method {m}") }),
    };

    builder = builder
        .header("Authorization", &format!("Bearer {token}"))
        .header("X-RailOps-Ts",  &ts.to_string())
        .header("X-RailOps-Sig", &sig);

    let resp = if let Some(b) = body {
        let body_str = serde_json::to_string(b).map_err(|e| ApiError {
            status: 0, message: e.to_string(),
        })?;
        builder
            .header("Content-Type", "application/json")
            .body(body_str)
            .map_err(|e| ApiError { status: 0, message: e.to_string() })?
            .send()
            .await
    } else {
        builder
            .send()
            .await
    }
    .map_err(|e| ApiError { status: 0, message: e.to_string() })?;

    if resp.ok() {
        Ok(resp)
    } else {
        let status = resp.status();
        let text   = resp.text().await.unwrap_or_default();
        // Try to extract the backend's structured error message.
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_owned()))
            .unwrap_or(text);
        Err(ApiError { status, message })
    }
}

pub(crate) async fn parse_json<T: DeserializeOwned>(resp: Response) -> ApiResult<T> {
    if resp.ok() {
        resp.json::<T>()
            .await
            .map_err(|e| ApiError { status: 0, message: e.to_string() })
    } else {
        let status = resp.status();
        let text   = resp.text().await.unwrap_or_default();
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_owned()))
            .unwrap_or(text);
        Err(ApiError { status, message })
    }
}

pub(crate) fn hmac_sign(token: &str, message: &str) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(token.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub(crate) fn unix_now() -> i64 {
    // chrono wasmbind feature provides JS Date.now() based time in WASM.
    chrono::Utc::now().timestamp()
}
