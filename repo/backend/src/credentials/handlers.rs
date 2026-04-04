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
use crate::db::audit::write_audit_required;
use crate::domain::credentials::{
    models::{CreateCredential, CreateEsignature},
    repo::{CredAuditRepo, CredentialRepo, EsignatureRepo},
};
use crate::domain::staffing::repo::ContractorRepo;
use crate::error::{AppError, AppResult};

// ── Object-level access control ───────────────────────────────────────────────

/// Enforce object-scope authorization for credential document access.
///
/// Policy:
/// - Admin / Dispatcher / OpsAgent: unrestricted access to all credentials
/// - Users with a linked contractor profile: can only access credentials
///   belonging to their own contractor record
/// - Any other user with ViewCredentials: denied (no contractor link = no scope)
///
/// Returns `Ok(())` if access is permitted, or `Err(Forbidden)` otherwise.
async fn enforce_credential_scope(
    pool:           &sqlx::PgPool,
    auth_user_id:   Uuid,
    auth_role:      &shared::UserRole,
    credential_contractor_id: Uuid,
) -> AppResult<()> {
    // Admin, Dispatcher, and OpsAgent have broad access.
    if matches!(auth_role, shared::UserRole::Admin | shared::UserRole::Dispatcher | shared::UserRole::OpsAgent) {
        return Ok(());
    }

    // For other roles: check if the user has a linked contractor profile
    // and whether that contractor owns this credential.
    let linked: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM contractors WHERE user_id = $1"
    )
    .bind(auth_user_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?;

    match linked {
        Some((contractor_id,)) if contractor_id == credential_contractor_id => Ok(()),
        _ => Err(AppError::Forbidden(
            "You do not have access to this credential document".into()
        )),
    }
}

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
    pub signature_data: Option<String>, // base64 encoded drawn signature
}

#[derive(Debug, Deserialize)]
pub struct CreateEsignBody {
    pub entity_type:  String,
    pub entity_id:    Uuid,
    pub signer_name:  String,
    pub signed_date:  String,
    pub signature_data: Option<String>,
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

    // Object-level scope check.
    enforce_credential_scope(&pool, auth.id, &auth.role, cred.contractor_id).await?;

    // Log access with watermark info.
    let watermark = format!(
        "Viewed by {} (user:{}) at {}",
        auth.full_name.as_deref().unwrap_or(&auth.username),
        auth.id,
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
    );
    CredAuditRepo::new(&pool)
        .insert(id, "viewed", Some(auth.id), auth.full_name.as_deref(), None,
            Some(json!({"watermark": &watermark})))
        .await?;

    Ok(HttpResponse::Ok().json(json!({
        "credential": &cred,
        "watermark": &watermark,
    })))
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
                // content_disposition() returns Option<&ContentDisposition>
                // in actix-multipart 0.7.x; use and_then to reach get_filename.
                file_name = field
                    .content_disposition()
                    .and_then(|cd| cd.get_filename())
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

    // ── MIME type validation (declared type) ──────────────────────────────
    if !matches!(
        mime_type.as_str(),
        "application/pdf" | "image/jpeg" | "image/png"
    ) {
        return Err(AppError::UnsupportedFileType(mime_type));
    }

    // ── Magic-byte validation (server-side; do not trust client MIME) ─────
    if !validate_magic_bytes(&file_bytes, &mime_type) {
        return Err(AppError::UnsupportedFileType(format!(
            "File content does not match declared MIME type '{mime_type}'. \
             Upload a valid PDF, JPEG, or PNG."
        )));
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

    // Encrypt file at rest using AES-256-GCM
    let encrypted = encrypt_file_bytes(&file_bytes, &upload_secret())?;
    tokio::fs::write(&absolute_path, &encrypted).await?;

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

    write_audit_required(
        &pool,
        "credential_uploaded", "credential", Some(cred_id),
        Some(auth.id), "upload",
        None,
        Some(json!({ "document_type": &document_type, "contractor_id": contractor_id })),
        None,
    )
    .await?;

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

    write_audit_required(
        &pool,
        "credential_reviewed", "credential", Some(id),
        Some(auth.id), action,
        Some(json!({ "status": "pending" })),
        Some(json!({ "status": &body.status, "notes": &body.review_notes })),
        None,
    )
    .await?;

    Ok(HttpResponse::Ok().json(json!({ "ok": true, "status": &body.status })))
}

/// GET /credentials/{id}/audit — immutable audit trail for a credential.
pub async fn get_credential_audit(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    auth:  RequireViewCredentials,
) -> AppResult<HttpResponse> {
    let id = path.into_inner();
    // Verify credential exists.
    let cred = CredentialRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Credential {id}")))?;

    // Object-level scope check.
    enforce_credential_scope(&pool, auth.id, &auth.role, cred.contractor_id).await?;

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

    // Save drawn signature if provided — fail the request if storage fails.
    let sig_path = if let Some(ref sig_data) = body.signature_data {
        if !sig_data.is_empty() {
            let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "/app/uploads".to_owned());
            let sig_dir = format!("{upload_dir}/signatures");
            tokio::fs::create_dir_all(&sig_dir).await?;
            let ts = chrono::Utc::now().timestamp();
            let sig_file = format!("{sig_dir}/{id}_{ts}.svg");
            tokio::fs::write(&sig_file, sig_data.as_bytes()).await?;
            Some(format!("signatures/{id}_{ts}.svg"))
        } else { None }
    } else { None };

    let cmd = CreateEsignature {
        entity_type:          "credential".to_owned(),
        entity_id:            id,
        signer_name:          body.signer_name.trim().to_owned(),
        signed_date,
        signature_image_path: sig_path,
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

    write_audit_required(
        &pool,
        "credential_esigned", "credential", Some(id),
        Some(auth.id), "esign",
        None,
        Some(json!({ "sig_id": sig_id, "signer_name": body.signer_name.trim() })),
        None,
    )
    .await?;

    Ok(HttpResponse::Created().json(json!({ "id": sig_id })))
}

/// POST /credentials/expire — mark approved credentials whose expiry date has
/// passed as `expired`.  Returns the number of rows updated.
pub async fn run_expiry_sweep(
    pool:  web::Data<sqlx::PgPool>,
    auth:  RequireApproveCredentials,
) -> AppResult<HttpResponse> {
    let count = CredentialRepo::new(&pool).expire_outdated().await?;
    write_audit_required(
        &pool,
        "credential_expiry_sweep", "credential", None,
        Some(auth.id), "expire_sweep",
        None,
        Some(json!({ "expired_count": count })),
        None,
    )
    .await?;
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
///
/// Requires `ApproveCredentials` (Admin or Dispatcher).  Credential viewers
/// (OpsAgent) may not perform write-sign actions.
pub async fn create_esignature(
    pool:  web::Data<sqlx::PgPool>,
    body:  web::Json<CreateEsignBody>,
    auth:  RequireApproveCredentials,
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

    // Validate that the target entity exists before creating an orphaned signature.
    let entity_exists: bool = match body.entity_type.as_str() {
        "credential" => sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM credentials WHERE id = $1)",
        )
        .bind(body.entity_id)
        .fetch_one(pool.get_ref())
        .await
        .map_err(AppError::Database)?,

        "order" => sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM orders WHERE id = $1)",
        )
        .bind(body.entity_id)
        .fetch_one(pool.get_ref())
        .await
        .map_err(AppError::Database)?,

        "assignment" => sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM shift_assignments WHERE id = $1)",
        )
        .bind(body.entity_id)
        .fetch_one(pool.get_ref())
        .await
        .map_err(AppError::Database)?,

        _ => false,
    };
    if !entity_exists {
        return Err(AppError::NotFound(format!(
            "{} {} not found",
            body.entity_type, body.entity_id
        )));
    }
    let signed_date = NaiveDate::parse_from_str(&body.signed_date, "%Y-%m-%d")
        .map_err(|_| AppError::Validation("signed_date must be YYYY-MM-DD".into()))?;

    let sig_path = if let Some(ref sig_data) = body.signature_data {
        if !sig_data.is_empty() {
            let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "/app/uploads".to_owned());
            let sig_dir = format!("{upload_dir}/signatures");
            tokio::fs::create_dir_all(&sig_dir).await?;
            let ts = chrono::Utc::now().timestamp();
            let sig_file = format!("{sig_dir}/{}_{ts}.svg", body.entity_id);
            tokio::fs::write(&sig_file, sig_data.as_bytes()).await?;
            Some(format!("signatures/{}_{ts}.svg", body.entity_id))
        } else { None }
    } else { None };

    let cmd = CreateEsignature {
        entity_type:          body.entity_type.clone(),
        entity_id:            body.entity_id,
        signer_name:          body.signer_name.trim().to_owned(),
        signed_date,
        signature_image_path: sig_path,
    };
    let sig_id = EsignatureRepo::new(&pool).create(&cmd).await?;

    write_audit_required(
        &pool,
        "esignature_created", &body.entity_type, Some(body.entity_id),
        Some(auth.id), "esign",
        None,
        Some(json!({ "sig_id": sig_id, "signer_name": body.signer_name.trim() })),
        None,
    )
    .await?;

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

/// Encrypt raw bytes with AES-256-GCM. Returns nonce (12B) || ciphertext.
fn encrypt_file_bytes(data: &[u8], secret: &str) -> Result<Vec<u8>, AppError> {
    use aes_gcm::{aead::{Aead, AeadCore, KeyInit, OsRng}, Aes256Gcm, Nonce};
    let key_bytes = crate::crypto::derive_aes_key(secret);
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("AES key error: {e}")))?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher.encrypt(&nonce, data)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("AES encrypt error: {e}")))?;
    let mut out = nonce.to_vec();
    out.extend(ciphertext);
    Ok(out)
}

/// Decrypt bytes produced by encrypt_file_bytes.
fn decrypt_file_bytes(blob: &[u8], secret: &str) -> Result<Vec<u8>, AppError> {
    use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
    if blob.len() < 12 {
        return Err(AppError::Validation("Encrypted blob too short".into()));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(12);
    let key_bytes = crate::crypto::derive_aes_key(secret);
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("AES key error: {e}")))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("AES decrypt error: {e}")))
}

fn upload_secret() -> String {
    std::env::var("SESSION_SECRET")
        .unwrap_or_else(|_| "change_this_to_a_random_64_char_string_before_going_live_!!1".to_owned())
}

// ── Magic-byte validation ──────────────────────────────────────────────────────

/// Validate that the first bytes of `data` match the expected magic signature
/// for `declared_mime`.  Rejects files where the content doesn't match the
/// client-declared MIME type, preventing extension/MIME spoofing attacks.
fn validate_magic_bytes(data: &[u8], declared_mime: &str) -> bool {
    match declared_mime {
        "application/pdf" => data.starts_with(b"%PDF"),
        "image/jpeg"      => data.len() >= 3
                                && data[0] == 0xFF
                                && data[1] == 0xD8
                                && data[2] == 0xFF,
        "image/png"       => data.starts_with(b"\x89PNG\r\n\x1A\n"),
        _                 => false,
    }
}

// ── Document download (authenticated, watermarked) ────────────────────────────

/// GET /credentials/{id}/download
///
/// Decrypts the stored document, injects a viewer + timestamp watermark, and
/// returns the raw file bytes with the correct `Content-Type` header.  Every
/// download is logged in the credential audit trail.
///
/// For PDF files, a `%%RailOps-Watermark` comment is appended to the byte
/// stream (harmless to standards-compliant PDF readers).
/// For images, the watermark is conveyed via the `X-Watermark` response header.
pub async fn download_credential(
    pool:  web::Data<sqlx::PgPool>,
    path:  web::Path<Uuid>,
    auth:  RequireViewCredentials,
) -> AppResult<HttpResponse> {
    let id   = path.into_inner();
    let cred = CredentialRepo::new(&pool)
        .find_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Credential {id}")))?;

    // Object-level scope check.
    enforce_credential_scope(&pool, auth.id, &auth.role, cred.contractor_id).await?;

    let upload_dir  = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "/app/uploads".to_owned());
    let abs_path    = format!("{upload_dir}/{}", cred.file_path);
    let encrypted   = tokio::fs::read(&abs_path).await
        .map_err(|_| AppError::NotFound(format!("File not found for credential {id}")))?;

    let plaintext = decrypt_file_bytes(&encrypted, &upload_secret())?;

    let watermark_text = format!(
        "Viewed by {} (user:{}) at {}",
        auth.full_name.as_deref().unwrap_or(&auth.username),
        auth.id,
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
    );

    // For PDFs, append a watermark comment to the byte stream.
    let mut output = plaintext;
    if cred.mime_type == "application/pdf" {
        let wm_comment = format!("\n%%RailOps-Watermark: {watermark_text}\n");
        output.extend_from_slice(wm_comment.as_bytes());
    }

    // ── Audit every download ──────────────────────────────────────────────
    CredAuditRepo::new(&pool)
        .insert(
            id, "downloaded", Some(auth.id),
            auth.full_name.as_deref(), None,
            Some(json!({"watermark": &watermark_text, "action": "file_download"})),
        )
        .await?;

    write_audit_required(
        &pool,
        "credential_downloaded", "credential", Some(id),
        Some(auth.id), "download",
        None,
        Some(json!({ "watermark": &watermark_text })),
        None,
    )
    .await?;

    // Sanitize filename for RFC 6266-safe Content-Disposition header:
    // strip directory separators and replace any character that is not
    // alphanumeric, hyphen, underscore, period, or space with an underscore.
    let safe_filename: String = cred
        .file_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe_filename = safe_filename.trim_matches('.').trim();

    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type",        cred.mime_type.as_str()))
        .insert_header(("Content-Disposition", format!("attachment; filename=\"{safe_filename}\"")))
        .insert_header(("X-Watermark",         watermark_text))
        .body(output))
}
