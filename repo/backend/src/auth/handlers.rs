//! Auth HTTP handlers
//!
//! POST   /api/v1/auth/login   — verify credentials, open session
//! DELETE /api/v1/auth/logout  — invalidate current session
//! GET    /api/v1/auth/me      — return current user info

use actix_web::{web, HttpRequest, HttpResponse};
use chrono::Utc;
use tracing::info;

use crate::{
    config::AppConfig,
    crypto,
    error::{AppError, AppResult},
};

use super::{
    middleware::AuthUser,
    models::{LoginRequest, LoginResponse, UserInfo},
    session,
};

// ── Login ─────────────────────────────────────────────────────────────────────

pub async fn login(
    req:  HttpRequest,
    pool: web::Data<sqlx::PgPool>,
    cfg:  web::Data<AppConfig>,
    body: web::Json<LoginRequest>,
) -> AppResult<HttpResponse> {
    let ip = req.peer_addr().map(|a| a.ip().to_string());
    let ua = req.headers()
        .get("User-Agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    // ── Load user ─────────────────────────────────────────────────────────
    let row: Option<UserRow> = sqlx::query_as(
        "SELECT id, password_hash, role, full_name, is_active,
                failed_login_attempts, locked_until
         FROM   users
         WHERE  username = $1",
    )
    .bind(&body.username)
    .fetch_optional(pool.as_ref())
    .await?;

    let Some(user) = row else {
        // Record generic failure so we don't leak user existence.
        write_audit(
            &pool, "auth.login_failed", None,
            &format!("Unknown username: {}", body.username),
            ip.as_deref(),
        ).await;
        return Err(AppError::Unauthorized("Invalid credentials".into()));
    };

    if !user.is_active {
        return Err(AppError::Forbidden("Account is disabled".into()));
    }

    // ── Lockout check ─────────────────────────────────────────────────────
    if let Some(locked_until) = user.locked_until {
        if locked_until > Utc::now() {
            write_audit(
                &pool, "auth.login_blocked", Some(user.id),
                "Login attempted while account locked",
                ip.as_deref(),
            ).await;
            return Err(AppError::AccountLocked(locked_until.to_rfc3339()));
        }
        // Lock period has passed — reset the counter.
        reset_lockout(&pool, user.id).await?;
    }

    // ── Password verification ─────────────────────────────────────────────
    let valid = crypto::verify_password(&body.password, &user.password_hash)
        .unwrap_or(false);

    if !valid {
        let new_count = user.failed_login_attempts + 1;
        let max       = cfg.security.max_failed_logins as i32;

        if new_count >= max {
            lock_account(&pool, user.id, cfg.security.lockout_minutes).await;
            write_audit(
                &pool, "auth.lockout", Some(user.id),
                &format!("{new_count} failed attempts — account locked"),
                ip.as_deref(),
            ).await;
            return Err(AppError::AccountLocked("Account locked after too many failed attempts".into()));
        }

        increment_failures(&pool, user.id, new_count).await;
        write_audit(
            &pool, "auth.login_failed", Some(user.id),
            &format!("Bad password ({new_count}/{max} attempts)"),
            ip.as_deref(),
        ).await;
        return Err(AppError::Unauthorized("Invalid credentials".into()));
    }

    // ── Issue session ─────────────────────────────────────────────────────
    reset_lockout(&pool, user.id).await?;
    let (token, expires_at) =
        session::create_session(&pool, user.id, ip.as_deref(), ua.as_deref()).await?;

    let role = parse_role(&user.role);

    write_audit(&pool, "auth.login", Some(user.id), "Login successful", ip.as_deref()).await;
    info!(user = %body.username, role = %user.role, "Login successful");

    Ok(HttpResponse::Ok().json(LoginResponse {
        token,
        expires_at,
        user: UserInfo {
            id:        user.id,
            username:  body.username.clone(),
            role,
            full_name: user.full_name.clone(),
        },
    }))
}

// ── Logout ────────────────────────────────────────────────────────────────────

pub async fn logout(
    auth: AuthUser,
    pool: web::Data<sqlx::PgPool>,
    req:  HttpRequest,
) -> AppResult<HttpResponse> {
    let ip = req.peer_addr().map(|a| a.ip().to_string());
    session::invalidate_session(&pool, &auth.token_hash).await?;
    write_audit(&pool, "auth.logout", Some(auth.id), "Logout", ip.as_deref()).await;
    info!(user = %auth.username, "Logout");
    Ok(HttpResponse::NoContent().finish())
}

// ── Me ────────────────────────────────────────────────────────────────────────

pub async fn me(auth: AuthUser) -> AppResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(UserInfo {
        id:        auth.id,
        username:  auth.username.clone(),
        role:      auth.role.clone(),
        full_name: auth.full_name.clone(),
    }))
}

// ── DB helpers ────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct UserRow {
    id:                    uuid::Uuid,
    password_hash:         String,
    role:                  String,
    full_name:             Option<String>,
    is_active:             bool,
    failed_login_attempts: i32,
    locked_until:          Option<chrono::DateTime<Utc>>,
}

async fn increment_failures(pool: &sqlx::PgPool, user_id: uuid::Uuid, count: i32) {
    let _ = sqlx::query(
        "UPDATE users SET failed_login_attempts = $1, updated_at = NOW() WHERE id = $2",
    )
    .bind(count)
    .bind(user_id)
    .execute(pool)
    .await;
}

async fn lock_account(pool: &sqlx::PgPool, user_id: uuid::Uuid, lockout_minutes: u64) {
    let locked_until = Utc::now() + chrono::Duration::minutes(lockout_minutes as i64);
    let _ = sqlx::query(
        "UPDATE users
         SET locked_until = $1, failed_login_attempts = 0, updated_at = NOW()
         WHERE id = $2",
    )
    .bind(locked_until)
    .bind(user_id)
    .execute(pool)
    .await;
}

async fn reset_lockout(pool: &sqlx::PgPool, user_id: uuid::Uuid) -> AppResult<()> {
    sqlx::query(
        "UPDATE users
         SET locked_until = NULL, failed_login_attempts = 0, updated_at = NOW()
         WHERE id = $1",
    )
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn write_audit(
    pool:       &sqlx::PgPool,
    event_type: &str,
    user_id:    Option<uuid::Uuid>,
    action:     &str,
    ip_address: Option<&str>,
) {
    let _ = sqlx::query(
        "INSERT INTO audit_logs (event_type, entity_type, user_id, action, ip_address)
         VALUES ($1, 'auth', $2, $3, $4::inet)",
    )
    .bind(event_type)
    .bind(user_id)
    .bind(action)
    .bind(ip_address)
    .execute(pool)
    .await;
}

fn parse_role(s: &str) -> shared::UserRole {
    match s {
        "admin"      => shared::UserRole::Admin,
        "ops_agent"  => shared::UserRole::OpsAgent,
        "cs_agent"   => shared::UserRole::CsAgent,
        "dispatcher" => shared::UserRole::Dispatcher,
        _            => shared::UserRole::Kiosk,
    }
}
