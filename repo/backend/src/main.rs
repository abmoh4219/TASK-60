//! RailOps backend — entry point.
//!
//! Responsibilities at startup:
//!   1. Initialise structured tracing.
//!   2. Load configuration from environment variables.
//!   3. Generate a self-signed TLS certificate if none exists (dev/local).
//!   4. Connect to PostgreSQL and run pending migrations.
//!   5. Seed the admin user's password on first boot.
//!   6. Start the Actix-web HTTPS server.

mod config;
mod crypto;
mod error;

use actix_files::Files;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use config::AppConfig;

#[actix_web::main]
async fn main() -> Result<()> {
    init_tracing();

    let cfg = AppConfig::from_env().context("Failed to load configuration")?;

    info!(host = %cfg.server.host, port = cfg.server.port, "Starting RailOps backend");

    // ── TLS ────────────────────────────────────────────────────────────────
    ensure_tls_certs(&cfg.tls).context("TLS certificate setup failed")?;
    let tls_config = build_tls_config(&cfg.tls).context("Failed to build TLS config")?;

    // ── Database ───────────────────────────────────────────────────────────
    let pool = PgPoolOptions::new()
        .max_connections(cfg.database.max_connections)
        .connect(&cfg.database.url())
        .await
        .context("Failed to connect to PostgreSQL")?;

    info!("Connected to PostgreSQL");

    // Run embedded migrations (SQL files compiled into the binary at build time).
    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .context("Database migration failed")?;

    info!("Migrations applied");

    // ── First-boot admin seed ──────────────────────────────────────────────
    seed_admin_password(&pool, &cfg.security.admin_seed_password).await?;

    // ── HTTP server ────────────────────────────────────────────────────────
    let pool      = web::Data::new(pool);
    let cfg_data  = web::Data::new(cfg.clone());
    let static_dir = cfg.server.static_dir.clone();
    let bind_addr  = format!("{}:{}", cfg.server.host, cfg.server.port);

    info!(addr = %bind_addr, "Listening (HTTPS)");

    HttpServer::new(move || {
        App::new()
            .app_data(pool.clone())
            .app_data(cfg_data.clone())
            // Structured access logging
            .wrap(middleware::Logger::default())
            // ── Health probe (unauthenticated) ─────────────────────────
            .route("/health", web::get().to(health))
            // ── API routes added in later steps ─────────────────────
            // .service(web::scope("/api/v1") …)
            // ── Serve compiled Yew WASM app (catch-all last) ──────────
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

// ── Health endpoint ───────────────────────────────────────────────────────────

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "status":  "ok",
        "service": "railops-backend",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// ── TLS helpers ───────────────────────────────────────────────────────────────

/// Generate a self-signed certificate + key if the configured paths do not exist.
fn ensure_tls_certs(tls: &config::TlsConfig) -> Result<()> {
    let cert_path = std::path::Path::new(&tls.cert_path);
    let key_path  = std::path::Path::new(&tls.key_path);

    if cert_path.exists() && key_path.exists() {
        return Ok(());
    }

    warn!("TLS certificate not found — generating self-signed cert for local use");

    let subject_alt_names = vec!["localhost".to_owned(), "railops.local".to_owned()];
    let cert = rcgen::generate_simple_self_signed(subject_alt_names)
        .context("rcgen certificate generation failed")?;

    if let Some(parent) = cert_path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create certs directory")?;
    }

    std::fs::write(cert_path, cert.serialize_pem().context("Cert serialisation failed")?)
        .context("Failed to write cert.pem")?;
    std::fs::write(key_path, cert.serialize_private_key_pem())
        .context("Failed to write key.pem")?;

    info!(cert = %tls.cert_path, "Self-signed TLS certificate written");
    Ok(())
}

fn build_tls_config(tls: &config::TlsConfig) -> Result<rustls::ServerConfig> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls_pemfile::{certs, private_key};
    use std::{fs::File, io::BufReader};

    let cert_chain: Vec<CertificateDer<'static>> =
        certs(&mut BufReader::new(File::open(&tls.cert_path)?))
            .collect::<std::result::Result<_, _>>()
            .context("Failed to parse TLS certificate chain")?;

    let key: PrivateKeyDer<'static> =
        private_key(&mut BufReader::new(File::open(&tls.key_path)?))
            .context("Failed to read TLS private key")?
            .ok_or_else(|| anyhow::anyhow!("No private key found in {}", tls.key_path))?;

    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .context("Invalid TLS certificate/key pair")
}

// ── Admin password seeding ────────────────────────────────────────────────────

/// On first boot the migration inserts the admin row with hash = `SEED_PENDING`.
/// Here we replace it with a real Argon2id hash derived from `ADMIN_SEED_PASSWORD`.
async fn seed_admin_password(pool: &sqlx::PgPool, seed_password: &str) -> Result<()> {
    let needs_seed: Option<bool> = sqlx::query_scalar(
        "SELECT password_hash = 'SEED_PENDING' FROM users WHERE username = 'admin'",
    )
    .fetch_optional(pool)
    .await
    .context("Failed to query admin seed status")?;

    if needs_seed != Some(true) {
        return Ok(());
    }

    let hash = crypto::hash_password(seed_password)
        .context("Failed to hash admin seed password")?;

    sqlx::query(
        "UPDATE users SET password_hash = $1 \
         WHERE username = 'admin' AND password_hash = 'SEED_PENDING'",
    )
    .bind(&hash)
    .execute(pool)
    .await
    .context("Failed to seed admin password")?;

    info!("Admin password seeded from ADMIN_SEED_PASSWORD");
    Ok(())
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
