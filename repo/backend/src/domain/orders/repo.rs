//! Orders domain repositories.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use shared::PaginatedResponse;

use super::models::*;

// ── PassengerRepo ─────────────────────────────────────────────────────────────

pub struct PassengerRepo<'a>(pub &'a PgPool);

impl<'a> PassengerRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<Passenger>> {
        sqlx::query_as(
            "SELECT id, full_name, phone_last4, created_at,
                    pii_purge_requested_at, pii_purged_at
             FROM passengers WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    /// Fuzzy name search using pg_trgm GIN index.
    pub async fn search(
        &self,
        query:    &str,
        page:     i64,
        per_page: i64,
    ) -> AppResult<PaginatedResponse<Passenger>> {
        let per_page = per_page.clamp(1, 100);
        let offset   = (page.max(1) - 1) * per_page;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM passengers
             WHERE full_name ILIKE '%' || $1 || '%'
               AND pii_purged_at IS NULL",
        )
        .bind(query)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<Passenger> = sqlx::query_as(
            "SELECT id, full_name, phone_last4, created_at,
                    pii_purge_requested_at, pii_purged_at
             FROM passengers
             WHERE full_name ILIKE '%' || $1 || '%'
               AND pii_purged_at IS NULL
             ORDER BY similarity(full_name, $1) DESC, full_name
             LIMIT $2 OFFSET $3",
        )
        .bind(query)
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }

    pub async fn create(&self, new: &NewPassenger) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO passengers
                 (full_name, phone_encrypted, phone_last4, email_encrypted)
             VALUES ($1, $2, $3, $4)
             RETURNING id",
        )
        .bind(&new.full_name)
        .bind(&new.phone_encrypted)
        .bind(&new.phone_last4)
        .bind(&new.email_encrypted)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }

    /// Mark a PII purge request; actual purge happens via a scheduled job.
    pub async fn request_pii_purge(&self, id: Uuid) -> AppResult<()> {
        sqlx::query(
            "UPDATE passengers
             SET pii_purge_requested_at = NOW()
             WHERE id = $1 AND pii_purge_requested_at IS NULL",
        )
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    /// Execute the purge: overwrite encrypted fields with NULL.
    pub async fn execute_pii_purge(&self, id: Uuid) -> AppResult<()> {
        sqlx::query(
            "UPDATE passengers
             SET phone_encrypted = NULL, email_encrypted = NULL,
                 phone_last4 = NULL, pii_purged_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }
}

// ── OrderRepo ─────────────────────────────────────────────────────────────────

pub struct OrderRepo<'a>(pub &'a PgPool);

impl<'a> OrderRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<Order>> {
        sqlx::query_as(
            "SELECT id, order_number, passenger_id, schedule_id, seat_class_id,
                    seat_number, status, fare_amount, fee_override_amount,
                    fee_override_reason, hold_expires_at, cancellation_reason,
                    refund_amount, disruption_flag, created_at, updated_at,
                    created_by
             FROM orders WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn find_by_number(&self, number: &str) -> AppResult<Option<Order>> {
        sqlx::query_as(
            "SELECT id, order_number, passenger_id, schedule_id, seat_class_id,
                    seat_number, status, fare_amount, fee_override_amount,
                    fee_override_reason, hold_expires_at, cancellation_reason,
                    refund_amount, disruption_flag, created_at, updated_at,
                    created_by
             FROM orders WHERE order_number = $1",
        )
        .bind(number)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    /// Rich detail view (joins passenger + schedule + route + seat_class).
    pub async fn find_detail(&self, id: Uuid) -> AppResult<Option<OrderDetail>> {
        sqlx::query_as(
            "SELECT o.id, o.order_number, o.status, o.fare_amount,
                    o.disruption_flag, o.seat_number, o.hold_expires_at,
                    o.created_at,
                    p.id  AS passenger_id, p.full_name AS passenger_name,
                    p.phone_last4,
                    s.id  AS schedule_id, s.train_number,
                    s.departure_time, s.arrival_time,
                    r.route_code, r.name AS route_name,
                    sc.code AS seat_class_code, sc.name AS seat_class_name
             FROM orders o
             JOIN passengers p  ON p.id  = o.passenger_id
             JOIN schedules  s  ON s.id  = o.schedule_id
             JOIN routes     r  ON r.id  = s.route_id
             JOIN seat_classes sc ON sc.id = o.seat_class_id
             WHERE o.id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn list(
        &self,
        params: &ListOrdersParams,
    ) -> AppResult<PaginatedResponse<Order>> {
        let per_page = params.pagination.per_page();
        let offset   = params.pagination.offset();
        let page     = params.pagination.page();

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM orders
             WHERE ($1::UUID IS NULL OR passenger_id = $1)
               AND ($2::UUID IS NULL OR schedule_id  = $2)
               AND ($3::TEXT IS NULL OR status       = $3)",
        )
        .bind(params.passenger_id)
        .bind(params.schedule_id)
        .bind(&params.status)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<Order> = sqlx::query_as(
            "SELECT id, order_number, passenger_id, schedule_id, seat_class_id,
                    seat_number, status, fare_amount, fee_override_amount,
                    fee_override_reason, hold_expires_at, cancellation_reason,
                    refund_amount, disruption_flag, created_at, updated_at,
                    created_by
             FROM orders
             WHERE ($1::UUID IS NULL OR passenger_id = $1)
               AND ($2::UUID IS NULL OR schedule_id  = $2)
               AND ($3::TEXT IS NULL OR status       = $3)
             ORDER BY created_at DESC
             LIMIT $4 OFFSET $5",
        )
        .bind(params.passenger_id)
        .bind(params.schedule_id)
        .bind(&params.status)
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }

    /// Creates a new order using the DB sequence for order_number.
    /// Returns `(new_id, order_number)`.
    pub async fn create(&self, new: &NewOrder) -> AppResult<(Uuid, String)> {
        let row: (Uuid, String) = sqlx::query_as(
            "INSERT INTO orders
                 (order_number, passenger_id, schedule_id, seat_class_id,
                  seat_number, fare_amount, created_by)
             VALUES (
                 'ORD-' || LPAD(nextval('order_number_seq')::text, 6, '0'),
                 $1, $2, $3, $4, $5, $6
             )
             RETURNING id, order_number",
        )
        .bind(new.passenger_id)
        .bind(new.schedule_id)
        .bind(new.seat_class_id)
        .bind(&new.seat_number)
        .bind(new.fare_amount)
        .bind(new.created_by)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row)
    }

    pub async fn confirm(&self, id: Uuid) -> AppResult<()> {
        sqlx::query(
            "UPDATE orders
             SET status = 'confirmed', hold_expires_at = NULL, updated_at = NOW()
             WHERE id = $1 AND status IN ('pending','held')",
        )
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn hold(&self, id: Uuid, expires_at: chrono::DateTime<chrono::Utc>) -> AppResult<()> {
        sqlx::query(
            "UPDATE orders
             SET status = 'held', hold_expires_at = $1, updated_at = NOW()
             WHERE id = $2 AND status = 'pending'",
        )
        .bind(expires_at)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn cancel(
        &self,
        id:              Uuid,
        reason:          &str,
        disruption_flag: bool,
        refund_amount:   Option<rust_decimal::Decimal>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE orders
             SET status = 'cancelled',
                 cancellation_reason = $1,
                 disruption_flag = $2,
                 refund_amount = $3,
                 hold_expires_at = NULL,
                 updated_at = NOW()
             WHERE id = $4",
        )
        .bind(reason)
        .bind(disruption_flag)
        .bind(refund_amount)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn set_refunded(&self, id: Uuid, amount: rust_decimal::Decimal) -> AppResult<()> {
        sqlx::query(
            "UPDATE orders
             SET status = 'refunded', refund_amount = $1, updated_at = NOW()
             WHERE id = $2",
        )
        .bind(amount)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn apply_fee_override(
        &self,
        id:     Uuid,
        amount: rust_decimal::Decimal,
        reason: &str,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE orders
             SET fee_override_amount = $1, fee_override_reason = $2,
                 updated_at = NOW()
             WHERE id = $3",
        )
        .bind(amount)
        .bind(reason)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn set_disruption(&self, id: Uuid) -> AppResult<()> {
        sqlx::query(
            "UPDATE orders SET disruption_flag = TRUE, updated_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    /// Batch-expire all `held` orders whose `hold_expires_at` has passed.
    /// Returns the number of orders expired.
    pub async fn expire_holds(&self) -> AppResult<u64> {
        let result = sqlx::query(
            "UPDATE orders
             SET status = 'cancelled',
                 cancellation_reason = 'Hold expired',
                 updated_at = NOW()
             WHERE status = 'held' AND hold_expires_at < NOW()",
        )
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(result.rows_affected())
    }
}

// ── OrderEventRepo ────────────────────────────────────────────────────────────

pub struct OrderEventRepo<'a>(pub &'a PgPool);

impl<'a> OrderEventRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn insert(&self, ev: &NewOrderEvent) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO order_events
                 (order_id, event_type, performed_by, reason, data)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(ev.order_id)
        .bind(&ev.event_type)
        .bind(ev.performed_by)
        .bind(&ev.reason)
        .bind(&ev.data)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn list_for_order(&self, order_id: Uuid) -> AppResult<Vec<OrderEvent>> {
        sqlx::query_as(
            "SELECT id, order_id, event_type, performed_by, reason, data, created_at
             FROM order_events
             WHERE order_id = $1
             ORDER BY created_at ASC",
        )
        .bind(order_id)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }
}
