//! RailOps backend — entry point.
//!
//! Startup sequence:
//!   1. Structured tracing
//!   2. Config from environment variables
//!   3. Self-signed TLS cert (if absent)
//!   4. PostgreSQL connection + migrations
//!   5. Admin (and seeded dev-user) password hashing on first boot
//!   6. Actix-web HTTPS server with auth routes

mod auth;
mod config;
mod crypto;
mod error;

use actix_files::Files;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use auth::middleware::RateLimiter;
use config::AppConfig;

#[actix_web::main]
async fn main() -> Result<()> {
    init_tracing();

    let cfg = AppConfig::from_env().context("Failed to load configuration")?;
    info!(host = %cfg.server.host, port = cfg.server.port, "Starting RailOps backend");

    ensure_tls_certs(&cfg.tls).context("TLS setup failed")?;
    let tls_config = build_tls_config(&cfg.tls).context("Failed to build TLS config")?;

    let pool = PgPoolOptions::new()
        .max_connections(cfg.database.max_connections)
        .connect(&cfg.database.url())
        .await
        .context("Failed to connect to PostgreSQL")?;
    info!("Connected to PostgreSQL");

    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .context("Database migration failed")?;
    info!("Migrations applied");

    seed_pending_passwords(&pool, &cfg.security.admin_seed_password).await?;

    // ── Shared app state ───────────────────────────────────────────────────
    let pool_data    = web::Data::new(pool);
    let cfg_data     = web::Data::new(cfg.clone());
    let rate_limiter = web::Data::new(RateLimiter::new(cfg.security.rate_limit_rpm));
    let static_dir   = cfg.server.static_dir.clone();
    let bind_addr    = format!("{}:{}", cfg.server.host, cfg.server.port);

    info!(addr = %bind_addr, "Listening (HTTPS)");

    HttpServer::new(move || {
        App::new()
            .app_data(pool_data.clone())
            .app_data(cfg_data.clone())
            .app_data(rate_limiter.clone())
            // Structured access log
            .wrap(middleware::Logger::default())
            // ── Unauthenticated ────────────────────────────────────────
            .route("/health", web::get().to(health))
            // ── Auth endpoints (no signature required on /login) ───────
            .service(
                web::scope("/api/v1/auth")
                    .route("/login",  web::post().to(auth::handlers::login))
                    .route("/logout", web::delete().to(auth::handlers::logout))
                    .route("/me",     web::get().to(auth::handlers::me)),
            )
            // ── Domain API routes added in later steps ─────────────────
            // .service(web::scope("/api/v1/schedules")…)
            // .service(web::scope("/api/v1/orders")…)
            // ── Yew SPA (served last; catch-all for any non-API path) ──
            .service(
                Files::new("/", &static_dir)
                    .index_file("index.html")
                    .use_last_modified(true),
            )
    })
    .bind_rustls_0_23(bind_addr, tls_config)?
    .run()
    .await
    .context("HTTP server error")?;

    Ok(())
}

// ── Health ────────────────────────────────────────────────────────────────────

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok", "service": "railops-backend",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// ── First-boot password seeding ───────────────────────────────────────────────

/// Replace every `SEED_PENDING` hash in the users table with a real Argon2id hash.
/// Admin gets `admin_seed_password`; all other seeded users get the same value
/// (dev convenience — the admin must change these via the UI in production).
async fn seed_pending_passwords(pool: &sqlx::PgPool, seed_pw: &str) -> Result<()> {
    let pending: Vec<(uuid::Uuid, String)> = sqlx::query_as(
        "SELECT id, username FROM users WHERE password_hash = 'SEED_PENDING'",
    )
    .fetch_all(pool)
    .await
    .context("Failed to query pending password seeds")?;

    for (id, username) in &pending {
        let hash = crypto::hash_password(seed_pw)
            .context("Failed to hash seed password")?;
        sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
            .bind(&hash)
            .bind(id)
            .execute(pool)
            .await
            .context("Failed to update seeded password")?;
        info!(username = %username, "Password seeded");
    }
    Ok(())
}

// ── TLS ───────────────────────────────────────────────────────────────────────

fn ensure_tls_certs(tls: &config::TlsConfig) -> Result<()> {
    let cert_path = std::path::Path::new(&tls.cert_path);
    let key_path  = std::path::Path::new(&tls.key_path);
    if cert_path.exists() && key_path.exists() { return Ok(()); }

    warn!("Generating self-signed TLS certificate");
    let cert = rcgen::generate_simple_self_signed(
        vec!["localhost".to_owned(), "railops.local".to_owned()],
    )
    .context("rcgen failed")?;

    if let Some(p) = cert_path.parent() { std::fs::create_dir_all(p)?; }
    std::fs::write(cert_path, cert.serialize_pem().context("cert PEM")?)?;
    std::fs::write(key_path,  cert.serialize_private_key_pem())?;
    info!(cert = %tls.cert_path, "TLS certificate written");
    Ok(())
}

fn build_tls_config(tls: &config::TlsConfig) -> Result<rustls::ServerConfig> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls_pemfile::{certs, private_key};
    use std::{fs::File, io::BufReader};

    let cert_chain: Vec<CertificateDer<'static>> =
        certs(&mut BufReader::new(File::open(&tls.cert_path)?))
            .collect::<std::result::Result<_, _>>()
            .context("Failed to parse cert chain")?;
    let key: PrivateKeyDer<'static> =
        private_key(&mut BufReader::new(File::open(&tls.key_path)?))
            .context("Failed to read private key")?
            .ok_or_else(|| anyhow::anyhow!("No private key in {}", tls.key_path))?;

    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .context("Invalid TLS cert/key pair")
}

// ── Tracing ───────────────────────────────────────────────────────────────────

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend=info,actix_web=info,sqlx=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();
}
