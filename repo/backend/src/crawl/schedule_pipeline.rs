//! Schedule aggregation pipeline.
//!
//! Transforms raw schedule JSON data from offline sources into validated
//! schedule and inventory records.  Parallel to the content pipeline
//! (`pipeline.rs`), but targets `schedules` + `inventory_snapshots` tables
//! instead of `content_pages`.
//!
//! ## Raw schema (source JSON)
//!
//! ```json
//! {
//!   "route_code":      "EW-001",
//!   "train_number":    "T-1234",
//!   "departure_time":  "2026-04-10T08:30:00Z",
//!   "arrival_time":    "2026-04-10T12:00:00Z",
//!   "platform":        "3A",
//!   "status":          "scheduled",
//!   "delay_minutes":   0,
//!   "inventory": [
//!     { "seat_class_code": "ECO", "total_seats": 200, "available_seats": 180 }
//!   ]
//! }
//! ```
//!
//! ## Validation / anomaly checks
//!
//! - Arrival must be after departure
//! - Departure must not be in the distant past (>30 days)
//! - Seat counts must be non-negative; available ≤ total
//! - Route code must resolve to an existing route
//! - Duplicate train_number + departure_time → skip (upsert)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// ── Raw input ────────────────────────────────────────────────────────────────

/// Raw schedule record deserialized from a source JSON file.
#[derive(Debug, Clone, Deserialize)]
pub struct RawScheduleItem {
    pub route_code:      Option<String>,
    pub train_number:    Option<String>,
    pub departure_time:  Option<String>,
    pub arrival_time:    Option<String>,
    pub platform:        Option<String>,
    pub status:          Option<String>,
    pub delay_minutes:   Option<i32>,
    pub inventory:       Option<Vec<RawInventoryItem>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawInventoryItem {
    pub seat_class_code:  Option<String>,
    pub total_seats:      Option<i32>,
    pub available_seats:  Option<i32>,
}

// ── Validated output ─────────────────────────────────────────────────────────

/// Validated, parsed schedule ready for DB upsert.
#[derive(Debug, Clone, Serialize)]
pub struct TransformedSchedule {
    pub route_code:     String,
    pub train_number:   String,
    pub departure_time: DateTime<Utc>,
    pub arrival_time:   DateTime<Utc>,
    pub platform:       Option<String>,
    pub status:         String,
    pub delay_minutes:  i32,
    pub inventory:      Vec<ValidatedInventory>,
    pub issues:         Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidatedInventory {
    pub seat_class_code:  String,
    pub total_seats:      i32,
    pub available_seats:  i32,
}

// ── Transform ────────────────────────────────────────────────────────────────

/// Transform a raw schedule item into a validated schedule.
/// Returns `None` if required fields are missing.
pub fn transform_schedule(raw: &RawScheduleItem) -> Option<TransformedSchedule> {
    let route_code = raw.route_code.as_deref()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())?;
    let train_number = raw.train_number.as_deref()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())?;

    let departure_time = raw.departure_time.as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok()
            .or_else(|| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()))
        .map(|dt| dt.with_timezone(&Utc))?;
    let arrival_time = raw.arrival_time.as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok()
            .or_else(|| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()))
        .map(|dt| dt.with_timezone(&Utc))?;

    let status = raw.status.as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| matches!(s.as_str(), "scheduled" | "delayed" | "cancelled" | "completed"))
        .unwrap_or_else(|| "scheduled".to_owned());

    let delay_minutes = raw.delay_minutes.unwrap_or(0).max(0);
    let platform = raw.platform.as_deref().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());

    let mut issues = Vec::new();

    // Validate inventory items.
    let inventory: Vec<ValidatedInventory> = raw.inventory
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|inv| {
            let code = inv.seat_class_code.as_deref()
                .map(|s| s.trim().to_uppercase())
                .filter(|s| !s.is_empty())?;
            let total = inv.total_seats.unwrap_or(0);
            let avail = inv.available_seats.unwrap_or(0);
            Some(ValidatedInventory {
                seat_class_code: code,
                total_seats: total.max(0),
                available_seats: avail.max(0).min(total.max(0)),
            })
        })
        .collect();

    Some(TransformedSchedule {
        route_code,
        train_number,
        departure_time,
        arrival_time,
        platform,
        status,
        delay_minutes,
        inventory,
        issues,
    })
}

// ── Validation / anomaly checks ──────────────────────────────────────────────

/// Run domain-specific anomaly checks on a transformed schedule.
/// Returns accumulated issues (informational) and whether the item should be
/// rejected outright.
pub fn validate_schedule(item: &mut TransformedSchedule) -> bool {
    let mut reject = false;

    // Arrival must be after departure.
    if item.arrival_time <= item.departure_time {
        item.issues.push("arrival_before_departure".into());
        reject = true;
    }

    // Departure in the distant past (>30 days) is likely stale data.
    let days_past = (Utc::now() - item.departure_time).num_days();
    if days_past > 30 {
        item.issues.push(format!("stale_schedule:{}d_past", days_past));
        reject = true;
    } else if days_past > 0 {
        item.issues.push(format!("departure_past:{}d_ago", days_past));
        // Not rejected — recent departures may still be useful for status updates.
    }

    // Inventory sanity: available ≤ total, non-negative.
    for inv in &item.inventory {
        if inv.available_seats > inv.total_seats {
            item.issues.push(format!(
                "inventory_overflow:{}:avail={}>total={}",
                inv.seat_class_code, inv.available_seats, inv.total_seats
            ));
        }
        if inv.total_seats < 0 || inv.available_seats < 0 {
            item.issues.push(format!(
                "negative_seats:{}", inv.seat_class_code
            ));
        }
    }

    // Delay consistency: if status is "scheduled", delay should be 0.
    if item.status == "scheduled" && item.delay_minutes > 0 {
        item.issues.push("delay_on_scheduled_status".into());
    }

    reject
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_rejects_missing_required_fields() {
        println!("[schedule_pipeline] transform_rejects_missing_required_fields");
        let raw = RawScheduleItem {
            route_code: None, train_number: None,
            departure_time: None, arrival_time: None,
            platform: None, status: None, delay_minutes: None, inventory: None,
        };
        assert!(transform_schedule(&raw).is_none());
        println!("[schedule_pipeline] transform_rejects_missing_required_fields: PASS");
    }

    #[test]
    fn transform_parses_valid_schedule() {
        println!("[schedule_pipeline] transform_parses_valid_schedule");
        let raw = RawScheduleItem {
            route_code: Some("EW-001".into()),
            train_number: Some("T-100".into()),
            departure_time: Some("2099-06-15T08:30:00Z".into()),
            arrival_time: Some("2099-06-15T12:00:00Z".into()),
            platform: Some("3A".into()),
            status: Some("scheduled".into()),
            delay_minutes: Some(0),
            inventory: Some(vec![RawInventoryItem {
                seat_class_code: Some("ECO".into()),
                total_seats: Some(200),
                available_seats: Some(180),
            }]),
        };
        let result = transform_schedule(&raw).unwrap();
        assert_eq!(result.route_code, "EW-001");
        assert_eq!(result.train_number, "T-100");
        assert_eq!(result.status, "scheduled");
        assert_eq!(result.inventory.len(), 1);
        assert_eq!(result.inventory[0].seat_class_code, "ECO");
        assert_eq!(result.inventory[0].total_seats, 200);
        assert_eq!(result.inventory[0].available_seats, 180);
        println!("[schedule_pipeline] transform_parses_valid_schedule: PASS");
    }

    #[test]
    fn validate_rejects_arrival_before_departure() {
        println!("[schedule_pipeline] validate_rejects_arrival_before_departure");
        let raw = RawScheduleItem {
            route_code: Some("EW-001".into()),
            train_number: Some("T-200".into()),
            departure_time: Some("2099-06-15T12:00:00Z".into()),
            arrival_time: Some("2099-06-15T08:00:00Z".into()), // before departure
            platform: None, status: None, delay_minutes: None, inventory: None,
        };
        let mut item = transform_schedule(&raw).unwrap();
        let reject = validate_schedule(&mut item);
        assert!(reject, "Should reject schedule with arrival before departure");
        assert!(item.issues.iter().any(|i| i.contains("arrival_before_departure")));
        println!("[schedule_pipeline] validate_rejects_arrival_before_departure: PASS");
    }

    #[test]
    fn validate_caps_available_seats() {
        println!("[schedule_pipeline] validate_caps_available_seats");
        let raw = RawScheduleItem {
            route_code: Some("EW-001".into()),
            train_number: Some("T-300".into()),
            departure_time: Some("2099-06-15T08:00:00Z".into()),
            arrival_time: Some("2099-06-15T12:00:00Z".into()),
            platform: None, status: None, delay_minutes: None,
            inventory: Some(vec![RawInventoryItem {
                seat_class_code: Some("ECO".into()),
                total_seats: Some(100),
                available_seats: Some(150), // exceeds total
            }]),
        };
        let result = transform_schedule(&raw).unwrap();
        // Transform caps available to total.
        assert_eq!(result.inventory[0].available_seats, 100);
        println!("[schedule_pipeline] validate_caps_available_seats: PASS");
    }

    #[test]
    fn validate_flags_delay_on_scheduled_status() {
        println!("[schedule_pipeline] validate_flags_delay_on_scheduled_status");
        let raw = RawScheduleItem {
            route_code: Some("EW-001".into()),
            train_number: Some("T-400".into()),
            departure_time: Some("2099-06-15T08:00:00Z".into()),
            arrival_time: Some("2099-06-15T12:00:00Z".into()),
            platform: None,
            status: Some("scheduled".into()),
            delay_minutes: Some(30),
            inventory: None,
        };
        let mut item = transform_schedule(&raw).unwrap();
        let _reject = validate_schedule(&mut item);
        assert!(item.issues.iter().any(|i| i.contains("delay_on_scheduled_status")));
        println!("[schedule_pipeline] validate_flags_delay_on_scheduled_status: PASS");
    }
}
