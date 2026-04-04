//! Typed API client for credential document management and e-signing.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wasm_bindgen::JsValue;

use super::{parse_json, signed_request, ApiError, ApiResult};

// Re-use the pagination wrapper from ops.
pub use crate::api::ops::Page;

// ── Response types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct CredentialRow {
    pub id:              Uuid,
    pub contractor_id:   Uuid,
    pub document_type:   String,
    pub file_name:       String,
    pub file_path:       String,
    pub file_size_bytes: i32,
    pub mime_type:       String,
    pub fingerprint:     String,
    pub expires_at:      Option<String>,
    pub status:          String,
    pub uploaded_by:     Option<Uuid>,
    pub uploaded_at:     String,
    pub reviewed_by:     Option<Uuid>,
    pub reviewed_at:     Option<String>,
    pub review_notes:    Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CredentialAuditEntry {
    pub id:            i64,
    pub credential_id: Uuid,
    pub action:        String,
    pub performed_by:  Option<Uuid>,
    pub viewer_name:   Option<String>,
    pub ip_address:    Option<String>,
    pub data:          Option<serde_json::Value>,
    pub created_at:    String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EsignatureRow {
    pub id:                   Uuid,
    pub entity_type:          String,
    pub entity_id:            Uuid,
    pub signer_name:          String,
    pub signed_date:          String,
    pub signature_image_path: Option<String>,
    pub created_at:           String,
}

// ── Request bodies ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ReviewBody {
    pub status:       String,
    pub review_notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EsignBody {
    pub signer_name: String,
    pub signed_date: String,
}

#[derive(Debug, Serialize)]
pub struct CreateEsignBody {
    pub entity_type: String,
    pub entity_id:   Uuid,
    pub signer_name: String,
    pub signed_date: String,
}

// ── API functions ──────────────────────────────────────────────────────────────

/// List credentials with optional status filter, paginated.
pub async fn list_credentials(
    token:         &str,
    contractor_id: Option<Uuid>,
    status:        Option<&str>,
    page:          i64,
) -> ApiResult<Page<CredentialRow>> {
    let mut path = format!("/api/v1/credentials?page={page}&per_page=20");
    if let Some(cid) = contractor_id {
        path.push_str(&format!("&contractor_id={cid}"));
    }
    if let Some(s) = status {
        path.push_str(&format!("&status={s}"));
    }
    let resp = signed_request("GET", &path, token, None::<&()>).await?;
    parse_json(resp).await
}

/// Backend response shape for a single credential GET.
/// Backend returns `{ "credential": {...}, "watermark": "..." }`.
#[derive(Debug, Clone, Deserialize)]
pub struct CredentialDetail {
    pub credential: CredentialRow,
    pub watermark:  String,
}

/// Get a single credential (also logs a "viewed" audit entry on the backend).
pub async fn get_credential(token: &str, id: Uuid) -> ApiResult<CredentialDetail> {
    let path = format!("/api/v1/credentials/{id}");
    let resp = signed_request("GET", &path, token, None::<&()>).await?;
    parse_json(resp).await
}

/// Upload a new credential document via multipart form.
///
/// Caller builds a `web_sys::FormData` with the fields:
///   `contractor_id`, `document_type`, `expires_at` (optional), `file`.
pub async fn upload_credential(
    token:     &str,
    form_data: web_sys::FormData,
) -> ApiResult<serde_json::Value> {
    let path = "/api/v1/credentials";
    let ts   = super::unix_now();
    let msg  = format!("POST\n{path}\n{ts}");
    let sig  = super::hmac_sign(token, &msg);

    let resp = gloo_net::http::Request::post(path)
        .header("Authorization", &format!("Bearer {token}"))
        .header("X-RailOps-Ts",  &ts.to_string())
        .header("X-RailOps-Sig", &sig)
        // Pass FormData as JsValue — browser sets Content-Type + boundary automatically.
        .body(JsValue::from(form_data))
        .map_err(|e| ApiError { status: 0, message: e.to_string() })?
        .send()
        .await
        .map_err(|e| ApiError { status: 0, message: e.to_string() })?;

    parse_json(resp).await
}

/// Approve or reject a pending credential.
pub async fn review_credential(
    token: &str,
    id:    Uuid,
    body:  &ReviewBody,
) -> ApiResult<serde_json::Value> {
    let path = format!("/api/v1/credentials/{id}/review");
    let resp = signed_request("PATCH", &path, token, Some(body)).await?;
    parse_json(resp).await
}

/// Fetch the immutable credential audit trail.
pub async fn get_credential_audit(
    token: &str,
    id:    Uuid,
) -> ApiResult<Vec<CredentialAuditEntry>> {
    let path = format!("/api/v1/credentials/{id}/audit");
    let resp = signed_request("GET", &path, token, None::<&()>).await?;
    parse_json(resp).await
}

/// Attach an internal e-signature to a credential.
pub async fn esign_credential(
    token: &str,
    id:    Uuid,
    body:  &EsignBody,
) -> ApiResult<serde_json::Value> {
    let path = format!("/api/v1/credentials/{id}/esign");
    let resp = signed_request("POST", &path, token, Some(body)).await?;
    parse_json(resp).await
}

/// Mark all approved credentials whose expiry date has passed as expired.
pub async fn run_expiry_sweep(token: &str) -> ApiResult<serde_json::Value> {
    let resp =
        signed_request("POST", "/api/v1/credentials/expire", token, None::<&()>).await?;
    parse_json(resp).await
}

/// List e-signatures for any supported entity (credential | order | assignment).
pub async fn list_esignatures(
    token:       &str,
    entity_type: &str,
    entity_id:   Uuid,
) -> ApiResult<Vec<EsignatureRow>> {
    let path = format!("/api/v1/esignatures/{entity_type}/{entity_id}");
    let resp = signed_request("GET", &path, token, None::<&()>).await?;
    parse_json(resp).await
}

/// Create an e-signature for any supported entity.
pub async fn create_esignature(
    token: &str,
    body:  &CreateEsignBody,
) -> ApiResult<serde_json::Value> {
    let resp =
        signed_request("POST", "/api/v1/esignatures", token, Some(body)).await?;
    parse_json(resp).await
}
