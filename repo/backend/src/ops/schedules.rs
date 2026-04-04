//! Operations console — schedule & inventory handlers.
//!
//! Route summary (all under /api/v1/ops):
//!
//!   GET    /routes                       list_routes          (ViewSchedules)
//!   GET    /seat-classes                 list_seat_classes    (ViewSchedules)
//!   GET    /schedules                    list_schedules       (ViewSchedules)
//!   GET    /schedules/{id}               get_schedule         (ViewSchedules)
//!   PATCH  /schedules/{id}/status        update_status        (ManageSchedules)
//!   POST   /schedules/{id}/inventory     correct_inventory    (ManageSchedules)

use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::rbac::{RequireManageSchedules, RequireViewSchedules};
use crate::db::audit::write_audit_required;
use crate::domain::rail::{
    models::{ListSchedulesParams, UpdateScheduleStatus},
    repo::{InventoryRepo, RouteRepo, ScheduleRepo, SeatClassRepo},
};
use crate::error::{AppError, AppResult};
use shared::{PaginatedResponse, PaginationParams};

// ── Query / body types ────────────────────────────────────────────────────────

/// Flat query params for listing schedules (avoids serde_urlencoded flatten issues).
#[derive(Debug, Deserialize)]
pub struct ScheduleListQuery {
    pub route_id:        Option<Uuid>,
    pub status:          Option<String>,
    pub departure_after: Option<DateTime<Utc>>,
    #[serde(default = "default_page")]
    pub page:            i64,
    #[serde(default = "default_per_page")]
    pub per_page:        i64,
}

#[derive(Debug, Deserialize)]
pub struct CorrectInventoryBody {
    pub seat_class_id:   Uuid,
    pub total_seats:     i32,
    pub available_seats: i32,
}

fn default_page() -> i64 { 1 }
fn default_per_page() -> i64 { 20 }

// ── Reference-data handlers ───────────────────────────────────────────────────

pub async fn list_routes(
    pool:  web::Data<PgPool>,
    _auth: RequireViewSchedules,
) -> AppResult<HttpResponse> {
    let routes = RouteRepo::new(&pool).list_all().await?;
    Ok(HttpResponse::Ok().json(routes))
}

pub async fn list_seat_classes(
    pool:  web::Data<PgPool>,
    _auth: RequireViewSchedules,
) -> AppResult<HttpResponse> {
    let classes = SeatClassRepo::new(&pool).list().await?;
    Ok(HttpResponse::Ok().json(classes))
}

// ── Schedule handlers ─────────────────────────────────────────────────────────

pub async fn list_schedules(
    pool:  web::Data<PgPool>,
    query: web::Query<ScheduleListQuery>,
    _auth: RequireViewSchedules,
) -> AppResult<HttpResponse> {
    let params = ListSchedulesParams {
        route_id:        query.route_id,
        status:          query.status.clone(),
        departure_after: query.departure_after,
        pagination:      PaginationParams {
            page:     Some(query.page),
            per_page: Some(query.per_page),
        },
    };
    let page = ScheduleRepo::new(&pool).list(&params).await?;
    Ok(HttpResponse::Ok().json(page))
}

pub async fn get_schedule(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    _auth: RequireViewSchedules,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();

    // JOIN with route so the frontend gets route details in one call.
    let schedule: Option<crate::domain::rail::models::ScheduleWithRoute> = sqlx::query_as(
        "SELECT s.id, s.route_id,
                r.route_code, r.name AS route_name,
                r.origin_city, r.destination_city,
                s.train_number, s.departure_time, s.arrival_time,
                s.status, s.delay_minutes, s.platform, s.updated_at
         FROM schedules s
         JOIN routes r ON r.id = s.route_id
         WHERE s.id = $1",
    )
    .bind(id)
    .fetch_optional(pool.as_ref())
    .await
    .map_err(AppError::Database)?;

    let schedule = schedule.ok_or_else(|| AppError::NotFound("Schedule".into()))?;
    let inventory = InventoryRepo::new(&pool).latest_for_schedule(id).await?;

    Ok(HttpResponse::Ok().json(json!({
        "schedule":  schedule,
        "inventory": inventory,
    })))
}

pub async fn update_schedule_status(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<UpdateScheduleStatus>,
    auth:  RequireManageSchedules,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    let valid = ["scheduled", "delayed", "cancelled", "completed"];
    if !valid.contains(&body.status.as_str()) {
        return Err(AppError::Validation(format!(
            "status must be one of: {}", valid.join(", ")
        )));
    }
    let delay = body.delay_minutes.unwrap_or(0);
    ScheduleRepo::new(&pool)
        .update_status(id, &body.status, delay, body.platform.as_deref())
        .await?;

    write_audit_required(
        &pool,
        "schedule_status_updated",
        "schedules",
        Some(id),
        Some(auth.id),
        "update_status",
        None,
        Some(json!({ "status": &body.status, "delay_minutes": delay, "platform": &body.platform })),
        None,
    ).await?;

    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn correct_inventory(
    pool:  web::Data<PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<CorrectInventoryBody>,
    auth:  RequireManageSchedules,
) -> AppResult<HttpResponse> {
    let schedule_id = path.into_inner();

    if body.available_seats < 0 || body.total_seats < 1 {
        return Err(AppError::Validation(
            "total_seats must be ≥ 1 and available_seats must be ≥ 0".into(),
        ));
    }
    if body.available_seats > body.total_seats {
        return Err(AppError::Validation(
            "available_seats cannot exceed total_seats".into(),
        ));
    }

    let snap_id = InventoryRepo::new(&pool)
        .insert_snapshot(
            schedule_id,
            body.seat_class_id,
            body.total_seats,
            body.available_seats,
        )
        .await?;

    write_audit_required(
        &pool,
        "inventory_corrected",
        "inventory_snapshots",
        Some(snap_id),
        Some(auth.id),
        "correct_inventory",
        None,
        Some(json!({
            "schedule_id":      schedule_id,
            "seat_class_id":    body.seat_class_id,
            "total_seats":      body.total_seats,
            "available_seats":  body.available_seats,
        })),
        None,
    ).await?;

    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}
