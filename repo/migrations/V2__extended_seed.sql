-- =============================================================================
-- RailOps  —  V2 extended seed data
-- PostgreSQL 16
--
-- Adds realistic operational data:
--   • order_number sequence
--   • order events for all V1 orders
--   • crawl tasks + runs + quality logs
--   • credentials (uploaded/reviewed) per contractor
--   • shift assignment (proposed)
--   • subscriptions
--   • additional passengers and orders
-- =============================================================================

-- ── Order number sequence (used by OrderRepo::create) ─────────────────────────
CREATE SEQUENCE IF NOT EXISTS order_number_seq START 9;

-- ── Order events ──────────────────────────────────────────────────────────────
INSERT INTO order_events (order_id, event_type, performed_by, reason, data) VALUES
    -- ORD-00001: confirmed ticket (Maria Gonzalez, EW-101 Economy)
    ('50000000-0000-0000-0000-000000000001', 'created',   '00000000-0000-0000-0000-000000000003', NULL, '{"channel":"counter","seat":"14C"}'),
    ('50000000-0000-0000-0000-000000000001', 'confirmed', '00000000-0000-0000-0000-000000000003', NULL, '{"payment_ref":"PAY-20240101-001","method":"card"}'),

    -- ORD-00002: confirmed ticket (James O'Brien, EW-101 Business)
    ('50000000-0000-0000-0000-000000000002', 'created',   '00000000-0000-0000-0000-000000000003', NULL, '{"channel":"phone","seat":"2A"}'),
    ('50000000-0000-0000-0000-000000000002', 'confirmed', '00000000-0000-0000-0000-000000000003', NULL, '{"payment_ref":"PAY-20240101-002","method":"card"}'),

    -- ORD-00003: held (Priya Sharma, MW-201 Economy)
    ('50000000-0000-0000-0000-000000000003', 'created',   '00000000-0000-0000-0000-000000000003', NULL, '{"channel":"kiosk","seat":"22D"}'),
    ('50000000-0000-0000-0000-000000000003', 'held',      '00000000-0000-0000-0000-000000000003', 'Passenger requested 15-minute hold', NULL),

    -- ORD-00004: confirmed (Liang Wei, NP-301 First Class)
    ('50000000-0000-0000-0000-000000000004', 'created',   '00000000-0000-0000-0000-000000000002', NULL, '{"channel":"counter","seat":"1B"}'),
    ('50000000-0000-0000-0000-000000000004', 'confirmed', '00000000-0000-0000-0000-000000000002', NULL, '{"payment_ref":"PAY-20240101-004","method":"card"}'),

    -- ORD-00005: cancelled (Fatima Al-Hassan, CL-501 Family Saver)
    ('50000000-0000-0000-0000-000000000005', 'created',   '00000000-0000-0000-0000-000000000003', NULL, '{"channel":"phone","seat":"8F"}'),
    ('50000000-0000-0000-0000-000000000005', 'cancelled', '00000000-0000-0000-0000-000000000003', 'Passenger travel plan changed', '{"refund_eligible":false}');

-- ── Crawl tasks ───────────────────────────────────────────────────────────────
INSERT INTO crawl_tasks (id, source_id, task_name, pagination_rules, incremental, last_crawled_at, next_crawl_at, status)
VALUES
    ('a0000000-0000-0000-0000-000000000001',
     '90000000-0000-0000-0000-000000000001',
     'fares-schedule-scraper',
     '{"page_param":"p","max_pages":20,"items_per_page":25}',
     TRUE, NOW() - INTERVAL '2 hours', NOW() + INTERVAL '22 hours', 'idle'),

    ('a0000000-0000-0000-0000-000000000002',
     '90000000-0000-0000-0000-000000000001',
     'delays-disruptions-scraper',
     '{"page_param":"page","max_pages":10,"items_per_page":50}',
     TRUE, NOW() - INTERVAL '6 hours', NOW() + INTERVAL '18 hours', 'idle'),

    ('a0000000-0000-0000-0000-000000000003',
     '90000000-0000-0000-0000-000000000002',
     'baggage-accessibility-scraper',
     '{"page_param":"offset","max_pages":5,"items_per_page":30}',
     FALSE, NOW() - INTERVAL '12 hours', NOW() + INTERVAL '12 hours', 'idle')
ON CONFLICT DO NOTHING;

-- ── Crawl runs ────────────────────────────────────────────────────────────────
INSERT INTO crawl_runs (id, task_id, started_at, finished_at, status, pages_fetched, items_ingested, items_quarantined, error_log)
VALUES
    -- Completed run for fares-schedule-scraper
    ('b0000000-0000-0000-0000-000000000001',
     'a0000000-0000-0000-0000-000000000001',
     NOW() - INTERVAL '2 hours 15 minutes',
     NOW() - INTERVAL '2 hours',
     'completed', 18, 38, 2, NULL),

    -- Currently running (in-progress for delays scraper)
    ('b0000000-0000-0000-0000-000000000002',
     'a0000000-0000-0000-0000-000000000002',
     NOW() - INTERVAL '30 minutes',
     NULL,
     'running', 6, 12, 0, NULL)
ON CONFLICT DO NOTHING;

-- ── Quality log entries (for the completed crawl run) ─────────────────────────
INSERT INTO quality_logs (crawl_run_id, content_id, url_fingerprint, completeness_score, accuracy_score, timeliness_score, quality_score, issues, is_quarantined, transformation_steps)
VALUES
    -- Content page 1 — high quality
    ('b0000000-0000-0000-0000-000000000001',
     '60000000-0000-0000-0000-000000000001',
     'f1e2d3c4b5a69788f1e2d3c4b5a69788f1e2d3c4b5a69788f1e2d3c4b5a69788',
     98.0, 97.0, 100.0, 97.9,
     '[]'::jsonb, FALSE,
     '["normalize_whitespace","strip_trailing_newlines","expand_abbreviations"]'::jsonb),

    -- Content page 2 — good quality
    ('b0000000-0000-0000-0000-000000000001',
     '60000000-0000-0000-0000-000000000002',
     'a2b3c4d5e6f78901a2b3c4d5e6f78901a2b3c4d5e6f78901a2b3c4d5e6f78901',
     95.0, 93.0, 98.0, 94.9,
     '[]'::jsonb, FALSE,
     '["normalize_whitespace","strip_trailing_newlines"]'::jsonb),

    -- Quarantined item — similarity too high (near-duplicate of content page 1)
    ('b0000000-0000-0000-0000-000000000001',
     NULL,
     'd4e5f6a7b8c90123d4e5f6a7b8c90123d4e5f6a7b8c90123d4e5f6a7b8c90123',
     NULL, NULL, NULL, NULL,
     '{"reason":"near_duplicate","similarity":0.96,"matched_slug":"fares-economy-guide"}'::jsonb,
     TRUE, '["quarantined"]'::jsonb),

    -- Quarantined item — content too short / low completeness
    ('b0000000-0000-0000-0000-000000000001',
     NULL,
     'e5f6a7b8c9012345e5f6a7b8c9012345e5f6a7b8c9012345e5f6a7b8c9012345',
     22.0, 40.0, 70.0, 34.0,
     '{"reason":"low_completeness","missing_fields":["title","body_length<50"]}'::jsonb,
     TRUE, '["quarantined","logged_for_review"]'::jsonb);

-- ── Credentials ───────────────────────────────────────────────────────────────
-- Derek Paulson — conductor_license (approved) + medical_cert (approved)
INSERT INTO credentials (id, contractor_id, document_type, file_name, file_path, file_size_bytes, mime_type, fingerprint, expires_at, status, uploaded_by, reviewed_by, reviewed_at, review_notes)
VALUES
    ('c0000000-0000-0000-0000-000000000001',
     '70000000-0000-0000-0000-000000000001',
     'conductor_license',
     'conductor_license_derek_paulson.pdf',
     'contractors/70000000-0000-0000-0000-000000000001/conductor_license.pdf',
     524288, 'application/pdf',
     '1a2b3c4d1a2b3c4d1a2b3c4d1a2b3c4d1a2b3c4d1a2b3c4d1a2b3c4d1a2b3c4d',
     '2026-12-31', 'approved',
     '00000000-0000-0000-0000-000000000002',
     '00000000-0000-0000-0000-000000000002',
     NOW() - INTERVAL '5 days',
     'License valid through 2026-12-31. Class A conductor. Background clear.'),

    ('c0000000-0000-0000-0000-000000000002',
     '70000000-0000-0000-0000-000000000001',
     'medical_certificate',
     'medical_cert_derek_paulson.pdf',
     'contractors/70000000-0000-0000-0000-000000000001/medical_cert.pdf',
     204800, 'application/pdf',
     '2b3c4d5e2b3c4d5e2b3c4d5e2b3c4d5e2b3c4d5e2b3c4d5e2b3c4d5e2b3c4d5e',
     '2025-06-30', 'approved',
     '00000000-0000-0000-0000-000000000002',
     '00000000-0000-0000-0000-000000000002',
     NOW() - INTERVAL '5 days',
     'DOT physical current. Expires 2025-06-30.'),

    -- Sandra Kim — conductor_license (approved)
    ('c0000000-0000-0000-0000-000000000003',
     '70000000-0000-0000-0000-000000000002',
     'conductor_license',
     'conductor_license_sandra_kim.pdf',
     'contractors/70000000-0000-0000-0000-000000000002/conductor_license.pdf',
     483328, 'application/pdf',
     '3c4d5e6f3c4d5e6f3c4d5e6f3c4d5e6f3c4d5e6f3c4d5e6f3c4d5e6f3c4d5e6f',
     '2027-03-15', 'approved',
     '00000000-0000-0000-0000-000000000002',
     '00000000-0000-0000-0000-000000000002',
     NOW() - INTERVAL '3 days',
     'Active Class A license. First Aid certified. Approved for critical shifts.'),

    -- Marcus Rivera — driver_license (approved)
    ('c0000000-0000-0000-0000-000000000004',
     '70000000-0000-0000-0000-000000000003',
     'driver_license',
     'driver_license_marcus_rivera.pdf',
     'contractors/70000000-0000-0000-0000-000000000003/driver_license.pdf',
     344064, 'application/pdf',
     '4d5e6f7a4d5e6f7a4d5e6f7a4d5e6f7a4d5e6f7a4d5e6f7a4d5e6f7a4d5e6f7a',
     '2028-01-01', 'approved',
     '00000000-0000-0000-0000-000000000002',
     '00000000-0000-0000-0000-000000000002',
     NOW() - INTERVAL '7 days',
     'CDL Class A. Night-shift endorsed. Approved.'),

    -- Yuki Tanaka — conductor_license (pending review)
    ('c0000000-0000-0000-0000-000000000005',
     '70000000-0000-0000-0000-000000000004',
     'conductor_license',
     'conductor_license_yuki_tanaka.pdf',
     'contractors/70000000-0000-0000-0000-000000000004/conductor_license.pdf',
     401408, 'application/pdf',
     '5e6f7a8b5e6f7a8b5e6f7a8b5e6f7a8b5e6f7a8b5e6f7a8b5e6f7a8b5e6f7a8b',
     '2026-08-20', 'pending',
     '00000000-0000-0000-0000-000000000002',
     NULL, NULL, NULL),

    -- Ahmed Osman — driver_license (approved)
    ('c0000000-0000-0000-0000-000000000006',
     '70000000-0000-0000-0000-000000000005',
     'driver_license',
     'driver_license_ahmed_osman.pdf',
     'contractors/70000000-0000-0000-0000-000000000005/driver_license.pdf',
     360448, 'application/pdf',
     '6f7a8b9c6f7a8b9c6f7a8b9c6f7a8b9c6f7a8b9c6f7a8b9c6f7a8b9c6f7a8b9c',
     '2027-06-30', 'approved',
     '00000000-0000-0000-0000-000000000002',
     '00000000-0000-0000-0000-000000000002',
     NOW() - INTERVAL '10 days',
     'CDL Class A. First Aid certified. License clear.')
ON CONFLICT (fingerprint) DO NOTHING;

-- ── Credential audit log ──────────────────────────────────────────────────────
INSERT INTO credential_audit_log (credential_id, action, performed_by, data)
VALUES
    ('c0000000-0000-0000-0000-000000000001', 'uploaded',  '00000000-0000-0000-0000-000000000002', '{"via":"web_upload"}'),
    ('c0000000-0000-0000-0000-000000000001', 'approved',  '00000000-0000-0000-0000-000000000002', '{"review_duration_secs":120}'),
    ('c0000000-0000-0000-0000-000000000002', 'uploaded',  '00000000-0000-0000-0000-000000000002', '{"via":"web_upload"}'),
    ('c0000000-0000-0000-0000-000000000002', 'approved',  '00000000-0000-0000-0000-000000000002', '{"review_duration_secs":90}'),
    ('c0000000-0000-0000-0000-000000000003', 'uploaded',  '00000000-0000-0000-0000-000000000002', '{"via":"web_upload"}'),
    ('c0000000-0000-0000-0000-000000000003', 'approved',  '00000000-0000-0000-0000-000000000002', '{"review_duration_secs":75}'),
    ('c0000000-0000-0000-0000-000000000004', 'uploaded',  '00000000-0000-0000-0000-000000000002', '{"via":"web_upload"}'),
    ('c0000000-0000-0000-0000-000000000004', 'approved',  '00000000-0000-0000-0000-000000000002', '{"review_duration_secs":60}'),
    ('c0000000-0000-0000-0000-000000000005', 'uploaded',  '00000000-0000-0000-0000-000000000002', '{"via":"web_upload"}'),
    ('c0000000-0000-0000-0000-000000000006', 'uploaded',  '00000000-0000-0000-0000-000000000002', '{"via":"web_upload"}'),
    ('c0000000-0000-0000-0000-000000000006', 'approved',  '00000000-0000-0000-0000-000000000002', '{"review_duration_secs":110}');

-- ── Shift assignment (Sandra Kim → critical conductor shift) ──────────────────
INSERT INTO shift_assignments (id, shift_id, contractor_id, match_score, match_reasons, assigned_by, status)
VALUES
    ('d0000000-0000-0000-0000-000000000001',
     '80000000-0000-0000-0000-000000000001',
     '70000000-0000-0000-0000-000000000002',
     95.5,
     '["Skills match: conductor, first_aid (required by shift)","Region match: mountain_west","Quality rating: 4.95 — highest available in region"]'::jsonb,
     '00000000-0000-0000-0000-000000000004',
     'proposed')
ON CONFLICT (shift_id, contractor_id) DO NOTHING;

-- ── Subscriptions ─────────────────────────────────────────────────────────────
INSERT INTO subscriptions (id, subscriber_type, subscriber_id, target_type, target_id)
VALUES
    ('e0000000-0000-0000-0000-000000000001',
     'dispatcher', '00000000-0000-0000-0000-000000000004',
     'shift',  '80000000-0000-0000-0000-000000000001'),
    ('e0000000-0000-0000-0000-000000000002',
     'dispatcher', '00000000-0000-0000-0000-000000000004',
     'route', '10000000-0000-0000-0000-000000000001')
ON CONFLICT (subscriber_type, subscriber_id, target_type, target_id) DO NOTHING;

-- ── Additional passengers ─────────────────────────────────────────────────────
INSERT INTO passengers (id, full_name, phone_last4)
VALUES
    ('40000000-0000-0000-0000-000000000006', 'Thomas Anderson', '1234'),
    ('40000000-0000-0000-0000-000000000007', 'Mei Lin',         '5678'),
    ('40000000-0000-0000-0000-000000000008', 'Carlos Mendez',   '4499'),
    ('40000000-0000-0000-0000-000000000009', 'Aisha Johnson',   '8812')
ON CONFLICT DO NOTHING;

-- ── Additional orders ─────────────────────────────────────────────────────────
INSERT INTO orders (id, order_number, passenger_id, schedule_id, seat_class_id, seat_number, status, hold_expires_at, fare_amount, created_by)
VALUES
    -- Thomas Anderson: EW-102 Business (+30 h departure)
    ('50000000-0000-0000-0000-000000000006', 'ORD-00006',
     '40000000-0000-0000-0000-000000000006',
     '30000000-0000-0000-0000-000000000002',
     '20000000-0000-0000-0000-000000000002', '1A', 'confirmed', NULL, 119.00,
     '00000000-0000-0000-0000-000000000003'),

    -- Mei Lin: MW-202 Economy (delayed)
    ('50000000-0000-0000-0000-000000000007', 'ORD-00007',
     '40000000-0000-0000-0000-000000000007',
     '30000000-0000-0000-0000-000000000004',
     '20000000-0000-0000-0000-000000000001', '15C', 'confirmed', NULL, 49.00,
     '00000000-0000-0000-0000-000000000003'),

    -- Maria Gonzalez: CL-501 Sleeper (second booking)
    ('50000000-0000-0000-0000-000000000008', 'ORD-00008',
     '40000000-0000-0000-0000-000000000001',
     '30000000-0000-0000-0000-000000000007',
     '20000000-0000-0000-0000-000000000004', '5B', 'confirmed', NULL, 199.00,
     '00000000-0000-0000-0000-000000000002'),

    -- Carlos Mendez: SP-401 Economy
    ('50000000-0000-0000-0000-000000000009', 'ORD-00009',
     '40000000-0000-0000-0000-000000000008',
     '30000000-0000-0000-0000-000000000006',
     '20000000-0000-0000-0000-000000000001', '9D', 'pending', NULL, 49.00,
     '00000000-0000-0000-0000-000000000003'),

    -- Aisha Johnson: NP-301 Business — disruption flag set (demonstrate disruption flow)
    ('50000000-0000-0000-0000-00000000000a', 'ORD-00010',
     '40000000-0000-0000-0000-000000000009',
     '30000000-0000-0000-0000-000000000005',
     '20000000-0000-0000-0000-000000000002', '3F', 'confirmed', NULL, 119.00,
     '00000000-0000-0000-0000-000000000002')
ON CONFLICT DO NOTHING;

-- ── Order events for additional orders ────────────────────────────────────────
INSERT INTO order_events (order_id, event_type, performed_by, reason, data)
VALUES
    ('50000000-0000-0000-0000-000000000006', 'created',   '00000000-0000-0000-0000-000000000003', NULL, '{"channel":"online","seat":"1A"}'),
    ('50000000-0000-0000-0000-000000000006', 'confirmed', '00000000-0000-0000-0000-000000000003', NULL, '{"payment_ref":"PAY-20240102-006","method":"card"}'),
    ('50000000-0000-0000-0000-000000000007', 'created',   '00000000-0000-0000-0000-000000000003', NULL, '{"channel":"counter","seat":"15C"}'),
    ('50000000-0000-0000-0000-000000000007', 'confirmed', '00000000-0000-0000-0000-000000000003', NULL, '{"payment_ref":"PAY-20240102-007","method":"cash"}'),
    ('50000000-0000-0000-0000-000000000008', 'created',   '00000000-0000-0000-0000-000000000002', NULL, '{"channel":"counter","seat":"5B"}'),
    ('50000000-0000-0000-0000-000000000008', 'confirmed', '00000000-0000-0000-0000-000000000002', NULL, '{"payment_ref":"PAY-20240102-008","method":"card"}'),
    ('50000000-0000-0000-0000-000000000009', 'created',   '00000000-0000-0000-0000-000000000003', NULL, '{"channel":"kiosk","seat":"9D"}'),
    ('50000000-0000-0000-00000-00000000000a', 'created',  '00000000-0000-0000-0000-000000000002', NULL, '{"channel":"counter","seat":"3F"}'),
    ('50000000-0000-0000-00000-00000000000a', 'confirmed','00000000-0000-0000-0000-000000000002', NULL, '{"payment_ref":"PAY-20240102-010","method":"card"}');

-- ── Additional availability windows ───────────────────────────────────────────
-- Extend Derek's availability window
INSERT INTO contractor_availability (contractor_id, available_from, available_to, notes)
VALUES
    ('70000000-0000-0000-0000-000000000001',
     NOW() + INTERVAL '30 days', NOW() + INTERVAL '60 days',
     'Extended availability — confirmed via phone')
ON CONFLICT DO NOTHING;
