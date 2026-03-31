//! Configurable business-rules engine.
//!
//! Rule values live in the `business_rules` PostgreSQL table (seeded by
//! V2 migration) and are read fresh on every policy evaluation so that
//! admin changes take effect without a server restart.
//!
//! Public surface:
//!   `RulesEngine<'_>` — stateless, pool-borrowing struct with policy methods.
//!   `handlers`         — Actix-web CRUD handlers at /api/v1/rules.

pub mod engine;
pub mod handlers;

pub use engine::RulesEngine;
