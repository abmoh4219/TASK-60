//! Background crawl engine.
//!
//! [`CrawlEngine`] runs as a long-lived tokio task that wakes every 60 seconds
//! (or immediately when triggered via the [`TriggerHandle`]), queries the DB
//! for due tasks, and spawns a bounded worker per task.
//!
//! ## Worker lifecycle
//!
//! ```text
//! list_due() → [task₁, task₂, …]
//!   for each task (bounded by Semaphore):
//!     set_running(task)
//!     run_id = CrawlRunRepo::start()
//!     loop {
//!       read_page(cursor) → (items, next_cursor)
//!       for each item:
//!         transform → fingerprint-dedup → score → ingest / quarantine
//!       increment_counters
//!       cursor = next_cursor or break if None
//!     }
//!     CrawlRunRepo::finish()
//!     CrawlTaskRepo::complete(cursor, next_crawl_at)
//! ```

use std::sync::Arc;

use chrono::Utc;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tokio::time::{interval, Duration, MissedTickBehavior};
use tracing::{error, info, warn};

use crate::domain::content::models::CreateContent;
use crate::domain::content::repo::ContentRepo;
use crate::domain::crawl::models::{CrawlTask, NewQualityLog};
use crate::domain::crawl::repo::{CrawlRunRepo, CrawlSourceRepo, CrawlTaskRepo, QualityRepo};

use super::pipeline;
use super::quality::QualityScorer;
use super::reader::reader_for;

// ── Trigger handle ────────────────────────────────────────────────────────────

/// Allows HTTP handlers to wake the engine without waiting for the next tick.
pub struct TriggerHandle {
    tx: tokio::sync::watch::Sender<()>,
}

impl TriggerHandle {
    /// Create a handle and its paired receiver (passed to `CrawlEngine::spawn`).
    pub fn new() -> (Self, tokio::sync::watch::Receiver<()>) {
        let (tx, rx) = tokio::sync::watch::channel(());
        (Self { tx }, rx)
    }

    /// Send a wake signal to the engine.
    pub fn notify(&self) {
        let _ = self.tx.send(());
    }
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct CrawlEngine {
    pool:      PgPool,
    semaphore: Arc<Semaphore>,
}

impl CrawlEngine {
    /// Create a new engine. `max_workers` bounds concurrent crawl workers.
    pub fn new(pool: PgPool, max_workers: usize) -> Self {
        Self {
            pool,
            semaphore: Arc::new(Semaphore::new(max_workers.max(1))),
        }
    }

    /// Spawn the engine as a background task.
    ///
    /// The engine wakes either on its 60-second tick or when the
    /// `trigger_rx` channel receives a value.
    pub fn spawn(self, mut trigger_rx: tokio::sync::watch::Receiver<()>) {
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(60));
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

            info!(
                max_workers = self.semaphore.available_permits(),
                "CrawlEngine started"
            );

            loop {
                tokio::select! {
                    _ = tick.tick()          => {}
                    _ = trigger_rx.changed() => {
                        info!("CrawlEngine woken by trigger");
                    }
                }
                self.dispatch_due_tasks().await;
            }
        });
    }

    async fn dispatch_due_tasks(&self) {
        let due = match CrawlTaskRepo::new(&self.pool).list_due().await {
            Ok(v)  => v,
            Err(e) => {
                error!(error = %e, "Failed to query due crawl tasks");
                return;
            }
        };

        if due.is_empty() { return; }
        info!(count = due.len(), "Dispatching due crawl tasks");

        for task in due {
            let permit = match self.semaphore.clone().try_acquire_owned() {
                Ok(p)  => p,
                Err(_) => {
                    warn!(task_id = %task.id, "Concurrency limit reached; deferring task");
                    break;
                }
            };

            let pool = self.pool.clone();
            tokio::spawn(async move {
                let _permit = permit; // held until worker finishes
                run_task(pool, task).await;
            });
        }
    }
}

// ── Worker ────────────────────────────────────────────────────────────────────

async fn run_task(pool: PgPool, task: CrawlTask) {
    info!(task_id = %task.id, task_name = %task.task_name, "Crawl worker started");

    if let Err(e) = CrawlTaskRepo::new(&pool).set_running(task.id).await {
        error!(task_id = %task.id, error = %e, "Failed to mark task running");
        return;
    }

    let run_id = match CrawlRunRepo::new(&pool).start(task.id).await {
        Ok(id) => id,
        Err(e) => {
            error!(task_id = %task.id, error = %e, "Failed to start crawl run");
            let _ = CrawlTaskRepo::new(&pool).set_failed(task.id).await;
            return;
        }
    };

    let source = match CrawlSourceRepo::new(&pool).find_by_id(task.source_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            error!(source_id = %task.source_id, "Source not found");
            abort(&pool, run_id, task.id, "source not found").await;
            return;
        }
        Err(e) => {
            error!(error = %e, "Failed to load crawl source");
            abort(&pool, run_id, task.id, &e.to_string()).await;
            return;
        }
    };

    let reader = reader_for(&source.source_type);
    let mut cursor = if task.incremental { task.resume_cursor.clone() } else { None };

    let mut pages_fetched    : i32 = 0;
    let mut items_ingested   : i32 = 0;
    let mut items_quarantined: i32 = 0;
    let mut error_log: Option<String> = None;

    // ── Page loop ─────────────────────────────────────────────────────────
    loop {
        let (raw_items, next_cursor) = match reader.read_page(&source, cursor.clone()).await {
            Ok(v)  => v,
            Err(e) => { warn!(error = %e, "Reader error"); error_log = Some(e); break; }
        };

        pages_fetched += 1;

        if raw_items.is_empty() { cursor = None; break; }

        let mut page_ingested    = 0i32;
        let mut page_quarantined = 0i32;

        for raw in &raw_items {
            let Some(transformed) = pipeline::transform(raw) else {
                warn!("Skipping untransformable item");
                continue;
            };

            // Skip exact fingerprint duplicates.
            match ContentRepo::new(&pool).find_by_fingerprint(&transformed.fingerprint).await {
                Ok(Some(_)) => {
                    info!(fp = %transformed.fingerprint, "Duplicate — skipping");
                    continue;
                }
                Ok(None) => {}
                Err(e) => warn!(error = %e, "Fingerprint check failed"),
            }

            let scored = match QualityScorer::score(&transformed, &pool, None).await {
                Ok(s)  => s,
                Err(e) => { warn!(error = %e, "Quality scoring failed"); continue; }
            };

            let q = &scored.score;

            let quality_log = NewQualityLog {
                crawl_run_id:         Some(run_id),
                content_id:           None,
                url_fingerprint:      Some(transformed.fingerprint.clone()),
                completeness_score:   Some(q.completeness),
                accuracy_score:       Some(q.accuracy),
                timeliness_score:     Some(q.timeliness),
                quality_score:        Some(q.total),
                issues:               Some(json!(scored.issues)),
                is_quarantined:       scored.is_quarantined,
                quarantine_reason:    scored.quarantine_reason.clone(),
                transformation_steps: Some(json!(transformed.transformation_steps)),
            };

            if scored.is_quarantined {
                let _ = QualityRepo::new(&pool).insert(&quality_log).await;
                page_quarantined += 1;
                continue;
            }

            let slug = make_unique_slug(&pool, &transformed.slug).await;
            let cmd = CreateContent {
                slug,
                title:              transformed.title.clone(),
                body:               transformed.body.clone(),
                category:           transformed.category.clone(),
                route_id:           None,
                publish_date:       transformed.publish_date,
                source_url:         transformed.source_url.clone(),
                source_fingerprint: Some(transformed.fingerprint.clone()),
                quality_score:      Some(q.total),
                tags:               transformed.tags.clone(),
            };

            match ContentRepo::new(&pool).create(&cmd).await {
                Ok(content_id) => {
                    // Record quality log with content_id linked.
                    let mut linked = quality_log;
                    linked.content_id = Some(content_id);
                    let _ = QualityRepo::new(&pool).insert(&linked).await;

                    // Auto-publish if quality is sufficient.
                    if q.is_publishable() {
                        let _ = ContentRepo::new(&pool).publish(content_id).await;
                    }

                    page_ingested += 1;
                }
                Err(e) => {
                    warn!(error = %e, slug = %cmd.slug, "Failed to ingest content");
                    let _ = QualityRepo::new(&pool).insert(&quality_log).await;
                }
            }
        }

        items_ingested    += page_ingested;
        items_quarantined += page_quarantined;

        let _ = CrawlRunRepo::new(&pool)
            .increment_counters(run_id, 1, page_ingested, page_quarantined)
            .await;

        cursor = next_cursor;
        if cursor.is_none() { break; }
    }

    // ── Finish run ────────────────────────────────────────────────────────
    let status = if error_log.is_some() { "failed" } else { "completed" };

    if let Err(e) = CrawlRunRepo::new(&pool)
        .finish(run_id, status, pages_fetched, items_ingested, items_quarantined, error_log.as_deref())
        .await
    {
        error!(run_id = %run_id, error = %e, "Failed to finish crawl run");
    }

    let next_crawl = Some(Utc::now() + chrono::Duration::hours(1));
    if let Err(e) = CrawlTaskRepo::new(&pool).complete(task.id, cursor, next_crawl).await {
        error!(task_id = %task.id, error = %e, "Failed to complete crawl task");
    }

    info!(
        task_id     = %task.id,
        run_id      = %run_id,
        ingested    = items_ingested,
        quarantined = items_quarantined,
        status      = status,
        "Crawl worker finished"
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn abort(pool: &PgPool, run_id: uuid::Uuid, task_id: uuid::Uuid, reason: &str) {
    let _ = CrawlRunRepo::new(pool)
        .finish(run_id, "failed", 0, 0, 0, Some(reason))
        .await;
    let _ = CrawlTaskRepo::new(pool).set_failed(task_id).await;
}

/// Return a slug that does not already exist in `content_pages`.
/// If `base_slug` is taken, appends a numeric suffix.
async fn make_unique_slug(pool: &PgPool, base_slug: &str) -> String {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM content_pages WHERE slug LIKE $1 || '%'",
    )
    .bind(base_slug)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    if count == 0 {
        base_slug.to_owned()
    } else {
        format!("{base_slug}-{count}")
    }
}
