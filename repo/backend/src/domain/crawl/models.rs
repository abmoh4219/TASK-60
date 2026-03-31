//! Data ingestion / crawl engine domain value types.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── CrawlSource ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrawlSource {
    pub id:               Uuid,
    pub name:             String,
    pub source_type:      String,
    pub base_path:        Option<String>,
    pub city:             Option<String>,
    pub keywords:         Vec<String>,
    pub rate_limit_rps:   Decimal,
    pub is_active:        bool,
    pub created_at:       DateTime<Utc>,
}

// ── CrawlTask ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrawlTask {
    pub id:               Uuid,
    pub source_id:        Uuid,
    pub task_name:        String,
    pub pagination_rules: Option<Value>,
    pub incremental:      bool,
    pub last_crawled_at:  Option<DateTime<Utc>>,
    pub next_crawl_at:    Option<DateTime<Utc>>,
    /// Raw DB value.
    pub status:           String,
    pub resume_cursor:    Option<Value>,
    pub created_at:       DateTime<Utc>,
}

// ── CrawlRun ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrawlRun {
    pub id:                Uuid,
    pub task_id:           Uuid,
    pub started_at:        DateTime<Utc>,
    pub finished_at:       Option<DateTime<Utc>>,
    /// Raw DB value.
    pub status:            String,
    pub pages_fetched:     i32,
    pub items_ingested:    i32,
    pub items_quarantined: i32,
    pub error_log:         Option<String>,
}

// ── QualityLog ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct QualityLog {
    pub id:                   i64,
    pub crawl_run_id:         Option<Uuid>,
    pub content_id:           Option<Uuid>,
    pub url_fingerprint:      Option<String>,
    pub completeness_score:   Option<Decimal>,
    pub accuracy_score:       Option<Decimal>,
    pub timeliness_score:     Option<Decimal>,
    pub quality_score:        Option<Decimal>,
    pub issues:               Option<Value>,
    pub is_quarantined:       bool,
    pub quarantine_reason:    Option<String>,
    pub transformation_steps: Option<Value>,
    pub created_at:           DateTime<Utc>,
}

// ── Command types ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct NewQualityLog {
    pub crawl_run_id:         Option<Uuid>,
    pub content_id:           Option<Uuid>,
    pub url_fingerprint:      Option<String>,
    pub completeness_score:   Option<Decimal>,
    pub accuracy_score:       Option<Decimal>,
    pub timeliness_score:     Option<Decimal>,
    pub quality_score:        Option<Decimal>,
    pub issues:               Option<Value>,
    pub is_quarantined:       bool,
    pub quarantine_reason:    Option<String>,
    pub transformation_steps: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCrawlSource {
    pub name:           String,
    pub source_type:    String,
    pub base_path:      Option<String>,
    pub city:           Option<String>,
    pub keywords:       Vec<String>,
    pub rate_limit_rps: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct CreateCrawlTask {
    pub source_id:        Uuid,
    pub task_name:        String,
    pub pagination_rules: Option<Value>,
    pub incremental:      bool,
}
