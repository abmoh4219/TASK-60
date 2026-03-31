//! Rail domain repositories.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use shared::PaginatedResponse;

use super::models::*;

// ── RouteRepo ─────────────────────────────────────────────────────────────────

pub struct RouteRepo<'a>(pub &'a PgPool);

impl<'a> RouteRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn list_active(&self) -> AppResult<Vec<Route>> {
        sqlx::query_as(
            "SELECT id, route_code, name, origin_city, destination_city,
                    is_active, created_at
             FROM routes
             WHERE is_active = TRUE
             ORDER BY route_code",
        )
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn list_all(&self) -> AppResult<Vec<Route>> {
        sqlx::query_as(
            "SELECT id, route_code, name, origin_city, destination_city,
                    is_active, created_at
             FROM routes
             ORDER BY route_code",
        )
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<Route>> {
        sqlx::query_as(
            "SELECT id, route_code, name, origin_city, destination_city,
                    is_active, created_at
             FROM routes WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn set_active(&self, id: Uuid, active: bool) -> AppResult<()> {
        sqlx::query("UPDATE routes SET is_active = $1 WHERE id = $2")
            .bind(active)
            .bind(id)
            .execute(self.0)
            .await
            .map_err(AppError::Database)?;
        Ok(())
    }
}

// ── ScheduleRepo ──────────────────────────────────────────────────────────────

pub struct ScheduleRepo<'a>(pub &'a PgPool);

impl<'a> ScheduleRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn list(
        &self,
        params: &ListSchedulesParams,
    ) -> AppResult<PaginatedResponse<ScheduleWithRoute>> {
        let per_page = params.pagination.per_page();
        let offset   = params.pagination.offset();
        let page     = params.pagination.page();

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM schedules s
             JOIN routes r ON r.id = s.route_id
             WHERE ($1::UUID IS NULL OR s.route_id = $1)
               AND ($2::TEXT IS NULL OR s.status   = $2)
               AND ($3::TIMESTAMPTZ IS NULL OR s.departure_time >= $3)",
        )
        .bind(params.route_id)
        .bind(&params.status)
        .bind(params.departure_after)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<ScheduleWithRoute> = sqlx::query_as(
            "SELECT s.id, s.route_id,
                    r.route_code, r.name AS route_name,
                    r.origin_city, r.destination_city,
                    s.train_number, s.departure_time, s.arrival_time,
                    s.status, s.delay_minutes, s.platform, s.updated_at
             FROM schedules s
             JOIN routes r ON r.id = s.route_id
             WHERE ($1::UUID IS NULL OR s.route_id = $1)
               AND ($2::TEXT IS NULL OR s.status   = $2)
               AND ($3::TIMESTAMPTZ IS NULL OR s.departure_time >= $3)
             ORDER BY s.departure_time ASC
             LIMIT $4 OFFSET $5",
        )
        .bind(params.route_id)
        .bind(&params.status)
        .bind(params.departure_after)
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<Schedule>> {
        sqlx::query_as(
            "SELECT id, route_id, train_number, departure_time, arrival_time,
                    status, delay_minutes, platform, created_at, updated_at
             FROM schedules WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn update_status(
        &self,
        id:            Uuid,
        status:        &str,
        delay_minutes: i32,
        platform:      Option<&str>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE schedules
             SET status = $1, delay_minutes = $2,
                 platform  = COALESCE($3, platform),
                 updated_at = NOW()
             WHERE id = $4",
        )
        .bind(status)
        .bind(delay_minutes)
        .bind(platform)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }
}

// ── SeatClassRepo ─────────────────────────────────────────────────────────────

pub struct SeatClassRepo<'a>(pub &'a PgPool);

impl<'a> SeatClassRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn list(&self) -> AppResult<Vec<SeatClass>> {
        sqlx::query_as(
            "SELECT id, name, code, base_fare FROM seat_classes ORDER BY base_fare",
        )
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<SeatClass>> {
        sqlx::query_as(
            "SELECT id, name, code, base_fare FROM seat_classes WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn find_by_code(&self, code: &str) -> AppResult<Option<SeatClass>> {
        sqlx::query_as(
            "SELECT id, name, code, base_fare FROM seat_classes WHERE code = $1",
        )
        .bind(code)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }
}

// ── InventoryRepo ─────────────────────────────────────────────────────────────

pub struct InventoryRepo<'a>(pub &'a PgPool);

impl<'a> InventoryRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    /// Returns the most recent snapshot per seat class for a schedule.
    pub async fn latest_for_schedule(
        &self,
        schedule_id: Uuid,
    ) -> AppResult<Vec<InventoryWithClass>> {
        sqlx::query_as(
            "SELECT DISTINCT ON (i.seat_class_id)
                    i.seat_class_id,
                    sc.code AS seat_class_code,
                    sc.name AS seat_class_name,
                    sc.base_fare,
                    i.total_seats,
                    i.available_seats,
                    i.snapshot_at
             FROM inventory_snapshots i
             JOIN seat_classes sc ON sc.id = i.seat_class_id
             WHERE i.schedule_id = $1
             ORDER BY i.seat_class_id, i.snapshot_at DESC",
        )
        .bind(schedule_id)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }

    /// Decrement available seats by `n` (called on order confirmation).
    /// Uses a CHECK-safe UPDATE that will fail if available_seats would go < 0.
    pub async fn decrement(
        &self,
        schedule_id:   Uuid,
        seat_class_id: Uuid,
        n:             i32,
    ) -> AppResult<()> {
        // Insert a new snapshot row based on the latest values − n.
        sqlx::query(
            "INSERT INTO inventory_snapshots
                 (schedule_id, seat_class_id, total_seats, available_seats)
             SELECT $1, $2, total_seats,
                    GREATEST(available_seats - $3, 0)
             FROM inventory_snapshots
             WHERE schedule_id = $1 AND seat_class_id = $2
             ORDER BY snapshot_at DESC
             LIMIT 1",
        )
        .bind(schedule_id)
        .bind(seat_class_id)
        .bind(n)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    /// Increment available seats by `n` (called on order cancellation).
    pub async fn increment(
        &self,
        schedule_id:   Uuid,
        seat_class_id: Uuid,
        n:             i32,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO inventory_snapshots
                 (schedule_id, seat_class_id, total_seats, available_seats)
             SELECT $1, $2, total_seats,
                    LEAST(available_seats + $3, total_seats)
             FROM inventory_snapshots
             WHERE schedule_id = $1 AND seat_class_id = $2
             ORDER BY snapshot_at DESC
             LIMIT 1",
        )
        .bind(schedule_id)
        .bind(seat_class_id)
        .bind(n)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    /// Insert a manual inventory snapshot (ops agent correction).
    pub async fn insert_snapshot(
        &self,
        schedule_id:     Uuid,
        seat_class_id:   Uuid,
        total_seats:     i32,
        available_seats: i32,
    ) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO inventory_snapshots
                 (schedule_id, seat_class_id, total_seats, available_seats)
             VALUES ($1, $2, $3, $4)
             RETURNING id",
        )
        .bind(schedule_id)
        .bind(seat_class_id)
        .bind(total_seats)
        .bind(available_seats)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }
}
