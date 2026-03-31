//! Configurable offline multi-source collection engine.
//!
//! Modules:
//!   - `pipeline`  — HTML stripping, normalisation, fingerprinting, slug generation
//!   - `quality`   — completeness / accuracy / timeliness scoring + similarity quarantine
//!   - `reader`    — `SourceReader` trait + `LocalPackageReader` / `InternalMirrorReader`
//!   - `engine`    — tokio background scheduler with per-task worker pool
//!   - `handlers`  — Actix-web REST handlers for the crawl management API

pub mod engine;
pub mod handlers;
pub mod pipeline;
pub mod quality;
pub mod reader;
