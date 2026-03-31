//! Credential document management and internal e-signing API handlers.
//!
//! Route map (all under `/api/v1/credentials` and `/api/v1/esignatures`):
//!
//! ```text
//! GET    /credentials                        — paginated list          (ViewCredentials)
//! POST   /credentials                        — upload document         (ViewCredentials)
//! POST   /credentials/expire                 — run expiry sweep        (ApproveCredentials)
//! GET    /credentials/{id}                   — detail + auto-log view  (ViewCredentials)
//! PATCH  /credentials/{id}/review            — approve / reject        (ApproveCredentials)
//! GET    /credentials/{id}/audit             — credential audit trail  (ViewCredentials)
//! POST   /credentials/{id}/esign             — attach e-signature      (ApproveCredentials)
//!
//! GET    /esignatures/{entity_type}/{entity_id} — list sigs for entity (ViewCredentials)
//! POST   /esignatures                            — sign any entity      (ViewCredentials)
//! ```
//!
//! File upload uses `actix-multipart`.  Files are stored under
//! `$UPLOAD_DIR/contractors/{contractor_id}/{fingerprint_prefix}_{filename}`.
//! A SHA-256 fingerprint is computed over raw file bytes for duplicate detection.
//! Only `application/pdf`, `image/jpeg`, and `image/png` are accepted (max 10 MB).

use actix_multipart::Multipart;
use actix_web::{web, HttpResponse};
use bytes::BytesMut;
use chrono::{NaiveDate, Utc};
use futures::TryStreamExt;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use shared::rules::MAX_UPLOAD_BYTES;

use crate::auth::rbac::{RequireApproveCredentials, RequireViewCredentials};
use crate::db::audit::write_audit;
use crate::domain::credentials::{
    models::{CreateCredential, CreateEsignature},
    repo::{CredAuditRepo, CredentialRepo, EsignatureRepo},
};
use crate::domain::staffing::repo::ContractorRepo;
use crate::error::{AppError, AppResult};

// ── Request bodies ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListCredentialsQuery {
    pub contractor_id: Option<Uuid>,
    pub status:        Option<String>,
    pub page:          Option<i64>,
    pub per_page:      Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ReviewBody {
    pub status:       String,
    pub review_notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EsignBody {
    pub signer_name: String,
    pub signed_date: String, // YYYY-MM-DD
}

#[derive(Debug, Deserialize)]
pub struct CreateEsignBody {
    pub entity_type:  String,
    pub entity_id:    Uuid,
    pub signer_name:  String,
    pub signed_date:  String,
}

// ── Credential handlers ────────────────────────────────────────────────────────

/// GET /credentials — paginated list with optional filters.
pub async fn list_credentials(
    pool:  web::Data<sqlx::PgPool>,
    query: web::Query<ListCredentialsQuery>,
    _auth: RequireViewCredentials,
) -> AppResult<HttpResponse> {
    let q        = query.into_inner();
    let page     = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);

    use crate::domain::credentials::models::{ListCredentialsParams};
    use shared::PaginationParams;

    let params = ListCredentialsParams {
        contractor_id: q.contractor_id,
        status:        q.status,
        pagination:    PaginationParams {
            page:     Some(page),
            per_page: Some(per_page),
        },
    };
    let result = CredentialRepo::new(&pool).list(&params).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// GET /credentials/{id} — get full credential detail and log a "viewed" entry.
pub async fn get_credential(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    auth:  RequireViewCredentials,
) -> AppResult<HttpResponse> {
    let id   = path.into_inner();
    let cred = CredentialRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Credential {id}")))?;

    // Log access.
    CredAuditRepo::new(&pool)
        .insert(id, "viewed", Some(auth.id), auth.full_name.as_deref(), None, None)
        .await?;

    Ok(HttpResponse::Ok().json(&cred))
}

/// POST /credentials — upload a credential document via multipart form.
///
/// Form fields:
///   `contractor_id`  (text, UUID)
///   `document_type`  (text, e.g. "conductor_license")
///   `expires_at`     (text, YYYY-MM-DD, optional)
///   `file`           (binary, PDF/JPEG/PNG, ≤ 10 MB)
pub async fn upload_credential(
    pool:  web::Data<sqlx::PgPool>,
    mut mp: Multipart,
    auth:  RequireViewCredentials,
) -> AppResult<HttpResponse> {
    let mut contractor_id: Option<Uuid>   = None;
    let mut document_type: Option<String> = None;
    let mut expires_at:    Option<NaiveDate> = None;
    let mut file_name:     Option<String> = None;
    let mut file_bytes:    Option<BytesMut> = None;
    let mut mime_type:     Option<String> = None;

    // ── Parse multipart fields ─────────────────────────────────────────────
    while let Some(mut field) = mp
        .try_next()
        .await
        .map_err(|e| AppError::Validation(e.to_string()))?
    {
        let name = field.name().map(|s| s.to_owned()).unwrap_or_default();

        match name.as_str() {
            "contractor_id" => {
                let text = read_text_field(&mut field).await?;
                contractor_id = Some(
                    Uuid::parse_str(text.trim())
                        .map_err(|_| AppError::Validation("Invalid contractor_id UUID".into()))?,
                );
            }
            "document_type" => {
                document_type = Some(read_text_field(&mut field).await?);
            }
            "expires_at" => {
                let s = read_text_field(&mut field).await?;
                if !s.trim().is_empty() {
                    expires_at = Some(
                        NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
                            .map_err(|_| {
                                AppError::Validation(
                                    "expires_at must be YYYY-MM-DD".into(),
                                )
                            })?,
                    );
                }
            }
            "file" => {
                // Content-Type for this part.
                let ct = field
                    .content_type()
                    .map(|m| m.to_string())
                    .unwrap_or_default();
                mime_type = Some(ct);

                // Filename from Content-Disposition.
                file_name = field
                    .content_disposition()
                    .get_filename()
                    .map(|s| s.to_owned());

                // Stream bytes with size guard.
                let mut buf = BytesMut::new();
                while let Some(chunk) = field
                    .try_next()
                    .await
                    .map_err(|e| AppError::Validation(e.to_string()))?
                {
                    buf.extend_from_slice(&chunk);
                    if buf.len() > MAX_UPLOAD_BYTES as usize {
                        return Err(AppError::FileTooLarge);
                    }
                }
                file_bytes = Some(buf);
            }
            _ => {
                // Drain unknown fields.
                while let Some(_) = field
                    .try_next()
                    .await
                    .map_err(|e| AppError::Validation(e.to_string()))?
                {}
            }
        }
    }

    // ── Validate required fields ───────────────────────────────────────────
    let contractor_id = contractor_id
        .ok_or_else(|| AppError::Validation("contractor_id is required".into()))?;
    let document_type = document_type
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Validation("document_type is required".into()))?;
    let file_bytes = file_bytes
        .ok_or_else(|| AppError::Validation("file field is required".into()))?;
    let file_name = file_name
        .ok_or_else(|| AppError::Validation("file must include a filename".into()))?;
    let mime_type = mime_type.unwrap_or_default();

    if file_bytes.is_empty() {
        return Err(AppError::Validation("file is empty".into()));
    }

    // ── MIME type validation ───────────────────────────────────────────────
    if !matches!(
        mime_type.as_str(),
        "application/pdf" | "image/jpeg" | "image/png"
    ) {
        return Err(AppError::UnsupportedFileType(mime_type));
    }

    // ── SHA-256 fingerprint + duplicate check ──────────────────────────────
    let mut hasher = Sha256::new();
    hasher.update(&file_bytes);
    let fingerprint = hex::encode(hasher.finalize());

    if let Some(dup_id) = CredentialRepo::new(&pool)
        .find_by_fingerprint(&fingerprint)
        .await?
    {
        return Err(AppError::Conflict(format!(
            "Duplicate document — already stored as credential {dup_id}"
        )));
    }

    // ── Verify contractor exists ───────────────────────────────────────────
    ContractorRepo::new(&pool)
        .find_by_id(contractor_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Contractor {contractor_id}")))?;

    // ── Store file ─────────────────────────────────────────────────────────
    let upload_dir =
        std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "/app/uploads".to_owned());
    let ctr_dir = format!("{upload_dir}/contractors/{contractor_id}");
    tokio::fs::create_dir_all(&ctr_dir).await?;

    // Sanitize filename (strip path components and dangerous chars).
    let safe_name = file_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect::<String>();
    let fp_prefix = &fingerprint[..8];
    let stored_name    = format!("{fp_prefix}_{safe_name}");
    let relative_path  = format!("contractors/{contractor_id}/{stored_name}");
    let absolute_path  = format!("{upload_dir}/{relative_path}");

    tokio::fs::write(&absolute_path, &file_bytes).await?;

    // ── Create credential record ───────────────────────────────────────────
    let cmd = CreateCredential {
        contractor_id,
        document_type: document_type.clone(),
        file_name,
        file_path:       relative_path,
        file_size_bytes: file_bytes.len() as i32,
        mime_type,
        fingerprint,
        expires_at,
        uploaded_by: Some(auth.id),
    };
    let cred_id = CredentialRepo::new(&pool).create(&cmd).await?;

    // ── Audit records ──────────────────────────────────────────────────────
    CredAuditRepo::new(&pool)
        .insert(
            cred_id, "uploaded", Some(auth.id), None, None,
            Some(json!({ "via": "web_upload", "size_bytes": file_bytes.len() })),
        )
        .await?;

    write_audit(
        &pool,
        "credential_uploaded", "credential", Some(cred_id),
        Some(auth.id), "upload",
        None,
        Some(json!({ "document_type": &document_type, "contractor_id": contractor_id })),
        None,
    )
    .await;

    Ok(HttpResponse::Created().json(json!({ "id": cred_id })))
}

/// PATCH /credentials/{id}/review — approve or reject a pending credential.
pub async fn review_credential(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<ReviewBody>,
    auth:  RequireApproveCredentials,
) -> AppResult<HttpResponse> {
    let id   = path.into_inner();
    let cred = CredentialRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Credential {id}")))?;

    if cred.status != "pending" {
        return Err(AppError::Validation(format!(
            "Credential is already '{}' — only pending credentials can be reviewed",
            cred.status
        )));
    }
    if !matches!(body.status.as_str(), "approved" | "rejected") {
        return Err(AppError::Validation(
            "status must be 'approved' or 'rejected'".into(),
        ));
    }

    CredentialRepo::new(&pool)
        .review(id, &body.status, auth.id, body.review_notes.as_deref(), Utc::now())
        .await?;

    let action = body.status.as_str(); // "approved" | "rejected"
    let data = body.review_notes.as_ref().map(|n| json!({ "notes": n }));

    CredAuditRepo::new(&pool)
        .insert(id, action, Some(auth.id), None, None, data.clone())
        .await?;

    write_audit(
        &pool,
        "credential_reviewed", "credential", Some(id),
        Some(auth.id), action,
        Some(json!({ "status": "pending" })),
        Some(json!({ "status": &body.status, "notes": &body.review_notes })),
        None,
    )
    .await;

    Ok(HttpResponse::Ok().json(json!({ "ok": true, "status": &body.status })))
}

/// GET /credentials/{id}/audit — immutable audit trail for a credential.
pub async fn get_credential_audit(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    _auth: RequireViewCredentials,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    // Verify credential exists.
    CredentialRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Credential {id}")))?;

    let entries = CredAuditRepo::new(&pool).list_for_credential(id).await?;
    Ok(HttpResponse::Ok().json(entries))
}

/// POST /credentials/{id}/esign — attach an internal e-signature to a credential.
pub async fn esign_credential(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    body:  web::Json<EsignBody>,
    auth:  RequireApproveCredentials,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    CredentialRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Credential {id}")))?;

    if body.signer_name.trim().is_empty() {
        return Err(AppError::Validation("signer_name is required".into()));
    }
    let signed_date = NaiveDate::parse_from_str(&body.signed_date, "%Y-%m-%d")
        .map_err(|_| AppError::Validation("signed_date must be YYYY-MM-DD".into()))?;

    let cmd = CreateEsignature {
        entity_type:          "credential".to_owned(),
        entity_id:            id,
        signer_name:          body.signer_name.trim().to_owned(),
        signed_date,
        signature_image_path: None,
    };
    let sig_id = EsignatureRepo::new(&pool).create(&cmd).await?;

    CredAuditRepo::new(&pool)
        .insert(
            id, "esigned", Some(auth.id),
            Some(body.signer_name.trim()),
            None,
            Some(json!({ "sig_id": sig_id, "signed_date": &body.signed_date })),
        )
        .await?;

    write_audit(
        &pool,
        "credential_esigned", "credential", Some(id),
        Some(auth.id), "esign",
        None,
        Some(json!({ "sig_id": sig_id, "signer_name": body.signer_name.trim() })),
        None,
    )
    .await;

    Ok(HttpResponse::Created().json(json!({ "id": sig_id })))
}

/// POST /credentials/expire — mark approved credentials whose expiry date has
/// passed as `expired`.  Returns the number of rows updated.
pub async fn run_expiry_sweep(
    pool:  web::Data<sqlx::PgPool>,
    auth:  RequireApproveCredentials,
) -> AppResult<HttpResponse> {
    let count = CredentialRepo::new(&pool).expire_outdated().await?;
    write_audit(
        &pool,
        "credential_expiry_sweep", "credential", None,
        Some(auth.id), "expire_sweep",
        None,
        Some(json!({ "expired_count": count })),
        None,
    )
    .await;
    Ok(HttpResponse::Ok().json(json!({ "ok": true, "expired_count": count })))
}

// ── E-signature handlers ───────────────────────────────────────────────────────

/// GET /esignatures/{entity_type}/{entity_id} — list all e-signatures for an entity.
pub async fn list_esignatures(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<(String, Uuid)>,
    _auth: RequireViewCredentials,
) -> AppResult<HttpResponse> {
    let (entity_type, entity_id) = path.into_inner();
    if !matches!(entity_type.as_str(), "credential" | "order" | "assignment") {
        return Err(AppError::Validation(
            "entity_type must be credential | order | assignment".into(),
        ));
    }
    let sigs = EsignatureRepo::new(&pool)
        .list_for(&entity_type, entity_id)
        .await?;
    Ok(HttpResponse::Ok().json(sigs))
}

/// POST /esignatures — create an internal e-signature for any supported entity.
pub async fn create_esignature(
    pool:  web::Data<sqlx::PgPool>,
    body:  web::Json<CreateEsignBody>,
    auth:  RequireViewCredentials,
) -> AppResult<HttpResponse> {
    if !matches!(
        body.entity_type.as_str(),
        "credential" | "order" | "assignment"
    ) {
        return Err(AppError::Validation(
            "entity_type must be credential | order | assignment".into(),
        ));
    }
    if body.signer_name.trim().is_empty() {
        return Err(AppError::Validation("signer_name is required".into()));
    }
    let signed_date = NaiveDate::parse_from_str(&body.signed_date, "%Y-%m-%d")
        .map_err(|_| AppError::Validation("signed_date must be YYYY-MM-DD".into()))?;

    let cmd = CreateEsignature {
        entity_type:          body.entity_type.clone(),
        entity_id:            body.entity_id,
        signer_name:          body.signer_name.trim().to_owned(),
        signed_date,
        signature_image_path: None,
    };
    let sig_id = EsignatureRepo::new(&pool).create(&cmd).await?;

    write_audit(
        &pool,
        "esignature_created", &body.entity_type, Some(body.entity_id),
        Some(auth.id), "esign",
        None,
        Some(json!({ "sig_id": sig_id, "signer_name": body.signer_name.trim() })),
        None,
    )
    .await;

    Ok(HttpResponse::Created().json(json!({ "id": sig_id })))
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Drain a non-file multipart field into a UTF-8 String.
async fn read_text_field(field: &mut actix_multipart::Field) -> AppResult<String> {
    let mut buf = BytesMut::new();
    while let Some(chunk) = field
        .try_next()
        .await
        .map_err(|e| AppError::Validation(e.to_string()))?
    {
        buf.extend_from_slice(&chunk);
        if buf.len() > 4096 {
            return Err(AppError::Validation("Form field value too long".into()));
        }
    }
    String::from_utf8(buf.to_vec())
        .map_err(|_| AppError::Validation("Form field is not valid UTF-8".into()))
}
