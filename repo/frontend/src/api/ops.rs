//! Typed API client for the operations console endpoints.
//!
//! All requests are HMAC-signed with the caller's Bearer token.
//! Every function takes `token: &str` as the first argument — callers
//! obtain it from `AuthContext.token`.

use serde::{Deserialize, Serialize};

use super::{parse_json, signed_request, ApiResult};

// ── Shared pagination wrapper ─────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Page<T> {
    pub items:       Vec<T>,
    pub total:       i64,
    pub page:        i64,
    pub per_page:    i64,
    pub total_pages: i64,
}

// ── Reference data types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RouteItem {
    pub id:               String,
    pub route_code:       String,
    pub name:             String,
    pub origin_city:      String,
    pub destination_city: String,
    pub is_active:        bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SeatClassItem {
    pub id:        String,
    pub name:      String,
    pub code:      String,
    /// `rust_decimal::Decimal` serialises as a JSON string on the backend.
    pub base_fare: String,
}

// ── Schedule types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ScheduleRow {
    pub id:               String,
    pub route_id:         String,
    pub route_code:       String,
    pub route_name:       String,
    pub origin_city:      String,
    pub destination_city: String,
    pub train_number:     String,
    /// ISO-8601 from backend.
    pub departure_time:   String,
    pub arrival_time:     String,
    pub status:           String,
    pub delay_minutes:    i32,
    pub platform:         Option<String>,
    pub updated_at:       String,
}

impl ScheduleRow {
    /// Short display label: `"EW-001 → EastWest Express"`
    pub fn route_label(&self) -> String {
        format!("{} — {}", self.route_code, self.route_name)
    }

    /// Tailwind badge classes for the status.
    pub fn status_color(&self) -> &'static str {
        match self.status.as_str() {
            "delayed"   => "bg-amber-100 text-amber-800",
            "cancelled" => "bg-red-100 text-red-800",
            "completed" => "bg-green-100 text-green-800",
            _           => "bg-blue-100 text-blue-800",
        }
    }

    /// Format departure_time as `"Mon 15 Jan 14:30"`.
    pub fn fmt_departure(&self) -> String {
        fmt_dt(&self.departure_time)
    }
    pub fn fmt_arrival(&self) -> String {
        fmt_dt(&self.arrival_time)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct InventoryItem {
    pub seat_class_id:   String,
    pub seat_class_code: String,
    pub seat_class_name: String,
    pub base_fare:       String,
    pub total_seats:     i32,
    pub available_seats: i32,
    pub snapshot_at:     String,
}

impl InventoryItem {
    pub fn occupancy_pct(&self) -> u32 {
        if self.total_seats == 0 { return 0; }
        let occupied = self.total_seats - self.available_seats;
        ((occupied as f64 / self.total_seats as f64) * 100.0).round() as u32
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ScheduleDetail {
    pub schedule:  ScheduleRow,
    pub inventory: Vec<InventoryItem>,
}

// ── Passenger types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PassengerItem {
    pub id:                      String,
    pub full_name:               String,
    pub phone_last4:             Option<String>,
    pub created_at:              String,
    pub pii_purge_requested_at:  Option<String>,
    pub pii_purged_at:           Option<String>,
}

impl PassengerItem {
    pub fn masked_phone(&self) -> String {
        match &self.phone_last4 {
            Some(l4) => format!("•••• {}", l4),
            None     => "—".into(),
        }
    }
    pub fn is_purged(&self) -> bool { self.pii_purged_at.is_some() }
}

// ── Order types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OrderItem {
    pub id:                  String,
    pub order_number:        String,
    pub passenger_id:        String,
    pub schedule_id:         String,
    pub seat_class_id:       String,
    pub seat_number:         Option<String>,
    pub status:              String,
    pub fare_amount:         String,
    pub fee_override_amount: Option<String>,
    pub fee_override_reason: Option<String>,
    pub hold_expires_at:     Option<String>,
    pub cancellation_reason: Option<String>,
    pub refund_amount:       Option<String>,
    pub disruption_flag:     bool,
    pub created_at:          String,
}

impl OrderItem {
    pub fn status_color(&self) -> &'static str {
        match self.status.as_str() {
            "confirmed"  => "bg-green-100 text-green-800",
            "held"       => "bg-yellow-100 text-yellow-800",
            "cancelled"  => "bg-red-100 text-red-800",
            "refunded"   => "bg-purple-100 text-purple-800",
            "completed"  => "bg-gray-100 text-gray-700",
            _            => "bg-blue-100 text-blue-800",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OrderDetail {
    pub id:              String,
    pub order_number:    String,
    pub status:          String,
    pub fare_amount:     String,
    pub disruption_flag: bool,
    pub seat_number:     Option<String>,
    pub hold_expires_at: Option<String>,
    pub created_at:      String,
    // Passenger
    pub passenger_id:    String,
    pub passenger_name:  String,
    pub phone_last4:     Option<String>,
    // Schedule / route
    pub schedule_id:     String,
    pub train_number:    String,
    pub departure_time:  String,
    pub arrival_time:    String,
    pub route_code:      String,
    pub route_name:      String,
    // Seat class
    pub seat_class_code: String,
    pub seat_class_name: String,
}

impl OrderDetail {
    pub fn status_color(&self) -> &'static str {
        match self.status.as_str() {
            "confirmed"  => "bg-green-100 text-green-800",
            "held"       => "bg-yellow-100 text-yellow-800",
            "cancelled"  => "bg-red-100 text-red-800",
            "refunded"   => "bg-purple-100 text-purple-800",
            "completed"  => "bg-gray-100 text-gray-700",
            _            => "bg-blue-100 text-blue-800",
        }
    }
    pub fn masked_phone(&self) -> String {
        match &self.phone_last4 {
            Some(l4) => format!("•••• {}", l4),
            None     => "—".into(),
        }
    }
    pub fn fmt_departure(&self) -> String { fmt_dt(&self.departure_time) }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OrderEventItem {
    pub id:           i64,
    pub order_id:     String,
    pub event_type:   String,
    pub performed_by: Option<String>,
    pub reason:       Option<String>,
    pub created_at:   String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OrderDetailResponse {
    pub order:  OrderDetail,
    pub events: Vec<OrderEventItem>,
}

// ── Request bodies ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatusBody {
    pub status:        String,
    pub delay_minutes: Option<i32>,
    pub platform:      Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CorrectInventoryBody {
    pub seat_class_id:   String,
    pub total_seats:     i32,
    pub available_seats: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatePassengerBody {
    pub full_name: String,
    pub phone:     Option<String>,
    pub email:     Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateOrderBody {
    pub passenger_id:  String,
    pub schedule_id:   String,
    pub seat_class_id: String,
    pub seat_number:   Option<String>,
    pub fare_amount:   String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CancelBody {
    pub reason:          String,
    pub disruption_flag: bool,
    pub refund_amount:   Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefundBody {
    pub amount: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeeOverrideBody {
    pub override_amount: String,
    pub reason:          String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatedOrder {
    pub id:           String,
    pub order_number: String,
}

// ── API functions — reference data ────────────────────────────────────────────

pub async fn list_routes(token: &str) -> ApiResult<Vec<RouteItem>> {
    let resp = signed_request("GET", "/api/v1/ops/routes", token, None::<&()>).await?;
    parse_json(resp).await
}

pub async fn list_seat_classes(token: &str) -> ApiResult<Vec<SeatClassItem>> {
    let resp = signed_request("GET", "/api/v1/ops/seat-classes", token, None::<&()>).await?;
    parse_json(resp).await
}

// ── API functions — schedules ─────────────────────────────────────────────────

pub async fn list_schedules(
    token:           &str,
    route_id:        Option<&str>,
    status:          Option<&str>,
    departure_after: Option<&str>,
    page:            i64,
    per_page:        i64,
) -> ApiResult<Page<ScheduleRow>> {
    let mut url = format!(
        "/api/v1/ops/schedules?page={page}&per_page={per_page}"
    );
    if let Some(r) = route_id.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&route_id={r}"));
    }
    if let Some(s) = status.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&status={s}"));
    }
    if let Some(d) = departure_after.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&departure_after={}", urlenc(d)));
    }
    let resp = signed_request("GET", &url, token, None::<&()>).await?;
    parse_json(resp).await
}

pub async fn get_schedule(token: &str, id: &str) -> ApiResult<ScheduleDetail> {
    let resp = signed_request(
        "GET", &format!("/api/v1/ops/schedules/{id}"), token, None::<&()>,
    ).await?;
    parse_json(resp).await
}

pub async fn update_schedule_status(
    token: &str, id: &str, body: &UpdateStatusBody,
) -> ApiResult<()> {
    signed_request("PATCH", &format!("/api/v1/ops/schedules/{id}/status"), token, Some(body))
        .await?;
    Ok(())
}

pub async fn correct_inventory(
    token: &str, schedule_id: &str, body: &CorrectInventoryBody,
) -> ApiResult<()> {
    signed_request(
        "POST",
        &format!("/api/v1/ops/schedules/{schedule_id}/inventory"),
        token,
        Some(body),
    ).await?;
    Ok(())
}

// ── API functions — passengers ────────────────────────────────────────────────

pub async fn search_passengers(
    token: &str, q: &str, page: i64,
) -> ApiResult<Page<PassengerItem>> {
    let url = format!(
        "/api/v1/ops/passengers?q={}&page={page}&per_page=20", urlenc(q)
    );
    let resp = signed_request("GET", &url, token, None::<&()>).await?;
    parse_json(resp).await
}

pub async fn create_passenger(
    token: &str, body: &CreatePassengerBody,
) -> ApiResult<PassengerItem> {
    let resp = signed_request("POST", "/api/v1/ops/passengers", token, Some(body)).await?;
    parse_json(resp).await
}

pub async fn request_pii_purge(token: &str, passenger_id: &str) -> ApiResult<()> {
    signed_request(
        "POST",
        &format!("/api/v1/ops/passengers/{passenger_id}/pii-purge"),
        token,
        None::<&()>,
    ).await?;
    Ok(())
}

// ── API functions — orders ────────────────────────────────────────────────────

pub async fn list_orders(
    token:        &str,
    passenger_id: Option<&str>,
    schedule_id:  Option<&str>,
    status:       Option<&str>,
    page:         i64,
) -> ApiResult<Page<OrderItem>> {
    let mut url = format!("/api/v1/ops/orders?page={page}&per_page=20");
    if let Some(p) = passenger_id.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&passenger_id={p}"));
    }
    if let Some(s) = schedule_id.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&schedule_id={s}"));
    }
    if let Some(s) = status.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&status={s}"));
    }
    let resp = signed_request("GET", &url, token, None::<&()>).await?;
    parse_json(resp).await
}

pub async fn find_order_by_number(token: &str, num: &str) -> ApiResult<OrderItem> {
    let resp = signed_request(
        "GET",
        &format!("/api/v1/ops/orders/by-number/{}", urlenc(num)),
        token,
        None::<&()>,
    ).await?;
    parse_json(resp).await
}

pub async fn get_order(token: &str, id: &str) -> ApiResult<OrderDetailResponse> {
    let resp = signed_request(
        "GET", &format!("/api/v1/ops/orders/{id}"), token, None::<&()>,
    ).await?;
    parse_json(resp).await
}

pub async fn create_order(token: &str, body: &CreateOrderBody) -> ApiResult<CreatedOrder> {
    let resp = signed_request("POST", "/api/v1/ops/orders", token, Some(body)).await?;
    parse_json(resp).await
}

pub async fn confirm_order(token: &str, id: &str) -> ApiResult<()> {
    signed_request("POST", &format!("/api/v1/ops/orders/{id}/confirm"), token, None::<&()>)
        .await?;
    Ok(())
}

pub async fn cancel_order(token: &str, id: &str, body: &CancelBody) -> ApiResult<()> {
    signed_request("POST", &format!("/api/v1/ops/orders/{id}/cancel"), token, Some(body))
        .await?;
    Ok(())
}

pub async fn process_refund(token: &str, id: &str, body: &RefundBody) -> ApiResult<()> {
    signed_request("POST", &format!("/api/v1/ops/orders/{id}/refund"), token, Some(body))
        .await?;
    Ok(())
}

pub async fn apply_fee_override(token: &str, id: &str, body: &FeeOverrideBody) -> ApiResult<()> {
    signed_request("POST", &format!("/api/v1/ops/orders/{id}/fee-override"), token, Some(body))
        .await?;
    Ok(())
}

pub async fn flag_disruption(token: &str, id: &str) -> ApiResult<()> {
    signed_request("POST", &format!("/api/v1/ops/orders/{id}/disruption"), token, None::<&()>)
        .await?;
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn urlenc(s: &str) -> String {
    js_sys::encode_uri_component(s)
        .as_string()
        .unwrap_or_else(|| s.to_owned())
}

/// Parse an ISO-8601 date-time string into a short human label.
/// e.g. `"2024-06-15T09:30:00Z"` → `"Jun 15 09:30"`
pub fn fmt_dt(iso: &str) -> String {
    // Expect at minimum "YYYY-MM-DDTHH:MM"
    if iso.len() < 16 { return iso.to_owned(); }
    let date = &iso[..10];   // "YYYY-MM-DD"
    let time = &iso[11..16]; // "HH:MM"
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() < 3 { return iso.to_owned(); }
    let month = match parts[1].parse::<u32>().unwrap_or(0) {
        1  => "Jan", 2  => "Feb", 3  => "Mar", 4  => "Apr",
        5  => "May", 6  => "Jun", 7  => "Jul", 8  => "Aug",
        9  => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _  => "???",
    };
    format!("{} {} {}", month, parts[2].trim_start_matches('0'), time)
}
