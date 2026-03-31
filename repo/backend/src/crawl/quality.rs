//! Content quality scoring and similarity-based quarantine detection.
//!
//! ## Scoring weights
//!
//! | Dimension    | Weight | Basis                                               |
//! |--------------|--------|-----------------------------------------------------|
//! | Completeness | 50 %   | presence of title, body, category, tags, date, URL  |
//! | Accuracy     | 30 %   | structural validity (URL scheme, body/title length) |
//! | Timeliness   | 20 %   | publish_date age (recent = higher score)            |
//!
//! Total below 85 blocks publishing (see `QualityScore::is_publishable`).
//!
//! ## Similarity quarantine
//!
//! Uses PostgreSQL pg_trgm `similarity()` to detect near-duplicates in
//! `content_pages.body` at the threshold defined in `shared::rules`.

use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use shared::QualityScore;

use super::pipeline::TransformedItem;
use crate::error::AppResult;

// ── Result ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ScoredItem {
    pub score:             QualityScore,
    pub issues:            Vec<String>,
    pub is_quarantined:    bool,
    pub quarantine_reason: Option<String>,
}

// ── Scorer ────────────────────────────────────────────────────────────────────

pub struct QualityScorer;

impl QualityScorer {
    /// Score a transformed item.
    ///
    /// `existing_content_id` — when re-scoring an existing page, pass its UUID
    /// so the similarity check excludes that page from the comparison set.
    pub async fn score(
        item:                &TransformedItem,
        pool:                &PgPool,
        existing_content_id: Option<Uuid>,
    ) -> AppResult<ScoredItem> {
        let mut issues: Vec<String> = Vec::new();

        let completeness = score_completeness(item, &mut issues);
        let accuracy     = score_accuracy(item, &mut issues);
        let timeliness   = score_timeliness(item.publish_date, &mut issues);

        let score = QualityScore::compute(
            dec(completeness),
            dec(accuracy),
            dec(timeliness),
        );

        let (is_quarantined, quarantine_reason) =
            check_similarity(item, pool, existing_content_id).await?;

        Ok(ScoredItem { score, issues, is_quarantined, quarantine_reason })
    }
}

// ── Completeness (50 %) ───────────────────────────────────────────────────────

fn score_completeness(item: &TransformedItem, issues: &mut Vec<String>) -> f64 {
    let mut present = 0usize;
    const FIELDS: usize = 6;

    if !item.title.is_empty()    { present += 1; } else { issues.push("missing_title".into()); }
    if !item.body.is_empty()     { present += 1; } else { issues.push("missing_body".into()); }
    if item.category != "general"{ present += 1; } else { issues.push("default_category".into()); }
    if !item.tags.is_empty()     { present += 1; } else { issues.push("no_tags".into()); }
    if item.publish_date.is_some(){ present += 1; } else { issues.push("missing_publish_date".into()); }
    if item.source_url.is_some() { present += 1; } else { issues.push("missing_source_url".into()); }

    (present as f64 / FIELDS as f64) * 100.0
}

// ── Accuracy (30 %) ───────────────────────────────────────────────────────────

fn score_accuracy(item: &TransformedItem, issues: &mut Vec<String>) -> f64 {
    let mut score: f64 = 100.0;

    if item.body.len() < 50 {
        score -= 40.0;
        issues.push("short_body".into());
    }
    if item.title.len() < 5 {
        score -= 20.0;
        issues.push("short_title".into());
    }
    if let Some(url) = &item.source_url {
        let ok = url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("file://");
        if !ok {
            score -= 20.0;
            issues.push("invalid_source_url_scheme".into());
        }
    }
    if item.tags.len() > 20 {
        score -= 10.0;
        issues.push("too_many_tags".into());
    }

    score.max(0.0)
}

// ── Timeliness (20 %) ─────────────────────────────────────────────────────────

fn score_timeliness(publish_date: Option<NaiveDate>, issues: &mut Vec<String>) -> f64 {
    let today = Utc::now().date_naive();
    match publish_date {
        None => {
            issues.push("unknown_publish_date".into());
            50.0
        }
        Some(date) => {
            let age = (today - date).num_days();
            if age < 0      { 100.0 }
            else if age <= 30  { 100.0 }
            else if age <= 90  { 70.0 }
            else if age <= 365 { 40.0 }
            else {
                issues.push("stale_content".into());
                10.0
            }
        }
    }
}

// ── Similarity quarantine ─────────────────────────────────────────────────────

/// Check whether the item body is a near-duplicate of any existing content.
///
/// Returns `(true, Some(reason))` when a duplicate is found at the configured
/// similarity threshold (`shared::rules::SIMILARITY_QUARANTINE`).
async fn check_similarity(
    item:                &TransformedItem,
    pool:                &PgPool,
    existing_content_id: Option<Uuid>,
) -> AppResult<(bool, Option<String>)> {
    // Short stubs would produce too many false-positive similarity matches.
    if item.body.len() < 100 {
        return Ok((false, None));
    }

    let threshold = shared::rules::SIMILARITY_QUARANTINE as f32;

    // similarity() returns float4 (real) in PostgreSQL, so we compare as real.
    // We cast the result to float8 for decoding into Rust f64.
    let row: Option<(Uuid, f64)> = sqlx::query_as(
        "SELECT id, similarity(body, $1)::float8 AS sim
         FROM content_pages
         WHERE ($2::UUID IS NULL OR id <> $2)
           AND similarity(body, $1) >= $3::real
         ORDER BY sim DESC
         LIMIT 1",
    )
    .bind(&item.body)
    .bind(existing_content_id)
    .bind(threshold)
    .fetch_optional(pool)
    .await
    .map_err(crate::error::AppError::Database)?;

    match row {
        Some((dup_id, sim)) => Ok((
            true,
            Some(format!("near_duplicate of content_id={dup_id} (similarity={sim:.2})")),
        )),
        None => Ok((false, None)),
    }
}

// ── Decimal helper ────────────────────────────────────────────────────────────

fn dec(v: f64) -> Decimal {
    Decimal::try_from(v).unwrap_or_default()
}
