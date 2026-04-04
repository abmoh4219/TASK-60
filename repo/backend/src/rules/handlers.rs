//! Admin API — list and update configurable business rules.
//!
//! Route summary (all under /api/v1/rules, all require ConfigureRules):
//!
//!   GET   /             list_rules    — paginated list of all rules
//!   GET   /{key}        get_rule      — single rule by key
//!   PATCH /{key}        update_rule   — change a rule value (audit-logged)

use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use crate::auth::rbac::RequireConfigureRules;
use crate::db::audit::write_audit_required;
use crate::domain::rules::repo::BusinessRuleRepo;
use crate::error::{AppError, AppResult};

// ── Body / param types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateRuleBody {
    /// New raw string value for the rule.
    pub value: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Return every rule, ordered alphabetically by `rule_key`.
pub async fn list_rules(
    pool:  web::Data<PgPool>,
    _auth: RequireConfigureRules,
) -> AppResult<HttpResponse> {
    let rules = BusinessRuleRepo::new(&pool).list().await?;
    Ok(HttpResponse::Ok().json(rules))
}

/// Return a single rule by key.
pub async fn get_rule(
    pool:  web::Data<PgPool>,
    path:  web::Path<String>,
    _auth: RequireConfigureRules,
) -> AppResult<HttpResponse> {
    let key  = path.into_inner();
    let rule = BusinessRuleRepo::new(&pool)
        .get(&key)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Rule '{key}'")))?;
    Ok(HttpResponse::Ok().json(rule))
}

/// Update the value of an existing rule.
///
/// Only known rule keys (already seeded) may be updated.  The new value is
/// validated for type and range before being written.  The change is
/// audit-logged with old and new values.
pub async fn update_rule(
    pool:  web::Data<PgPool>,
    path:  web::Path<String>,
    body:  web::Json<UpdateRuleBody>,
    auth:  RequireConfigureRules,
) -> AppResult<HttpResponse> {
    let key = path.into_inner();

    // Only update seeded keys — prevent creation of arbitrary rules.
    let existing = BusinessRuleRepo::new(&pool)
        .get(&key)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Rule '{key}'")))?;

    let value = body.value.trim().to_owned();
    if value.is_empty() {
        return Err(AppError::Validation("Rule value must not be empty".into()));
    }

    // Type + range validation before writing.
    validate_rule_value(&key, &value)?;

    let old_value = existing.rule_value.clone();

    BusinessRuleRepo::new(&pool)
        .set(&key, &value, Some(auth.id))
        .await?;

    write_audit_required(
        &pool,
        "rule_updated",
        "business_rules",
        Some(existing.id),
        Some(auth.id),
        "update_rule",
        Some(json!({ "rule_key": &key, "old_value": &old_value })),
        Some(json!({ "rule_key": &key, "new_value": &value })),
        None,
    ).await?;

    let updated = BusinessRuleRepo::new(&pool).get(&key).await?;
    Ok(HttpResponse::Ok().json(updated))
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Validate that a proposed rule value is parseable as the expected type
/// and falls within a sane range.
fn validate_rule_value(key: &str, value: &str) -> AppResult<()> {
    // Integer rules — must parse as a positive i64.
    const INT_KEYS: &[&str] = &[
        "order_hold_ttl_minutes",
        "refund_full_hours",
        "refund_partial_hours",
        "quality_publish_threshold",
        "session_idle_minutes",
        "rate_limit_rpm",
        "max_failed_logins",
        "lockout_minutes",
        "pii_purge_days",
        "crawl_max_workers",
    ];
    if INT_KEYS.contains(&key) {
        let n: i64 = value.parse().map_err(|_| {
            AppError::Validation(format!(
                "Rule '{}' requires a positive integer; got '{}'", key, value
            ))
        })?;
        if n <= 0 {
            return Err(AppError::Validation(format!(
                "Rule '{}' must be > 0", key
            )));
        }
        // Domain-specific upper bounds.
        match key {
            "quality_publish_threshold" if n > 100 => {
                return Err(AppError::Validation(
                    "quality_publish_threshold must be 1–100".into()
                ));
            }
            "rate_limit_rpm" if n > 10_000 => {
                return Err(AppError::Validation(
                    "rate_limit_rpm must be ≤ 10 000".into()
                ));
            }
            _ => {}
        }
        return Ok(());
    }

    // Decimal / float rules — must parse as a non-negative f64.
    const FLOAT_KEYS: &[&str] = &[
        "refund_processing_fee_usd",
        "similarity_quarantine",
        "crawl_rate_limit_rps",
        "sample_review_pct",
    ];
    if FLOAT_KEYS.contains(&key) {
        let f: f64 = value.parse().map_err(|_| {
            AppError::Validation(format!(
                "Rule '{}' requires a non-negative number; got '{}'", key, value
            ))
        })?;
        if f < 0.0 {
            return Err(AppError::Validation(format!(
                "Rule '{}' must be ≥ 0", key
            )));
        }
        if key == "similarity_quarantine" && f > 1.0 {
            return Err(AppError::Validation(
                "similarity_quarantine must be in 0.0–1.0".into()
            ));
        }
        return Ok(());
    }

    Ok(())
}
