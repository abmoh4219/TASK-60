use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use shared::UserRole;

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token:      String,
    pub user:       UserInfo,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserInfo {
    pub id:        Uuid,
    pub username:  String,
    pub role:      UserRole,
    pub full_name: Option<String>,
}

// ── Database row used in session.rs ──────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
pub struct SessionRecord {
    pub user_id:        Uuid,
    pub username:       String,
    pub role:           String,
    pub full_name:      Option<String>,
    pub expires_at:     DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
}

impl SessionRecord {
    /// Parse the role string stored in PostgreSQL into the shared `UserRole` enum.
    pub fn user_role(&self) -> UserRole {
        match self.role.as_str() {
            "admin"      => UserRole::Admin,
            "ops_agent"  => UserRole::OpsAgent,
            "cs_agent"   => UserRole::CsAgent,
            "dispatcher" => UserRole::Dispatcher,
            _            => UserRole::Kiosk,
        }
    }
}
