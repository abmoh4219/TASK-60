//! `AuthUser` — Actix `FromRequest` extractor that:
//!   1. Reads `Authorization: Bearer <token>`
//!   2. Verifies the HMAC-SHA-256 request signature (`X-RailOps-Sig` / `X-RailOps-Ts`)
//!   3. Looks up the session by `sha256(token)`, enforcing idle + absolute expiry
//!   4. Enforces per-session rate limiting (default 60 req/min)
//!   5. Bumps `last_active_at` to reset the idle window
//!
//! Signing protocol:
//!   message  = "METHOD\nPATH\nUNIX_TIMESTAMP_SECS"
//!   key      = raw Bearer token
//!   sig      = HMAC-SHA-256(key, message) — hex-encoded, in X-RailOps-Sig header
//!   Timestamp must be within ±120 s of server time.

use actix_web::{dev::Payload, web, FromRequest, HttpRequest};
use chrono::Utc;
use dashmap::DashMap;
use futures::future::LocalBoxFuture;
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

use shared::UserRole;

use crate::error::{AppError, AppResult};

use super::session;

// ── Rate limiter ──────────────────────────────────────────────────────────────

/// In-memory, lock-free rate limiter keyed by session `token_hash`.
/// Each entry is `(request_count, window_start_unix_secs)`.
pub struct RateLimiter {
    store:   DashMap<String, (u32, i64)>,
    max_rpm: u32,
}

impl RateLimiter {
    pub fn new(max_rpm: u32) -> Self {
        Self { store: DashMap::new(), max_rpm }
    }

    /// Increment the counter for `key`.  Returns `Err(RateLimited)` if exceeded.
    pub fn check_and_increment(&self, key: &str) -> AppResult<()> {
        let now = Utc::now().timestamp();
        let window = 60_i64; // 1-minute fixed window

        let mut entry = self.store.entry(key.to_owned()).or_insert((0, now));
        let (count, window_start) = entry.value_mut();

        if now - *window_start >= window {
            // New window
            *window_start = now;
            *count = 1;
        } else {
            *count += 1;
            if *count > self.max_rpm {
                let retry_after = (window - (now - *window_start)).max(1) as u64;
                return Err(AppError::RateLimited { retry_after });
            }
        }
        Ok(())
    }

    /// Evict entries for expired windows (call periodically to bound memory use).
    pub fn evict_expired(&self) {
        let now = Utc::now().timestamp();
        self.store.retain(|_, (_, window_start)| now - *window_start < 120);
    }
}

// ── AuthUser ──────────────────────────────────────────────────────────────────

/// The authenticated identity injected into every protected handler.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id:         Uuid,
    pub username:   String,
    pub role:       UserRole,
    pub full_name:  Option<String>,
    /// SHA-256 of the raw token — used for logout / rate-limit key.
    pub token_hash: String,
}

impl FromRequest for AuthUser {
    type Error = AppError;
    type Future = LocalBoxFuture<'static, AppResult<AuthUser>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        // Extract everything we need from the request before entering the async block.
        let pool    = req.app_data::<web::Data<PgPool>>()
            .cloned()
            .expect("PgPool not registered in app_data");
        let limiter = req.app_data::<web::Data<RateLimiter>>()
            .cloned()
            .expect("RateLimiter not registered in app_data");

        let method = req.method().as_str().to_uppercase();
        let path   = req.path().to_owned();

        let token = extract_bearer(req);
        let ts    = extract_header_i64(req, "X-RailOps-Ts");
        let sig   = extract_header_str(req, "X-RailOps-Sig");

        Box::pin(async move {
            // ── 1. Token present ─────────────────────────────────────────
            let raw_token = token.ok_or_else(|| {
                AppError::Unauthorized("Missing Authorization: Bearer token".into())
            })?;

            // ── 2. Signature + timestamp ─────────────────────────────────
            let ts = ts.ok_or_else(|| AppError::Unauthorized("Missing X-RailOps-Ts".into()))?;
            let sig = sig.ok_or_else(|| AppError::Unauthorized("Missing X-RailOps-Sig".into()))?;

            let now = Utc::now().timestamp();
            if (now - ts).abs() > 120 {
                return Err(AppError::Unauthorized("Request timestamp out of window".into()));
            }

            let message = format!("{method}\n{path}\n{ts}");
            if !crate::crypto::hmac_verify(&raw_token, &message, &sig) {
                warn!(path = %path, "Invalid request signature");
                return Err(AppError::Unauthorized("Invalid request signature".into()));
            }

            // ── 3. Session lookup ────────────────────────────────────────
            let token_hash = crate::crypto::sha256_hex(&raw_token);

            let record = session::find_active_session(&pool, &token_hash)
                .await?
                .ok_or_else(|| AppError::Unauthorized("Session not found or expired".into()))?;

            // ── 4. Rate limiting ─────────────────────────────────────────
            limiter.check_and_increment(&token_hash)?;

            // ── 5. Touch session (reset idle expiry) ─────────────────────
            session::touch_session(&pool, &token_hash).await?;

            Ok(AuthUser {
                id:         record.user_id,
                username:   record.username,
                role:       record.user_role(),
                full_name:  record.full_name,
                token_hash,
            })
        })
    }
}

// ── Header extraction helpers ─────────────────────────────────────────────────

fn extract_bearer(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_owned())
}

fn extract_header_i64(req: &HttpRequest, name: &str) -> Option<i64> {
    req.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
}

fn extract_header_str(req: &HttpRequest, name: &str) -> Option<String> {
    req.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned())
}
