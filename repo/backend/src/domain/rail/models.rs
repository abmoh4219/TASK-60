//! Rail domain value types.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use shared::{ScheduleStatus, PaginationParams};

// ── Route ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Route {
    pub id:               Uuid,
    pub route_code:       String,
    pub name:             String,
    pub origin_city:      String,
    pub destination_city: String,
    pub is_active:        bool,
    pub created_at:       DateTime<Utc>,
}

// ── Schedule ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Schedule {
    pub id:             Uuid,
    pub route_id:       Uuid,
    pub train_number:   String,
    pub departure_time: DateTime<Utc>,
    pub arrival_time:   DateTime<Utc>,
    /// Raw DB value — use `Schedule::status()` for the typed enum.
    pub status:         String,
    pub delay_minutes:  i32,
    pub platform:       Option<String>,
    pub created_at:     DateTime<Utc>,
    pub updated_at:     DateTime<Utc>,
}

impl Schedule {
    pub fn status_enum(&self) -> ScheduleStatus {
        match self.status.as_str() {
            "delayed"   => ScheduleStatus::Delayed,
            "cancelled" => ScheduleStatus::Cancelled,
            "completed" => ScheduleStatus::Completed,
            _           => ScheduleStatus::Scheduled,
        }
    }
}

/// Joined view of a schedule with its route details.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ScheduleWithRoute {
    pub id:               Uuid,
    pub route_id:         Uuid,
    pub route_code:       String,
    pub route_name:       String,
    pub origin_city:      String,
    pub destination_city: String,
    pub train_number:     String,
    pub departure_time:   DateTime<Utc>,
    pub arrival_time:     DateTime<Utc>,
    pub status:           String,
    pub delay_minutes:    i32,
    pub platform:         Option<String>,
    pub updated_at:       DateTime<Utc>,
}

// ── Seat class ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SeatClass {
    pub id:        Uuid,
    pub name:      String,
    pub code:      String,
    pub base_fare: Decimal,
}

// ── Inventory snapshot ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InventorySnapshot {
    pub id:              Uuid,
    pub schedule_id:     Uuid,
    pub seat_class_id:   Uuid,
    pub total_seats:     i32,
    pub available_seats: i32,
    pub snapshot_at:     DateTime<Utc>,
}

/// Inventory for a schedule with seat-class details attached.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InventoryWithClass {
    pub seat_class_id:   Uuid,
    pub seat_class_code: String,
    pub seat_class_name: String,
    pub base_fare:       Decimal,
    pub total_seats:     i32,
    pub available_seats: i32,
    pub snapshot_at:     DateTime<Utc>,
}

// ── Command types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListSchedulesParams {
    pub route_id:        Option<Uuid>,
    /// Filter by status string (e.g. "scheduled", "delayed").
    pub status:          Option<String>,
    pub departure_after: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub pagination:      PaginationParams,
}

#[derive(Debug, Deserialize)]
pub struct UpdateScheduleStatus {
    pub status:        String,
    pub delay_minutes: Option<i32>,
    pub platform:      Option<String>,
}
