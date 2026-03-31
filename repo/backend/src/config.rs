//! Configuration loaded exclusively from environment variables (no .env files).
//! All required vars that are absent will cause the process to exit at startup.

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server:   ServerConfig,
    pub database: DatabaseConfig,
    pub tls:      TlsConfig,
    pub security: SecurityConfig,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host:       String,
    pub port:       u16,
    pub static_dir: String,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub host:            String,
    pub port:            u16,
    pub name:            String,
    pub user:            String,
    pub password:        String,
    pub max_connections: u32,
}

impl DatabaseConfig {
    pub fn url(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.user, self.password, self.host, self.port, self.name
        )
    }
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path:  String,
}

#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Signing secret for HMAC-authenticated API requests (≥ 32 chars).
    pub session_secret:       String,
    /// Admin seed password used once on first boot (from env).
    pub admin_seed_password:  String,
    pub max_failed_logins:    u32,
    pub lockout_minutes:      u64,
    pub session_idle_minutes: u64,
    pub rate_limit_rpm:       u32,
    pub min_password_len:     usize,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        Ok(AppConfig {
            server: ServerConfig {
                host: env_or("SERVER_HOST", "0.0.0.0"),
                port: env_parse("SERVER_PORT", 8443)?,
                static_dir: env_or("STATIC_DIR", "/app/static"),
            },
            database: DatabaseConfig {
                host:            env_or("DB_HOST", "localhost"),
                port:            env_parse("DB_PORT", 5432)?,
                name:            env_or("DB_NAME", "railops"),
                user:            env_or("DB_USER", "railops"),
                password:        env_required("DB_PASSWORD")?,
                max_connections: env_parse("DB_MAX_CONNECTIONS", 20)?,
            },
            tls: TlsConfig {
                cert_path: env_or("TLS_CERT_PATH", "/app/certs/cert.pem"),
                key_path:  env_or("TLS_KEY_PATH",  "/app/certs/key.pem"),
            },
            security: SecurityConfig {
                session_secret:      env_required("SESSION_SECRET")?,
                admin_seed_password: env_or("ADMIN_SEED_PASSWORD", "AdminRailOps2024!"),
                max_failed_logins:    shared::rules::MAX_FAILED_LOGINS,
                lockout_minutes:      shared::rules::LOCKOUT_MINUTES,
                session_idle_minutes: shared::rules::SESSION_IDLE_MINUTES,
                rate_limit_rpm:       shared::rules::RATE_LIMIT_RPM,
                min_password_len:     shared::rules::MIN_PASSWORD_LEN,
            },
        })
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn env_required(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("Required env var `{key}` is not set"))
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match std::env::var(key) {
        Ok(v) => v.parse::<T>().with_context(|| format!("Invalid value for env var `{key}`")),
        Err(_) => Ok(default),
    }
}
