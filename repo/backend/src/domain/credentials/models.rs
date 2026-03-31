//! Credential and document management domain value types.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use shared::{CredentialStatus, PaginationParams};

// ── Credential ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Credential {
    pub id:              Uuid,
    pub contractor_id:   Uuid,
    pub document_type:   String,
    pub file_name:       String,
    pub file_path:       String,
    pub file_size_bytes: i32,
    pub mime_type:       String,
    pub fingerprint:     String,
    pub expires_at:      Option<NaiveDate>,
    /// Raw DB value — use `Credential::status_enum()`.
    pub status:          String,
    pub uploaded_by:     Option<Uuid>,
    pub uploaded_at:     DateTime<Utc>,
    pub reviewed_by:     Option<Uuid>,
    pub reviewed_at:     Option<DateTime<Utc>>,
    pub review_notes:    Option<String>,
}

impl Credential {
    pub fn status_enum(&self) -> CredentialStatus {
        match self.status.as_str() {
            "approved" => CredentialStatus::Approved,
            "rejected" => CredentialStatus::Rejected,
            "expired"  => CredentialStatus::Expired,
            _          => CredentialStatus::Pending,
        }
    }
}

// ── CredentialAuditEntry ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CredentialAuditEntry {
    pub id:            i64,
    pub credential_id: Uuid,
    pub action:        String,
    pub performed_by:  Option<Uuid>,
    pub viewer_name:   Option<String>,
    pub ip_address:    Option<String>,
    pub data:          Option<Value>,
    pub created_at:    DateTime<Utc>,
}

// ── Esignature ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Esignature {
    pub id:                   Uuid,
    pub entity_type:          String,
    pub entity_id:            Uuid,
    pub signer_name:          String,
    pub signed_date:          NaiveDate,
    pub signature_image_path: Option<String>,
    pub created_at:           DateTime<Utc>,
}

// ── Command types ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct CreateCredential {
    pub contractor_id:   Uuid,
    pub document_type:   String,
    pub file_name:       String,
    pub file_path:       String,
    pub file_size_bytes: i32,
    pub mime_type:       String,
    pub fingerprint:     String,
    pub expires_at:      Option<NaiveDate>,
    pub uploaded_by:     Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ReviewCredential {
    pub status:       String,
    pub review_notes: Option<String>,
}

#[derive(Debug)]
pub struct CreateEsignature {
    pub entity_type:          String,
    pub entity_id:            Uuid,
    pub signer_name:          String,
    pub signed_date:          NaiveDate,
    pub signature_image_path: Option<String>,
}

/// Query params for listing credentials.
#[derive(Debug, Default, Deserialize)]
pub struct ListCredentialsParams {
    pub contractor_id: Option<Uuid>,
    pub status:        Option<String>,
    #[serde(flatten)]
    pub pagination:    PaginationParams,
}
