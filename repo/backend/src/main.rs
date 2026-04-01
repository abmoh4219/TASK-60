//! RailOps backend — entry point.
//!
//! Startup sequence:
//!   1. Structured tracing
//!   2. Config from environment variables
//!   3. Self-signed TLS cert (if absent)
//!   4. PostgreSQL connection + migrations
//!   5. Admin (and seeded dev-user) password hashing on first boot
//!   6. Crawl engine background task
//!   7. Actix-web HTTPS server with auth + crawl routes

mod auth;
mod config;
mod crawl;
mod credentials;
mod crypto;
mod db;
mod domain;
mod error;
mod kiosk;
mod ops;
mod rules;
mod staffing;

use actix_files::Files;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use auth::middleware::RateLimiter;
use config::AppConfig;
use crawl::engine::{CrawlEngine, TriggerHandle};
use crawl::handlers as crawl_handlers;
use credentials::handlers as credentials_handlers;
use kiosk::handlers as kiosk_handlers;
use ops::orders as ops_orders;
use ops::schedules as ops_schedules;
use rules::handlers as rules_handlers;
use staffing::handlers as staffing_handlers;

#[actix_web::main]
async fn main() -> Result<()> {
    // rustls 0.23 requires an explicit process-level CryptoProvider.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls ring crypto provider");

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

    // ── Crawl engine ───────────────────────────────────────────────────────────
    let (trigger_handle, trigger_rx) = TriggerHandle::new();
    CrawlEngine::new(pool.clone(), shared::rules::CRAWL_MAX_WORKERS).spawn(trigger_rx);
    info!("CrawlEngine started in background");

    // ── Background jobs: hold-expiry + PII-purge ──────────────────────────────
    {
        let pool_bg = pool.clone();
        tokio::spawn(async move {
            use crate::domain::orders::repo::{OrderRepo, OrderEventRepo, PassengerRepo};
            use crate::domain::orders::models::NewOrderEvent;
            use crate::db::audit::write_audit;
            use serde_json::json;

            let mut hold_tick = tokio::time::interval(Duration::from_secs(30));
            hold_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut purge_tick = tokio::time::interval(Duration::from_secs(300));
            purge_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = hold_tick.tick() => {
                        // Expire held orders past their hold_expires_at
                        // First get the IDs that will be expired so we can log events
                        let expired_ids: Vec<(uuid::Uuid,)> = sqlx::query_as(
                            "SELECT id FROM orders WHERE status = 'held' AND hold_expires_at < NOW()"
                        )
                        .fetch_all(&pool_bg)
                        .await
                        .unwrap_or_default();

                        if !expired_ids.is_empty() {
                            match OrderRepo::new(&pool_bg).expire_holds().await {
                                Ok(n) if n > 0 => {
                                    for (oid,) in &expired_ids {
                                        let _ = OrderEventRepo::new(&pool_bg).insert(&NewOrderEvent {
                                            order_id: *oid,
                                            event_type: "hold_expired".into(),
                                            performed_by: None,
                                            reason: Some("Automatic hold expiry after TTL".into()),
                                            data: None,
                                        }).await;
                                        write_audit(
                                            &pool_bg, "hold_expired", "orders", Some(*oid),
                                            None, "auto_expire", None,
                                            Some(json!({"reason": "hold TTL exceeded"})), None,
                                        ).await;
                                    }
                                    info!(count = n, "Auto-expired held orders");
                                }
                                Ok(_) => {}
                                Err(e) => warn!(error = %e, "Hold expiry check failed"),
                            }
                        }
                    }
                    _ = purge_tick.tick() => {
                        // PII purge: find passengers with purge requested, whose most
                        // recent order departure is > 30 days ago
                        let eligible: Vec<(uuid::Uuid,)> = sqlx::query_as(
                            "SELECT p.id FROM passengers p
                             WHERE p.pii_purge_requested_at IS NOT NULL
                               AND p.pii_purged_at IS NULL
                               AND NOT EXISTS (
                                   SELECT 1 FROM orders o
                                   JOIN schedules s ON s.id = o.schedule_id
                                   WHERE o.passenger_id = p.id
                                     AND s.departure_time > NOW() - INTERVAL '30 days'
                               )"
                        )
                        .fetch_all(&pool_bg)
                        .await
                        .unwrap_or_default();

                        for (pid,) in &eligible {
                            match PassengerRepo::new(&pool_bg).execute_pii_purge(*pid).await {
                                Ok(()) => {
                                    write_audit(
                                        &pool_bg, "pii_purged", "passengers", Some(*pid),
                                        None, "auto_purge", None,
                                        Some(json!({"reason": "30 days post-trip + purge request"})),
                                        None,
                                    ).await;
                                    info!(passenger_id = %pid, "PII purge executed");
                                }
                                Err(e) => warn!(passenger_id = %pid, error = %e, "PII purge failed"),
                            }
                        }
                    }
                }
            }
        });
        info!("Background jobs started (hold-expiry + PII-purge)");
    }

    // ── Shared app state ───────────────────────────────────────────────────────
    let pool_data    = web::Data::new(pool);
    let cfg_data     = web::Data::new(cfg.clone());
    let rate_limiter = web::Data::new(RateLimiter::new(cfg.security.rate_limit_rpm));
    let trigger_data = web::Data::new(trigger_handle);
    let static_dir   = cfg.server.static_dir.clone();
    let bind_addr    = format!("{}:{}", cfg.server.host, cfg.server.port);

    info!(addr = %bind_addr, "Listening (HTTPS)");

    HttpServer::new(move || {
        App::new()
            .app_data(pool_data.clone())
            .app_data(cfg_data.clone())
            .app_data(rate_limiter.clone())
            .app_data(trigger_data.clone())
            // Structured access log
            .wrap(middleware::Logger::default())
            // ── Unauthenticated ────────────────────────────────────────────
            .route("/health", web::get().to(health))
            // ── Auth endpoints ─────────────────────────────────────────────
            .service(
                web::scope("/api/v1/auth")
                    .route("/login",  web::post().to(auth::handlers::login))
                    .route("/logout", web::delete().to(auth::handlers::logout))
                    .route("/me",     web::get().to(auth::handlers::me)),
            )
            // ── Crawl management API ───────────────────────────────────────
            .service(
                web::scope("/api/v1/crawl")
                    .route("/sources",
                        web::get().to(crawl_handlers::list_sources))
                    .route("/sources",
                        web::post().to(crawl_handlers::create_source))
                    .route("/sources/{id}",
                        web::get().to(crawl_handlers::get_source))
                    .route("/sources/{id}/tasks",
                        web::get().to(crawl_handlers::list_tasks))
                    .route("/sources/{id}/tasks",
                        web::post().to(crawl_handlers::create_task))
                    .route("/tasks/{id}/run",
                        web::post().to(crawl_handlers::trigger_run))
                    .route("/tasks/{id}/runs",
                        web::get().to(crawl_handlers::list_runs))
                    .route("/runs/{id}",
                        web::get().to(crawl_handlers::get_run))
                    .route("/runs/{id}/quality",
                        web::get().to(crawl_handlers::quality_for_run))
                    .route("/quality/quarantined",
                        web::get().to(crawl_handlers::list_quarantined)),
            )
            // ── Public kiosk API (no auth required) ───────────────────────
            .service(
                web::scope("/api/v1/kiosk")
                    .route("/content",
                        web::get().to(kiosk_handlers::list_content))
                    .route("/content/{slug}",
                        web::get().to(kiosk_handlers::get_article))
                    .route("/archive",
                        web::get().to(kiosk_handlers::get_archive))
                    .route("/categories",
                        web::get().to(kiosk_handlers::list_categories))
                    .route("/tags",
                        web::get().to(kiosk_handlers::list_tags)),
            )
            // ── Operations console API ────────────────────────────────────
            .service(
                web::scope("/api/v1/ops")
                    // Reference data
                    .route("/routes",
                        web::get().to(ops_schedules::list_routes))
                    .route("/seat-classes",
                        web::get().to(ops_schedules::list_seat_classes))
                    // Schedules
                    .route("/schedules",
                        web::get().to(ops_schedules::list_schedules))
                    .route("/schedules/{id}",
                        web::get().to(ops_schedules::get_schedule))
                    .route("/schedules/{id}/status",
                        web::patch().to(ops_schedules::update_schedule_status))
                    .route("/schedules/{id}/inventory",
                        web::post().to(ops_schedules::correct_inventory))
                    // Passengers
                    .route("/passengers",
                        web::get().to(ops_orders::search_passengers))
                    .route("/passengers",
                        web::post().to(ops_orders::create_passenger))
                    .route("/passengers/{id}/pii-purge",
                        web::post().to(ops_orders::request_pii_purge))
                    // Orders
                    .route("/orders",
                        web::get().to(ops_orders::list_orders))
                    .route("/orders",
                        web::post().to(ops_orders::create_order))
                    .route("/orders/by-number/{num}",
                        web::get().to(ops_orders::find_by_number))
                    .route("/orders/{id}",
                        web::get().to(ops_orders::get_order))
                    .route("/orders/{id}/hold",
                        web::post().to(ops_orders::hold_order))
                    .route("/orders/{id}/confirm",
                        web::post().to(ops_orders::confirm_order))
                    .route("/orders/{id}/cancel",
                        web::post().to(ops_orders::cancel_order))
                    .route("/orders/{id}/refund",
                        web::post().to(ops_orders::process_refund))
                    .route("/orders/{id}/fee-override",
                        web::post().to(ops_orders::apply_fee_override))
                    .route("/orders/{id}/disruption",
                        web::post().to(ops_orders::flag_disruption))
                    .route("/orders/{id}/rebook",
                        web::post().to(ops_orders::rebook_order))
                    .route("/orders/{id}/events",
                        web::get().to(ops_orders::list_order_events)),
            )
            // ── Business rules admin API ──────────────────────────────────
            .service(
                web::scope("/api/v1/rules")
                    .route("",      web::get().to(rules_handlers::list_rules))
                    .route("/{key}", web::get().to(rules_handlers::get_rule))
                    .route("/{key}", web::patch().to(rules_handlers::update_rule)),
            )
            // ── Staffing console API ───────────────────────────────────────
            .service(
                web::scope("/api/v1/staffing")
                    // Contractors
                    .route("/contractors",
                        web::get().to(staffing_handlers::list_contractors))
                    .route("/contractors",
                        web::post().to(staffing_handlers::create_contractor))
                    .route("/contractors/{id}",
                        web::get().to(staffing_handlers::get_contractor))
                    .route("/contractors/{id}/active",
                        web::patch().to(staffing_handlers::set_contractor_active))
                    .route("/contractors/{id}/availability",
                        web::post().to(staffing_handlers::add_availability))
                    // Shifts
                    .route("/shifts",
                        web::get().to(staffing_handlers::list_shifts))
                    .route("/shifts",
                        web::post().to(staffing_handlers::create_shift))
                    .route("/shifts/{id}",
                        web::get().to(staffing_handlers::get_shift))
                    .route("/shifts/{id}/status",
                        web::patch().to(staffing_handlers::update_shift_status))
                    .route("/shifts/{id}/candidates",
                        web::get().to(staffing_handlers::get_candidates))
                    .route("/shifts/{id}/propose",
                        web::post().to(staffing_handlers::propose_assignment))
                    // Assignments
                    .route("/assignments/{id}/respond",
                        web::patch().to(staffing_handlers::respond_assignment))
                    // Subscriptions
                    .route("/subscriptions",
                        web::get().to(staffing_handlers::list_subscriptions))
                    .route("/subscriptions",
                        web::post().to(staffing_handlers::subscribe))
                    .route("/subscriptions",
                        web::delete().to(staffing_handlers::unsubscribe)),
            )
            // ── Credential document management API ───────────────────────
            .service(
                web::scope("/api/v1/credentials")
                    .route("",
                        web::get().to(credentials_handlers::list_credentials))
                    .route("",
                        web::post().to(credentials_handlers::upload_credential))
                    .route("/expire",
                        web::post().to(credentials_handlers::run_expiry_sweep))
                    .route("/{id}",
                        web::get().to(credentials_handlers::get_credential))
                    .route("/{id}/review",
                        web::patch().to(credentials_handlers::review_credential))
                    .route("/{id}/audit",
                        web::get().to(credentials_handlers::get_credential_audit))
                    .route("/{id}/esign",
                        web::post().to(credentials_handlers::esign_credential)),
            )
            // ── E-signature API ───────────────────────────────────────────
            .service(
                web::scope("/api/v1/esignatures")
                    .route("",
                        web::post().to(credentials_handlers::create_esignature))
                    .route("/{entity_type}/{entity_id}",
                        web::get().to(credentials_handlers::list_esignatures)),
            )
            // ── Yew SPA (served last; catch-all for any non-API path) ──────
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
