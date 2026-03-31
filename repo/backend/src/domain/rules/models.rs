//! Configurable business rules domain value types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── BusinessRule ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct BusinessRule {
    pub id:          Uuid,
    pub rule_key:    String,
    pub rule_value:  String,
    pub description: Option<String>,
    pub updated_by:  Option<Uuid>,
    pub updated_at:  DateTime<Utc>,
}

impl BusinessRule {
    /// Parse `rule_value` as an integer, falling back to `default`.
    pub fn as_i64(&self, default: i64) -> i64 {
        self.rule_value.parse().unwrap_or(default)
    }

    /// Parse `rule_value` as a float, falling back to `default`.
    pub fn as_f64(&self, default: f64) -> f64 {
        self.rule_value.parse().unwrap_or(default)
    }
}
