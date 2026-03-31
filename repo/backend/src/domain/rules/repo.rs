//! Business rules repository.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use super::models::BusinessRule;

pub struct BusinessRuleRepo<'a>(pub &'a PgPool);

impl<'a> BusinessRuleRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn list(&self) -> AppResult<Vec<BusinessRule>> {
        sqlx::query_as(
            "SELECT id, rule_key, rule_value, description, updated_by, updated_at
             FROM business_rules ORDER BY rule_key",
        )
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn get(&self, key: &str) -> AppResult<Option<BusinessRule>> {
        sqlx::query_as(
            "SELECT id, rule_key, rule_value, description, updated_by, updated_at
             FROM business_rules WHERE rule_key = $1",
        )
        .bind(key)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    /// Get the raw string value for `key`, or return `default` if missing.
    pub async fn get_value(&self, key: &str, default: &str) -> String {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT rule_value FROM business_rules WHERE rule_key = $1",
        )
        .bind(key)
        .fetch_optional(self.0)
        .await
        .ok()
        .flatten();
        row.map(|r| r.0).unwrap_or_else(|| default.to_owned())
    }

    pub async fn set(
        &self,
        key:        &str,
        value:      &str,
        updated_by: Option<Uuid>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO business_rules (rule_key, rule_value, updated_by, updated_at)
             VALUES ($1, $2, $3, NOW())
             ON CONFLICT (rule_key) DO UPDATE
                 SET rule_value = EXCLUDED.rule_value,
                     updated_by = EXCLUDED.updated_by,
                     updated_at = NOW()",
        )
        .bind(key)
        .bind(value)
        .bind(updated_by)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }
}
