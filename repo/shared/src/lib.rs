//! Shared domain types consumed by both the Actix-web backend and the Yew frontend.
//! Keep this crate `no_std`-compatible where possible so it compiles to WASM cleanly.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Re-exports used across both crates ────────────────────────────────────────
pub use chrono;
pub use rust_decimal;
pub use uuid;

// ── Role-based access control ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    /// Full system access, user management, business rule configuration.
    Admin,
    /// Operations admin: schedules, inventory, all orders.
    OpsAgent,
    /// Customer service: orders search, rebooking, cancellation.
    CsAgent,
    /// Staffing dispatcher: contractor matching, shift management.
    Dispatcher,
    /// Read-only station kiosk (unauthenticated public view).
    Kiosk,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::Admin      => "admin",
            UserRole::OpsAgent   => "ops_agent",
            UserRole::CsAgent    => "cs_agent",
            UserRole::Dispatcher => "dispatcher",
            UserRole::Kiosk      => "kiosk",
        }
    }
}

// ── Order domain ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Pending,
    Confirmed,
    /// Temporarily held; expires after 15 minutes if not confirmed.
    Held,
    Cancelled,
    Refunded,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefundOutcome {
    /// > 24 h before departure: fare − $5.00 fee.
    FullMinusFee,
    /// 2–24 h before departure: 50 % of fare.
    HalfFare,
    /// < 2 h before departure: blocked unless service disruption.
    Blocked,
    /// Service disruption exception applied.
    ServiceDisruption,
}

// ── Schedule domain ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleStatus {
    Scheduled,
    Delayed,
    Cancelled,
    Completed,
}

// ── Staffing domain ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShiftStatus {
    Open,
    Assigned,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentStatus {
    Proposed,
    Accepted,
    Rejected,
}

// ── Credential domain ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

// ── Content / kiosk domain ───────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentCategory {
    Fares,
    Delays,
    Baggage,
    Accessibility,
    General,
}

// ── Data quality ──────────────────────────────────────────────────────────────

/// Quality score breakdown (all 0.0–100.0).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityScore {
    /// Completeness weight: 50 %.
    pub completeness: Decimal,
    /// Accuracy weight: 30 %.
    pub accuracy: Decimal,
    /// Timeliness weight: 20 %.
    pub timeliness: Decimal,
    /// Weighted total (block publish if < 85).
    pub total: Decimal,
}

impl QualityScore {
    pub const PUBLISH_THRESHOLD: f64 = 85.0;

    pub fn compute(completeness: Decimal, accuracy: Decimal, timeliness: Decimal) -> Self {
        // weights: completeness 50 %, accuracy 30 %, timeliness 20 %
        let w50 = Decimal::new(50, 2); // 0.50
        let w30 = Decimal::new(30, 2); // 0.30
        let w20 = Decimal::new(20, 2); // 0.20
        let total = completeness * w50 + accuracy * w30 + timeliness * w20;
        Self { completeness, accuracy, timeliness, total }
    }

    pub fn is_publishable(&self) -> bool {
        self.total >= Decimal::try_from(Self::PUBLISH_THRESHOLD).unwrap_or_default()
    }
}

// ── Generic API envelope ─────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code:       u16,
    pub message:    String,
    #[serde(rename = "type")]
    pub error_type: String,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self { success: true, data: Some(data), error: None }
    }
}

impl ApiResponse<()> {
    pub fn err(code: u16, message: impl Into<String>, error_type: impl Into<String>) -> Self {
        Self {
            success: false,
            data:    None,
            error:   Some(ApiError { code, message: message.into(), error_type: error_type.into() }),
        }
    }
}

// ── Pagination ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items:       Vec<T>,
    pub total:       i64,
    pub page:        i64,
    pub per_page:    i64,
    pub total_pages: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PaginationParams {
    pub page:     Option<i64>,
    pub per_page: Option<i64>,
}

impl PaginationParams {
    pub fn page(&self)     -> i64 { self.page.unwrap_or(1).max(1) }
    pub fn per_page(&self) -> i64 { self.per_page.unwrap_or(20).clamp(1, 100) }
    pub fn offset(&self)   -> i64 { (self.page() - 1) * self.per_page() }
}

// ── Match scoring (staffing) ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct MatchResult {
    pub contractor_id: Uuid,
    pub score:         Decimal,
    /// Exactly three reasons why this candidate was ranked.
    pub top_reasons:   [String; 3],
}

// ── PII masking helpers ───────────────────────────────────────────────────────

/// Mask a phone number to the display format `(XXX) XXX-1234`.
pub fn mask_phone(last4: &str) -> String {
    format!("(XXX) XXX-{}", last4)
}

// ── Business rule constants ───────────────────────────────────────────────────

pub mod rules {
    use std::time::Duration;

    /// Hold expires after this duration if order not confirmed.
    pub const ORDER_HOLD_TTL_MINUTES:   u64 = 15;
    /// Refund processing fee for full refunds.
    pub const REFUND_PROCESSING_FEE_USD: &str = "5.00";
    /// Full-refund window (more than this before departure).
    pub const FULL_REFUND_HOURS:         i64 = 24;
    /// Partial-refund window (2–24 h before departure).
    pub const PARTIAL_REFUND_HOURS:      i64 = 2;
    /// Refund percentage for partial window.
    pub const PARTIAL_REFUND_PCT:        u32 = 50;
    /// Minimum password length.
    pub const MIN_PASSWORD_LEN:          usize = 12;
    /// Failed login attempts before lockout.
    pub const MAX_FAILED_LOGINS:         u32 = 5;
    /// Lockout duration in minutes.
    pub const LOCKOUT_MINUTES:           u64 = 15;
    /// Idle session expiry in minutes.
    pub const SESSION_IDLE_MINUTES:      u64 = 30;
    /// API rate limit per session per minute.
    pub const RATE_LIMIT_RPM:            u32 = 60;
    /// Quality score below which publishing is blocked.
    pub const QUALITY_PUBLISH_THRESHOLD: u32 = 85;
    /// Content similarity threshold for quarantine.
    pub const SIMILARITY_QUARANTINE:     f64 = 0.92;
    /// Default crawl rate limit (requests per second per source).
    pub const CRAWL_RATE_LIMIT_RPS:      f64 = 2.0;
    /// Default global crawl concurrency cap.
    pub const CRAWL_MAX_WORKERS:         usize = 10;
    /// Percentage of records sampled per batch for review.
    pub const SAMPLE_REVIEW_PCT:         f64 = 0.02;
    /// PII purge deadline after trip completion (days).
    pub const PII_PURGE_DAYS:            i64 = 30;
    /// Audit log retention years.
    pub const AUDIT_RETENTION_YEARS:     i64 = 7;
    /// Max file upload size (10 MB).
    pub const MAX_UPLOAD_BYTES:          u64 = 10 * 1024 * 1024;
}

// ── Unit tests ────────────────────────────────────────────────────────────────
//
// These tests cover shared utility functions used by both the frontend (Yew)
// and backend.  They run on native targets (`cargo test -p shared`) so they
// execute inside the Docker tester container without a WASM runtime.

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    // ── QualityScore ──────────────────────────────────────────────────────────

    #[test]
    fn quality_score_compute_weights() {
        println!("[shared] quality_score_compute_weights");
        // completeness 100, accuracy 100, timeliness 100 → total 100
        let s = QualityScore::compute(
            Decimal::new(100, 0),
            Decimal::new(100, 0),
            Decimal::new(100, 0),
        );
        assert_eq!(s.total, Decimal::new(100, 0), "all-100 inputs → 100 total");

        // completeness 0, accuracy 0, timeliness 0 → total 0
        let z = QualityScore::compute(
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
        );
        assert_eq!(z.total, Decimal::ZERO, "all-zero inputs → 0 total");
        println!("[shared] quality_score_compute_weights: PASS");
    }

    #[test]
    fn quality_score_weighted_formula() {
        println!("[shared] quality_score_weighted_formula");
        // 50% completeness, 30% accuracy, 20% timeliness
        // completeness=80, accuracy=60, timeliness=40
        // expected = 80*0.5 + 60*0.3 + 40*0.2 = 40 + 18 + 8 = 66
        let s = QualityScore::compute(
            Decimal::new(80, 0),
            Decimal::new(60, 0),
            Decimal::new(40, 0),
        );
        assert_eq!(s.total, Decimal::new(66, 0), "weighted total must equal 66");
        assert_eq!(s.completeness, Decimal::new(80, 0));
        assert_eq!(s.accuracy,     Decimal::new(60, 0));
        assert_eq!(s.timeliness,   Decimal::new(40, 0));
        println!("[shared] quality_score_weighted_formula: PASS");
    }

    #[test]
    fn quality_score_publishable_threshold() {
        println!("[shared] quality_score_publishable_threshold");
        // Score exactly at threshold (85) → publishable
        let at_threshold = QualityScore::compute(
            Decimal::new(85, 0),
            Decimal::new(85, 0),
            Decimal::new(85, 0),
        );
        assert!(at_threshold.is_publishable(),
            "score == 85 should be publishable");

        // Score just below threshold → not publishable
        // completeness=80 accuracy=80 timeliness=80 → total = 80
        let below = QualityScore::compute(
            Decimal::new(80, 0),
            Decimal::new(80, 0),
            Decimal::new(80, 0),
        );
        assert!(!below.is_publishable(),
            "score == 80 should NOT be publishable (threshold is 85)");

        println!("[shared] quality_score_publishable_threshold: PASS");
    }

    // ── PaginationParams ──────────────────────────────────────────────────────

    #[test]
    fn pagination_params_defaults() {
        println!("[shared] pagination_params_defaults");
        let p = PaginationParams::default();
        assert_eq!(p.page(),     1,  "default page is 1");
        assert_eq!(p.per_page(), 20, "default per_page is 20");
        assert_eq!(p.offset(),   0,  "offset for page 1 is 0");
        println!("[shared] pagination_params_defaults: PASS");
    }

    #[test]
    fn pagination_params_offset_calculation() {
        println!("[shared] pagination_params_offset_calculation");
        let p = PaginationParams { page: Some(3), per_page: Some(10) };
        assert_eq!(p.page(),     3,  "page=3");
        assert_eq!(p.per_page(), 10, "per_page=10");
        assert_eq!(p.offset(),   20, "offset for page 3 with per_page 10 is 20");
        println!("[shared] pagination_params_offset_calculation: PASS");
    }

    #[test]
    fn pagination_params_clamps_per_page() {
        println!("[shared] pagination_params_clamps_per_page");
        let too_large = PaginationParams { page: Some(1), per_page: Some(500) };
        assert_eq!(too_large.per_page(), 100, "per_page clamped to max 100");

        let zero = PaginationParams { page: Some(1), per_page: Some(0) };
        assert_eq!(zero.per_page(), 1, "per_page clamped to min 1");
        println!("[shared] pagination_params_clamps_per_page: PASS");
    }

    #[test]
    fn pagination_params_page_min_is_one() {
        println!("[shared] pagination_params_page_min_is_one");
        let negative = PaginationParams { page: Some(-5), per_page: None };
        assert_eq!(negative.page(), 1, "negative page clamped to 1");
        println!("[shared] pagination_params_page_min_is_one: PASS");
    }

    // ── mask_phone ────────────────────────────────────────────────────────────

    #[test]
    fn mask_phone_formats_last_four() {
        println!("[shared] mask_phone_formats_last_four");
        assert_eq!(mask_phone("1234"), "(XXX) XXX-1234");
        assert_eq!(mask_phone("9999"), "(XXX) XXX-9999");
        assert_eq!(mask_phone("0000"), "(XXX) XXX-0000");
        println!("[shared] mask_phone_formats_last_four: PASS");
    }

    // ── Rules constants ───────────────────────────────────────────────────────

    #[test]
    fn rules_constants_sanity() {
        println!("[shared] rules_constants_sanity");
        use rules::*;
        // These constants drive the business rules engine; verify they're sane.
        assert!(FULL_REFUND_HOURS > PARTIAL_REFUND_HOURS,
            "full-refund window must be wider than partial window");
        assert!(PARTIAL_REFUND_PCT <= 100,
            "partial refund % must not exceed 100");
        assert!(QUALITY_PUBLISH_THRESHOLD <= 100,
            "quality threshold must be a valid percentage");
        assert!(SIMILARITY_QUARANTINE > 0.0 && SIMILARITY_QUARANTINE <= 1.0,
            "similarity threshold must be in (0, 1]");
        assert!(MIN_PASSWORD_LEN >= 8,
            "minimum password length must be at least 8");
        assert!(MAX_FAILED_LOGINS >= 3,
            "lockout threshold must allow at least 3 attempts");
        println!("[shared] rules_constants_sanity: PASS");
    }
}
