//! Domain modules — one per bounded context.
//!
//! Each module exposes:
//!   • `models` — `sqlx::FromRow` structs + command types (NewXxx, UpdateXxx)
//!   • `repo`   — async repository functions that take `&PgPool`

pub mod content;
pub mod crawl;
pub mod credentials;
pub mod orders;
pub mod rail;
pub mod rules;
pub mod staffing;
