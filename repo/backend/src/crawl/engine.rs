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
use crate::domain::rail::repo::{RouteRepo, ScheduleRepo, SeatClassRepo, InventoryRepo};

use super::pipeline;
use super::quality::QualityScorer;
use super::reader::reader_for;
use super::schedule_pipeline;

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

    let source_record = match CrawlSourceRepo::new(&pool).find_by_id(task.source_id).await {
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

    // Dispatch to the appropriate pipeline based on source type.
    if source_record.source_type == "schedule_feed" {
        run_schedule_task(pool, task, run_id, source_record).await;
        return;
    }

    // Otherwise, run the content pipeline (original path).
    run_content_task(pool, task, run_id, source_record).await;
}

/// Schedule aggregation pipeline — ingests schedule and inventory data.
async fn run_schedule_task(
    pool:   PgPool,
    task:   CrawlTask,
    run_id: uuid::Uuid,
    source: crate::domain::crawl::models::CrawlSource,
) {
    info!(task_id = %task.id, "Running schedule aggregation pipeline");

    let base_path = source.base_path.as_deref().unwrap_or("");
    let mut cursor = if task.incremental { task.resume_cursor.clone() } else { None };

    let mut pages_fetched     = 0i32;
    let mut items_ingested    = 0i32;
    let mut items_quarantined = 0i32;
    let mut error_log: Option<String> = None;

    loop {
        let (schedule_items, next_cursor) = match super::reader::read_schedule_page(
            base_path, cursor.clone(),
        ).await {
            Ok(v) => v,
            Err(e) => { warn!(error = %e, "Schedule reader error"); error_log = Some(e); break; }
        };
        pages_fetched += 1;

        if schedule_items.is_empty() { cursor = None; break; }

        let mut page_ingested = 0i32;

        for sched_raw in &schedule_items {
            let Some(mut transformed) = schedule_pipeline::transform_schedule(sched_raw) else {
                warn!("Skipping untransformable schedule item");
                continue;
            };

            let reject = schedule_pipeline::validate_schedule(&mut transformed);
            if reject {
                info!(
                    train = %transformed.train_number,
                    issues = ?transformed.issues,
                    "Schedule rejected by validation"
                );
                items_quarantined += 1;
                continue;
            }

            // Resolve route_code → route_id.
            let route = match sqlx::query_as::<_, (uuid::Uuid,)>(
                "SELECT id FROM routes WHERE route_code = $1"
            )
            .bind(&transformed.route_code)
            .fetch_optional(&pool)
            .await
            {
                Ok(Some((id,))) => id,
                Ok(None) => {
                    warn!(route_code = %transformed.route_code, "Unknown route code — skipping");
                    items_quarantined += 1;
                    continue;
                }
                Err(e) => {
                    warn!(error = %e, "Route lookup failed");
                    continue;
                }
            };

            // Upsert schedule: update if same train_number + date range exists, else insert.
            let schedule_id: uuid::Uuid = match sqlx::query_as::<_, (uuid::Uuid,)>(
                "INSERT INTO schedules (route_id, train_number, departure_time, arrival_time,
                                        status, delay_minutes, platform)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (train_number, departure_time)
                 DO UPDATE SET arrival_time  = EXCLUDED.arrival_time,
                               status        = EXCLUDED.status,
                               delay_minutes = EXCLUDED.delay_minutes,
                               platform      = COALESCE(EXCLUDED.platform, schedules.platform),
                               updated_at    = NOW()
                 RETURNING id"
            )
            .bind(route)
            .bind(&transformed.train_number)
            .bind(transformed.departure_time)
            .bind(transformed.arrival_time)
            .bind(&transformed.status)
            .bind(transformed.delay_minutes)
            .bind(&transformed.platform)
            .fetch_one(&pool)
            .await
            {
                Ok((id,)) => id,
                Err(e) => {
                    warn!(error = %e, train = %transformed.train_number,
                          "Schedule upsert failed");
                    continue;
                }
            };

            // Upsert inventory snapshots for each seat class.
            for inv in &transformed.inventory {
                let seat_class = match sqlx::query_as::<_, (uuid::Uuid,)>(
                    "SELECT id FROM seat_classes WHERE code = $1"
                )
                .bind(&inv.seat_class_code)
                .fetch_optional(&pool)
                .await
                {
                    Ok(Some((id,))) => id,
                    Ok(None) => {
                        warn!(code = %inv.seat_class_code, "Unknown seat class — skipping inventory");
                        continue;
                    }
                    Err(e) => {
                        warn!(error = %e, "Seat class lookup failed");
                        continue;
                    }
                };

                let _ = InventoryRepo::new(&pool)
                    .insert_snapshot(schedule_id, seat_class, inv.total_seats, inv.available_seats)
                    .await;
            }

            // Audit log for schedule ingestion.
            crate::db::audit::write_audit(
                &pool, "schedule_ingested", "schedules", Some(schedule_id),
                None, "crawl_engine", None,
                Some(serde_json::json!({
                    "train_number": transformed.train_number,
                    "route_code": transformed.route_code,
                    "issues": transformed.issues,
                    "run_id": run_id.to_string(),
                })),
                None,
            ).await;

            page_ingested += 1;
        }

        items_ingested += page_ingested;
        let _ = CrawlRunRepo::new(&pool)
            .increment_counters(run_id, 1, page_ingested, 0)
            .await;

        cursor = next_cursor;
        if cursor.is_none() { break; }
    }

    // Finish run.
    let status = if error_log.is_some() { "failed" } else { "completed" };
    let _ = CrawlRunRepo::new(&pool)
        .finish(run_id, status, pages_fetched, items_ingested, items_quarantined, error_log.as_deref())
        .await;
    let next_crawl = Some(Utc::now() + chrono::Duration::hours(1));
    let _ = CrawlTaskRepo::new(&pool).complete(task.id, cursor, next_crawl).await;
    info!(
        task_id     = %task.id,
        run_id      = %run_id,
        ingested    = items_ingested,
        quarantined = items_quarantined,
        status      = status,
        "Schedule aggregation worker finished"
    );
}

/// Content pipeline — original ingestion path for content pages.
async fn run_content_task(
    pool:   PgPool,
    task:   CrawlTask,
    run_id: uuid::Uuid,
    source: crate::domain::crawl::models::CrawlSource,
) {
    let reader = reader_for(&source.source_type);
    let mut cursor = if task.incremental { task.resume_cursor.clone() } else { None };

    // ── Resolve task-definition controls ─────────────────────────────────
    // city / keywords come from the source; pagination_rules from the task.
    let city_filter: Option<String> = source.city.as_ref()
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty());

    let keyword_filters: Vec<String> = source.keywords.iter()
        .map(|k| k.trim().to_lowercase())
        .filter(|k| !k.is_empty())
        .collect();

    let max_pages: Option<i32> = task.pagination_rules
        .as_ref()
        .and_then(|v| v.get("max_pages"))
        .and_then(|v| v.as_i64())
        .map(|n| n.max(1) as i32);

    let max_items: Option<i32> = task.pagination_rules
        .as_ref()
        .and_then(|v| v.get("max_items"))
        .and_then(|v| v.as_i64())
        .map(|n| n.max(1) as i32);

    if let Some(city) = &city_filter {
        info!(task_id = %task.id, city = %city, "City filter active");
    }
    if !keyword_filters.is_empty() {
        info!(task_id = %task.id, keywords = ?keyword_filters, "Keyword filter active");
    }
    if let Some(mp) = max_pages {
        info!(task_id = %task.id, max_pages = mp, "Pagination limit active");
    }

    let mut pages_fetched    : i32 = 0;
    let mut items_ingested   : i32 = 0;
    let mut items_quarantined: i32 = 0;
    let mut error_log: Option<String> = None;

    // ── Page loop ─────────────────────────────────────────────────────────
    loop {
        // Enforce max_pages from pagination_rules.
        if let Some(mp) = max_pages {
            if pages_fetched >= mp {
                info!(task_id = %task.id, "Reached max_pages={mp} — stopping");
                break;
            }
        }

        let (raw_items, next_cursor) = match reader.read_page(&source, cursor.clone()).await {
            Ok(v)  => v,
            Err(e) => { warn!(error = %e, "Reader error"); error_log = Some(e); break; }
        };

        pages_fetched += 1;

        // Per-source rate limiting: sleep to enforce configured rate_limit_rps
        let delay_ms = (1000.0 / source.rate_limit_rps.to_string().parse::<f64>().unwrap_or(2.0)) as u64;
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }

        if raw_items.is_empty() { cursor = None; break; }

        let mut page_ingested    = 0i32;
        let mut page_quarantined = 0i32;
        let mut stop_after_page  = false; // set true when max_items is reached

        for raw in &raw_items {
            // Enforce max_items from pagination_rules.
            if let Some(mi) = max_items {
                if items_ingested + page_ingested >= mi {
                    info!(task_id = %task.id, "Reached max_items={mi} — stopping");
                    stop_after_page = true;
                    break;
                }
            }

            let Some(transformed) = pipeline::transform(raw) else {
                warn!("Skipping untransformable item");
                continue;
            };

            // ── City filter ───────────────────────────────────────────────
            // If the source has a city constraint, only ingest items that
            // mention the city in title, body, or tags (case-insensitive).
            if let Some(city) = &city_filter {
                let haystack = format!(
                    "{} {} {}",
                    transformed.title.to_lowercase(),
                    transformed.body.to_lowercase(),
                    transformed.tags.join(" ").to_lowercase(),
                );
                if !haystack.contains(city.as_str()) {
                    info!(city = %city, "Skipping item — city filter not matched");
                    continue;
                }
            }

            // ── Keyword filter ────────────────────────────────────────────
            // If the source has keyword constraints, at least one keyword
            // must appear in the title or body (case-insensitive).
            if !keyword_filters.is_empty() {
                let haystack = format!(
                    "{} {}",
                    transformed.title.to_lowercase(),
                    transformed.body.to_lowercase(),
                );
                let any_match = keyword_filters.iter()
                    .any(|kw| haystack.contains(kw.as_str()));
                if !any_match {
                    info!(keywords = ?keyword_filters, "Skipping item — keyword filter not matched");
                    continue;
                }
            }

            // ── Dedup: URL fingerprint (first-class) ──────────────────────
            // If a content page with the same source URL already exists,
            // skip re-ingest regardless of title/body changes.
            if let Some(ref url_fp) = transformed.url_fingerprint {
                match ContentRepo::new(&pool).find_by_url_fingerprint(url_fp).await {
                    Ok(Some(_)) => {
                        info!(url_fp = %url_fp, "URL duplicate — skipping");
                        continue;
                    }
                    Ok(None) => {}
                    Err(e) => warn!(error = %e, "URL fingerprint check failed"),
                }
            }

            // ── Dedup: composite content fingerprint (exact match) ────────
            match ContentRepo::new(&pool).find_by_fingerprint(&transformed.fingerprint).await {
                Ok(Some(_)) => {
                    info!(fp = %transformed.fingerprint, "Content duplicate — skipping");
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
                url_fingerprint:    transformed.url_fingerprint.clone(),
                quality_score:      Some(q.total),
                tags:               transformed.tags.clone(),
            };

            match ContentRepo::new(&pool).create(&cmd).await {
                Ok(content_id) => {
                    // Record quality log with content_id linked.
                    let mut linked = quality_log;
                    linked.content_id = Some(content_id);
                    let _ = QualityRepo::new(&pool).insert(&linked).await;

                    // Auto-publish if quality is sufficient (runtime-configurable threshold).
                    let publish_threshold: rust_decimal::Decimal =
                        crate::domain::rules::repo::BusinessRuleRepo::new(&pool)
                            .get_value("quality_publish_threshold",
                                       &shared::rules::QUALITY_PUBLISH_THRESHOLD.to_string())
                            .await
                            .parse()
                            .unwrap_or_else(|_| rust_decimal::Decimal::from(
                                shared::rules::QUALITY_PUBLISH_THRESHOLD));
                    if q.total >= publish_threshold {
                        let _ = ContentRepo::new(&pool)
                            .publish_with_threshold(content_id, publish_threshold)
                            .await;
                    }

                    // Sampling-based review: mark ~2% of items for manual review
                    let sample_pct: f64 = crate::domain::rules::repo::BusinessRuleRepo::new(&pool)
                        .get_value("sample_review_pct", "0.02")
                        .await
                        .parse()
                        .unwrap_or(0.02);
                    if rand::random::<f64>() < sample_pct {
                        let _ = sqlx::query(
                            "UPDATE quality_logs SET sampled_for_review = TRUE
                             WHERE content_id = $1
                             ORDER BY created_at DESC LIMIT 1"
                        )
                        .bind(content_id)
                        .execute(&pool)
                        .await;
                        info!(content_id = %content_id, "Sampled for review");
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

        if stop_after_page {
            cursor = None;
            break;
        }

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
