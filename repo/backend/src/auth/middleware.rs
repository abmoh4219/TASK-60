//! `AuthUser` — Actix `FromRequest` extractor that:
//!   1. Reads `Authorization: Bearer <token>`
//!   2. Verifies the HMAC-SHA-256 request signature (`X-RailOps-Sig` / `X-RailOps-Ts`)
//!   3. Looks up the session by `sha256(token)`, enforcing idle + absolute expiry
//!   4. Enforces IP binding: request IP must match the IP stored at login time
//!   5. Enforces per-session rate limiting (default 60 req/min)
//!   6. Bumps `last_active_at` to reset the idle window
//!
//! IP strategy: uses the direct TCP peer address (`req.peer_addr()`) — not any
//! forwarded/proxy header — so it cannot be spoofed by a client.  Sessions that
//! pre-date the IP binding feature (stored ip_address IS NULL) are allowed through
//! to preserve backward compatibility with existing sessions.
//!
//! Signing protocol:
//!   message  = "METHOD\nPATH_WITH_QUERY\nUNIX_TIMESTAMP_SECS"
//!   key      = raw Bearer token
//!   sig      = HMAC-SHA-256(key, message) — hex-encoded, in X-RailOps-Sig header
//!   Timestamp must be within ±120 s of server time.
//!
//!   PATH_WITH_QUERY = path + "?" + query_string  (if query_string is non-empty)
//!                   = path                        (if query_string is empty)
//!
//! This ensures query parameters are covered by the signature, preventing
//! replay attacks that tamper with query string values.

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
///
/// `max_rpm` is stored as an `AtomicU32` so the eviction background task
/// can reload the value from business rules without a restart.
pub struct RateLimiter {
    store:   DashMap<String, (u32, i64)>,
    max_rpm: std::sync::atomic::AtomicU32,
}

impl RateLimiter {
    pub fn new(max_rpm: u32) -> Self {
        Self {
            store: DashMap::new(),
            max_rpm: std::sync::atomic::AtomicU32::new(max_rpm),
        }
    }

    /// Update the max RPM at runtime (called from eviction task after DB reload).
    pub fn set_max_rpm(&self, rpm: u32) {
        self.max_rpm.store(rpm, std::sync::atomic::Ordering::Relaxed);
    }

    /// Increment the counter for `key`.  Returns `Err(RateLimited)` if exceeded.
    pub fn check_and_increment(&self, key: &str) -> AppResult<()> {
        let now = Utc::now().timestamp();
        let window = 60_i64; // 1-minute fixed window
        let max = self.max_rpm.load(std::sync::atomic::Ordering::Relaxed);

        let mut entry = self.store.entry(key.to_owned()).or_insert((0, now));
        let (count, window_start) = entry.value_mut();

        if now - *window_start >= window {
            // New window
            *window_start = now;
            *count = 1;
        } else {
            *count += 1;
            if *count > max {
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

        let method     = req.method().as_str().to_uppercase();
        let path       = req.path().to_owned();
        let qs         = req.query_string().to_owned();
        // Extract IP from direct TCP peer; strip port to get bare IP string.
        let client_ip: Option<String> = req.peer_addr()
            .map(|addr| addr.ip().to_string());

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

            // Include query string in the signed message to prevent query-param tampering.
            let path_with_query = if qs.is_empty() {
                path.clone()
            } else {
                format!("{path}?{qs}")
            };
            let message = format!("{method}\n{path_with_query}\n{ts}");
            if !crate::crypto::hmac_verify(&raw_token, &message, &sig) {
                warn!(path = %path_with_query, "Invalid request signature");
                return Err(AppError::Unauthorized("Invalid request signature".into()));
            }

            // ── 3. Session lookup ────────────────────────────────────────
            let token_hash = crate::crypto::sha256_hex(&raw_token);

            // Load session idle timeout from business rules (runtime-configurable).
            let idle_minutes: i64 = crate::domain::rules::repo::BusinessRuleRepo::new(&pool)
                .get_value("session_idle_minutes", &shared::rules::SESSION_IDLE_MINUTES.to_string())
                .await
                .parse()
                .unwrap_or(shared::rules::SESSION_IDLE_MINUTES as i64);

            let record = session::find_active_session(&pool, &token_hash, idle_minutes)
                .await?
                .ok_or_else(|| AppError::Unauthorized("Session not found or expired".into()))?;

            // ── 4. Session IP binding ────────────────────────────────────
            // If the session has a stored IP (set at login), the request IP
            // must match.  NULL stored IP means the session pre-dates this
            // feature — skip enforcement.
            //
            // Fail-closed: if the session has a bound IP but we cannot
            // resolve the request peer address, reject the request rather
            // than silently bypassing the security control.
            if let Some(stored_ip) = &record.ip_address {
                match &client_ip {
                    None => {
                        warn!(
                            stored_ip = %stored_ip,
                            "Session has bound IP but request peer address is unavailable \
                             — rejecting (fail-closed)"
                        );
                        return Err(AppError::Unauthorized(
                            "Cannot verify session IP binding".into()
                        ));
                    }
                    Some(req_ip) if stored_ip != req_ip => {
                        warn!(
                            stored_ip = %stored_ip,
                            client_ip = %req_ip,
                            "Session IP mismatch — rejecting request"
                        );
                        return Err(AppError::Unauthorized(
                            "Session IP mismatch".into()
                        ));
                    }
                    _ => {} // IPs match — continue
                }
            }

            // ── 5. Rate limiting ─────────────────────────────────────────
            limiter.check_and_increment(&token_hash)?;

            // ── 5. Touch session (reset idle expiry) ─────────────────────
            session::touch_session(&pool, &token_hash).await?;

            // Compute role before moving username out of record (E0382 fix:
            // user_role() borrows self, which must happen before partial move).
            let role = record.user_role();
            Ok(AuthUser {
                id:         record.user_id,
                username:   record.username,
                role,
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
