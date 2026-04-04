//! Content / kiosk domain value types.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use shared::{ContentCategory, PaginationParams};

// ── ContentPage ───────────────────────────────────────────────────────────────

/// The `search_vector` TSVECTOR column is managed by a DB trigger and is
/// never included in SELECT results.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ContentPage {
    pub id:                 Uuid,
    pub slug:               String,
    pub title:              String,
    pub body:               String,
    pub category:           String,
    pub route_id:           Option<Uuid>,
    pub publish_date:       Option<NaiveDate>,
    pub is_published:       bool,
    pub quality_score:      Option<Decimal>,
    pub source_url:         Option<String>,
    pub source_fingerprint: Option<String>,
    pub created_at:         DateTime<Utc>,
    pub updated_at:         DateTime<Utc>,
}

impl ContentPage {
    pub fn category_enum(&self) -> ContentCategory {
        match self.category.as_str() {
            "fares"         => ContentCategory::Fares,
            "delays"        => ContentCategory::Delays,
            "baggage"       => ContentCategory::Baggage,
            "accessibility" => ContentCategory::Accessibility,
            _               => ContentCategory::General,
        }
    }
}

/// Content page with its tags attached (for API responses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPageWithTags {
    #[serde(flatten)]
    pub page: ContentPage,
    pub tags: Vec<String>,
}

/// Content page summary for list views (no body).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ContentSummary {
    pub id:           Uuid,
    pub slug:         String,
    pub title:        String,
    pub category:     String,
    pub route_id:     Option<Uuid>,
    pub is_published: bool,
    pub quality_score: Option<Decimal>,
    pub publish_date: Option<NaiveDate>,
    pub updated_at:   DateTime<Utc>,
}

// ── Command types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateContent {
    pub slug:               String,
    pub title:              String,
    pub body:               String,
    pub category:           String,
    pub route_id:           Option<Uuid>,
    pub publish_date:       Option<NaiveDate>,
    pub source_url:         Option<String>,
    pub source_fingerprint: Option<String>,
    /// SHA-256 hash of the source URL only — first-class URL dedup signal.
    pub url_fingerprint:    Option<String>,
    pub quality_score:      Option<Decimal>,
    pub tags:               Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateContent {
    pub title:              Option<String>,
    pub body:               Option<String>,
    pub category:           Option<String>,
    pub route_id:           Option<Uuid>,
    pub publish_date:       Option<NaiveDate>,
    pub source_url:         Option<String>,
    pub source_fingerprint: Option<String>,
    pub quality_score:      Option<Decimal>,
    pub tags:               Option<Vec<String>>,
}

/// Query params for content listing / search.
#[derive(Debug, Default, Deserialize)]
pub struct ListContentParams {
    pub query:        Option<String>,
    pub category:     Option<String>,
    pub is_published: Option<bool>,
    pub route_id:     Option<Uuid>,
    #[serde(flatten)]
    pub pagination:   PaginationParams,
}
