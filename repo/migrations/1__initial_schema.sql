-- =============================================================================
-- RailOps  —  V1 initial schema + seed data
-- PostgreSQL 16
--
-- Seeded admin credentials
--   username : admin
--   password : AdminRailOps2024!   (Argon2id hash injected at first boot)
-- =============================================================================

-- ── Extensions ────────────────────────────────────────────────────────────────
CREATE EXTENSION IF NOT EXISTS "pg_trgm";   -- trigram similarity for fuzzy search
CREATE EXTENSION IF NOT EXISTS "unaccent";  -- accent-insensitive search

-- =============================================================================
-- AUTH
-- =============================================================================

CREATE TABLE users (
    id                   UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    username             VARCHAR(100) NOT NULL UNIQUE,
    password_hash        TEXT         NOT NULL,
    role                 VARCHAR(50)  NOT NULL
                             CHECK (role IN ('admin','ops_agent','cs_agent','dispatcher','kiosk')),
    full_name            VARCHAR(200),
    email                VARCHAR(200),
    is_active            BOOLEAN      NOT NULL DEFAULT TRUE,
    failed_login_attempts INT         NOT NULL DEFAULT 0,
    locked_until         TIMESTAMPTZ,
    last_login_at        TIMESTAMPTZ,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE TABLE sessions (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash     VARCHAR(64) NOT NULL UNIQUE,  -- SHA-256 hex of the bearer token
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at     TIMESTAMPTZ NOT NULL,
    last_active_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ip_address     INET,
    user_agent     TEXT
);

CREATE INDEX idx_sessions_user    ON sessions(user_id);
CREATE INDEX idx_sessions_expires ON sessions(expires_at);

-- =============================================================================
-- IMMUTABLE AUDIT LOG  (INSERT-ONLY — never UPDATE or DELETE)
-- =============================================================================

CREATE TABLE audit_logs (
    id          BIGSERIAL    PRIMARY KEY,
    event_type  VARCHAR(100) NOT NULL,
    entity_type VARCHAR(100),
    entity_id   UUID,
    user_id     UUID         REFERENCES users(id) ON DELETE SET NULL,
    action      TEXT         NOT NULL,
    old_data    JSONB,
    new_data    JSONB,
    ip_address  INET,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_entity   ON audit_logs(entity_type, entity_id);
CREATE INDEX idx_audit_user     ON audit_logs(user_id);
CREATE INDEX idx_audit_created  ON audit_logs(created_at);

-- Prevent modification of audit rows
CREATE RULE audit_no_update AS ON UPDATE TO audit_logs DO INSTEAD NOTHING;
CREATE RULE audit_no_delete AS ON DELETE TO audit_logs DO INSTEAD NOTHING;

-- =============================================================================
-- RAIL — routes, schedules, inventory
-- =============================================================================

CREATE TABLE routes (
    id               UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    route_code       VARCHAR(50)  NOT NULL UNIQUE,
    name             VARCHAR(200) NOT NULL,
    origin_city      VARCHAR(100) NOT NULL,
    destination_city VARCHAR(100) NOT NULL,
    is_active        BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE TABLE schedules (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    route_id       UUID        NOT NULL REFERENCES routes(id),
    train_number   VARCHAR(50) NOT NULL,
    departure_time TIMESTAMPTZ NOT NULL,
    arrival_time   TIMESTAMPTZ NOT NULL,
    status         VARCHAR(50) NOT NULL DEFAULT 'scheduled'
                       CHECK (status IN ('scheduled','delayed','cancelled','completed')),
    delay_minutes  INT         NOT NULL DEFAULT 0,
    platform       VARCHAR(20),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_schedules_route     ON schedules(route_id);
CREATE INDEX idx_schedules_departure ON schedules(departure_time);
CREATE INDEX idx_schedules_status    ON schedules(status);

CREATE TABLE seat_classes (
    id         UUID           PRIMARY KEY DEFAULT gen_random_uuid(),
    name       VARCHAR(100)   NOT NULL,
    code       VARCHAR(20)    NOT NULL UNIQUE,
    base_fare  NUMERIC(10,2)  NOT NULL CHECK (base_fare >= 0)
);

CREATE TABLE inventory_snapshots (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    schedule_id     UUID        NOT NULL REFERENCES schedules(id),
    seat_class_id   UUID        NOT NULL REFERENCES seat_classes(id),
    total_seats     INT         NOT NULL CHECK (total_seats >= 0),
    available_seats INT         NOT NULL CHECK (available_seats >= 0),
    snapshot_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (schedule_id, seat_class_id, snapshot_at)
);

CREATE INDEX idx_inventory_schedule ON inventory_snapshots(schedule_id);

-- =============================================================================
-- ORDERS
-- =============================================================================

CREATE TABLE passengers (
    id                      UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    full_name               VARCHAR(200) NOT NULL,
    -- Phone stored AES-256-GCM encrypted; only last 4 digits kept in clear.
    phone_encrypted         BYTEA,
    phone_last4             VARCHAR(4),
    email_encrypted         BYTEA,
    created_at              TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    pii_purge_requested_at  TIMESTAMPTZ,
    pii_purged_at           TIMESTAMPTZ
);

CREATE INDEX idx_passengers_name ON passengers USING gin (full_name gin_trgm_ops);

CREATE TABLE orders (
    id                   UUID           PRIMARY KEY DEFAULT gen_random_uuid(),
    order_number         VARCHAR(50)    NOT NULL UNIQUE,
    passenger_id         UUID           NOT NULL REFERENCES passengers(id),
    schedule_id          UUID           NOT NULL REFERENCES schedules(id),
    seat_class_id        UUID           NOT NULL REFERENCES seat_classes(id),
    seat_number          VARCHAR(20),
    status               VARCHAR(50)    NOT NULL DEFAULT 'pending'
                             CHECK (status IN ('pending','confirmed','held','cancelled','refunded','completed')),
    fare_amount          NUMERIC(10,2)  NOT NULL CHECK (fare_amount >= 0),
    fee_override_amount  NUMERIC(10,2),
    fee_override_reason  TEXT,
    hold_expires_at      TIMESTAMPTZ,
    cancellation_reason  TEXT,
    refund_amount        NUMERIC(10,2),
    disruption_flag      BOOLEAN        NOT NULL DEFAULT FALSE,
    created_at           TIMESTAMPTZ    NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ    NOT NULL DEFAULT NOW(),
    created_by           UUID           REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX idx_orders_passenger  ON orders(passenger_id);
CREATE INDEX idx_orders_schedule   ON orders(schedule_id);
CREATE INDEX idx_orders_status     ON orders(status);
CREATE INDEX idx_orders_hold_exp   ON orders(hold_expires_at) WHERE status = 'held';

CREATE TABLE order_events (
    id           BIGSERIAL    PRIMARY KEY,
    order_id     UUID         NOT NULL REFERENCES orders(id),
    event_type   VARCHAR(100) NOT NULL,
    performed_by UUID         REFERENCES users(id) ON DELETE SET NULL,
    reason       TEXT,
    data         JSONB,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_order_events_order ON order_events(order_id);

-- =============================================================================
-- CONTENT / KIOSK
-- =============================================================================

CREATE TABLE content_pages (
    id                UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    slug              VARCHAR(200) NOT NULL UNIQUE,
    title             VARCHAR(500) NOT NULL,
    body              TEXT         NOT NULL,
    category          VARCHAR(100) NOT NULL DEFAULT 'general'
                          CHECK (category IN ('fares','delays','baggage','accessibility','general')),
    route_id          UUID         REFERENCES routes(id) ON DELETE SET NULL,
    publish_date      DATE,
    is_published      BOOLEAN      NOT NULL DEFAULT FALSE,
    quality_score     NUMERIC(5,2),
    source_url        VARCHAR(500),
    source_fingerprint VARCHAR(64),
    -- Full-text search vector (updated by trigger below)
    search_vector     TSVECTOR,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_content_search   ON content_pages USING gin(search_vector);
CREATE INDEX idx_content_category ON content_pages(category);
CREATE INDEX idx_content_date     ON content_pages(publish_date);
CREATE INDEX idx_content_route    ON content_pages(route_id);

CREATE TABLE content_tags (
    content_id UUID         NOT NULL REFERENCES content_pages(id) ON DELETE CASCADE,
    tag        VARCHAR(100) NOT NULL,
    PRIMARY KEY (content_id, tag)
);

CREATE INDEX idx_content_tags_tag ON content_tags(tag);

-- Auto-update search_vector on insert/update
CREATE OR REPLACE FUNCTION content_search_vector_update() RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector :=
        setweight(to_tsvector('english', coalesce(NEW.title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.body,  '')), 'B');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_content_search_vector
    BEFORE INSERT OR UPDATE ON content_pages
    FOR EACH ROW EXECUTE FUNCTION content_search_vector_update();

-- =============================================================================
-- DATA INGESTION ENGINE
-- =============================================================================

CREATE TABLE crawl_sources (
    id           UUID           PRIMARY KEY DEFAULT gen_random_uuid(),
    name         VARCHAR(200)   NOT NULL,
    source_type  VARCHAR(50)    NOT NULL DEFAULT 'local_package'
                     CHECK (source_type IN ('local_package','internal_mirror')),
    base_path    VARCHAR(500),
    city         VARCHAR(100),
    keywords     TEXT[],
    rate_limit_rps NUMERIC(5,2) NOT NULL DEFAULT 2.0 CHECK (rate_limit_rps > 0),
    is_active    BOOLEAN        NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ    NOT NULL DEFAULT NOW()
);

CREATE TABLE crawl_tasks (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id        UUID        NOT NULL REFERENCES crawl_sources(id),
    task_name        VARCHAR(200) NOT NULL,
    pagination_rules JSONB,
    incremental      BOOLEAN     NOT NULL DEFAULT TRUE,
    last_crawled_at  TIMESTAMPTZ,
    next_crawl_at    TIMESTAMPTZ,
    status           VARCHAR(50) NOT NULL DEFAULT 'idle'
                         CHECK (status IN ('idle','running','paused','failed')),
    resume_cursor    JSONB,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_crawl_tasks_source ON crawl_tasks(source_id);
CREATE INDEX idx_crawl_tasks_status ON crawl_tasks(status);

CREATE TABLE crawl_runs (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id          UUID        NOT NULL REFERENCES crawl_tasks(id),
    started_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at      TIMESTAMPTZ,
    status           VARCHAR(50) NOT NULL DEFAULT 'running'
                         CHECK (status IN ('running','completed','failed','interrupted')),
    pages_fetched    INT         NOT NULL DEFAULT 0,
    items_ingested   INT         NOT NULL DEFAULT 0,
    items_quarantined INT        NOT NULL DEFAULT 0,
    error_log        TEXT
);

CREATE INDEX idx_crawl_runs_task ON crawl_runs(task_id);

CREATE TABLE quality_logs (
    id                   BIGSERIAL    PRIMARY KEY,
    crawl_run_id         UUID         REFERENCES crawl_runs(id),
    content_id           UUID         REFERENCES content_pages(id),
    url_fingerprint      VARCHAR(64),
    completeness_score   NUMERIC(5,2),
    accuracy_score       NUMERIC(5,2),
    timeliness_score     NUMERIC(5,2),
    quality_score        NUMERIC(5,2),
    issues               JSONB,
    is_quarantined       BOOLEAN      NOT NULL DEFAULT FALSE,
    quarantine_reason    TEXT,
    transformation_steps JSONB,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_quality_run     ON quality_logs(crawl_run_id);
CREATE INDEX idx_quality_content ON quality_logs(content_id);
CREATE INDEX idx_quality_score   ON quality_logs(quality_score);

-- =============================================================================
-- STAFFING / CONTRACTOR MATCHING
-- =============================================================================

CREATE TABLE contractors (
    id              UUID           PRIMARY KEY DEFAULT gen_random_uuid(),
    full_name       VARCHAR(200)   NOT NULL,
    phone_encrypted BYTEA,
    phone_last4     VARCHAR(4),
    email_encrypted BYTEA,
    region          VARCHAR(100),
    quality_rating  NUMERIC(3,2)   NOT NULL DEFAULT 5.00
                        CHECK (quality_rating BETWEEN 0 AND 5),
    is_active       BOOLEAN        NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ    NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ    NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_contractors_region ON contractors(region);
CREATE INDEX idx_contractors_name   ON contractors USING gin (full_name gin_trgm_ops);

CREATE TABLE contractor_tags (
    contractor_id UUID         NOT NULL REFERENCES contractors(id) ON DELETE CASCADE,
    tag           VARCHAR(100) NOT NULL,
    PRIMARY KEY (contractor_id, tag)
);

CREATE INDEX idx_contractor_tags_tag ON contractor_tags(tag);

CREATE TABLE contractor_availability (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    contractor_id  UUID        NOT NULL REFERENCES contractors(id) ON DELETE CASCADE,
    available_from TIMESTAMPTZ NOT NULL,
    available_to   TIMESTAMPTZ NOT NULL,
    notes          TEXT,
    CHECK (available_to > available_from)
);

CREATE INDEX idx_availability_contractor ON contractor_availability(contractor_id);
CREATE INDEX idx_availability_window     ON contractor_availability(available_from, available_to);

CREATE TABLE shifts (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    schedule_id   UUID        REFERENCES schedules(id) ON DELETE SET NULL,
    role          VARCHAR(100) NOT NULL CHECK (role IN ('conductor','driver')),
    region        VARCHAR(100),
    required_tags TEXT[],
    shift_start   TIMESTAMPTZ NOT NULL,
    shift_end     TIMESTAMPTZ NOT NULL,
    status        VARCHAR(50) NOT NULL DEFAULT 'open'
                      CHECK (status IN ('open','assigned','completed','cancelled')),
    is_critical   BOOLEAN     NOT NULL DEFAULT FALSE,
    created_by    UUID        REFERENCES users(id) ON DELETE SET NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (shift_end > shift_start)
);

CREATE INDEX idx_shifts_status   ON shifts(status);
CREATE INDEX idx_shifts_region   ON shifts(region);
CREATE INDEX idx_shifts_start    ON shifts(shift_start);

CREATE TABLE shift_assignments (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    shift_id       UUID        NOT NULL REFERENCES shifts(id),
    contractor_id  UUID        NOT NULL REFERENCES contractors(id),
    match_score    NUMERIC(5,2),
    match_reasons  JSONB,          -- top-3 reasons array
    assigned_by    UUID        REFERENCES users(id) ON DELETE SET NULL,
    assigned_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status         VARCHAR(50) NOT NULL DEFAULT 'proposed'
                       CHECK (status IN ('proposed','accepted','rejected')),
    UNIQUE (shift_id, contractor_id)
);

CREATE TABLE subscriptions (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    subscriber_type VARCHAR(50) NOT NULL CHECK (subscriber_type IN ('dispatcher','contractor')),
    subscriber_id   UUID        NOT NULL,
    target_type     VARCHAR(50) NOT NULL CHECK (target_type IN ('shift','route')),
    target_id       UUID        NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (subscriber_type, subscriber_id, target_type, target_id)
);

CREATE INDEX idx_subscriptions_subscriber ON subscriptions(subscriber_type, subscriber_id);
CREATE INDEX idx_subscriptions_target     ON subscriptions(target_type, target_id);

-- =============================================================================
-- CREDENTIAL / DOCUMENT MANAGEMENT
-- =============================================================================

CREATE TABLE credentials (
    id             UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    contractor_id  UUID         NOT NULL REFERENCES contractors(id),
    document_type  VARCHAR(100) NOT NULL,
    file_name      VARCHAR(500) NOT NULL,
    -- Path relative to /app/uploads
    file_path      VARCHAR(500) NOT NULL UNIQUE,
    file_size_bytes INT          NOT NULL CHECK (file_size_bytes > 0),
    mime_type      VARCHAR(100) NOT NULL
                       CHECK (mime_type IN ('application/pdf','image/jpeg','image/png')),
    -- SHA-256 hex fingerprint of the raw file bytes
    fingerprint    VARCHAR(64)  NOT NULL UNIQUE,
    expires_at     DATE,
    status         VARCHAR(50)  NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending','approved','rejected','expired')),
    uploaded_by    UUID         REFERENCES users(id) ON DELETE SET NULL,
    uploaded_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    reviewed_by    UUID         REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at    TIMESTAMPTZ,
    review_notes   TEXT
);

CREATE INDEX idx_credentials_contractor ON credentials(contractor_id);
CREATE INDEX idx_credentials_expires    ON credentials(expires_at);
CREATE INDEX idx_credentials_status     ON credentials(status);

CREATE TABLE credential_audit_log (
    id             BIGSERIAL    PRIMARY KEY,
    credential_id  UUID         NOT NULL REFERENCES credentials(id),
    action         VARCHAR(100) NOT NULL
                       CHECK (action IN ('uploaded','viewed','approved','rejected','watermarked','esigned')),
    performed_by   UUID         REFERENCES users(id) ON DELETE SET NULL,
    viewer_name    VARCHAR(200),
    ip_address     INET,
    data           JSONB,
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_cred_audit_credential ON credential_audit_log(credential_id);
CREATE INDEX idx_cred_audit_created    ON credential_audit_log(created_at);

-- Tamper-evident: no updates or deletes
CREATE RULE cred_audit_no_update AS ON UPDATE TO credential_audit_log DO INSTEAD NOTHING;
CREATE RULE cred_audit_no_delete AS ON DELETE TO credential_audit_log DO INSTEAD NOTHING;

-- =============================================================================
-- E-SIGNATURES  (internal capture — no third-party service)
-- =============================================================================

CREATE TABLE esignatures (
    id                   UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type          VARCHAR(100) NOT NULL
                             CHECK (entity_type IN ('credential','order','assignment')),
    entity_id            UUID         NOT NULL,
    signer_name          VARCHAR(200) NOT NULL,
    signed_date          DATE         NOT NULL,
    -- Optional path to drawn signature image stored under /app/uploads
    signature_image_path VARCHAR(500),
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_esig_entity ON esignatures(entity_type, entity_id);

-- =============================================================================
-- CONFIGURABLE BUSINESS RULES  (editable by Admin role)
-- =============================================================================

CREATE TABLE business_rules (
    id          UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_key    VARCHAR(100) NOT NULL UNIQUE,
    rule_value  TEXT         NOT NULL,
    description TEXT,
    updated_by  UUID         REFERENCES users(id) ON DELETE SET NULL,
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- =============================================================================
-- SEED DATA
-- =============================================================================

-- ── Users ─────────────────────────────────────────────────────────────────────
-- Admin password hash is replaced on first boot via seed_admin_password().
INSERT INTO users (id, username, password_hash, role, full_name, email)
VALUES
    ('00000000-0000-0000-0000-000000000001',
     'admin',     'SEED_PENDING', 'admin',      'System Administrator', 'admin@railops.local'),
    ('00000000-0000-0000-0000-000000000002',
     'ops_agent1','SEED_PENDING', 'ops_agent',  'Alice Nguyen',         'alice@railops.local'),
    ('00000000-0000-0000-0000-000000000003',
     'cs_agent1', 'SEED_PENDING', 'cs_agent',   'Bob Reyes',            'bob@railops.local'),
    ('00000000-0000-0000-0000-000000000004',
     'dispatch1', 'SEED_PENDING', 'dispatcher', 'Carol Zhang',          'carol@railops.local')
ON CONFLICT (username) DO NOTHING;

-- ── Default business rules ────────────────────────────────────────────────────
INSERT INTO business_rules (rule_key, rule_value, description) VALUES
    ('order_hold_ttl_minutes',      '15',   'Minutes before an unconfirmed held order expires'),
    ('refund_full_hours',           '24',   'Hours before departure for full refund (minus fee)'),
    ('refund_partial_hours',        '2',    'Hours before departure for 50 % partial refund'),
    ('refund_processing_fee_usd',   '5.00', 'Fixed fee deducted on full refunds'),
    ('quality_publish_threshold',   '85',   'Minimum quality score to publish content'),
    ('similarity_quarantine',       '0.92', 'Content similarity threshold for quarantine'),
    ('crawl_rate_limit_rps',        '2.0',  'Default crawl rate per source (req/sec)'),
    ('crawl_max_workers',           '10',   'Global concurrent crawl worker cap'),
    ('sample_review_pct',           '0.02', 'Fraction of records sampled for batch review'),
    ('pii_purge_days',              '30',   'Days after trip completion before PII is purged'),
    ('session_idle_minutes',        '30',   'Idle session expiry in minutes'),
    ('rate_limit_rpm',              '60',   'API requests per minute per session'),
    ('max_failed_logins',           '5',    'Failed logins before account lockout'),
    ('lockout_minutes',             '15',   'Account lockout duration in minutes')
ON CONFLICT (rule_key) DO NOTHING;

-- ── Routes ────────────────────────────────────────────────────────────────────
INSERT INTO routes (id, route_code, name, origin_city, destination_city) VALUES
    ('10000000-0000-0000-0000-000000000001', 'EW-001', 'East-West Express',    'Denver',       'Chicago'),
    ('10000000-0000-0000-0000-000000000002', 'MW-002', 'Mountain West Link',   'Denver',       'Salt Lake City'),
    ('10000000-0000-0000-0000-000000000003', 'NP-003', 'Northern Plains Rail', 'Billings',     'Minneapolis'),
    ('10000000-0000-0000-0000-000000000004', 'SP-004', 'Southern Pacific Run', 'Albuquerque',  'Oklahoma City'),
    ('10000000-0000-0000-0000-000000000005', 'CL-005', 'Coastal Loop',         'Portland',     'Seattle')
ON CONFLICT (route_code) DO NOTHING;

-- ── Seat classes ──────────────────────────────────────────────────────────────
INSERT INTO seat_classes (id, name, code, base_fare) VALUES
    ('20000000-0000-0000-0000-000000000001', 'Economy',        'ECO',  49.00),
    ('20000000-0000-0000-0000-000000000002', 'Business',       'BIZ', 119.00),
    ('20000000-0000-0000-0000-000000000003', 'First Class',    'FST', 249.00),
    ('20000000-0000-0000-0000-000000000004', 'Sleeper',        'SLP', 199.00),
    ('20000000-0000-0000-0000-000000000005', 'Family Saver',   'FAM',  79.00)
ON CONFLICT (code) DO NOTHING;

-- ── Schedules (departure times relative to NOW so data stays current) ─────────
INSERT INTO schedules (id, route_id, train_number, departure_time, arrival_time, status, platform)
VALUES
    ('30000000-0000-0000-0000-000000000001',
     '10000000-0000-0000-0000-000000000001', 'EW-101',
     NOW() + INTERVAL  '6 hours',  NOW() + INTERVAL '20 hours', 'scheduled', '3A'),
    ('30000000-0000-0000-0000-000000000002',
     '10000000-0000-0000-0000-000000000001', 'EW-102',
     NOW() + INTERVAL '30 hours',  NOW() + INTERVAL '44 hours', 'scheduled', '3B'),
    ('30000000-0000-0000-0000-000000000003',
     '10000000-0000-0000-0000-000000000002', 'MW-201',
     NOW() + INTERVAL  '4 hours',  NOW() + INTERVAL '12 hours', 'scheduled', '1A'),
    ('30000000-0000-0000-0000-000000000004',
     '10000000-0000-0000-0000-000000000002', 'MW-202',
     NOW() + INTERVAL '28 hours',  NOW() + INTERVAL '36 hours', 'delayed',   '1B'),
    ('30000000-0000-0000-0000-000000000005',
     '10000000-0000-0000-0000-000000000003', 'NP-301',
     NOW() + INTERVAL  '8 hours',  NOW() + INTERVAL '22 hours', 'scheduled', '2C'),
    ('30000000-0000-0000-0000-000000000006',
     '10000000-0000-0000-0000-000000000004', 'SP-401',
     NOW() + INTERVAL '10 hours',  NOW() + INTERVAL '18 hours', 'scheduled', '4A'),
    ('30000000-0000-0000-0000-000000000007',
     '10000000-0000-0000-0000-000000000005', 'CL-501',
     NOW() + INTERVAL  '2 hours',  NOW() + INTERVAL  '5 hours', 'scheduled', '5A')
ON CONFLICT DO NOTHING;

-- ── Inventory snapshots ───────────────────────────────────────────────────────
INSERT INTO inventory_snapshots (schedule_id, seat_class_id, total_seats, available_seats)
SELECT s.id, sc.id,
    CASE sc.code WHEN 'ECO' THEN 120 WHEN 'BIZ' THEN 40 WHEN 'FST' THEN 12
                 WHEN 'SLP' THEN 24  WHEN 'FAM' THEN 30  END,
    CASE sc.code WHEN 'ECO' THEN  88 WHEN 'BIZ' THEN 31 WHEN 'FST' THEN 10
                 WHEN 'SLP' THEN 18  WHEN 'FAM' THEN 22  END
FROM schedules s CROSS JOIN seat_classes sc
ON CONFLICT DO NOTHING;

-- ── Passengers ────────────────────────────────────────────────────────────────
INSERT INTO passengers (id, full_name, phone_last4) VALUES
    ('40000000-0000-0000-0000-000000000001', 'Maria Gonzalez',  '4821'),
    ('40000000-0000-0000-0000-000000000002', 'James O''Brien',  '7703'),
    ('40000000-0000-0000-0000-000000000003', 'Priya Sharma',    '2244'),
    ('40000000-0000-0000-0000-000000000004', 'Liang Wei',       '9910'),
    ('40000000-0000-0000-0000-000000000005', 'Fatima Al-Hassan','3356')
ON CONFLICT DO NOTHING;

-- ── Orders ────────────────────────────────────────────────────────────────────
INSERT INTO orders (id, order_number, passenger_id, schedule_id, seat_class_id, seat_number, status, hold_expires_at, fare_amount, created_by)
VALUES
    ('50000000-0000-0000-0000-000000000001', 'ORD-00001',
     '40000000-0000-0000-0000-000000000001',
     '30000000-0000-0000-0000-000000000001',
     '20000000-0000-0000-0000-000000000001', '14C', 'confirmed', NULL, 49.00,
     '00000000-0000-0000-0000-000000000003'),
    ('50000000-0000-0000-0000-000000000002', 'ORD-00002',
     '40000000-0000-0000-0000-000000000002',
     '30000000-0000-0000-0000-000000000001',
     '20000000-0000-0000-0000-000000000002', '2A', 'confirmed', NULL, 119.00,
     '00000000-0000-0000-0000-000000000003'),
    ('50000000-0000-0000-0000-000000000003', 'ORD-00003',
     '40000000-0000-0000-0000-000000000003',
     '30000000-0000-0000-0000-000000000003',
     '20000000-0000-0000-0000-000000000001', '22D', 'held',
     NOW() + INTERVAL '15 minutes', 49.00,
     '00000000-0000-0000-0000-000000000003'),
    ('50000000-0000-0000-0000-000000000004', 'ORD-00004',
     '40000000-0000-0000-0000-000000000004',
     '30000000-0000-0000-0000-000000000005',
     '20000000-0000-0000-0000-000000000003', '1B', 'confirmed', NULL, 249.00,
     '00000000-0000-0000-0000-000000000002'),
    ('50000000-0000-0000-0000-000000000005', 'ORD-00005',
     '40000000-0000-0000-0000-000000000005',
     '30000000-0000-0000-0000-000000000007',
     '20000000-0000-0000-0000-000000000005', '8F', 'cancelled', NULL, 79.00,
     '00000000-0000-0000-0000-000000000003')
ON CONFLICT DO NOTHING;

-- ── Content pages (kiosk) ─────────────────────────────────────────────────────
INSERT INTO content_pages (id, slug, title, body, category, route_id, publish_date, is_published, quality_score)
VALUES
    ('60000000-0000-0000-0000-000000000001',
     'fares-economy-guide',
     'Economy Class Fares Guide',
     'Economy seats start at $49 on regional routes. Advance booking discounts '
     'of up to 30 % are available when purchasing 14 or more days ahead. '
     'Children under 5 ride free with a paying adult. Baggage allowance is '
     'one carry-on (max 22 lb) and one checked bag (max 50 lb).',
     'fares', NULL, CURRENT_DATE, TRUE, 97.5),

    ('60000000-0000-0000-0000-000000000002',
     'delays-policy',
     'Service Delay & Compensation Policy',
     'Passengers affected by delays of 60 minutes or more are entitled to a '
     'travel voucher worth 25 % of their ticket price. Delays of 3 hours or '
     'more qualify for a full refund or rebooking at no extra cost. '
     'Service disruption codes must be issued by operations staff.',
     'delays', NULL, CURRENT_DATE, TRUE, 95.0),

    ('60000000-0000-0000-0000-000000000003',
     'baggage-policy',
     'Baggage Rules & Oversized Items',
     'Standard baggage: one carry-on up to 22 lb and one checked bag up to 50 lb. '
     'Additional bags cost $25 each. Oversized items (bicycles, skis) require '
     'advance notice and incur a $40 handling fee. Prohibited items include '
     'flammable materials, loose lithium batteries, and live animals (except '
     'approved service animals).',
     'baggage', NULL, CURRENT_DATE, TRUE, 91.0),

    ('60000000-0000-0000-0000-000000000004',
     'east-west-express-guide',
     'East-West Express Route Guide',
     'The East-West Express (EW-001) connects Denver to Chicago with stops at '
     'Omaha and Des Moines. Journey time is approximately 14 hours. On-board '
     'amenities include a dining car, Wi-Fi in Business and First Class, '
     'and accessible seating throughout.',
     'general', '10000000-0000-0000-0000-000000000001',
     CURRENT_DATE, TRUE, 93.0),

    ('60000000-0000-0000-0000-000000000005',
     'accessibility-services',
     'Accessibility & Special Assistance',
     'RailOps provides wheelchair ramps, priority boarding, and reserved '
     'accessible seating on all routes. Passengers requiring special assistance '
     'should notify staff at least 48 hours before departure. Service animals '
     'travel free in the cabin. TTY services are available at all staffed stations.',
     'accessibility', NULL, CURRENT_DATE, TRUE, 98.0)
ON CONFLICT (slug) DO NOTHING;

INSERT INTO content_tags (content_id, tag) VALUES
    ('60000000-0000-0000-0000-000000000001', 'fares'),
    ('60000000-0000-0000-0000-000000000001', 'economy'),
    ('60000000-0000-0000-0000-000000000001', 'booking'),
    ('60000000-0000-0000-0000-000000000002', 'delays'),
    ('60000000-0000-0000-0000-000000000002', 'compensation'),
    ('60000000-0000-0000-0000-000000000002', 'refund'),
    ('60000000-0000-0000-0000-000000000003', 'baggage'),
    ('60000000-0000-0000-0000-000000000003', 'luggage'),
    ('60000000-0000-0000-0000-000000000004', 'east-west'),
    ('60000000-0000-0000-0000-000000000004', 'route'),
    ('60000000-0000-0000-0000-000000000004', 'schedule'),
    ('60000000-0000-0000-0000-000000000005', 'accessibility'),
    ('60000000-0000-0000-0000-000000000005', 'wheelchair'),
    ('60000000-0000-0000-0000-000000000005', 'special-assistance')
ON CONFLICT DO NOTHING;

-- ── Contractors ───────────────────────────────────────────────────────────────
INSERT INTO contractors (id, full_name, phone_last4, region, quality_rating) VALUES
    ('70000000-0000-0000-0000-000000000001', 'Derek Paulson',  '5512', 'mountain_west', 4.80),
    ('70000000-0000-0000-0000-000000000002', 'Sandra Kim',     '8831', 'mountain_west', 4.95),
    ('70000000-0000-0000-0000-000000000003', 'Marcus Rivera',  '3307', 'great_plains',  4.60),
    ('70000000-0000-0000-0000-000000000004', 'Yuki Tanaka',    '0024', 'pacific_nw',    4.75),
    ('70000000-0000-0000-0000-000000000005', 'Ahmed Osman',    '6619', 'southern',      4.50)
ON CONFLICT DO NOTHING;

INSERT INTO contractor_tags (contractor_id, tag) VALUES
    ('70000000-0000-0000-0000-000000000001', 'conductor'),
    ('70000000-0000-0000-0000-000000000001', 'hazmat'),
    ('70000000-0000-0000-0000-000000000002', 'conductor'),
    ('70000000-0000-0000-0000-000000000002', 'first_aid'),
    ('70000000-0000-0000-0000-000000000003', 'driver'),
    ('70000000-0000-0000-0000-000000000003', 'night_shift'),
    ('70000000-0000-0000-0000-000000000004', 'conductor'),
    ('70000000-0000-0000-0000-000000000004', 'multilingual'),
    ('70000000-0000-0000-0000-000000000005', 'driver'),
    ('70000000-0000-0000-0000-000000000005', 'first_aid')
ON CONFLICT DO NOTHING;

INSERT INTO contractor_availability (contractor_id, available_from, available_to) VALUES
    ('70000000-0000-0000-0000-000000000001', NOW(), NOW() + INTERVAL '30 days'),
    ('70000000-0000-0000-0000-000000000002', NOW(), NOW() + INTERVAL '14 days'),
    ('70000000-0000-0000-0000-000000000003', NOW() + INTERVAL '2 days', NOW() + INTERVAL '21 days'),
    ('70000000-0000-0000-0000-000000000004', NOW(), NOW() + INTERVAL '7 days'),
    ('70000000-0000-0000-0000-000000000005', NOW() + INTERVAL '1 day', NOW() + INTERVAL '10 days')
ON CONFLICT DO NOTHING;

-- ── Open shifts ───────────────────────────────────────────────────────────────
INSERT INTO shifts (id, schedule_id, role, region, required_tags, shift_start, shift_end, is_critical, created_by)
VALUES
    ('80000000-0000-0000-0000-000000000001',
     '30000000-0000-0000-0000-000000000001',
     'conductor', 'mountain_west', ARRAY['conductor','first_aid'],
     NOW() + INTERVAL '5 hours', NOW() + INTERVAL '21 hours',
     TRUE, '00000000-0000-0000-0000-000000000004'),

    ('80000000-0000-0000-0000-000000000002',
     '30000000-0000-0000-0000-000000000003',
     'driver', 'mountain_west', ARRAY['driver'],
     NOW() + INTERVAL '3 hours', NOW() + INTERVAL '11 hours',
     FALSE, '00000000-0000-0000-0000-000000000004'),

    ('80000000-0000-0000-0000-000000000003',
     '30000000-0000-0000-0000-000000000005',
     'conductor', 'great_plains', ARRAY['conductor','night_shift'],
     NOW() + INTERVAL '7 hours', NOW() + INTERVAL '23 hours',
     TRUE, '00000000-0000-0000-0000-000000000004')
ON CONFLICT DO NOTHING;

-- ── Crawl sources (offline / local packages) ──────────────────────────────────
INSERT INTO crawl_sources (id, name, source_type, base_path, city, keywords, rate_limit_rps) VALUES
    ('90000000-0000-0000-0000-000000000001',
     'Denver Station Local Package', 'local_package',
     '/data/packages/denver', 'Denver',
     ARRAY['fares','schedule','delays'], 2.0),
    ('90000000-0000-0000-0000-000000000002',
     'Chicago Internal Mirror', 'internal_mirror',
     '/data/mirrors/chicago', 'Chicago',
     ARRAY['baggage','accessibility'], 2.0)
ON CONFLICT DO NOTHING;
