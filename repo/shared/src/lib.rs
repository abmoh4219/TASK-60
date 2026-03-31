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
