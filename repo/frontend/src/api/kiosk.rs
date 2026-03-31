//! Typed API client for the public kiosk endpoints.
//!
//! All requests are unsigned (no Bearer token required).

use gloo_net::http::Request;
use serde::{Deserialize, Serialize};

use super::{ApiError, ApiResult};

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ContentSummary {
    pub id:            String,
    pub slug:          String,
    pub title:         String,
    pub category:      String,
    pub route_id:      Option<String>,
    pub is_published:  bool,
    /// rust_decimal serializes as a JSON string, e.g. `"92.50"`.
    pub quality_score: Option<String>,
    /// ISO date `"YYYY-MM-DD"` or null.
    pub publish_date:  Option<String>,
    pub updated_at:    String,
}

impl ContentSummary {
    /// Short human-readable date, e.g. `"Jan 15, 2024"`.
    pub fn display_date(&self) -> String {
        self.publish_date
            .as_deref()
            .and_then(parse_date_display)
            .unwrap_or_default()
    }

    pub fn category_label(&self) -> &str {
        category_label(&self.category)
    }

    pub fn category_color(&self) -> &str {
        category_color(&self.category)
    }
}

/// Full article with body and tags.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ContentArticle {
    pub id:                 String,
    pub slug:               String,
    pub title:              String,
    pub body:               String,
    pub category:           String,
    pub route_id:           Option<String>,
    pub publish_date:       Option<String>,
    pub is_published:       bool,
    pub quality_score:      Option<String>,
    pub source_url:         Option<String>,
    pub source_fingerprint: Option<String>,
    pub created_at:         String,
    pub updated_at:         String,
    pub tags:               Vec<String>,
}

impl ContentArticle {
    pub fn display_date(&self) -> String {
        self.publish_date
            .as_deref()
            .and_then(parse_date_display)
            .unwrap_or_default()
    }

    pub fn category_label(&self) -> &str {
        category_label(&self.category)
    }

    pub fn category_color(&self) -> &str {
        category_color(&self.category)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArticleResponse {
    pub article: ContentArticle,
    pub related: Vec<ContentSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResponse {
    pub items:       Vec<ContentSummary>,
    pub total:       i64,
    pub page:        i64,
    pub per_page:    i64,
    pub total_pages: i64,
    /// `"fts"` | `"fuzzy"` | `null`
    pub search_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ArchiveEntry {
    pub year:  i32,
    pub month: i32,
    pub count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArchiveIndexResponse {
    pub mode:    String,
    pub entries: Vec<ArchiveEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArchiveArticlesResponse {
    pub mode:        String,
    pub year:        i32,
    pub month:       i32,
    pub items:       Vec<ContentSummary>,
    pub total:       i64,
    pub page:        i64,
    pub per_page:    i64,
    pub total_pages: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CategoryCount {
    pub category: String,
    pub count:    i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct TagCount {
    pub tag:   String,
    pub count: i64,
}

// ── API functions ─────────────────────────────────────────────────────────────

/// Search / list published content.
pub async fn search_content(
    q:        Option<&str>,
    category: Option<&str>,
    tag:      Option<&str>,
    page:     i64,
    per_page: i64,
) -> ApiResult<SearchResponse> {
    let mut url = format!("/api/v1/kiosk/content?page={page}&per_page={per_page}");
    if let Some(q) = q.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&q={}", urlenc(q)));
    }
    if let Some(c) = category.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&category={}", urlenc(c)));
    }
    if let Some(t) = tag.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&tag={}", urlenc(t)));
    }
    get_public(&url).await
}

/// Fetch full article + related by slug.
pub async fn get_article(slug: &str) -> ApiResult<ArticleResponse> {
    get_public(&format!("/api/v1/kiosk/content/{}", urlenc(slug))).await
}

/// Archive index (year/month buckets) or filtered articles.
pub async fn get_archive_index(
    year:     Option<i32>,
    category: Option<&str>,
) -> ApiResult<ArchiveIndexResponse> {
    let mut url = "/api/v1/kiosk/archive?".to_owned();
    if let Some(y) = year     { url.push_str(&format!("year={y}&")); }
    if let Some(c) = category { url.push_str(&format!("category={}&", urlenc(c))); }
    get_public(&url).await
}

pub async fn get_archive_articles(
    year:     i32,
    month:    i32,
    category: Option<&str>,
    page:     i64,
) -> ApiResult<ArchiveArticlesResponse> {
    let mut url = format!("/api/v1/kiosk/archive?year={year}&month={month}&page={page}");
    if let Some(c) = category { url.push_str(&format!("&category={}", urlenc(c))); }
    get_public(&url).await
}

/// Category list with article counts.
pub async fn list_categories() -> ApiResult<Vec<CategoryCount>> {
    get_public("/api/v1/kiosk/categories").await
}

/// Top tags with article counts.
pub async fn list_tags() -> ApiResult<Vec<TagCount>> {
    get_public("/api/v1/kiosk/tags").await
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn get_public<T: for<'de> Deserialize<'de>>(url: &str) -> ApiResult<T> {
    let resp = Request::get(url)
        .send()
        .await
        .map_err(|e| ApiError { status: 0, message: e.to_string() })?;

    if resp.ok() {
        resp.json::<T>().await
            .map_err(|e| ApiError { status: 0, message: e.to_string() })
    } else {
        let status  = resp.status();
        let text    = resp.text().await.unwrap_or_default();
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
            .unwrap_or(text);
        Err(ApiError { status, message })
    }
}

fn urlenc(s: &str) -> String {
    js_sys::encode_uri_component(s).as_string().unwrap_or_else(|| s.to_owned())
}

// ── Display helpers ───────────────────────────────────────────────────────────

fn parse_date_display(iso: &str) -> Option<String> {
    let parts: Vec<&str> = iso.split('-').collect();
    if parts.len() < 3 { return None; }
    let year  = parts[0];
    let month = parts[1].parse::<u32>().ok()?;
    let day   = parts[2].parse::<u32>().ok()?;
    let month_name = match month {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => return None,
    };
    Some(format!("{month_name} {day}, {year}"))
}

pub fn month_name(m: i32) -> &'static str {
    match m {
        1 => "January", 2 => "February", 3 => "March",
        4 => "April",   5 => "May",      6 => "June",
        7 => "July",    8 => "August",   9 => "September",
        10 => "October", 11 => "November", 12 => "December",
        _ => "Unknown",
    }
}

pub fn category_label(cat: &str) -> &str {
    match cat {
        "fares"         => "Fares",
        "delays"        => "Delays",
        "baggage"       => "Baggage",
        "accessibility" => "Accessibility",
        _               => "General",
    }
}

pub fn category_color(cat: &str) -> &str {
    match cat {
        "fares"         => "bg-green-100 text-green-800",
        "delays"        => "bg-red-100 text-red-800",
        "baggage"       => "bg-amber-100 text-amber-800",
        "accessibility" => "bg-purple-100 text-purple-800",
        _               => "bg-gray-100 text-gray-800",
    }
}
