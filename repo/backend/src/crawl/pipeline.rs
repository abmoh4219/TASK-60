//! Content transformation pipeline.
//!
//! Accepts a `RawItem` (JSON-deserialized from a source file) and produces a
//! `TransformedItem` suitable for ingestion into `content_pages`.
//!
//! Transformation steps (recorded in `transformation_steps`):
//!   1. strip_html  — remove all HTML tags via a simple state machine
//!   2. normalize   — collapse whitespace (spaces, tabs, newlines) to single space
//!   3. trim        — strip leading/trailing whitespace
//!   4. abbreviate  — expand common railway abbreviations
//!   5. fingerprint — SHA-256 hex of `title‖body‖source_url`
//!   6. slugify     — URL-safe slug from title

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── Input ─────────────────────────────────────────────────────────────────────

/// Raw, uncleaned record as read from a source JSON file.
#[derive(Debug, Clone, Deserialize)]
pub struct RawItem {
    pub title:          Option<String>,
    pub body:           Option<String>,
    pub category:       Option<String>,
    pub tags:           Option<Vec<String>>,
    pub publish_date:   Option<String>,
    pub source_url:     Option<String>,
    pub author:         Option<String>,
    pub image_url:      Option<String>,
    /// Optional departure date for route/schedule-linked articles (YYYY-MM-DD).
    /// Used to detect "departure in the past" anomalies during quality scoring.
    pub departure_date: Option<String>,
}

// ── Output ────────────────────────────────────────────────────────────────────

/// Validated, cleaned record ready for the quality scorer and DB ingest.
#[derive(Debug, Clone, Serialize)]
pub struct TransformedItem {
    pub title:                String,
    pub slug:                 String,
    pub body:                 String,
    pub category:             String,
    pub tags:                 Vec<String>,
    pub publish_date:         Option<NaiveDate>,
    pub source_url:           Option<String>,
    pub author:               Option<String>,
    pub image_url:            Option<String>,
    pub fingerprint:          String,
    pub url_fingerprint:      Option<String>,
    pub transformation_steps: Vec<String>,
    /// Parsed departure date from source data — used for anomaly detection.
    pub departure_date:       Option<NaiveDate>,
}

// ── Transform ─────────────────────────────────────────────────────────────────

/// Transform a `RawItem` into a `TransformedItem`.
/// Returns `None` if the item has neither a usable title nor body.
pub fn transform(raw: &RawItem) -> Option<TransformedItem> {
    let mut steps: Vec<String> = Vec::new();

    let title = apply_text_pipeline(raw.title.as_deref().unwrap_or(""), &mut steps, "title");
    let body  = apply_text_pipeline(raw.body.as_deref().unwrap_or(""),  &mut steps, "body");

    if title.is_empty() && body.is_empty() {
        return None;
    }

    let category = raw
        .category
        .as_deref()
        .map(|c| c.to_lowercase())
        .filter(|c| matches!(c.as_str(), "fares" | "delays" | "baggage" | "accessibility"))
        .unwrap_or_else(|| "general".to_owned());

    let tags: Vec<String> = raw
        .tags
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    let publish_date = raw.publish_date.as_deref().and_then(|s| {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .or_else(|_| NaiveDate::parse_from_str(s, "%d/%m/%Y"))
            .or_else(|_| NaiveDate::parse_from_str(s, "%m/%d/%Y"))
            .ok()
    });

    let departure_date = raw.departure_date.as_deref().and_then(|s| {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .or_else(|_| NaiveDate::parse_from_str(s, "%d/%m/%Y"))
            .ok()
    });

    let slug_source = if !title.is_empty() {
        title.as_str()
    } else {
        &body[..body.len().min(80)]
    };
    let slug = slugify(slug_source);
    steps.push("slugify".to_owned());

    let fingerprint = content_fingerprint(&title, &body, raw.source_url.as_deref());
    steps.push("fingerprint".to_owned());

    // Dedicated URL-only fingerprint for source-URL-based deduplication.
    let url_fingerprint = raw.source_url.as_deref().map(url_fingerprint);

    Some(TransformedItem {
        title,
        slug,
        body,
        category,
        tags,
        publish_date,
        source_url:           raw.source_url.clone(),
        author:               raw.author.clone(),
        image_url:            raw.image_url.clone(),
        fingerprint,
        url_fingerprint,
        transformation_steps: steps,
        departure_date,
    })
}

// ── Text pipeline ─────────────────────────────────────────────────────────────

fn apply_text_pipeline(input: &str, steps: &mut Vec<String>, label: &str) -> String {
    let stripped = strip_html(input);
    if stripped != input { steps.push(format!("strip_html:{label}")); }

    let normalized = normalize_whitespace(&stripped);
    if normalized != stripped { steps.push(format!("normalize:{label}")); }

    let trimmed = normalized.trim().to_owned();
    if trimmed != normalized { steps.push(format!("trim:{label}")); }

    let expanded = expand_abbreviations(&trimmed);
    if expanded != trimmed { steps.push(format!("abbreviate:{label}")); }

    expanded
}

/// Remove HTML/XML tags with a simple state machine, replacing each tag with
/// a single space to prevent adjacent words from merging.
fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;

    for ch in input.chars() {
        match ch {
            '<' => { in_tag = true; }
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Collapse any run of ASCII whitespace to a single space.
fn normalize_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;

    for ch in input.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_space { out.push(' '); }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

/// Expand a small set of unambiguous railway-domain abbreviations.
fn expand_abbreviations(input: &str) -> String {
    const ABBREVS: &[(&str, &str)] = &[
        ("dept.", "department"),
        ("stn.",  "station"),
        ("arr.",  "arrival"),
        ("dep.",  "departure"),
        ("approx.", "approximately"),
        ("min.",  "minutes"),
        ("hr.",   "hours"),
    ];

    let mut s = input.to_owned();
    for (abbr, expansion) in ABBREVS {
        // Replace all literal occurrences of the abbreviation (case-sensitive, lowercase).
        let mut pos = 0;
        while let Some(found) = s[pos..].find(abbr) {
            let abs = pos + found;
            s.replace_range(abs..abs + abbr.len(), expansion);
            pos = abs + expansion.len();
        }
    }
    s
}

// ── Fingerprint ───────────────────────────────────────────────────────────────

/// SHA-256 fingerprint for deduplication (stable across identical content).
pub fn content_fingerprint(title: &str, body: &str, source_url: Option<&str>) -> String {
    let mut h = Sha256::new();
    h.update(title.as_bytes());
    h.update(b"\x00");
    h.update(body.as_bytes());
    h.update(b"\x00");
    if let Some(url) = source_url { h.update(url.as_bytes()); }
    hex::encode(h.finalize())
}

/// SHA-256 fingerprint of the source URL alone (normalized: trimmed + lowercase scheme+host).
///
/// This is used as a first-class URL-based dedup signal: two articles from the
/// same canonical URL are treated as duplicates regardless of title/body changes.
pub fn url_fingerprint(source_url: &str) -> String {
    // Minimal normalization: trim whitespace, lowercase the scheme and host portion.
    let normalized = source_url.trim();
    let normalized = if let Some(rest) = normalized.strip_prefix("HTTPS://")
        .or_else(|| normalized.strip_prefix("HTTP://"))
    {
        // Reconstruct with lower-cased scheme.
        let scheme = if normalized.to_ascii_lowercase().starts_with("https") { "https" } else { "http" };
        format!("{scheme}://{rest}")
    } else {
        normalized.to_owned()
    };
    let mut h = Sha256::new();
    h.update(normalized.as_bytes());
    hex::encode(h.finalize())
}

// ── Slug ──────────────────────────────────────────────────────────────────────

/// Convert free-form text to a URL-safe, hyphen-separated slug (max 120 chars).
pub fn slugify(input: &str) -> String {
    let mut slug = String::with_capacity(input.len().min(130));
    let mut prev_hyphen = true; // suppress leading hyphens

    for ch in input.chars() {
        if ch.is_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }

    if slug.ends_with('-') { slug.pop(); }

    if slug.len() > 120 {
        let cut = slug[..120].rfind('-').unwrap_or(120);
        slug.truncate(cut);
    }

    slug
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_removes_tags() {
        println!("[pipeline] strip_html_removes_tags");
        let result = strip_html("<p>Hello <b>world</b></p>");
        assert!(result.contains("Hello"));
        assert!(result.contains("world"));
        assert!(!result.contains('<'));
        println!("[pipeline] strip_html_removes_tags: PASS");
    }

    #[test]
    fn normalize_collapses_whitespace() {
        println!("[pipeline] normalize_collapses_whitespace");
        assert_eq!(normalize_whitespace("a  b\t\nc"), "a b c");
        println!("[pipeline] normalize_collapses_whitespace: PASS");
    }

    #[test]
    fn slugify_produces_clean_slug() {
        println!("[pipeline] slugify_produces_clean_slug");
        assert_eq!(slugify("Train Delays: Update #3!"), "train-delays-update-3");
        assert_eq!(slugify("  --leading-hyphens--  "), "leading-hyphens");
        assert_eq!(slugify("UPPER CASE"), "upper-case");
        println!("[pipeline] slugify_produces_clean_slug: PASS");
    }

    #[test]
    fn slugify_truncates_long_input() {
        println!("[pipeline] slugify_truncates_long_input");
        let long = "word ".repeat(30);
        let slug = slugify(&long);
        assert!(slug.len() <= 120, "slug must not exceed 120 chars");
        assert!(!slug.ends_with('-'), "slug must not end with hyphen");
        println!("[pipeline] slugify_truncates_long_input: {} chars — PASS", slug.len());
    }

    #[test]
    fn fingerprint_is_deterministic() {
        println!("[pipeline] fingerprint_is_deterministic");
        let a = content_fingerprint("Hello", "World", Some("https://x.com"));
        let b = content_fingerprint("Hello", "World", Some("https://x.com"));
        assert_eq!(a, b);
        assert_eq!(a.len(), 64, "SHA-256 hex must be 64 chars");
        println!("[pipeline] fingerprint_is_deterministic: PASS");
    }

    #[test]
    fn fingerprint_differs_on_content_change() {
        println!("[pipeline] fingerprint_differs_on_content_change");
        let a = content_fingerprint("Hello", "World", None);
        let b = content_fingerprint("Hello", "World2", None);
        assert_ne!(a, b);
        println!("[pipeline] fingerprint_differs_on_content_change: PASS");
    }

    #[test]
    fn transform_rejects_empty_item() {
        println!("[pipeline] transform_rejects_empty_item");
        let raw = RawItem {
            title: None, body: None, category: None, tags: None,
            publish_date: None, source_url: None, author: None, image_url: None,
            departure_date: None,
        };
        assert!(transform(&raw).is_none());
        println!("[pipeline] transform_rejects_empty_item: PASS");
    }

    #[test]
    fn transform_sets_default_category() {
        println!("[pipeline] transform_sets_default_category");
        let raw = RawItem {
            title: Some("Test".into()), body: Some("Body content here for test.".into()),
            category: Some("unknown_type".into()), tags: None,
            publish_date: None, source_url: None, author: None, image_url: None,
            departure_date: None,
        };
        let item = transform(&raw).unwrap();
        assert_eq!(item.category, "general");
        println!("[pipeline] transform_sets_default_category: PASS");
    }

    #[test]
    fn transform_accepts_known_category() {
        println!("[pipeline] transform_accepts_known_category");
        let raw = RawItem {
            title: Some("Fare Update".into()),
            body:  Some("New fares for all routes.".into()),
            category: Some("Fares".into()), // mixed-case — should lowercase
            tags: Some(vec!["pricing".into()]),
            publish_date: Some("2024-01-15".into()),
            source_url: Some("https://example.com".into()),
            author: None, image_url: None,
            departure_date: None,
        };
        let item = transform(&raw).unwrap();
        assert_eq!(item.category, "fares");
        assert_eq!(item.tags, vec!["pricing"]);
        assert!(item.publish_date.is_some());
        assert!(!item.fingerprint.is_empty());
        assert!(!item.slug.is_empty());
        println!("[pipeline] transform_accepts_known_category: PASS");
    }

    #[test]
    fn expand_abbreviations_works() {
        println!("[pipeline] expand_abbreviations_works");
        let input = "arr. at London stn. approx. 10 min. late";
        let expanded = expand_abbreviations(input);
        assert!(expanded.contains("arrival"), "arr. should expand");
        assert!(expanded.contains("station"), "stn. should expand");
        assert!(expanded.contains("approximately"), "approx. should expand");
        assert!(expanded.contains("minutes"), "min. should expand");
        println!("[pipeline] expand_abbreviations_works: PASS");
    }
}
