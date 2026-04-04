//! Core policy engine.
//!
//! `RulesEngine` is constructed cheaply per-request and fetches each rule
//! value from the DB on demand.  Because the table is small and the read path
//! is already DB-bound, there is no in-memory cache — changes propagate
//! instantly without a restart.
//!
//! # Refund tier logic
//!
//! ```text
//!   hours_until_departure > refund_full_hours (24)
//!       → FullMinusFee   — fare − processing_fee ($5.00)
//!
//!   refund_partial_hours (2) < hours ≤ refund_full_hours (24)
//!       → HalfFare       — fare × 50 %
//!
//!   hours ≤ refund_partial_hours (2) AND disruption_flag = true
//!       → ServiceDisruption — full fare
//!
//!   hours ≤ refund_partial_hours (2) AND disruption_flag = false
//!       → RefundBlocked  (AppError::RefundBlocked)
//! ```

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::domain::rules::repo::BusinessRuleRepo;
use crate::error::{AppError, AppResult};
use shared::{RefundOutcome, rules};

// ── Decision type ─────────────────────────────────────────────────────────────

/// Result of evaluating refund eligibility for a specific order.
#[derive(Debug)]
pub struct RefundDecision {
    /// Which refund tier applied.
    pub outcome:    RefundOutcome,
    /// Maximum refund amount that may be issued under this tier.
    /// The caller must ensure the requested amount does not exceed this.
    pub max_amount: Decimal,
    /// Human-readable explanation included in the audit / event record.
    pub reason:     String,
}

// ── RulesEngine ───────────────────────────────────────────────────────────────

/// Stateless policy engine backed by the `business_rules` table.
///
/// Construct once per request; it borrows the pool and calls `get_value`
/// for each rule key it needs.
pub struct RulesEngine<'a> {
    repo: BusinessRuleRepo<'a>,
}

impl<'a> RulesEngine<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { repo: BusinessRuleRepo::new(pool) }
    }

    // ── Order-hold policy ─────────────────────────────────────────────────

    /// Live hold-TTL in minutes (`order_hold_ttl_minutes` rule).
    pub async fn hold_ttl_minutes(&self) -> i64 {
        self.load_i64("order_hold_ttl_minutes", rules::ORDER_HOLD_TTL_MINUTES as i64).await
    }

    /// Compute the wall-clock expiry for a new held order.
    pub async fn hold_expiry(&self) -> DateTime<Utc> {
        Utc::now() + chrono::Duration::minutes(self.hold_ttl_minutes().await)
    }

    // ── Refund policy ─────────────────────────────────────────────────────

    /// Evaluate refund eligibility for a cancelled order.
    ///
    /// All four possible outcomes map to distinct DB rule keys so that ops
    /// can tune each threshold independently without redeploying.
    ///
    /// Returns `Err(AppError::RefundBlocked)` when the order is past the
    /// late window and no service disruption is flagged.
    pub async fn evaluate_refund(
        &self,
        departure_time:  DateTime<Utc>,
        fare_amount:     Decimal,
        disruption_flag: bool,
    ) -> AppResult<RefundDecision> {
        let full_hours    = self.load_i64("refund_full_hours",    rules::FULL_REFUND_HOURS).await;
        let partial_hours = self.load_i64("refund_partial_hours", rules::PARTIAL_REFUND_HOURS).await;

        let fee_str = self.repo
            .get_value("refund_processing_fee_usd", rules::REFUND_PROCESSING_FEE_USD)
            .await;
        let processing_fee: Decimal = fee_str
            .parse()
            .unwrap_or_else(|_| rules::REFUND_PROCESSING_FEE_USD.parse().expect("valid constant"));

        // Use minutes for sub-hour precision; convert back to fractional hours for messages.
        let mins_until   = (departure_time - Utc::now()).num_minutes();
        let hours_until  = mins_until / 60; // truncated, kept for display only

        let (outcome, max_amount, reason) = if mins_until > full_hours * 60 {
            let amt = (fare_amount - processing_fee).max(Decimal::ZERO);
            (
                RefundOutcome::FullMinusFee,
                amt,
                format!(
                    "Departure in {}h (>{} h window) — full refund minus ${} processing fee",
                    hours_until, full_hours, processing_fee,
                ),
            )
        } else if mins_until > partial_hours * 60 {
            let half = Decimal::new(5, 1); // 0.5
            let amt  = (fare_amount * half).round_dp(2);
            (
                RefundOutcome::HalfFare,
                amt,
                format!(
                    "Departure in {}h ({}-{} h window) — 50% partial refund",
                    hours_until, partial_hours, full_hours,
                ),
            )
        } else if disruption_flag {
            (
                RefundOutcome::ServiceDisruption,
                fare_amount,
                format!(
                    "Service disruption exception — full fare refund despite {}h window",
                    hours_until,
                ),
            )
        } else {
            return Err(AppError::RefundBlocked(format!(
                "Departure in {}h — refund window is closed (<{} h). \
                 Flag a service disruption first if an incident applies.",
                hours_until, partial_hours,
            )));
        };

        Ok(RefundDecision { outcome, max_amount, reason })
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    async fn load_i64(&self, key: &str, default: i64) -> i64 {
        self.repo
            .get_value(key, &default.to_string())
            .await
            .parse::<i64>()
            .unwrap_or(default)
    }
}
