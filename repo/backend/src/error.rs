//! Centralised error type for the RailOps backend.
//!
//! Every handler returns `AppResult<T>` which is `Result<T, AppError>`.
//! `AppError` implements `actix_web::ResponseError` so Actix converts it to
//! a structured JSON response automatically.

use actix_web::{http::StatusCode, HttpResponse, ResponseError};
use serde_json::json;
use thiserror::Error;
use tracing::error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    // ── Auth / access ─────────────────────────────────────────────────────
    #[error("Authentication required: {0}")]
    Unauthorized(String),

    #[error("Permission denied: {0}")]
    Forbidden(String),

    #[error("Account locked until {0}")]
    AccountLocked(String),

    // ── Input / validation ────────────────────────────────────────────────
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    // ── Business rules ────────────────────────────────────────────────────
    #[error("Refund blocked: {0}")]
    RefundBlocked(String),

    #[error("Order hold expired")]
    HoldExpired,

    #[error("Quality score too low to publish ({0}/100)")]
    QualityBelowThreshold(String),

    // ── Rate limiting ─────────────────────────────────────────────────────
    #[error("Rate limit exceeded — try again in {retry_after}s")]
    RateLimited { retry_after: u64 },

    // ── File / upload ─────────────────────────────────────────────────────
    #[error("Unsupported file type: {0}")]
    UnsupportedFileType(String),

    #[error("File too large (max 10 MB)")]
    FileTooLarge,

    // ── Infrastructure ────────────────────────────────────────────────────
    #[error("Database error")]
    Database(#[from] sqlx::Error),

    #[error("IO error")]
    Io(#[from] std::io::Error),

    #[error("Serialization error")]
    Serialization(#[from] serde_json::Error),

    /// Catch-all for unexpected failures — avoids leaking internals.
    #[error("Internal server error")]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    fn status_code_inner(&self) -> StatusCode {
        match self {
            AppError::Unauthorized(_)         => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_)            => StatusCode::FORBIDDEN,
            AppError::AccountLocked(_)        => StatusCode::FORBIDDEN,
            AppError::Validation(_)           => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::NotFound(_)             => StatusCode::NOT_FOUND,
            AppError::Conflict(_)             => StatusCode::CONFLICT,
            AppError::RefundBlocked(_)        => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::HoldExpired             => StatusCode::GONE,
            AppError::QualityBelowThreshold(_)=> StatusCode::UNPROCESSABLE_ENTITY,
            AppError::RateLimited { .. }      => StatusCode::TOO_MANY_REQUESTS,
            AppError::UnsupportedFileType(_)  => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            AppError::FileTooLarge            => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::Database(_)
            | AppError::Io(_)
            | AppError::Serialization(_)
            | AppError::Internal(_)           => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_type(&self) -> &'static str {
        match self {
            AppError::Unauthorized(_)          => "unauthorized",
            AppError::Forbidden(_)             => "forbidden",
            AppError::AccountLocked(_)         => "account_locked",
            AppError::Validation(_)            => "validation_error",
            AppError::NotFound(_)              => "not_found",
            AppError::Conflict(_)              => "conflict",
            AppError::RefundBlocked(_)         => "refund_blocked",
            AppError::HoldExpired              => "hold_expired",
            AppError::QualityBelowThreshold(_) => "quality_below_threshold",
            AppError::RateLimited { .. }       => "rate_limited",
            AppError::UnsupportedFileType(_)   => "unsupported_file_type",
            AppError::FileTooLarge             => "file_too_large",
            AppError::Database(_)              => "database_error",
            AppError::Io(_)                    => "io_error",
            AppError::Serialization(_)         => "serialization_error",
            AppError::Internal(_)              => "internal_error",
        }
    }
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        self.status_code_inner()
    }

    fn error_response(&self) -> HttpResponse {
        let status = self.status_code_inner();

        // Log internal errors with full detail; never expose them to clients.
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            error!(error = %self, "Internal server error");
        }

        // The client-facing message strips internal detail for 5xx.
        let client_message = match status.is_server_error() {
            true  => "An internal error occurred. Please contact support.",
            false => &self.to_string(),
        };

        let mut body = json!({
            "error": {
                "type":    self.error_type(),
                "code":    status.as_u16(),
                "message": client_message,
            }
        });

        // Add Retry-After hint for rate-limited responses.
        if let AppError::RateLimited { retry_after } = self {
            body["error"]["retry_after"] = json!(retry_after);
        }

        HttpResponse::build(status).json(body)
    }
}
