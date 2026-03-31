//! Orders domain value types.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use shared::{OrderStatus, PaginationParams};

// ── Passenger ─────────────────────────────────────────────────────────────────

/// Passenger record — PII fields (`phone_encrypted`, `email_encrypted`) are
/// never selected; only the masked `phone_last4` is exposed to the API.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Passenger {
    pub id:                     Uuid,
    pub full_name:              String,
    pub phone_last4:            Option<String>,
    pub created_at:             DateTime<Utc>,
    pub pii_purge_requested_at: Option<DateTime<Utc>>,
    pub pii_purged_at:          Option<DateTime<Utc>>,
}

impl Passenger {
    /// Display-safe masked phone number.
    pub fn masked_phone(&self) -> Option<String> {
        self.phone_last4.as_ref().map(|l4| shared::mask_phone(l4))
    }
}

/// Input for creating a new passenger.
#[derive(Debug, Deserialize)]
pub struct NewPassenger {
    pub full_name:       String,
    /// Raw phone bytes (already AES-encrypted by the handler).
    pub phone_encrypted: Option<Vec<u8>>,
    pub phone_last4:     Option<String>,
    /// Raw email bytes (already AES-encrypted by the handler).
    pub email_encrypted: Option<Vec<u8>>,
}

// ── Order ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Order {
    pub id:                  Uuid,
    pub order_number:        String,
    pub passenger_id:        Uuid,
    pub schedule_id:         Uuid,
    pub seat_class_id:       Uuid,
    pub seat_number:         Option<String>,
    /// Raw DB value — use `Order::status_enum()` for the typed variant.
    pub status:              String,
    pub fare_amount:         Decimal,
    pub fee_override_amount: Option<Decimal>,
    pub fee_override_reason: Option<String>,
    pub hold_expires_at:     Option<DateTime<Utc>>,
    pub cancellation_reason: Option<String>,
    pub refund_amount:       Option<Decimal>,
    pub disruption_flag:     bool,
    pub created_at:          DateTime<Utc>,
    pub updated_at:          DateTime<Utc>,
    pub created_by:          Option<Uuid>,
}

impl Order {
    pub fn status_enum(&self) -> OrderStatus {
        match self.status.as_str() {
            "confirmed"  => OrderStatus::Confirmed,
            "held"       => OrderStatus::Held,
            "cancelled"  => OrderStatus::Cancelled,
            "refunded"   => OrderStatus::Refunded,
            "completed"  => OrderStatus::Completed,
            _            => OrderStatus::Pending,
        }
    }
}

/// Rich join view — order + passenger name + schedule/route details.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OrderDetail {
    pub id:              Uuid,
    pub order_number:    String,
    pub status:          String,
    pub fare_amount:     Decimal,
    pub disruption_flag: bool,
    pub seat_number:     Option<String>,
    pub hold_expires_at: Option<DateTime<Utc>>,
    pub created_at:      DateTime<Utc>,
    // Passenger
    pub passenger_id:    Uuid,
    pub passenger_name:  String,
    pub phone_last4:     Option<String>,
    // Schedule / route
    pub schedule_id:     Uuid,
    pub train_number:    String,
    pub departure_time:  DateTime<Utc>,
    pub arrival_time:    DateTime<Utc>,
    pub route_code:      String,
    pub route_name:      String,
    // Seat class
    pub seat_class_code: String,
    pub seat_class_name: String,
}

/// Input for creating a new order.
#[derive(Debug, Deserialize)]
pub struct NewOrder {
    pub passenger_id:  Uuid,
    pub schedule_id:   Uuid,
    pub seat_class_id: Uuid,
    pub seat_number:   Option<String>,
    pub fare_amount:   Decimal,
    pub created_by:    Option<Uuid>,
}

/// Input for cancelling an order.
#[derive(Debug, Deserialize)]
pub struct CancelOrder {
    pub reason:         String,
    pub disruption_flag: bool,
}

/// Input for fee override.
#[derive(Debug, Deserialize)]
pub struct FeeOverride {
    pub override_amount: Decimal,
    pub reason:          String,
}

/// Query params for listing orders.
#[derive(Debug, Default, Deserialize)]
pub struct ListOrdersParams {
    pub passenger_id: Option<Uuid>,
    pub schedule_id:  Option<Uuid>,
    pub status:       Option<String>,
    #[serde(flatten)]
    pub pagination:   PaginationParams,
}

// ── Order event ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OrderEvent {
    pub id:           i64,
    pub order_id:     Uuid,
    pub event_type:   String,
    pub performed_by: Option<Uuid>,
    pub reason:       Option<String>,
    pub data:         Option<Value>,
    pub created_at:   DateTime<Utc>,
}

/// Input for appending an order event.
#[derive(Debug)]
pub struct NewOrderEvent {
    pub order_id:     Uuid,
    pub event_type:   String,
    pub performed_by: Option<Uuid>,
    pub reason:       Option<String>,
    pub data:         Option<Value>,
}
