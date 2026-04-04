//! Staffing / contractor matching repositories.

use chrono::DateTime;
use chrono::Utc;
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use shared::PaginatedResponse;

use super::models::*;

// ── ContractorRepo ────────────────────────────────────────────────────────────

pub struct ContractorRepo<'a>(pub &'a PgPool);

impl<'a> ContractorRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<Contractor>> {
        sqlx::query_as(
            "SELECT id, full_name, phone_last4, region, quality_rating,
                    is_active, created_at, updated_at
             FROM contractors WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    /// Look up the contractor UUID that is linked to a given user account.
    /// Returns `None` if no contractor profile has been associated with this user.
    pub async fn find_id_by_user_id(&self, user_id: Uuid) -> AppResult<Option<Uuid>> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM contractors WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.map(|(id,)| id))
    }

    pub async fn list(
        &self,
        region:    Option<&str>,
        is_active: Option<bool>,
        page:      i64,
        per_page:  i64,
    ) -> AppResult<PaginatedResponse<Contractor>> {
        let per_page = per_page.clamp(1, 100);
        let offset   = (page.max(1) - 1) * per_page;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM contractors
             WHERE ($1::TEXT    IS NULL OR region    = $1)
               AND ($2::BOOLEAN IS NULL OR is_active = $2)",
        )
        .bind(region)
        .bind(is_active)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<Contractor> = sqlx::query_as(
            "SELECT id, full_name, phone_last4, region, quality_rating,
                    is_active, created_at, updated_at
             FROM contractors
             WHERE ($1::TEXT    IS NULL OR region    = $1)
               AND ($2::BOOLEAN IS NULL OR is_active = $2)
             ORDER BY quality_rating DESC, full_name
             LIMIT $3 OFFSET $4",
        )
        .bind(region)
        .bind(is_active)
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }

    pub async fn tags_for(&self, contractor_id: Uuid) -> AppResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT tag FROM contractor_tags WHERE contractor_id = $1 ORDER BY tag",
        )
        .bind(contractor_id)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Find contractors available within the given window who hold all required tags.
    pub async fn available_for_shift(
        &self,
        shift_start:   DateTime<Utc>,
        shift_end:     DateTime<Utc>,
        required_tags: &[String],
    ) -> AppResult<Vec<Contractor>> {
        // Contractors with an overlapping availability window AND all required tags.
        sqlx::query_as(
            "SELECT DISTINCT c.id, c.full_name, c.phone_last4, c.region,
                    c.quality_rating, c.is_active, c.created_at, c.updated_at
             FROM contractors c
             JOIN contractor_availability ca ON ca.contractor_id = c.id
             WHERE c.is_active = TRUE
               AND ca.available_from <= $1
               AND ca.available_to   >= $2
               AND ($3::TEXT[] IS NULL OR
                    (SELECT COUNT(*) FROM contractor_tags ct
                     WHERE ct.contractor_id = c.id
                       AND ct.tag = ANY($3)) = array_length($3, 1))
             ORDER BY c.quality_rating DESC",
        )
        .bind(shift_end)   // available_from <= shift_end
        .bind(shift_start) // available_to   >= shift_start
        .bind(required_tags)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn create(&self, cmd: &CreateContractor) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO contractors
                 (full_name, phone_encrypted, phone_last4, email_encrypted, region)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
        )
        .bind(&cmd.full_name)
        .bind(&cmd.phone_encrypted)
        .bind(&cmd.phone_last4)
        .bind(&cmd.email_encrypted)
        .bind(&cmd.region)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        self.set_tags(row.0, &cmd.tags).await?;
        Ok(row.0)
    }

    pub async fn update_rating(&self, id: Uuid, rating: Decimal) -> AppResult<()> {
        sqlx::query(
            "UPDATE contractors SET quality_rating = $1, updated_at = NOW()
             WHERE id = $2",
        )
        .bind(rating)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn set_active(&self, id: Uuid, active: bool) -> AppResult<()> {
        sqlx::query(
            "UPDATE contractors SET is_active = $1, updated_at = NOW()
             WHERE id = $2",
        )
        .bind(active)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn set_tags(&self, contractor_id: Uuid, tags: &[String]) -> AppResult<()> {
        sqlx::query("DELETE FROM contractor_tags WHERE contractor_id = $1")
            .bind(contractor_id)
            .execute(self.0)
            .await
            .map_err(AppError::Database)?;

        for tag in tags {
            sqlx::query(
                "INSERT INTO contractor_tags (contractor_id, tag) VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
            )
            .bind(contractor_id)
            .bind(tag)
            .execute(self.0)
            .await
            .map_err(AppError::Database)?;
        }
        Ok(())
    }

    pub async fn add_availability(
        &self,
        contractor_id:  Uuid,
        available_from: DateTime<Utc>,
        available_to:   DateTime<Utc>,
        notes:          Option<&str>,
    ) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO contractor_availability
                 (contractor_id, available_from, available_to, notes)
             VALUES ($1, $2, $3, $4)
             RETURNING id",
        )
        .bind(contractor_id)
        .bind(available_from)
        .bind(available_to)
        .bind(notes)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }
}

// ── ShiftRepo ─────────────────────────────────────────────────────────────────

pub struct ShiftRepo<'a>(pub &'a PgPool);

impl<'a> ShiftRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<Shift>> {
        sqlx::query_as(
            "SELECT id, schedule_id, role, region, required_tags,
                    shift_start, shift_end, status, is_critical,
                    created_by, created_at
             FROM shifts WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn list(
        &self,
        params: &ListShiftsParams,
    ) -> AppResult<PaginatedResponse<Shift>> {
        let per_page = params.pagination.per_page();
        let offset   = params.pagination.offset();
        let page     = params.pagination.page();

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM shifts
             WHERE ($1::TEXT IS NULL OR status = $1)
               AND ($2::TEXT IS NULL OR region = $2)
               AND ($3::TEXT IS NULL OR role   = $3)",
        )
        .bind(&params.status)
        .bind(&params.region)
        .bind(&params.role)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<Shift> = sqlx::query_as(
            "SELECT id, schedule_id, role, region, required_tags,
                    shift_start, shift_end, status, is_critical,
                    created_by, created_at
             FROM shifts
             WHERE ($1::TEXT IS NULL OR status = $1)
               AND ($2::TEXT IS NULL OR region = $2)
               AND ($3::TEXT IS NULL OR role   = $3)
             ORDER BY is_critical DESC, shift_start ASC
             LIMIT $4 OFFSET $5",
        )
        .bind(&params.status)
        .bind(&params.region)
        .bind(&params.role)
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }

    pub async fn create(&self, cmd: &CreateShift) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO shifts
                 (schedule_id, role, region, required_tags,
                  shift_start, shift_end, is_critical, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id",
        )
        .bind(cmd.schedule_id)
        .bind(&cmd.role)
        .bind(&cmd.region)
        .bind(&cmd.required_tags)
        .bind(cmd.shift_start)
        .bind(cmd.shift_end)
        .bind(cmd.is_critical)
        .bind(cmd.created_by)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }

    pub async fn update_status(&self, id: Uuid, status: &str) -> AppResult<()> {
        sqlx::query("UPDATE shifts SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(id)
            .execute(self.0)
            .await
            .map_err(AppError::Database)?;
        Ok(())
    }
}

// ── AssignmentRepo ────────────────────────────────────────────────────────────

pub struct AssignmentRepo<'a>(pub &'a PgPool);

impl<'a> AssignmentRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<ShiftAssignment>> {
        sqlx::query_as(
            "SELECT id, shift_id, contractor_id, match_score, match_reasons,
                    assigned_by, assigned_at, status
             FROM shift_assignments WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn list_for_shift(
        &self,
        shift_id: Uuid,
    ) -> AppResult<Vec<AssignmentWithContractor>> {
        sqlx::query_as(
            "SELECT sa.id, sa.shift_id, sa.status, sa.match_score,
                    sa.match_reasons, sa.assigned_at,
                    c.id   AS contractor_id, c.full_name AS contractor_name,
                    c.region, c.quality_rating
             FROM shift_assignments sa
             JOIN contractors c ON c.id = sa.contractor_id
             WHERE sa.shift_id = $1
             ORDER BY sa.match_score DESC NULLS LAST",
        )
        .bind(shift_id)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn propose(
        &self,
        shift_id:      Uuid,
        contractor_id: Uuid,
        score:         Option<Decimal>,
        reasons:       Option<Value>,
        assigned_by:   Option<Uuid>,
    ) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO shift_assignments
                 (shift_id, contractor_id, match_score, match_reasons, assigned_by)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (shift_id, contractor_id) DO UPDATE
                 SET match_score = EXCLUDED.match_score,
                     match_reasons = EXCLUDED.match_reasons,
                     status = 'proposed',
                     assigned_at = NOW()
             RETURNING id",
        )
        .bind(shift_id)
        .bind(contractor_id)
        .bind(score)
        .bind(reasons)
        .bind(assigned_by)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }

    pub async fn respond(&self, id: Uuid, status: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE shift_assignments SET status = $1
             WHERE id = $2 AND status = 'proposed'",
        )
        .bind(status)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }
}

// ── SubscriptionRepo ──────────────────────────────────────────────────────────

pub struct SubscriptionRepo<'a>(pub &'a PgPool);

impl<'a> SubscriptionRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn subscribe(
        &self,
        subscriber_type: &str,
        subscriber_id:   Uuid,
        target_type:     &str,
        target_id:       Uuid,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO subscriptions
                 (subscriber_type, subscriber_id, target_type, target_id)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING",
        )
        .bind(subscriber_type)
        .bind(subscriber_id)
        .bind(target_type)
        .bind(target_id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn unsubscribe(
        &self,
        subscriber_type: &str,
        subscriber_id:   Uuid,
        target_type:     &str,
        target_id:       Uuid,
    ) -> AppResult<()> {
        sqlx::query(
            "DELETE FROM subscriptions
             WHERE subscriber_type = $1 AND subscriber_id = $2
               AND target_type     = $3 AND target_id     = $4",
        )
        .bind(subscriber_type)
        .bind(subscriber_id)
        .bind(target_type)
        .bind(target_id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn list_for_subscriber(
        &self,
        subscriber_type: &str,
        subscriber_id:   Uuid,
    ) -> AppResult<Vec<Subscription>> {
        sqlx::query_as(
            "SELECT id, subscriber_type, subscriber_id, target_type, target_id, created_at
             FROM subscriptions
             WHERE subscriber_type = $1 AND subscriber_id = $2
             ORDER BY created_at DESC",
        )
        .bind(subscriber_type)
        .bind(subscriber_id)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }
}
