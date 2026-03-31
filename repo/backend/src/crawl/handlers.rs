//! Crawl management REST handlers.
//!
//! All endpoints require `ManageCrawl` permission (Admin + OpsAgent).
//!
//! Routes (registered in `main.rs` under `/api/v1/crawl`):
//!
//! | Method | Path                           | Description                    |
//! |--------|--------------------------------|--------------------------------|
//! | GET    | /sources                       | List all crawl sources          |
//! | POST   | /sources                       | Create a crawl source           |
//! | GET    | /sources/{id}                  | Get a single source             |
//! | GET    | /sources/{id}/tasks            | List tasks for a source         |
//! | POST   | /sources/{id}/tasks            | Create a task                   |
//! | POST   | /tasks/{id}/run                | Trigger an immediate run        |
//! | GET    | /tasks/{id}/runs               | List runs for a task            |
//! | GET    | /runs/{id}                     | Get a single run                |
//! | GET    | /runs/{id}/quality             | Quality logs for a run          |
//! | GET    | /quality/quarantined           | All quarantined items           |

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::rbac::RequireManageCrawl;
use crate::domain::crawl::models::{CreateCrawlSource, CreateCrawlTask};
use crate::domain::crawl::repo::{CrawlRunRepo, CrawlSourceRepo, CrawlTaskRepo, QualityRepo};
use crate::error::{AppError, AppResult};
use shared::PaginationParams;

use super::engine::TriggerHandle;

// ── GET /sources ──────────────────────────────────────────────────────────────

pub async fn list_sources(
    _auth: RequireManageCrawl,
    pool:  web::Data<sqlx::PgPool>,
) -> AppResult<HttpResponse> {
    let sources = CrawlSourceRepo::new(&pool).list_active().await?;
    Ok(HttpResponse::Ok().json(sources))
}

// ── POST /sources ─────────────────────────────────────────────────────────────

pub async fn create_source(
    _auth: RequireManageCrawl,
    pool:  web::Data<sqlx::PgPool>,
    body:  web::Json<CreateCrawlSource>,
) -> AppResult<HttpResponse> {
    let id = CrawlSourceRepo::new(&pool).create(&body).await?;
    Ok(HttpResponse::Created().json(serde_json::json!({ "id": id })))
}

// ── GET /sources/{id} ─────────────────────────────────────────────────────────

pub async fn get_source(
    _auth: RequireManageCrawl,
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
) -> AppResult<HttpResponse> {
    let source = CrawlSourceRepo::new(&pool)
        .find_by_id(*path)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("crawl source {}", *path)))?;
    Ok(HttpResponse::Ok().json(source))
}

// ── GET /sources/{id}/tasks ───────────────────────────────────────────────────

pub async fn list_tasks(
    _auth:  RequireManageCrawl,
    pool:   web::Data<sqlx::PgPool>,
    path:   web::Path<Uuid>,
    params: web::Query<PaginationParams>,
) -> AppResult<HttpResponse> {
    let result = CrawlTaskRepo::new(&pool)
        .list_for_source(*path, params.page(), params.per_page())
        .await?;
    Ok(HttpResponse::Ok().json(result))
}

// ── POST /sources/{id}/tasks ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateTaskBody {
    pub task_name:        String,
    pub pagination_rules: Option<serde_json::Value>,
    pub incremental:      Option<bool>,
}

pub async fn create_task(
    _auth: RequireManageCrawl,
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<CreateTaskBody>,
) -> AppResult<HttpResponse> {
    // Confirm the source exists.
    CrawlSourceRepo::new(&pool)
        .find_by_id(*path)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("crawl source {}", *path)))?;

    let cmd = CreateCrawlTask {
        source_id:        *path,
        task_name:        body.task_name.clone(),
        pagination_rules: body.pagination_rules.clone(),
        incremental:      body.incremental.unwrap_or(false),
    };

    let id = CrawlTaskRepo::new(&pool).create(&cmd).await?;
    Ok(HttpResponse::Created().json(serde_json::json!({ "id": id })))
}

// ── POST /tasks/{id}/run ──────────────────────────────────────────────────────

/// Reset `next_crawl_at = NOW()` and notify the engine to dispatch immediately.
pub async fn trigger_run(
    auth:    RequireManageCrawl,
    pool:    web::Data<sqlx::PgPool>,
    req:     HttpRequest,
    path:    web::Path<Uuid>,
    trigger: web::Data<TriggerHandle>,
) -> AppResult<HttpResponse> {
    let task_id = *path;

    // Ensure the task exists.
    CrawlTaskRepo::new(&pool)
        .find_by_id(task_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("crawl task {task_id}")))?;

    // Reset to idle with next_crawl_at = NOW() so the engine picks it up.
    sqlx::query(
        "UPDATE crawl_tasks SET status = 'idle', next_crawl_at = NOW()
         WHERE id = $1 AND status <> 'running'",
    )
    .bind(task_id)
    .execute(pool.get_ref())
    .await
    .map_err(AppError::Database)?;

    // Wake the engine immediately.
    trigger.notify();

    let ip = req
        .connection_info()
        .realip_remote_addr()
        .map(str::to_owned);

    crate::db::audit::write_audit(
        pool.get_ref(),
        "manual_crawl_trigger",
        "crawl_task",
        Some(task_id),
        Some(auth.id),
        "trigger_run",
        None, None,
        ip.as_deref(),
    )
    .await;

    Ok(HttpResponse::Accepted().json(serde_json::json!({
        "message": "Task queued for immediate execution",
        "task_id": task_id,
    })))
}

// ── GET /tasks/{id}/runs ──────────────────────────────────────────────────────

pub async fn list_runs(
    _auth:  RequireManageCrawl,
    pool:   web::Data<sqlx::PgPool>,
    path:   web::Path<Uuid>,
    params: web::Query<PaginationParams>,
) -> AppResult<HttpResponse> {
    let result = CrawlRunRepo::new(&pool)
        .list_for_task(*path, params.page(), params.per_page())
        .await?;
    Ok(HttpResponse::Ok().json(result))
}

// ── GET /runs/{id} ────────────────────────────────────────────────────────────

pub async fn get_run(
    _auth: RequireManageCrawl,
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
) -> AppResult<HttpResponse> {
    let run = CrawlRunRepo::new(&pool)
        .find_by_id(*path)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("crawl run {}", *path)))?;
    Ok(HttpResponse::Ok().json(run))
}

// ── GET /runs/{id}/quality ────────────────────────────────────────────────────

pub async fn quality_for_run(
    _auth:  RequireManageCrawl,
    pool:   web::Data<sqlx::PgPool>,
    path:   web::Path<Uuid>,
    params: web::Query<PaginationParams>,
) -> AppResult<HttpResponse> {
    let result = QualityRepo::new(&pool)
        .list_for_run(*path, params.page(), params.per_page())
        .await?;
    Ok(HttpResponse::Ok().json(result))
}

// ── GET /quality/quarantined ──────────────────────────────────────────────────

pub async fn list_quarantined(
    _auth:  RequireManageCrawl,
    pool:   web::Data<sqlx::PgPool>,
    params: web::Query<PaginationParams>,
) -> AppResult<HttpResponse> {
    let result = QualityRepo::new(&pool)
        .list_quarantined(params.page(), params.per_page())
        .await?;
    Ok(HttpResponse::Ok().json(result))
}
