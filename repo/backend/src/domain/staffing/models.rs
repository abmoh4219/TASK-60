//! Staffing / contractor matching domain value types.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use shared::{AssignmentStatus, ShiftStatus, PaginationParams};

// ── Contractor ────────────────────────────────────────────────────────────────

/// Contractor record — encrypted PII never selected.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Contractor {
    pub id:             Uuid,
    pub full_name:      String,
    pub phone_last4:    Option<String>,
    pub region:         Option<String>,
    pub quality_rating: Decimal,
    pub is_active:      bool,
    pub created_at:     DateTime<Utc>,
    pub updated_at:     DateTime<Utc>,
}

/// Contractor with their skill tags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractorWithTags {
    #[serde(flatten)]
    pub contractor: Contractor,
    pub tags:       Vec<String>,
}

/// Contractor availability window.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ContractorAvailability {
    pub id:             Uuid,
    pub contractor_id:  Uuid,
    pub available_from: DateTime<Utc>,
    pub available_to:   DateTime<Utc>,
    pub notes:          Option<String>,
}

// ── Shift ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Shift {
    pub id:            Uuid,
    pub schedule_id:   Option<Uuid>,
    pub role:          String,
    pub region:        Option<String>,
    pub required_tags: Vec<String>,
    pub shift_start:   DateTime<Utc>,
    pub shift_end:     DateTime<Utc>,
    /// Raw DB value — use `Shift::status_enum()` for the typed variant.
    pub status:        String,
    pub is_critical:   bool,
    pub created_by:    Option<Uuid>,
    pub created_at:    DateTime<Utc>,
}

impl Shift {
    pub fn status_enum(&self) -> ShiftStatus {
        match self.status.as_str() {
            "assigned"  => ShiftStatus::Assigned,
            "completed" => ShiftStatus::Completed,
            "cancelled" => ShiftStatus::Cancelled,
            _           => ShiftStatus::Open,
        }
    }
}

// ── ShiftAssignment ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ShiftAssignment {
    pub id:            Uuid,
    pub shift_id:      Uuid,
    pub contractor_id: Uuid,
    pub match_score:   Option<Decimal>,
    pub match_reasons: Option<Value>,
    pub assigned_by:   Option<Uuid>,
    pub assigned_at:   DateTime<Utc>,
    /// Raw DB value — use `ShiftAssignment::status_enum()`.
    pub status:        String,
}

impl ShiftAssignment {
    pub fn status_enum(&self) -> AssignmentStatus {
        match self.status.as_str() {
            "accepted" => AssignmentStatus::Accepted,
            "rejected" => AssignmentStatus::Rejected,
            _          => AssignmentStatus::Proposed,
        }
    }
}

/// Assignment joined with contractor details.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AssignmentWithContractor {
    pub id:              Uuid,
    pub shift_id:        Uuid,
    pub status:          String,
    pub match_score:     Option<Decimal>,
    pub match_reasons:   Option<Value>,
    pub assigned_at:     DateTime<Utc>,
    pub contractor_id:   Uuid,
    pub contractor_name: String,
    pub region:          Option<String>,
    pub quality_rating:  Decimal,
}

// ── Subscription ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Subscription {
    pub id:              Uuid,
    pub subscriber_type: String,
    pub subscriber_id:   Uuid,
    pub target_type:     String,
    pub target_id:       Uuid,
    pub created_at:      DateTime<Utc>,
}

// ── Command types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateContractor {
    pub full_name:       String,
    pub phone_encrypted: Option<Vec<u8>>,
    pub phone_last4:     Option<String>,
    pub email_encrypted: Option<Vec<u8>>,
    pub region:          Option<String>,
    pub tags:            Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateShift {
    pub schedule_id:  Option<Uuid>,
    pub role:         String,
    pub region:       Option<String>,
    pub required_tags: Vec<String>,
    pub shift_start:  DateTime<Utc>,
    pub shift_end:    DateTime<Utc>,
    pub is_critical:  bool,
    pub created_by:   Option<Uuid>,
}

/// Scoring result from the contractor-matching algorithm.
#[derive(Debug, Serialize, Deserialize)]
pub struct MatchCandidate {
    pub contractor_id: Uuid,
    pub full_name:     String,
    pub score:         Decimal,
    pub reasons:       [String; 3],
}

/// Query params for listing shifts.
#[derive(Debug, Default, Deserialize)]
pub struct ListShiftsParams {
    pub status:     Option<String>,
    pub region:     Option<String>,
    pub role:       Option<String>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}
