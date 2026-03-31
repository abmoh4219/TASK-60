//! Content / kiosk domain repository.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use shared::PaginatedResponse;

use super::models::*;

pub struct ContentRepo<'a>(pub &'a PgPool);

impl<'a> ContentRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    // ── Reads ──────────────────────────────────────────────────────────────

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<ContentPage>> {
        sqlx::query_as(
            "SELECT id, slug, title, body, category, route_id, publish_date,
                    is_published, quality_score, source_url, source_fingerprint,
                    created_at, updated_at
             FROM content_pages WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn find_by_slug(&self, slug: &str) -> AppResult<Option<ContentPage>> {
        sqlx::query_as(
            "SELECT id, slug, title, body, category, route_id, publish_date,
                    is_published, quality_score, source_url, source_fingerprint,
                    created_at, updated_at
             FROM content_pages WHERE slug = $1",
        )
        .bind(slug)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn find_by_fingerprint(&self, fp: &str) -> AppResult<Option<Uuid>> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM content_pages WHERE source_fingerprint = $1",
        )
        .bind(fp)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.map(|r| r.0))
    }

    /// Full-text search + optional category/published filters.
    pub async fn list(
        &self,
        params: &ListContentParams,
    ) -> AppResult<PaginatedResponse<ContentSummary>> {
        let per_page = params.pagination.per_page();
        let offset   = params.pagination.offset();
        let page     = params.pagination.page();

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM content_pages
             WHERE ($1::TEXT    IS NULL OR search_vector @@ plainto_tsquery('english', $1))
               AND ($2::TEXT    IS NULL OR category    = $2)
               AND ($3::BOOLEAN IS NULL OR is_published = $3)
               AND ($4::UUID    IS NULL OR route_id    = $4)",
        )
        .bind(&params.query)
        .bind(&params.category)
        .bind(params.is_published)
        .bind(params.route_id)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<ContentSummary> = sqlx::query_as(
            "SELECT id, slug, title, category, route_id, is_published,
                    quality_score, publish_date, updated_at
             FROM content_pages
             WHERE ($1::TEXT    IS NULL OR search_vector @@ plainto_tsquery('english', $1))
               AND ($2::TEXT    IS NULL OR category     = $2)
               AND ($3::BOOLEAN IS NULL OR is_published = $3)
               AND ($4::UUID    IS NULL OR route_id     = $4)
             ORDER BY
                 CASE WHEN $1 IS NOT NULL
                      THEN ts_rank(search_vector, plainto_tsquery('english', $1))
                 END DESC NULLS LAST,
                 updated_at DESC
             LIMIT $5 OFFSET $6",
        )
        .bind(&params.query)
        .bind(&params.category)
        .bind(params.is_published)
        .bind(params.route_id)
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }

    pub async fn tags_for(&self, content_id: Uuid) -> AppResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT tag FROM content_tags WHERE content_id = $1 ORDER BY tag",
        )
        .bind(content_id)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    // ── Writes ─────────────────────────────────────────────────────────────

    pub async fn create(&self, cmd: &CreateContent) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO content_pages
                 (slug, title, body, category, route_id, publish_date,
                  source_url, source_fingerprint, quality_score)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             RETURNING id",
        )
        .bind(&cmd.slug)
        .bind(&cmd.title)
        .bind(&cmd.body)
        .bind(&cmd.category)
        .bind(cmd.route_id)
        .bind(cmd.publish_date)
        .bind(&cmd.source_url)
        .bind(&cmd.source_fingerprint)
        .bind(cmd.quality_score)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        self.set_tags(row.0, &cmd.tags).await?;
        Ok(row.0)
    }

    pub async fn update(&self, id: Uuid, cmd: &UpdateContent) -> AppResult<()> {
        sqlx::query(
            "UPDATE content_pages SET
                 title              = COALESCE($1, title),
                 body               = COALESCE($2, body),
                 category           = COALESCE($3, category),
                 route_id           = CASE WHEN $4::UUID IS NOT NULL THEN $4 ELSE route_id END,
                 publish_date       = COALESCE($5, publish_date),
                 source_url         = COALESCE($6, source_url),
                 source_fingerprint = COALESCE($7, source_fingerprint),
                 quality_score      = COALESCE($8, quality_score),
                 updated_at         = NOW()
             WHERE id = $9",
        )
        .bind(&cmd.title)
        .bind(&cmd.body)
        .bind(&cmd.category)
        .bind(cmd.route_id)
        .bind(cmd.publish_date)
        .bind(&cmd.source_url)
        .bind(&cmd.source_fingerprint)
        .bind(cmd.quality_score)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;

        if let Some(tags) = &cmd.tags {
            self.set_tags(id, tags).await?;
        }
        Ok(())
    }

    /// Publish a page — blocked if `quality_score` < 85.
    pub async fn publish(&self, id: Uuid) -> AppResult<()> {
        sqlx::query(
            "UPDATE content_pages
             SET is_published = TRUE,
                 publish_date = COALESCE(publish_date, CURRENT_DATE),
                 updated_at   = NOW()
             WHERE id = $1
               AND (quality_score IS NULL OR quality_score >= 85)",
        )
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn unpublish(&self, id: Uuid) -> AppResult<()> {
        sqlx::query(
            "UPDATE content_pages
             SET is_published = FALSE, updated_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn update_quality_score(&self, id: Uuid, score: rust_decimal::Decimal) -> AppResult<()> {
        sqlx::query(
            "UPDATE content_pages
             SET quality_score = $1, updated_at = NOW()
             WHERE id = $2",
        )
        .bind(score)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    /// Replace all tags for a content page atomically.
    pub async fn set_tags(&self, content_id: Uuid, tags: &[String]) -> AppResult<()> {
        sqlx::query("DELETE FROM content_tags WHERE content_id = $1")
            .bind(content_id)
            .execute(self.0)
            .await
            .map_err(AppError::Database)?;

        for tag in tags {
            sqlx::query(
                "INSERT INTO content_tags (content_id, tag) VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
            )
            .bind(content_id)
            .bind(tag)
            .execute(self.0)
            .await
            .map_err(AppError::Database)?;
        }
        Ok(())
    }
}
