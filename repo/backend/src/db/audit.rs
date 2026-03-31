//! Centralised, append-only audit log writer and reader.
//!
//! All domain operations that mutate data call `write_audit` so every change
//! is captured in `audit_logs` with full before/after JSON snapshots.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use shared::PaginatedResponse;

// ── Model ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuditLog {
    pub id:          i64,
    pub event_type:  String,
    pub entity_type: Option<String>,
    pub entity_id:   Option<Uuid>,
    pub user_id:     Option<Uuid>,
    pub action:      String,
    pub old_data:    Option<Value>,
    pub new_data:    Option<Value>,
    /// Decoded from INET column via `::text` cast in SELECT.
    pub ip_address:  Option<String>,
    pub created_at:  DateTime<Utc>,
}

// ── Write ─────────────────────────────────────────────────────────────────────

/// Append one row to `audit_logs`.  Never returns an error to the caller —
/// a failed audit write should not abort the business transaction.
pub async fn write_audit(
    pool:        &PgPool,
    event_type:  &str,
    entity_type: &str,
    entity_id:   Option<Uuid>,
    user_id:     Option<Uuid>,
    action:      &str,
    old_data:    Option<Value>,
    new_data:    Option<Value>,
    ip_address:  Option<&str>,
) {
    let _ = sqlx::query(
        "INSERT INTO audit_logs
             (event_type, entity_type, entity_id, user_id, action,
              old_data, new_data, ip_address)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::inet)",
    )
    .bind(event_type)
    .bind(entity_type)
    .bind(entity_id)
    .bind(user_id)
    .bind(action)
    .bind(old_data)
    .bind(new_data)
    .bind(ip_address)
    .execute(pool)
    .await;
}

// ── Read ──────────────────────────────────────────────────────────────────────

pub struct AuditFilter<'a> {
    pub entity_type: Option<&'a str>,
    pub entity_id:   Option<Uuid>,
    pub user_id:     Option<Uuid>,
}

pub async fn list_audit(
    pool:     &PgPool,
    filter:   AuditFilter<'_>,
    page:     i64,
    per_page: i64,
) -> AppResult<PaginatedResponse<AuditLog>> {
    let per_page = per_page.clamp(1, 100);
    let offset   = (page.max(1) - 1) * per_page;

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_logs
         WHERE ($1::TEXT  IS NULL OR entity_type = $1)
           AND ($2::UUID  IS NULL OR entity_id   = $2)
           AND ($3::UUID  IS NULL OR user_id     = $3)",
    )
    .bind(filter.entity_type)
    .bind(filter.entity_id)
    .bind(filter.user_id)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;

    let items: Vec<AuditLog> = sqlx::query_as(
        "SELECT id, event_type, entity_type, entity_id, user_id, action,
                old_data, new_data, ip_address::text as ip_address, created_at
         FROM audit_logs
         WHERE ($1::TEXT IS NULL OR entity_type = $1)
           AND ($2::UUID IS NULL OR entity_id   = $2)
           AND ($3::UUID IS NULL OR user_id     = $3)
         ORDER BY created_at DESC
         LIMIT $4 OFFSET $5",
    )
    .bind(filter.entity_type)
    .bind(filter.entity_id)
    .bind(filter.user_id)
    .bind(per_page)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    let total_pages = (total + per_page - 1) / per_page;
    Ok(PaginatedResponse { items, total, page, per_page, total_pages })
}
