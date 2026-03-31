//! Public kiosk REST handlers.
//!
//! Routes (registered in `main.rs` under `/api/v1/kiosk`, no auth):
//!
//! | Method | Path              | Description                              |
//! |--------|-------------------|------------------------------------------|
//! | GET    | /content          | Search / list published content           |
//! | GET    | /content/:slug    | Full article + related content            |
//! | GET    | /archive          | Archive index grouped by year & month     |
//! | GET    | /categories       | Published article count per category     |

use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::content::models::{ContentPageWithTags, ContentSummary};
use crate::error::{AppError, AppResult};

// ── Query param structs ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q:        Option<String>,
    pub category: Option<String>,
    pub tag:      Option<String>,
    pub page:     Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ArchiveParams {
    pub year:     Option<i32>,
    pub month:    Option<i32>,
    pub category: Option<String>,
    pub page:     Option<i64>,
    pub per_page: Option<i64>,
}

// ── Response structs ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub items:       Vec<ContentSummary>,
    pub total:       i64,
    pub page:        i64,
    pub per_page:    i64,
    pub total_pages: i64,
    /// `"fts"` — full-text search used; `"fuzzy"` — fallback trigram search;
    /// `null` — no query, plain listing.
    pub search_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ArticleResponse {
    pub article: ContentPageWithTags,
    pub related: Vec<ContentSummary>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ArchiveEntry {
    pub year:  i32,
    pub month: i32,
    pub count: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CategoryCount {
    pub category: String,
    pub count:    i64,
}

// ── GET /content ──────────────────────────────────────────────────────────────

pub async fn list_content(
    pool:   web::Data<PgPool>,
    params: web::Query<SearchParams>,
) -> AppResult<HttpResponse> {
    let per_page = params.per_page.unwrap_or(20).clamp(1, 50);
    let page     = params.page.unwrap_or(1).max(1);
    let offset   = (page - 1) * per_page;

    let q        = params.q.as_deref().filter(|s| !s.is_empty());
    let category = params.category.as_deref().filter(|s| !s.is_empty());
    let tag      = params.tag.as_deref().filter(|s| !s.is_empty());

    // ── Try full-text search ──────────────────────────────────────────────
    if let Some(query) = q {
        let fts_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM content_pages
             WHERE is_published = TRUE
               AND search_vector @@ plainto_tsquery('english', $1)
               AND ($2::TEXT IS NULL OR category = $2)
               AND ($3::TEXT IS NULL OR EXISTS (
                       SELECT 1 FROM content_tags ct
                       WHERE ct.content_id = content_pages.id AND ct.tag = $3))",
        )
        .bind(query)
        .bind(category)
        .bind(tag)
        .fetch_one(pool.get_ref())
        .await
        .map_err(AppError::Database)?;

        if fts_count > 0 {
            let items: Vec<ContentSummary> = sqlx::query_as(
                "SELECT id, slug, title, category, route_id, is_published,
                        quality_score, publish_date, updated_at
                 FROM content_pages
                 WHERE is_published = TRUE
                   AND search_vector @@ plainto_tsquery('english', $1)
                   AND ($2::TEXT IS NULL OR category = $2)
                   AND ($3::TEXT IS NULL OR EXISTS (
                           SELECT 1 FROM content_tags ct
                           WHERE ct.content_id = content_pages.id AND ct.tag = $3))
                 ORDER BY ts_rank(search_vector, plainto_tsquery('english', $1)) DESC,
                          updated_at DESC
                 LIMIT $4 OFFSET $5",
            )
            .bind(query).bind(category).bind(tag)
            .bind(per_page).bind(offset)
            .fetch_all(pool.get_ref())
            .await
            .map_err(AppError::Database)?;

            let total_pages = (fts_count + per_page - 1) / per_page;
            return Ok(HttpResponse::Ok().json(SearchResponse {
                items, total: fts_count, page, per_page, total_pages,
                search_type: Some("fts".into()),
            }));
        }

        // ── Fuzzy fallback (pg_trgm word_similarity) ──────────────────────
        let fuzzy_total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM content_pages
             WHERE is_published = TRUE
               AND ($2::TEXT IS NULL OR category = $2)
               AND ($3::TEXT IS NULL OR EXISTS (
                       SELECT 1 FROM content_tags ct
                       WHERE ct.content_id = content_pages.id AND ct.tag = $3))
               AND (word_similarity($1, title) + word_similarity($1, body)) > 0.1",
        )
        .bind(query).bind(category).bind(tag)
        .fetch_one(pool.get_ref())
        .await
        .map_err(AppError::Database)?;

        let items: Vec<ContentSummary> = sqlx::query_as(
            "SELECT id, slug, title, category, route_id, is_published,
                    quality_score, publish_date, updated_at
             FROM content_pages
             WHERE is_published = TRUE
               AND ($2::TEXT IS NULL OR category = $2)
               AND ($3::TEXT IS NULL OR EXISTS (
                       SELECT 1 FROM content_tags ct
                       WHERE ct.content_id = content_pages.id AND ct.tag = $3))
             ORDER BY (word_similarity($1, title) + word_similarity($1, body)) DESC,
                      updated_at DESC
             LIMIT $4 OFFSET $5",
        )
        .bind(query).bind(category).bind(tag)
        .bind(per_page).bind(offset)
        .fetch_all(pool.get_ref())
        .await
        .map_err(AppError::Database)?;

        let total_pages = (fuzzy_total + per_page - 1) / per_page;
        return Ok(HttpResponse::Ok().json(SearchResponse {
            items, total: fuzzy_total, page, per_page, total_pages,
            search_type: Some("fuzzy".into()),
        }));
    }

    // ── No query — plain listing with optional category/tag filters ───────
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM content_pages
         WHERE is_published = TRUE
           AND ($1::TEXT IS NULL OR category = $1)
           AND ($2::TEXT IS NULL OR EXISTS (
                   SELECT 1 FROM content_tags ct
                   WHERE ct.content_id = content_pages.id AND ct.tag = $2))",
    )
    .bind(category).bind(tag)
    .fetch_one(pool.get_ref())
    .await
    .map_err(AppError::Database)?;

    let items: Vec<ContentSummary> = sqlx::query_as(
        "SELECT id, slug, title, category, route_id, is_published,
                quality_score, publish_date, updated_at
         FROM content_pages
         WHERE is_published = TRUE
           AND ($1::TEXT IS NULL OR category = $1)
           AND ($2::TEXT IS NULL OR EXISTS (
                   SELECT 1 FROM content_tags ct
                   WHERE ct.content_id = content_pages.id AND ct.tag = $2))
         ORDER BY updated_at DESC
         LIMIT $3 OFFSET $4",
    )
    .bind(category).bind(tag)
    .bind(per_page).bind(offset)
    .fetch_all(pool.get_ref())
    .await
    .map_err(AppError::Database)?;

    let total_pages = (total + per_page - 1) / per_page;
    Ok(HttpResponse::Ok().json(SearchResponse {
        items, total, page, per_page, total_pages,
        search_type: None,
    }))
}

// ── GET /content/:slug ────────────────────────────────────────────────────────

pub async fn get_article(
    pool: web::Data<PgPool>,
    path: web::Path<String>,
) -> AppResult<HttpResponse> {
    let slug = path.into_inner();

    // Fetch the published article.
    let page = sqlx::query_as::<_, crate::domain::content::models::ContentPage>(
        "SELECT id, slug, title, body, category, route_id, publish_date,
                is_published, quality_score, source_url, source_fingerprint,
                created_at, updated_at
         FROM content_pages
         WHERE slug = $1 AND is_published = TRUE",
    )
    .bind(&slug)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(AppError::Database)?
    .ok_or_else(|| AppError::NotFound(format!("article '{slug}'")))?;

    // Fetch tags.
    let tags: Vec<String> = {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT tag FROM content_tags WHERE content_id = $1 ORDER BY tag",
        )
        .bind(page.id)
        .fetch_all(pool.get_ref())
        .await
        .map_err(AppError::Database)?;
        rows.into_iter().map(|r| r.0).collect()
    };

    // Fetch related: same category OR shared tags, ranked by category match.
    let related: Vec<ContentSummary> = sqlx::query_as(
        "SELECT DISTINCT cp.id, cp.slug, cp.title, cp.category, cp.route_id,
                cp.is_published, cp.quality_score, cp.publish_date, cp.updated_at
         FROM content_pages cp
         WHERE cp.is_published = TRUE
           AND cp.id <> $1
           AND (
               cp.category = $2
               OR EXISTS (
                   SELECT 1 FROM content_tags t1
                   JOIN content_tags t2 ON t1.tag = t2.tag
                   WHERE t1.content_id = $1 AND t2.content_id = cp.id
               )
           )
         ORDER BY
             CASE WHEN cp.category = $2 THEN 0 ELSE 1 END,
             cp.updated_at DESC
         LIMIT 5",
    )
    .bind(page.id)
    .bind(&page.category)
    .fetch_all(pool.get_ref())
    .await
    .map_err(AppError::Database)?;

    Ok(HttpResponse::Ok().json(ArticleResponse {
        article: ContentPageWithTags { page, tags },
        related,
    }))
}

// ── GET /archive ──────────────────────────────────────────────────────────────

/// When `year` and `month` are both given, returns the articles for that month.
/// Otherwise returns the archive index (year/month buckets with counts).
pub async fn get_archive(
    pool:   web::Data<PgPool>,
    params: web::Query<ArchiveParams>,
) -> AppResult<HttpResponse> {
    let category = params.category.as_deref().filter(|s| !s.is_empty());

    // If year + month provided, return paginated article list.
    if let (Some(year), Some(month)) = (params.year, params.month) {
        let per_page = params.per_page.unwrap_or(20).clamp(1, 50);
        let page     = params.page.unwrap_or(1).max(1);
        let offset   = (page - 1) * per_page;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM content_pages
             WHERE is_published = TRUE
               AND publish_date IS NOT NULL
               AND EXTRACT(YEAR  FROM publish_date)::integer = $1
               AND EXTRACT(MONTH FROM publish_date)::integer = $2
               AND ($3::TEXT IS NULL OR category = $3)",
        )
        .bind(year).bind(month).bind(category)
        .fetch_one(pool.get_ref())
        .await
        .map_err(AppError::Database)?;

        let items: Vec<ContentSummary> = sqlx::query_as(
            "SELECT id, slug, title, category, route_id, is_published,
                    quality_score, publish_date, updated_at
             FROM content_pages
             WHERE is_published = TRUE
               AND publish_date IS NOT NULL
               AND EXTRACT(YEAR  FROM publish_date)::integer = $1
               AND EXTRACT(MONTH FROM publish_date)::integer = $2
               AND ($3::TEXT IS NULL OR category = $3)
             ORDER BY publish_date DESC, updated_at DESC
             LIMIT $4 OFFSET $5",
        )
        .bind(year).bind(month).bind(category)
        .bind(per_page).bind(offset)
        .fetch_all(pool.get_ref())
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        return Ok(HttpResponse::Ok().json(serde_json::json!({
            "mode":        "articles",
            "year":        year,
            "month":       month,
            "items":       items,
            "total":       total,
            "page":        page,
            "per_page":    per_page,
            "total_pages": total_pages,
        })));
    }

    // Archive index — group by year + month.
    let entries: Vec<ArchiveEntry> = sqlx::query_as(
        "SELECT
             EXTRACT(YEAR  FROM publish_date)::integer AS year,
             EXTRACT(MONTH FROM publish_date)::integer AS month,
             COUNT(*)                                   AS count
         FROM content_pages
         WHERE is_published = TRUE
           AND publish_date IS NOT NULL
           AND ($1::INTEGER IS NULL OR EXTRACT(YEAR FROM publish_date)::integer = $1)
           AND ($2::TEXT    IS NULL OR category = $2)
         GROUP BY year, month
         ORDER BY year DESC, month DESC",
    )
    .bind(params.year)
    .bind(category)
    .fetch_all(pool.get_ref())
    .await
    .map_err(AppError::Database)?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "mode":    "index",
        "entries": entries,
    })))
}

// ── GET /categories ───────────────────────────────────────────────────────────

pub async fn list_categories(pool: web::Data<PgPool>) -> AppResult<HttpResponse> {
    let cats: Vec<CategoryCount> = sqlx::query_as(
        "SELECT category, COUNT(*)::bigint AS count
         FROM content_pages
         WHERE is_published = TRUE
         GROUP BY category
         ORDER BY count DESC",
    )
    .fetch_all(pool.get_ref())
    .await
    .map_err(AppError::Database)?;

    Ok(HttpResponse::Ok().json(cats))
}

// ── GET /tags ─────────────────────────────────────────────────────────────────

/// Top tags across published articles — used for filter suggestions.
pub async fn list_tags(pool: web::Data<PgPool>) -> AppResult<HttpResponse> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT ct.tag, COUNT(DISTINCT ct.content_id)::bigint AS count
         FROM content_tags ct
         JOIN content_pages cp ON cp.id = ct.content_id
         WHERE cp.is_published = TRUE
         GROUP BY ct.tag
         ORDER BY count DESC
         LIMIT 50",
    )
    .fetch_all(pool.get_ref())
    .await
    .map_err(AppError::Database)?;

    let tags: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(tag, count)| serde_json::json!({ "tag": tag, "count": count }))
        .collect();

    Ok(HttpResponse::Ok().json(tags))
}
