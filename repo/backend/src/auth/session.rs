//! Session CRUD — all token secrets stay in the application layer;
//! only the SHA-256 hash of each token is persisted in PostgreSQL.

use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppResult;

use super::models::SessionRecord;

// Sessions are valid for 24 h absolute; idle expiry (30 min) enforced in query.
const SESSION_TTL_HOURS: i64 = 24;

/// Generate a fresh session for `user_id`.
/// Returns `(raw_token, expires_at)`.  Caller must store `sha256_hex(raw_token)`.
pub async fn create_session(
    pool:       &PgPool,
    user_id:    Uuid,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) -> AppResult<(String, DateTime<Utc>)> {
    let raw_token = generate_token();
    let token_hash = crate::crypto::sha256_hex(&raw_token);
    let expires_at = Utc::now() + Duration::hours(SESSION_TTL_HOURS);

    sqlx::query(
        "INSERT INTO sessions (user_id, token_hash, expires_at, ip_address, user_agent)
         VALUES ($1, $2, $3, $4::inet, $5)",
    )
    .bind(user_id)
    .bind(&token_hash)
    .bind(expires_at)
    .bind(ip_address)
    .bind(user_agent)
    .execute(pool)
    .await?;

    Ok((raw_token, expires_at))
}

/// Load a session that is not expired (absolute) and not idle.
///
/// `idle_minutes` is loaded from the `session_idle_minutes` business rule
/// at the middleware layer, allowing runtime configuration without restarts.
pub async fn find_active_session(
    pool:         &PgPool,
    token_hash:   &str,
    idle_minutes: i64,
) -> AppResult<Option<SessionRecord>> {
    let row = sqlx::query_as::<_, SessionRecord>(
        "SELECT u.id        AS user_id,
                u.username,
                u.role,
                u.full_name,
                s.expires_at,
                s.last_active_at,
                HOST(s.ip_address) AS ip_address
         FROM   sessions s
         JOIN   users    u ON u.id = s.user_id
         WHERE  s.token_hash    = $1
           AND  s.expires_at    > NOW()
           AND  s.last_active_at > NOW() - ($2 || ' minutes')::INTERVAL
           AND  u.is_active     = TRUE",
    )
    .bind(token_hash)
    .bind(idle_minutes.to_string())
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Bump `last_active_at` so the idle-expiry window resets.
pub async fn touch_session(pool: &PgPool, token_hash: &str) -> AppResult<()> {
    sqlx::query("UPDATE sessions SET last_active_at = NOW() WHERE token_hash = $1")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Invalidate a session immediately (logout or forced expiry).
pub async fn invalidate_session(pool: &PgPool, token_hash: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Remove all sessions for a user (e.g., on password change or admin force-logout).
pub async fn invalidate_all_user_sessions(pool: &PgPool, user_id: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
