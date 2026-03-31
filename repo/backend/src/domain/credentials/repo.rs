//! Credential and document management repositories.

use chrono::DateTime;
use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use shared::PaginatedResponse;

use super::models::*;

// ── CredentialRepo ────────────────────────────────────────────────────────────

pub struct CredentialRepo<'a>(pub &'a PgPool);

impl<'a> CredentialRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<Credential>> {
        sqlx::query_as(
            "SELECT id, contractor_id, document_type, file_name, file_path,
                    file_size_bytes, mime_type, fingerprint, expires_at, status,
                    uploaded_by, uploaded_at, reviewed_by, reviewed_at, review_notes
             FROM credentials WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)
    }

    pub async fn find_by_fingerprint(&self, fp: &str) -> AppResult<Option<Uuid>> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM credentials WHERE fingerprint = $1",
        )
        .bind(fp)
        .fetch_optional(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.map(|r| r.0))
    }

    pub async fn list(
        &self,
        params: &ListCredentialsParams,
    ) -> AppResult<PaginatedResponse<Credential>> {
        let per_page = params.pagination.per_page();
        let offset   = params.pagination.offset();
        let page     = params.pagination.page();

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM credentials
             WHERE ($1::UUID IS NULL OR contractor_id = $1)
               AND ($2::TEXT IS NULL OR status        = $2)",
        )
        .bind(params.contractor_id)
        .bind(&params.status)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;

        let items: Vec<Credential> = sqlx::query_as(
            "SELECT id, contractor_id, document_type, file_name, file_path,
                    file_size_bytes, mime_type, fingerprint, expires_at, status,
                    uploaded_by, uploaded_at, reviewed_by, reviewed_at, review_notes
             FROM credentials
             WHERE ($1::UUID IS NULL OR contractor_id = $1)
               AND ($2::TEXT IS NULL OR status        = $2)
             ORDER BY uploaded_at DESC
             LIMIT $3 OFFSET $4",
        )
        .bind(params.contractor_id)
        .bind(&params.status)
        .bind(per_page)
        .bind(offset)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)?;

        let total_pages = (total + per_page - 1) / per_page;
        Ok(PaginatedResponse { items, total, page, per_page, total_pages })
    }

    pub async fn create(&self, cmd: &CreateCredential) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO credentials
                 (contractor_id, document_type, file_name, file_path,
                  file_size_bytes, mime_type, fingerprint, expires_at, uploaded_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             RETURNING id",
        )
        .bind(cmd.contractor_id)
        .bind(&cmd.document_type)
        .bind(&cmd.file_name)
        .bind(&cmd.file_path)
        .bind(cmd.file_size_bytes)
        .bind(&cmd.mime_type)
        .bind(&cmd.fingerprint)
        .bind(cmd.expires_at)
        .bind(cmd.uploaded_by)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }

    pub async fn review(
        &self,
        id:          Uuid,
        status:      &str,
        reviewer_id: Uuid,
        notes:       Option<&str>,
        at:          DateTime<Utc>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE credentials
             SET status = $1, reviewed_by = $2, reviewed_at = $3,
                 review_notes = $4
             WHERE id = $5 AND status = 'pending'",
        )
        .bind(status)
        .bind(reviewer_id)
        .bind(at)
        .bind(notes)
        .bind(id)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    /// Mark credentials as expired where expires_at < today.
    pub async fn expire_outdated(&self) -> AppResult<u64> {
        let result = sqlx::query(
            "UPDATE credentials SET status = 'expired'
             WHERE status = 'approved' AND expires_at < CURRENT_DATE",
        )
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(result.rows_affected())
    }
}

// ── CredAuditRepo ─────────────────────────────────────────────────────────────

pub struct CredAuditRepo<'a>(pub &'a PgPool);

impl<'a> CredAuditRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn insert(
        &self,
        credential_id: Uuid,
        action:        &str,
        performed_by:  Option<Uuid>,
        viewer_name:   Option<&str>,
        ip_address:    Option<&str>,
        data:          Option<Value>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO credential_audit_log
                 (credential_id, action, performed_by, viewer_name, ip_address, data)
             VALUES ($1, $2, $3, $4, $5::inet, $6)",
        )
        .bind(credential_id)
        .bind(action)
        .bind(performed_by)
        .bind(viewer_name)
        .bind(ip_address)
        .bind(data)
        .execute(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn list_for_credential(
        &self,
        credential_id: Uuid,
    ) -> AppResult<Vec<CredentialAuditEntry>> {
        sqlx::query_as(
            "SELECT id, credential_id, action, performed_by, viewer_name,
                    ip_address::text as ip_address, data, created_at
             FROM credential_audit_log
             WHERE credential_id = $1
             ORDER BY created_at DESC",
        )
        .bind(credential_id)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }
}

// ── EsignatureRepo ────────────────────────────────────────────────────────────

pub struct EsignatureRepo<'a>(pub &'a PgPool);

impl<'a> EsignatureRepo<'a> {
    pub fn new(pool: &'a PgPool) -> Self { Self(pool) }

    pub async fn create(&self, cmd: &CreateEsignature) -> AppResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO esignatures
                 (entity_type, entity_id, signer_name, signed_date,
                  signature_image_path)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
        )
        .bind(&cmd.entity_type)
        .bind(cmd.entity_id)
        .bind(&cmd.signer_name)
        .bind(cmd.signed_date)
        .bind(&cmd.signature_image_path)
        .fetch_one(self.0)
        .await
        .map_err(AppError::Database)?;
        Ok(row.0)
    }

    pub async fn list_for(
        &self,
        entity_type: &str,
        entity_id:   Uuid,
    ) -> AppResult<Vec<Esignature>> {
        sqlx::query_as(
            "SELECT id, entity_type, entity_id, signer_name, signed_date,
                    signature_image_path, created_at
             FROM esignatures
             WHERE entity_type = $1 AND entity_id = $2
             ORDER BY created_at DESC",
        )
        .bind(entity_type)
        .bind(entity_id)
        .fetch_all(self.0)
        .await
        .map_err(AppError::Database)
    }
}
