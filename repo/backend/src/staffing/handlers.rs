//! Staffing console API handlers.
//!
//! Route map (all under `/api/v1/staffing`):
//!
//! ```text
//! GET    /contractors                  — list contractors          (ViewContractors)
//! GET    /contractors/{id}             — get contractor + tags     (ViewContractors)
//! POST   /contractors                  — create contractor         (ManageContractors)
//! PATCH  /contractors/{id}/active      — toggle active             (ManageContractors)
//! POST   /contractors/{id}/availability— add availability window   (ManageContractors)
//! GET    /shifts                       — list shifts               (ViewContractors)
//! GET    /shifts/{id}                  — shift + assignments       (ViewContractors)
//! POST   /shifts                       — create shift              (ManageShifts)
//! PATCH  /shifts/{id}/status           — update shift status       (ManageShifts)
//! GET    /shifts/{id}/candidates       — ranked match candidates   (ManageShifts)
//! POST   /shifts/{id}/propose          — propose assignment        (ManageShifts)
//! PATCH  /assignments/{id}/respond     — accept / reject           (ManageShifts)
//! GET    /subscriptions                — my subscriptions          (ViewContractors)
//! POST   /subscriptions                — subscribe                 (ViewContractors)
//! DELETE /subscriptions                — unsubscribe               (ViewContractors)
//! ```

use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use shared::PaginationParams;

use crate::auth::rbac::{
    RequireManageContractors, RequireManageShifts, RequireViewContractors,
};
use crate::db::audit::write_audit;
use crate::domain::staffing::{
    models::{CreateContractor, CreateShift, ListShiftsParams},
    repo::{AssignmentRepo, ContractorRepo, ShiftRepo, SubscriptionRepo},
};
use crate::error::{AppError, AppResult};

use super::MatchEngine;

// ── Flat query structs ────────────────────────────────────────────────────────
// `#[serde(flatten)]` is incompatible with `web::Query`, so each handler defines
// its own flat struct and manually builds the domain type.

#[derive(Debug, Deserialize)]
pub struct ContractorListQuery {
    pub region:    Option<String>,
    pub is_active: Option<bool>,
    pub page:      Option<i64>,
    pub per_page:  Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ShiftListQuery {
    pub status:   Option<String>,
    pub region:   Option<String>,
    pub role:     Option<String>,
    pub page:     Option<i64>,
    pub per_page: Option<i64>,
}

// ── Request bodies ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateContractorBody {
    pub full_name:   String,
    pub phone_last4: Option<String>,
    pub region:      Option<String>,
    pub tags:        Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetActiveBody {
    pub active: bool,
}

#[derive(Debug, Deserialize)]
pub struct AddAvailabilityBody {
    pub available_from: DateTime<Utc>,
    pub available_to:   DateTime<Utc>,
    pub notes:          Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateShiftBody {
    pub schedule_id:   Option<Uuid>,
    pub role:          String,
    pub region:        Option<String>,
    pub required_tags: Vec<String>,
    pub shift_start:   DateTime<Utc>,
    pub shift_end:     DateTime<Utc>,
    pub is_critical:   bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateShiftStatusBody {
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct ProposeBody {
    pub contractor_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct RespondBody {
    pub status: String, // "accepted" | "rejected"
}

#[derive(Debug, Deserialize)]
pub struct SubscribeBody {
    pub subscriber_type: String, // "dispatcher" | "contractor"
    pub target_type:     String, // "shift" | "route"
    pub target_id:       Uuid,
}

// ── Contractor handlers ────────────────────────────────────────────────────────

pub async fn list_contractors(
    pool:  web::Data<sqlx::PgPool>,
    query: web::Query<ContractorListQuery>,
    _auth: RequireViewContractors,
) -> AppResult<HttpResponse> {
    let q        = query.into_inner();
    let page     = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let result   = ContractorRepo::new(&pool)
        .list(q.region.as_deref(), q.is_active, page, per_page)
        .await?;
    Ok(HttpResponse::Ok().json(result))
}

pub async fn get_contractor(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    _auth: RequireViewContractors,
) -> AppResult<HttpResponse> {
    let id   = path.into_inner();
    let repo = ContractorRepo::new(&pool);
    let contractor = repo
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Contractor {id}")))?;
    let tags = repo.tags_for(id).await?;
    Ok(HttpResponse::Ok().json(json!({ "contractor": contractor, "tags": tags })))
}

pub async fn create_contractor(
    pool:  web::Data<sqlx::PgPool>,
    body:  web::Json<CreateContractorBody>,
    auth:  RequireManageContractors,
) -> AppResult<HttpResponse> {
    let body = body.into_inner();
    if body.full_name.trim().is_empty() {
        return Err(AppError::Validation("full_name is required".into()));
    }
    let cmd = CreateContractor {
        full_name:       body.full_name.trim().to_owned(),
        phone_encrypted: None,   // full PII encrypted at the edge layer
        phone_last4:     body.phone_last4,
        email_encrypted: None,
        region:          body.region,
        tags:            body.tags,
    };
    let id = ContractorRepo::new(&pool).create(&cmd).await?;
    write_audit(
        &pool,
        "contractor_created", "contractor", Some(id),
        Some(auth.id), "create",
        None, Some(json!({ "full_name": &cmd.full_name })),
        None,
    ).await;
    Ok(HttpResponse::Created().json(json!({ "id": id })))
}

pub async fn set_contractor_active(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<SetActiveBody>,
    auth:  RequireManageContractors,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    ContractorRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Contractor {id}")))?;
    ContractorRepo::new(&pool).set_active(id, body.active).await?;
    write_audit(
        &pool,
        "contractor_active_changed", "contractor", Some(id),
        Some(auth.id), if body.active { "activate" } else { "deactivate" },
        None, Some(json!({ "active": body.active })),
        None,
    ).await;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn add_availability(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<AddAvailabilityBody>,
    auth:  RequireManageContractors,
) -> AppResult<HttpResponse> {
    let contractor_id = path.into_inner();
    ContractorRepo::new(&pool)
        .find_by_id(contractor_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Contractor {contractor_id}")))?;
    if body.available_to <= body.available_from {
        return Err(AppError::Validation(
            "available_to must be after available_from".into(),
        ));
    }
    let avail_id = ContractorRepo::new(&pool)
        .add_availability(
            contractor_id,
            body.available_from,
            body.available_to,
            body.notes.as_deref(),
        )
        .await?;
    write_audit(
        &pool,
        "availability_added", "contractor", Some(contractor_id),
        Some(auth.id), "add_availability",
        None,
        Some(json!({
            "avail_id": avail_id,
            "from": body.available_from,
            "to":   body.available_to,
        })),
        None,
    ).await;
    Ok(HttpResponse::Created().json(json!({ "id": avail_id })))
}

// ── Shift handlers ─────────────────────────────────────────────────────────────

pub async fn list_shifts(
    pool:  web::Data<sqlx::PgPool>,
    query: web::Query<ShiftListQuery>,
    _auth: RequireViewContractors,
) -> AppResult<HttpResponse> {
    let q      = query.into_inner();
    let params = ListShiftsParams {
        status:     q.status,
        region:     q.region,
        role:       q.role,
        pagination: PaginationParams {
            page:     q.page,
            per_page: q.per_page,
        },
    };
    let result = ShiftRepo::new(&pool).list(&params).await?;
    Ok(HttpResponse::Ok().json(result))
}

pub async fn get_shift(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    _auth: RequireViewContractors,
) -> AppResult<HttpResponse> {
    let id    = path.into_inner();
    let shift = ShiftRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Shift {id}")))?;
    let assignments = AssignmentRepo::new(&pool).list_for_shift(id).await?;
    Ok(HttpResponse::Ok().json(json!({
        "shift":       shift,
        "assignments": assignments,
    })))
}

pub async fn create_shift(
    pool:  web::Data<sqlx::PgPool>,
    body:  web::Json<CreateShiftBody>,
    auth:  RequireManageShifts,
) -> AppResult<HttpResponse> {
    let body = body.into_inner();
    if !matches!(body.role.as_str(), "conductor" | "driver") {
        return Err(AppError::Validation(
            "role must be 'conductor' or 'driver'".into(),
        ));
    }
    if body.shift_end <= body.shift_start {
        return Err(AppError::Validation(
            "shift_end must be after shift_start".into(),
        ));
    }
    let cmd = CreateShift {
        schedule_id:   body.schedule_id,
        role:          body.role.clone(),
        region:        body.region.clone(),
        required_tags: body.required_tags.clone(),
        shift_start:   body.shift_start,
        shift_end:     body.shift_end,
        is_critical:   body.is_critical,
        created_by:    Some(auth.id),
    };
    let id = ShiftRepo::new(&pool).create(&cmd).await?;
    write_audit(
        &pool,
        "shift_created", "shift", Some(id),
        Some(auth.id), "create",
        None,
        Some(json!({
            "role":      &cmd.role,
            "region":    &cmd.region,
            "critical":  body.is_critical,
        })),
        None,
    ).await;
    Ok(HttpResponse::Created().json(json!({ "id": id })))
}

pub async fn update_shift_status(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<UpdateShiftStatusBody>,
    auth:  RequireManageShifts,
) -> AppResult<HttpResponse> {
    let id    = path.into_inner();
    let shift = ShiftRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Shift {id}")))?;
    if !matches!(
        body.status.as_str(),
        "open" | "assigned" | "completed" | "cancelled"
    ) {
        return Err(AppError::Validation(
            "status must be open | assigned | completed | cancelled".into(),
        ));
    }
    let old_status = shift.status.clone();
    ShiftRepo::new(&pool).update_status(id, &body.status).await?;
    write_audit(
        &pool,
        "shift_status_changed", "shift", Some(id),
        Some(auth.id), "update_status",
        Some(json!({ "status": old_status })),
        Some(json!({ "status": &body.status })),
        None,
    ).await;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn get_candidates(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    _auth: RequireManageShifts,
) -> AppResult<HttpResponse> {
    let id    = path.into_inner();
    let shift = ShiftRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Shift {id}")))?;
    let candidates = MatchEngine::new(&pool).rank_candidates(&shift).await?;
    Ok(HttpResponse::Ok().json(json!({ "candidates": candidates })))
}

pub async fn propose_assignment(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<ProposeBody>,
    auth:  RequireManageShifts,
) -> AppResult<HttpResponse> {
    let shift_id      = path.into_inner();
    let contractor_id = body.contractor_id;

    let shift = ShiftRepo::new(&pool)
        .find_by_id(shift_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Shift {shift_id}")))?;

    let contractor = ContractorRepo::new(&pool)
        .find_by_id(contractor_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Contractor {contractor_id}")))?;

    // Score the specific contractor so we can store it with the assignment.
    let tags      = ContractorRepo::new(&pool).tags_for(contractor_id).await?;
    let candidate = MatchEngine::score(&contractor, &tags, &shift);
    let reasons   = serde_json::to_value(&candidate.reasons).ok();

    let assignment_id = AssignmentRepo::new(&pool)
        .propose(
            shift_id,
            contractor_id,
            Some(candidate.score),
            reasons,
            Some(auth.id),
        )
        .await?;

    write_audit(
        &pool,
        "assignment_proposed", "shift_assignment", Some(assignment_id),
        Some(auth.id), "propose",
        None,
        Some(json!({
            "shift_id":      shift_id,
            "contractor_id": contractor_id,
            "score":         candidate.score.to_string(),
        })),
        None,
    ).await;

    Ok(HttpResponse::Created().json(json!({
        "id":    assignment_id,
        "score": candidate.score,
    })))
}

pub async fn respond_assignment(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<RespondBody>,
    auth:  RequireManageShifts,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    if !matches!(body.status.as_str(), "accepted" | "rejected") {
        return Err(AppError::Validation(
            "status must be 'accepted' or 'rejected'".into(),
        ));
    }
    let assignment = AssignmentRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Assignment {id}")))?;
    if assignment.status != "proposed" {
        return Err(AppError::Validation(
            "Only proposed assignments can be accepted or rejected".into(),
        ));
    }

    AssignmentRepo::new(&pool).respond(id, &body.status).await?;

    // Accepting an assignment marks the shift as assigned.
    if body.status == "accepted" {
        ShiftRepo::new(&pool)
            .update_status(assignment.shift_id, "assigned")
            .await?;
    }

    write_audit(
        &pool,
        "assignment_responded", "shift_assignment", Some(id),
        Some(auth.id), &body.status,
        Some(json!({ "status": "proposed" })),
        Some(json!({ "status": &body.status })),
        None,
    ).await;

    Ok(HttpResponse::Ok().json(json!({ "ok": true, "status": &body.status })))
}

// ── Subscription handlers ──────────────────────────────────────────────────────

pub async fn list_subscriptions(
    pool: web::Data<sqlx::PgPool>,
    auth: RequireViewContractors,
) -> AppResult<HttpResponse> {
    // Returns subscriptions held by the current user as both dispatcher and contractor.
    let dispatcher_subs = SubscriptionRepo::new(&pool)
        .list_for_subscriber("dispatcher", auth.id)
        .await?;
    let contractor_subs = SubscriptionRepo::new(&pool)
        .list_for_subscriber("contractor", auth.id)
        .await?;
    let mut all = dispatcher_subs;
    all.extend(contractor_subs);
    Ok(HttpResponse::Ok().json(all))
}

pub async fn subscribe(
    pool: web::Data<sqlx::PgPool>,
    body: web::Json<SubscribeBody>,
    auth: RequireViewContractors,
) -> AppResult<HttpResponse> {
    let body = body.into_inner();
    if !matches!(body.subscriber_type.as_str(), "dispatcher" | "contractor") {
        return Err(AppError::Validation(
            "subscriber_type must be 'dispatcher' or 'contractor'".into(),
        ));
    }
    if !matches!(body.target_type.as_str(), "shift" | "route") {
        return Err(AppError::Validation(
            "target_type must be 'shift' or 'route'".into(),
        ));
    }
    SubscriptionRepo::new(&pool)
        .subscribe(&body.subscriber_type, auth.id, &body.target_type, body.target_id)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn unsubscribe(
    pool: web::Data<sqlx::PgPool>,
    body: web::Json<SubscribeBody>,
    auth: RequireViewContractors,
) -> AppResult<HttpResponse> {
    let body = body.into_inner();
    SubscriptionRepo::new(&pool)
        .unsubscribe(&body.subscriber_type, auth.id, &body.target_type, body.target_id)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}
