//! Contractor-matching engine.
//!
//! Scores each available contractor against a shift on three dimensions:
//!
//! | Component       | Max pts | Criterion                                    |
//! |-----------------|---------|----------------------------------------------|
//! | Quality rating  |  60     | `(rating / 5.0) × 60`, rounded to 1 dp       |
//! | Region match    |  25     | contractor.region == shift.region (non-null) |
//! | Tag breadth     |  15     | 5 pts per extra tag beyond required, capped 3|
//!
//! Total max = 100.  Only contractors that already pass the
//! `available_for_shift` availability + tag-coverage filter are scored.

use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::domain::staffing::{
    models::{Contractor, MatchCandidate, Shift},
    repo::ContractorRepo,
};
use crate::error::AppResult;

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct MatchEngine<'a> {
    pool: &'a PgPool,
}

impl<'a> MatchEngine<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Return all contractors eligible for `shift`, ranked by score descending.
    pub async fn rank_candidates(&self, shift: &Shift) -> AppResult<Vec<MatchCandidate>> {
        let repo = ContractorRepo::new(self.pool);
        let contractors = repo
            .available_for_shift(shift.shift_start, shift.shift_end, &shift.required_tags)
            .await?;

        let mut candidates = Vec::with_capacity(contractors.len());
        for c in &contractors {
            let tags = repo.tags_for(c.id).await?;
            candidates.push(Self::score(c, &tags, shift));
        }

        // Highest score first.
        candidates.sort_by(|a, b| b.score.cmp(&a.score));
        Ok(candidates)
    }

    /// Score a single contractor against a shift and produce three rationale strings.
    ///
    /// Exposed as `pub` so the `propose_assignment` handler can score a specific
    /// contractor without running the full ranking pipeline.
    pub fn score(contractor: &Contractor, tags: &[String], shift: &Shift) -> MatchCandidate {
        let five  = Decimal::new(5, 0);
        let sixty = Decimal::new(60, 0);

        // ── Quality component (0–60 pts) ──────────────────────────────────
        let quality_pts = (contractor.quality_rating / five * sixty)
            .round_dp(1)
            .min(Decimal::new(60, 0))
            .max(Decimal::ZERO);

        // ── Region match (25 pts) ─────────────────────────────────────────
        let region_match = shift.region.is_some()
            && shift.region.as_deref() == contractor.region.as_deref();
        let region_pts = if region_match {
            Decimal::new(25, 0)
        } else {
            Decimal::ZERO
        };

        // ── Tag breadth bonus (5 pts per extra tag, capped at 15 pts) ─────
        let n_req   = shift.required_tags.len() as i64;
        let n_total = tags.len() as i64;
        let extras  = (n_total - n_req).max(0).min(3);
        let tag_pts = Decimal::new(extras * 5, 0);

        let score = (quality_pts + region_pts + tag_pts)
            .max(Decimal::ZERO)
            .min(Decimal::new(100, 0));

        // ── Build rationale strings ────────────────────────────────────────
        let r0 = format!(
            "Quality rating {:.2}/5.00 → {quality_pts:.1} pts",
            contractor.quality_rating
        );

        let r1 = if region_match {
            format!(
                "Region match: {} (+25 pts)",
                shift.region.as_deref().unwrap_or("?")
            )
        } else {
            match (&shift.region, &contractor.region) {
                (Some(s), Some(c)) => format!("Cross-region: shift={s}, contractor={c}"),
                (Some(s), None)    => format!("No region set; shift requires {s}"),
                _                  => "No region constraint".to_owned(),
            }
        };

        let r2 = if n_req == 0 {
            format!("No required tags; holds {n_total} skill tag(s)")
        } else if extras > 0 {
            format!(
                "Holds {n_total} tags — {extras} beyond the {n_req} required (+{} pts)",
                extras * 5
            )
        } else {
            format!(
                "Holds exactly {n_req} required tag{}",
                if n_req == 1 { "" } else { "s" }
            )
        };

        MatchCandidate {
            contractor_id: contractor.id,
            full_name:     contractor.full_name.clone(),
            score,
            reasons:       [r0, r1, r2],
        }
    }
}
