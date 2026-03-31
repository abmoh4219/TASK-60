//! Operations console — passenger & order handlers.
//!
//! Route summary (all under /api/v1/ops):
//!
//!   GET    /passengers                   search_passengers    (ViewOrders)
//!   POST   /passengers                   create_passenger     (ManageOrders)
//!   POST   /passengers/{id}/pii-purge    request_pii_purge    (ManageOrders)
//!
//!   GET    /orders                        list_orders          (ViewOrders)
//!   GET    /orders/by-number/{num}        find_by_number       (ViewOrders)
//!   GET    /orders/{id}                   get_order            (ViewOrders)
//!   POST   /orders                        create_order         (ManageOrders)
//!   POST   /orders/{id}/hold              hold_order           (ManageOrders)
//!   POST   /orders/{id}/confirm           confirm_order        (ManageOrders)
//!   POST   /orders/{id}/cancel            cancel_order         (ManageOrders)
//!   POST   /orders/{id}/refund            process_refund       (ProcessRefunds)
//!   POST   /orders/{id}/fee-override      apply_fee_override   (OverrideFees)
//!   POST   /orders/{id}/disruption        flag_disruption      (ManageOrders)
//!   GET    /orders/{id}/events            list_order_events    (ViewOrders)

use actix_web::{web, HttpResponse};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::rbac::{
    RequireManageOrders, RequireOverrideFees, RequireProcessRefunds, RequireViewOrders,
};
use crate::rules::RulesEngine;
use crate::config::AppConfig;
use crate::crypto;
use crate::db::audit::write_audit;
use crate::domain::orders::{
    models::{CancelOrder, FeeOverride, ListOrdersParams, NewOrder, NewOrderEvent, NewPassenger},
    repo::{OrderEventRepo, OrderRepo, PassengerRepo},
};
use crate::error::{AppError, AppResult};
use shared::PaginationParams;

// ── Query / body types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PassengerSearchQuery {
    #[serde(default)]
    pub q:        String,
    #[serde(default = "default_page")]
    pub page:     i64,
    #[serde(default = "default_per_page")]
    pub per_page: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreatePassengerBody {
    pub full_name: String,
    /// Optional raw phone number (encrypted before storage).
    pub phone:     Option<String>,
    /// Optional raw email address (encrypted before storage).
    pub email:     Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OrderListQuery {
    pub passenger_id: Option<Uuid>,
    pub schedule_id:  Option<Uuid>,
    pub status:       Option<String>,
    #[serde(default = "default_page")]
    pub page:         i64,
    #[serde(default = "default_per_page")]
    pub per_page:     i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateOrderBody {
    pub passenger_id:  Uuid,
    pub schedule_id:   Uuid,
    pub seat_class_id: Uuid,
    pub seat_number:   Option<String>,
    pub fare_amount:   Decimal,
}

#[derive(Debug, Deserialize)]
pub struct CancelOrderBody {
    pub reason:          String,
    pub disruption_flag: bool,
    pub refund_amount:   Option<Decimal>,
}

#[derive(Debug, Deserialize)]
pub struct RefundBody {
    pub amount: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct FeeOverrideBody {
    pub override_amount: Decimal,
    pub reason:          String,
}

fn default_page() -> i64 { 1 }
fn default_per_page() -> i64 { 20 }

// ── Passenger handlers ────────────────────────────────────────────────────────

pub async fn search_passengers(
    pool:  web::Data<PgPool>,
    query: web::Query<PassengerSearchQuery>,
    _auth: RequireViewOrders,
) -> AppResult<HttpResponse> {
    let page = PassengerRepo::new(&pool)
        .search(&query.q, query.page, query.per_page)
        .await?;
    Ok(HttpResponse::Ok().json(page))
}

pub async fn create_passenger(
    pool:   web::Data<PgPool>,
    body:   web::Json<CreatePassengerBody>,
    config: web::Data<AppConfig>,
    auth:   RequireManageOrders,
) -> AppResult<HttpResponse> {
    let full_name = body.full_name.trim().to_owned();
    if full_name.is_empty() {
        return Err(AppError::Validation("full_name is required".into()));
    }

    let aes_key = crypto::derive_aes_key(&config.security.session_secret);

    let (phone_encrypted, phone_last4) = if let Some(ref raw) = body.phone {
        let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
        let last4 = if digits.len() >= 4 {
            Some(digits[digits.len() - 4..].to_owned())
        } else {
            None
        };
        let enc = crypto::encrypt_pii(raw, &aes_key)
            .map_err(|e| AppError::Internal(e))?;
        (Some(enc), last4)
    } else {
        (None, None)
    };

    let email_encrypted = if let Some(ref raw) = body.email {
        Some(
            crypto::encrypt_pii(raw, &aes_key)
                .map_err(|e| AppError::Internal(e))?,
        )
    } else {
        None
    };

    let new = NewPassenger {
        full_name: full_name.clone(),
        phone_encrypted,
        phone_last4,
        email_encrypted,
    };
    let id = PassengerRepo::new(&pool).create(&new).await?;

    write_audit(
        &pool,
        "passenger_created",
        "passengers",
        Some(id),
        Some(auth.id),
        "create_passenger",
        None,
        Some(json!({ "full_name": &full_name })),
        None,
    ).await;

    let passenger = PassengerRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Passenger".into()))?;

    Ok(HttpResponse::Created().json(passenger))
}

pub async fn request_pii_purge(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    auth:  RequireManageOrders,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    PassengerRepo::new(&pool).find_by_id(id).await?
        .ok_or_else(|| AppError::NotFound("Passenger".into()))?;

    PassengerRepo::new(&pool).request_pii_purge(id).await?;

    write_audit(
        &pool,
        "pii_purge_requested",
        "passengers",
        Some(id),
        Some(auth.id),
        "request_pii_purge",
        None,
        None,
        None,
    ).await;

    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

// ── Order handlers ────────────────────────────────────────────────────────────

pub async fn list_orders(
    pool:  web::Data<PgPool>,
    query: web::Query<OrderListQuery>,
    _auth: RequireViewOrders,
) -> AppResult<HttpResponse> {
    let params = ListOrdersParams {
        passenger_id: query.passenger_id,
        schedule_id:  query.schedule_id,
        status:       query.status.clone(),
        pagination:   PaginationParams {
            page:     Some(query.page),
            per_page: Some(query.per_page),
        },
    };
    let page = OrderRepo::new(&pool).list(&params).await?;
    Ok(HttpResponse::Ok().json(page))
}

pub async fn find_by_number(
    pool:  web::Data<PgPool>,
    path:  web::Path<String>,
    _auth: RequireViewOrders,
) -> AppResult<HttpResponse> {
    let order = OrderRepo::new(&pool)
        .find_by_number(&path.into_inner())
        .await?
        .ok_or_else(|| AppError::NotFound("Order".into()))?;
    Ok(HttpResponse::Ok().json(order))
}

pub async fn get_order(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    _auth: RequireViewOrders,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    let detail = OrderRepo::new(&pool)
        .find_detail(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Order".into()))?;
    let events = OrderEventRepo::new(&pool).list_for_order(id).await?;
    Ok(HttpResponse::Ok().json(json!({ "order": detail, "events": events })))
}

pub async fn create_order(
    pool:  web::Data<PgPool>,
    body:  web::Json<CreateOrderBody>,
    auth:  RequireManageOrders,
) -> AppResult<HttpResponse> {
    let new = NewOrder {
        passenger_id:  body.passenger_id,
        schedule_id:   body.schedule_id,
        seat_class_id: body.seat_class_id,
        seat_number:   body.seat_number.clone(),
        fare_amount:   body.fare_amount,
        created_by:    Some(auth.id),
    };
    let (id, order_number) = OrderRepo::new(&pool).create(&new).await?;

    // Record creation event.
    OrderEventRepo::new(&pool).insert(&NewOrderEvent {
        order_id:     id,
        event_type:   "created".into(),
        performed_by: Some(auth.id),
        reason:       None,
        data:         Some(json!({ "fare_amount": body.fare_amount })),
    }).await?;

    write_audit(
        &pool,
        "order_created",
        "orders",
        Some(id),
        Some(auth.id),
        "create_order",
        None,
        Some(json!({
            "order_number": &order_number,
            "passenger_id": body.passenger_id,
            "schedule_id":  body.schedule_id,
        })),
        None,
    ).await;

    Ok(HttpResponse::Created().json(json!({ "id": id, "order_number": order_number })))
}

/// Place an order in the held state.
///
/// The hold-expiry timestamp is computed from the live `order_hold_ttl_minutes`
/// rule (default 15 min) so that ops can tune the window without redeploying.
pub async fn hold_order(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    auth:  RequireManageOrders,
) -> AppResult<HttpResponse> {
    let id    = path.into_inner();
    let order = OrderRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Order".into()))?;

    if order.status != "pending" {
        return Err(AppError::Validation(format!(
            "Only pending orders can be held (current status: '{}')", order.status
        )));
    }

    let expires_at = RulesEngine::new(&pool).hold_expiry().await;

    OrderRepo::new(&pool).hold(id, expires_at).await?;
    OrderEventRepo::new(&pool).insert(&NewOrderEvent {
        order_id:     id,
        event_type:   "held".into(),
        performed_by: Some(auth.id),
        reason:       None,
        data:         Some(json!({ "hold_expires_at": expires_at })),
    }).await?;

    write_audit(
        &pool, "order_held", "orders", Some(id), Some(auth.id),
        "hold_order", None, Some(json!({ "hold_expires_at": expires_at })), None,
    ).await;

    Ok(HttpResponse::Ok().json(json!({ "ok": true, "hold_expires_at": expires_at })))
}

pub async fn confirm_order(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    auth:  RequireManageOrders,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    let order = OrderRepo::new(&pool).find_by_id(id).await?
        .ok_or_else(|| AppError::NotFound("Order".into()))?;

    if !matches!(order.status.as_str(), "pending" | "held") {
        return Err(AppError::Validation(format!(
            "Order cannot be confirmed in status '{}'", order.status
        )));
    }

    OrderRepo::new(&pool).confirm(id).await?;
    OrderEventRepo::new(&pool).insert(&NewOrderEvent {
        order_id: id, event_type: "confirmed".into(),
        performed_by: Some(auth.id), reason: None, data: None,
    }).await?;

    write_audit(&pool, "order_confirmed", "orders", Some(id), Some(auth.id),
        "confirm_order", None, None, None).await;

    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn cancel_order(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<CancelOrderBody>,
    auth:  RequireManageOrders,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    OrderRepo::new(&pool).find_by_id(id).await?
        .ok_or_else(|| AppError::NotFound("Order".into()))?;

    if body.reason.trim().is_empty() {
        return Err(AppError::Validation("Cancellation reason is required".into()));
    }

    OrderRepo::new(&pool)
        .cancel(id, &body.reason, body.disruption_flag, body.refund_amount)
        .await?;

    OrderEventRepo::new(&pool).insert(&NewOrderEvent {
        order_id:     id,
        event_type:   "cancelled".into(),
        performed_by: Some(auth.id),
        reason:       Some(body.reason.clone()),
        data:         Some(json!({
            "disruption_flag": body.disruption_flag,
            "refund_amount":   body.refund_amount,
        })),
    }).await?;

    write_audit(&pool, "order_cancelled", "orders", Some(id), Some(auth.id),
        "cancel_order",
        None,
        Some(json!({ "reason": &body.reason, "disruption_flag": body.disruption_flag })),
        None,
    ).await;

    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

/// Process a refund for a cancelled order.
///
/// The allowed refund amount is computed by `RulesEngine::evaluate_refund`,
/// which applies live-configurable tiered logic:
///
///   - > `refund_full_hours` before departure  → FullMinusFee
///   - 2–24 h before departure                → HalfFare
///   - < `refund_partial_hours` + disruption  → ServiceDisruption (full fare)
///   - < `refund_partial_hours`, no disruption → blocked (HTTP 422)
///
/// The caller may submit any amount ≤ the computed maximum; the maximum
/// is always returned in the response for UI display.
pub async fn process_refund(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<RefundBody>,
    auth:  RequireProcessRefunds,
) -> AppResult<HttpResponse> {
    let id    = path.into_inner();
    // Use find_detail to get departure_time and disruption_flag in one query.
    let order = OrderRepo::new(&pool)
        .find_detail(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Order".into()))?;

    if order.status != "cancelled" {
        return Err(AppError::Validation(
            "Refunds can only be processed for cancelled orders".into(),
        ));
    }
    if body.amount <= Decimal::ZERO {
        return Err(AppError::Validation("Refund amount must be positive".into()));
    }

    // Policy evaluation — may return RefundBlocked.
    let decision = RulesEngine::new(&pool)
        .evaluate_refund(order.departure_time, order.fare_amount, order.disruption_flag)
        .await?;

    if body.amount > decision.max_amount {
        return Err(AppError::RefundBlocked(format!(
            "Requested ${} exceeds the ${} maximum for outcome {:?} — {}",
            body.amount, decision.max_amount, decision.outcome, decision.reason,
        )));
    }

    OrderRepo::new(&pool).set_refunded(id, body.amount).await?;

    let outcome_label = format!("{:?}", decision.outcome);
    OrderEventRepo::new(&pool).insert(&NewOrderEvent {
        order_id:     id,
        event_type:   "refunded".into(),
        performed_by: Some(auth.id),
        reason:       Some(decision.reason.clone()),
        data:         Some(json!({
            "amount":      body.amount,
            "max_amount":  decision.max_amount,
            "outcome":     &outcome_label,
        })),
    }).await?;

    write_audit(
        &pool, "order_refunded", "orders", Some(id), Some(auth.id),
        "process_refund",
        None,
        Some(json!({
            "amount":      body.amount,
            "max_amount":  decision.max_amount,
            "outcome":     &outcome_label,
            "reason":      &decision.reason,
        })),
        None,
    ).await;

    Ok(HttpResponse::Ok().json(json!({
        "ok":         true,
        "outcome":    outcome_label,
        "max_amount": decision.max_amount,
        "reason":     decision.reason,
    })))
}

pub async fn apply_fee_override(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<FeeOverrideBody>,
    auth:  RequireOverrideFees,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    OrderRepo::new(&pool).find_by_id(id).await?
        .ok_or_else(|| AppError::NotFound("Order".into()))?;

    if body.reason.trim().is_empty() {
        return Err(AppError::Validation("Fee override reason is required".into()));
    }
    if body.override_amount < Decimal::ZERO {
        return Err(AppError::Validation("Override amount must be ≥ 0".into()));
    }

    OrderRepo::new(&pool)
        .apply_fee_override(id, body.override_amount, &body.reason)
        .await?;

    OrderEventRepo::new(&pool).insert(&NewOrderEvent {
        order_id:     id,
        event_type:   "fee_override".into(),
        performed_by: Some(auth.id),
        reason:       Some(body.reason.clone()),
        data:         Some(json!({ "override_amount": body.override_amount })),
    }).await?;

    write_audit(&pool, "fee_override_applied", "orders", Some(id), Some(auth.id),
        "fee_override",
        None,
        Some(json!({ "override_amount": body.override_amount, "reason": &body.reason })),
        None,
    ).await;

    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn flag_disruption(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    auth:  RequireManageOrders,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    OrderRepo::new(&pool).find_by_id(id).await?
        .ok_or_else(|| AppError::NotFound("Order".into()))?;

    OrderRepo::new(&pool).set_disruption(id).await?;
    OrderEventRepo::new(&pool).insert(&NewOrderEvent {
        order_id: id, event_type: "disruption_flagged".into(),
        performed_by: Some(auth.id), reason: None, data: None,
    }).await?;

    write_audit(&pool, "disruption_flagged", "orders", Some(id), Some(auth.id),
        "flag_disruption", None, None, None).await;

    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn list_order_events(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    _auth: RequireViewOrders,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    let events = OrderEventRepo::new(&pool).list_for_order(id).await?;
    Ok(HttpResponse::Ok().json(events))
}
