-- Migration 8: Schedule aggregation support.
--
-- Adds unique constraint for schedule upsert by crawl engine,
-- extends the source_type CHECK to include 'schedule_feed',
-- and seeds a schedule_feed crawl source for demonstration.

-- Extend the source_type CHECK constraint to include schedule_feed.
ALTER TABLE crawl_sources DROP CONSTRAINT IF EXISTS crawl_sources_source_type_check;
ALTER TABLE crawl_sources ADD CONSTRAINT crawl_sources_source_type_check
    CHECK (source_type IN ('local_package','internal_mirror','schedule_feed'));

-- Unique constraint for train_number + departure_time to allow ON CONFLICT upsert.
CREATE UNIQUE INDEX IF NOT EXISTS idx_schedules_train_departure
    ON schedules (train_number, departure_time);

-- Seed a schedule_feed crawl source for demonstration.
INSERT INTO crawl_sources (id, name, source_type, base_path, city, rate_limit_rps, keywords, is_active)
VALUES (
    '90000000-0000-0000-0000-000000000003',
    'Denver Schedule Feed',
    'schedule_feed',
    '/data/feeds/denver-schedules',
    'Denver',
    2.0,
    ARRAY['schedule'],
    TRUE
)
ON CONFLICT (id) DO NOTHING;
