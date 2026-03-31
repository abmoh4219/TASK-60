//! Source readers for the offline collection engine.
//!
//! All source types in this project are **offline** (file-system based).
//! Readers implement the [`SourceReader`] trait and return up to `PAGE_SIZE`
//! items per call together with a resumption cursor.
//!
//! ## Cursor format (JSON)
//!
//! ```json
//! { "file_index": 3, "item_offset": 12 }
//! ```
//!
//! - `file_index`  — 0-based index into the sorted list of `.json` files under
//!                   the source's `base_path`.
//! - `item_offset` — 0-based position within that file's item array.
//!
//! `None` next-cursor signals the source is exhausted for this run.

use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};

use super::pipeline::RawItem;
use crate::domain::crawl::models::CrawlSource;

const PAGE_SIZE: usize = 50;

// ── Cursor ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileCursor {
    pub file_index:  usize,
    pub item_offset: usize,
}

impl FileCursor {
    pub fn from_value(v: &Value) -> Self {
        serde_json::from_value(v.clone()).unwrap_or_default()
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Yields pages of raw items from an underlying data source.
#[async_trait]
pub trait SourceReader: Send + Sync {
    async fn read_page(
        &self,
        source: &CrawlSource,
        cursor: Option<Value>,
    ) -> Result<(Vec<RawItem>, Option<Value>), String>;
}

// ── LocalPackageReader ────────────────────────────────────────────────────────

/// Reads JSON packages from `source.base_path`.
///
/// Each `.json` file must be either a JSON array of objects or a single object.
pub struct LocalPackageReader;

#[async_trait]
impl SourceReader for LocalPackageReader {
    async fn read_page(
        &self,
        source: &CrawlSource,
        cursor: Option<Value>,
    ) -> Result<(Vec<RawItem>, Option<Value>), String> {
        let base = source
            .base_path
            .as_deref()
            .ok_or_else(|| "LocalPackageReader: source has no base_path".to_owned())?;

        read_from_dir(base, cursor).await
    }
}

// ── InternalMirrorReader ──────────────────────────────────────────────────────

/// Reads JSON packages from `{source.base_path}/mirror/`.
pub struct InternalMirrorReader;

#[async_trait]
impl SourceReader for InternalMirrorReader {
    async fn read_page(
        &self,
        source: &CrawlSource,
        cursor: Option<Value>,
    ) -> Result<(Vec<RawItem>, Option<Value>), String> {
        let base = source
            .base_path
            .as_deref()
            .ok_or_else(|| "InternalMirrorReader: source has no base_path".to_owned())?;

        let mirror = format!("{base}/mirror");
        read_from_dir(&mirror, cursor).await
    }
}

// ── Shared implementation ─────────────────────────────────────────────────────

async fn read_from_dir(
    dir:    &str,
    cursor: Option<Value>,
) -> Result<(Vec<RawItem>, Option<Value>), String> {
    let path = Path::new(dir);
    let mut files = collect_json_files(path).await
        .map_err(|e| format!("Cannot read directory '{dir}': {e}"))?;

    files.sort(); // deterministic order across runs

    if files.is_empty() {
        return Ok((Vec::new(), None));
    }

    let cur = cursor.as_ref().map(FileCursor::from_value).unwrap_or_default();
    let mut file_idx = cur.file_index;
    let mut item_off = cur.item_offset;
    let mut collected: Vec<RawItem> = Vec::with_capacity(PAGE_SIZE);

    while collected.len() < PAGE_SIZE && file_idx < files.len() {
        let file_path = &files[file_idx];
        debug!(file = %file_path.display(), "Reading source file");

        let bytes = tokio::fs::read(file_path)
            .await
            .map_err(|e| format!("Cannot read '{}': {e}", file_path.display()))?;

        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("JSON parse error in '{}': {e}", file_path.display()))?;

        let items_json: Vec<Value> = match json {
            Value::Array(arr)  => arr,
            obj @ Value::Object(_) => vec![obj],
            _ => {
                warn!(file = %file_path.display(), "Unexpected JSON root type — skipping");
                file_idx += 1;
                item_off  = 0;
                continue;
            }
        };

        let remaining = PAGE_SIZE - collected.len();
        let start     = item_off.min(items_json.len());
        let end       = (start + remaining).min(items_json.len());

        for v in &items_json[start..end] {
            match serde_json::from_value::<RawItem>(v.clone()) {
                Ok(item)  => collected.push(item),
                Err(e)    => warn!("Skipping malformed item: {e}"),
            }
        }

        item_off = end;
        if item_off >= items_json.len() {
            file_idx += 1;
            item_off  = 0;
        }
    }

    let next_cursor = if file_idx < files.len() {
        Some(FileCursor { file_index: file_idx, item_offset: item_off }.to_value())
    } else {
        None
    };

    Ok((collected, next_cursor))
}

/// Collect all `.json` file paths in a directory (non-recursive).
async fn collect_json_files(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut read_dir = tokio::fs::read_dir(dir).await?;
    let mut paths    = Vec::new();

    while let Some(entry) = read_dir.next_entry().await? {
        let p = entry.path();
        if p.extension().map(|x| x.eq_ignore_ascii_case("json")).unwrap_or(false) {
            paths.push(p);
        }
    }

    Ok(paths)
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Return the appropriate `SourceReader` for the given `source_type` string.
pub fn reader_for(source_type: &str) -> Box<dyn SourceReader> {
    match source_type {
        "internal_mirror" => Box::new(InternalMirrorReader),
        _                 => Box::new(LocalPackageReader),
    }
}
