//! API client for the configurable business rules endpoint.
//!
//! All routes require the `ConfigureRules` permission (Admin only).

use serde::{Deserialize, Serialize};

use super::{parse_json, signed_request, ApiResult};

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct BusinessRule {
    pub id:          String,
    pub rule_key:    String,
    pub rule_value:  String,
    pub description: Option<String>,
    pub updated_by:  Option<String>,
    pub updated_at:  String,
}

impl BusinessRule {
    /// Short human-readable category derived from the key prefix.
    pub fn category(&self) -> &'static str {
        if self.rule_key.starts_with("refund") || self.rule_key.starts_with("order_hold") {
            "Orders & Refunds"
        } else if self.rule_key.starts_with("quality") || self.rule_key.starts_with("similarity") {
            "Content Quality"
        } else if self.rule_key.starts_with("crawl") || self.rule_key.starts_with("sample") {
            "Data Ingestion"
        } else if self.rule_key.starts_with("session") || self.rule_key.starts_with("rate")
            || self.rule_key.starts_with("max_failed") || self.rule_key.starts_with("lockout")
        {
            "Security & Sessions"
        } else {
            "General"
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateRuleBody {
    pub value: String,
}

// ── API functions ─────────────────────────────────────────────────────────────

pub async fn list_rules(token: &str) -> ApiResult<Vec<BusinessRule>> {
    let resp = signed_request("GET", "/api/v1/rules", token, None::<&()>).await?;
    parse_json(resp).await
}

pub async fn update_rule(token: &str, key: &str, body: &UpdateRuleBody) -> ApiResult<BusinessRule> {
    let resp = signed_request(
        "PATCH",
        &format!("/api/v1/rules/{key}"),
        token,
        Some(body),
    ).await?;
    parse_json(resp).await
}
