-- Add dedicated URL-only fingerprint column to content_pages.
--
-- This supports first-class URL-based deduplication: two articles from the
-- same canonical source URL are deduped regardless of title/body changes.
-- The composite source_fingerprint (title+body+url hash) remains for
-- exact-content dedup; url_fingerprint handles same-URL updates.

ALTER TABLE content_pages
    ADD COLUMN IF NOT EXISTS url_fingerprint VARCHAR(64);

CREATE INDEX IF NOT EXISTS idx_content_url_fp ON content_pages(url_fingerprint)
    WHERE url_fingerprint IS NOT NULL;
