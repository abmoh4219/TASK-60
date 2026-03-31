//! Public kiosk API — no authentication required.
//!
//! These handlers power the station kiosk display: full-text search,
//! fuzzy matching, multi-condition filters, archive navigation, and related
//! content discovery.  All queries are restricted to `is_published = TRUE`.

pub mod handlers;
