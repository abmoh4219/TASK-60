//! Data ingestion / crawl engine repositories.

use chrono::DateTime;
use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use shared::PaginatedResponse;

use super::models::*;

// ── CrawlSourceRepo ───────────────────────────────────────────────────────────

pub struct CrawlSourceRepo<'a>(pub &'a PgPool);

impl<'a> CrawlSourceRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn list_active(&self) -> AppResult<Vec<CrawlSource>> {
        sqlx::query_as(
            "SELECT id, name, source_type, base_path, city, keywords,
                    rate_limit_rps, is_active, created_at
             FROM crawl_sources WHERE is_active = TRUE ORDER BY name",
        )
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<CrawlSource>> {
        sqlx::query_as(
            "SELECT id, name, source_type, base_path, city, keywords,
                    rate_limit_rps, is_active, created_at
             FROM crawl_sources WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn create(&self, cmd: &CreateCrawlSource) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO crawl_sources
                 (name, source_type, base_path, city, keywords, rate_limit_rps)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id",
        )
        .bind(&cmd.name)
        .bind(&cmd.source_type)
        .bind(&cmd.base_path)
        .bind(&cmd.city)
        .bind(&cmd.keywords)
        .bind(cmd.rate_limit_rps)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }
}

// ── CrawlTaskRepo ─────────────────────────────────────────────────────────────

pub struct CrawlTaskRepo<'a>(pub &'a PgPool);

impl<'a> CrawlTaskRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<CrawlTask>> {
        sqlx::query_as(
            "SELECT id, source_id, task_name, pagination_rules, incremental,
                    last_crawled_at, next_crawl_at, status, resume_cursor, created_at
             FROM crawl_tasks WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    /// Tasks that are due to run: idle and next_crawl_at ≤ NOW() (or never crawled).
    pub async fn list_due(&self) -> AppResult<Vec<CrawlTask>> {
        sqlx::query_as(
            "SELECT ct.id, ct.source_id, ct.task_name, ct.pagination_rules,
                    ct.incremental, ct.last_crawled_at, ct.next_crawl_at,
                    ct.status, ct.resume_cursor, ct.created_at
             FROM crawl_tasks ct
             JOIN crawl_sources cs ON cs.id = ct.source_id
             WHERE ct.status = 'idle'
               AND cs.is_active = TRUE
               AND (ct.next_crawl_at IS NULL OR ct.next_crawl_at <= NOW())
             ORDER BY ct.next_crawl_at ASC NULLS FIRST",
        )
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn list_for_source(
        &self,
        source_id: Uuid,
        page:      i64,
        per_page:  i64,
    ) -> AppResult<PaginatedResponse<CrawlTask>> {
        let per_page = per_page.clamp(1, 100);
        let offset   = (page.max(1) - 1) * per_page;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM crawl_tasks WHERE source_id = $1",
        )
        .bind(source_id)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<CrawlTask> = sqlx::query_as(
            "SELECT id, source_id, task_name, pagination_rules, incremental,
                    last_crawled_at, next_crawl_at, status, resume_cursor, created_at
             FROM crawl_tasks WHERE source_id = $1
             ORDER BY created_at DESC
             LIMIT $2 OFFSET $3",
        )
        .bind(source_id)
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }

    pub async fn create(&self, cmd: &CreateCrawlTask) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO crawl_tasks
                 (source_id, task_name, pagination_rules, incremental)
             VALUES ($1, $2, $3, $4)
             RETURNING id",
        )
        .bind(cmd.source_id)
        .bind(&cmd.task_name)
        .bind(&cmd.pagination_rules)
        .bind(cmd.incremental)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }

    /// Mark a task as running (called before a CrawlRun is started).
    pub async fn set_running(&self, id: Uuid) -> AppResult<()> {
        sqlx::query("UPDATE crawl_tasks SET status = 'running' WHERE id = $1")
            .bind(id)
            .execute(self.0)
            .await
            .map_err(AppError::Database)?;
        Ok(())
    }

    /// Mark a task as idle after completion and update last/next crawl timestamps.
    pub async fn complete(
        &self,
        id:            Uuid,
        cursor:        Option<Value>,
        next_crawl_at: Option<DateTime<Utc>>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE crawl_tasks
             SET status         = 'idle',
                 resume_cursor  = $1,
                 last_crawled_at = NOW(),
                 next_crawl_at  = $2
             WHERE id = $3",
        )
        .bind(cursor)
        .bind(next_crawl_at)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn set_failed(&self, id: Uuid) -> AppResult<()> {
        sqlx::query("UPDATE crawl_tasks SET status = 'failed' WHERE id = $1")
            .bind(id)
            .execute(self.0)
            .await
            .map_err(AppError::Database)?;
        Ok(())
    }
}

// ── CrawlRunRepo ──────────────────────────────────────────────────────────────

pub struct CrawlRunRepo<'a>(pub &'a PgPool);

impl<'a> CrawlRunRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<CrawlRun>> {
        sqlx::query_as(
            "SELECT id, task_id, started_at, finished_at, status,
                    pages_fetched, items_ingested, items_quarantined, error_log
             FROM crawl_runs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    /// Open a new run row (status = 'running').
    pub async fn start(&self, task_id: Uuid) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO crawl_runs (task_id) VALUES ($1) RETURNING id",
        )
        .bind(task_id)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }

    pub async fn finish(
        &self,
        id:                Uuid,
        status:            &str,
        pages_fetched:     i32,
        items_ingested:    i32,
        items_quarantined: i32,
        error_log:         Option<&str>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE crawl_runs
             SET finished_at        = NOW(),
                 status             = $1,
                 pages_fetched      = $2,
                 items_ingested     = $3,
                 items_quarantined  = $4,
                 error_log          = $5
             WHERE id = $6",
        )
        .bind(status)
        .bind(pages_fetched)
        .bind(items_ingested)
        .bind(items_quarantined)
        .bind(error_log)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    /// Increment live counters during a run.
    pub async fn increment_counters(
        &self,
        id:                Uuid,
        pages:             i32,
        items:             i32,
        quarantined:       i32,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE crawl_runs
             SET pages_fetched     = pages_fetched     + $1,
                 items_ingested    = items_ingested    + $2,
                 items_quarantined = items_quarantined + $3
             WHERE id = $4",
        )
        .bind(pages)
        .bind(items)
        .bind(quarantined)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn list_for_task(
        &self,
        task_id:  Uuid,
        page:     i64,
        per_page: i64,
    ) -> AppResult<PaginatedResponse<CrawlRun>> {
        let per_page = per_page.clamp(1, 100);
        let offset   = (page.max(1) - 1) * per_page;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM crawl_runs WHERE task_id = $1",
        )
        .bind(task_id)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<CrawlRun> = sqlx::query_as(
            "SELECT id, task_id, started_at, finished_at, status,
                    pages_fetched, items_ingested, items_quarantined, error_log
             FROM crawl_runs WHERE task_id = $1
             ORDER BY started_at DESC
             LIMIT $2 OFFSET $3",
        )
        .bind(task_id)
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }
}

// ── QualityRepo ───────────────────────────────────────────────────────────────

pub struct QualityRepo<'a>(pub &'a PgPool);

impl<'a> QualityRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn insert(&self, log: &NewQualityLog) -> AppResult<i64> {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO quality_logs
                 (crawl_run_id, content_id, url_fingerprint,
                  completeness_score, accuracy_score, timeliness_score,
                  quality_score, issues, is_quarantined, quarantine_reason,
                  transformation_steps)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             RETURNING id",
        )
        .bind(log.crawl_run_id)
        .bind(log.content_id)
        .bind(&log.url_fingerprint)
        .bind(log.completeness_score)
        .bind(log.accuracy_score)
        .bind(log.timeliness_score)
        .bind(log.quality_score)
        .bind(&log.issues)
        .bind(log.is_quarantined)
        .bind(&log.quarantine_reason)
        .bind(&log.transformation_steps)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }

    pub async fn list_for_run(
        &self,
        run_id:   Uuid,
        page:     i64,
        per_page: i64,
    ) -> AppResult<PaginatedResponse<QualityLog>> {
        let per_page = per_page.clamp(1, 100);
        let offset   = (page.max(1) - 1) * per_page;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM quality_logs WHERE crawl_run_id = $1",
        )
        .bind(run_id)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<QualityLog> = sqlx::query_as(
            "SELECT id, crawl_run_id, content_id, url_fingerprint,
                    completeness_score, accuracy_score, timeliness_score,
                    quality_score, issues, is_quarantined, quarantine_reason,
                    transformation_steps, created_at
             FROM quality_logs WHERE crawl_run_id = $1
             ORDER BY created_at DESC
             LIMIT $2 OFFSET $3",
        )
        .bind(run_id)
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }

    /// Quarantined items across all runs, paginated.
    pub async fn list_quarantined(
        &self,
        page:     i64,
        per_page: i64,
    ) -> AppResult<PaginatedResponse<QualityLog>> {
        let per_page = per_page.clamp(1, 100);
        let offset   = (page.max(1) - 1) * per_page;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM quality_logs WHERE is_quarantined = TRUE",
        )
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<QualityLog> = sqlx::query_as(
            "SELECT id, crawl_run_id, content_id, url_fingerprint,
                    completeness_score, accuracy_score, timeliness_score,
                    quality_score, issues, is_quarantined, quarantine_reason,
                    transformation_steps, created_at
             FROM quality_logs WHERE is_quarantined = TRUE
             ORDER BY created_at DESC
             LIMIT $1 OFFSET $2",
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }
}
