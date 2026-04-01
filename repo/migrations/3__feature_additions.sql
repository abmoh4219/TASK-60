-- =============================================================================
-- RailOps — V3 feature additions
-- PostgreSQL 16
--
-- Adds:
--   • sampled_for_review column on quality_logs (sampling-based review)
--   • city column on content_pages (city filter for kiosk)
--   • rebooked_from / rebooked_to columns on orders (rebooking flow)
--   • Additional content seed data (so kiosk pages are populated)
--   • Additional contractor subscriptions seed data
-- =============================================================================

-- ── Schema additions ─────────────────────────────────────────────────────────

-- Sampling-based review flag for crawl quality logs
ALTER TABLE quality_logs ADD COLUMN IF NOT EXISTS sampled_for_review BOOLEAN NOT NULL DEFAULT FALSE;

-- City column for kiosk city filter (derived from route origin/destination)
ALTER TABLE content_pages ADD COLUMN IF NOT EXISTS city VARCHAR(200);

-- Rebooking linkage on orders
ALTER TABLE orders ADD COLUMN IF NOT EXISTS rebooked_from UUID REFERENCES orders(id);
ALTER TABLE orders ADD COLUMN IF NOT EXISTS rebooked_to UUID REFERENCES orders(id);

-- Update order status CHECK to include 'rebooked'
ALTER TABLE orders DROP CONSTRAINT IF EXISTS orders_status_check;
ALTER TABLE orders ADD CONSTRAINT orders_status_check
    CHECK (status IN ('pending','confirmed','held','cancelled','refunded','completed','rebooked'));

-- ── Additional content pages seed data ───────────────────────────────────────
-- These ensure the kiosk is not empty on first boot.

INSERT INTO content_pages (id, slug, title, body, category, route_id, publish_date, is_published, quality_score, source_url, city)
VALUES
    ('60000000-0000-0000-0000-000000000006',
     'paddington-platform-upgrades',
     'London Paddington Platform Upgrades Complete',
     'London Paddington station has completed major platform upgrades across platforms 1-5. The refurbishment includes new LED signage, improved accessibility ramps, tactile paving, and expanded waiting areas. Passengers can expect smoother boarding for East-West Express and Coastal Link services. The project, which began 18 months ago, was delivered on time and under budget. Station management reports a 15% improvement in boarding efficiency during peak hours.',
     'general',
     '10000000-0000-0000-0000-000000000001',
     CURRENT_DATE - INTERVAL '5 days',
     TRUE, 91.00,
     'https://railops.local/news/paddington-upgrades',
     'London'),

    ('60000000-0000-0000-0000-000000000007',
     'cardiff-weekend-fares-summer',
     'Cardiff Central Weekend Fares — Summer 2026 Guide',
     'Summer weekend fares to Cardiff Central are now available with early-bird discounts of up to 30%. Economy tickets start from £29 for advance purchases. Business class offers complimentary WiFi and at-seat dining. Family Saver tickets include up to 2 children travelling free with each adult. Book before June 15 for the best availability on popular East-West Express departures. Off-peak services on Saturday mornings offer the lowest fares.',
     'fares',
     '10000000-0000-0000-0000-000000000001',
     CURRENT_DATE - INTERVAL '3 days',
     TRUE, 93.50,
     'https://railops.local/fares/cardiff-weekend-summer',
     'Cardiff'),

    ('60000000-0000-0000-0000-000000000008',
     'birmingham-delays-signal-work',
     'Birmingham New Street — Planned Signal Maintenance Delays',
     'Passengers travelling through Birmingham New Street should expect delays of 15-30 minutes on Monday and Tuesday due to planned signal maintenance. The Midlands West service (MW-201/202) will operate a revised timetable with fewer stops between Birmingham and Sheffield. Replacement bus services are available for affected local stations. National Rail apologises for the inconvenience and recommends checking live departure boards before travelling.',
     'delays',
     '10000000-0000-0000-0000-000000000002',
     CURRENT_DATE - INTERVAL '1 day',
     TRUE, 89.20,
     'https://railops.local/delays/birmingham-signal-work',
     'Birmingham'),

    ('60000000-0000-0000-0000-000000000009',
     'accessibility-guide-wheelchair-booking',
     'How to Book Wheelchair-Accessible Seats on RailOps Services',
     'RailOps provides dedicated wheelchair spaces on all services. Passengers requiring wheelchair-accessible seating can book through the kiosk, online portal, or by calling our accessibility helpline. Each train has a minimum of 4 designated wheelchair positions with companion seating. Priority boarding is available at all major stations. Assistance can be arranged with 24 hours notice, though walk-up requests are accommodated where possible. All new rolling stock meets the latest accessibility standards.',
     'accessibility',
     NULL,
     CURRENT_DATE - INTERVAL '10 days',
     TRUE, 95.00,
     'https://railops.local/accessibility/wheelchair-booking',
     NULL),

    ('60000000-0000-0000-0000-000000000010',
     'baggage-allowance-update-2026',
     'Updated Baggage Allowance Policy — Effective April 2026',
     'Effective April 1 2026, RailOps updates its baggage policy. Economy passengers may carry 2 items of hand luggage (max 15kg each) plus 1 checked item (max 23kg). Business and First Class passengers receive an additional checked item. Oversized items such as bicycles and sporting equipment require advance booking and a £10 surcharge. Luggage storage is available at London Paddington, Cardiff Central, Birmingham New Street, and Edinburgh Waverley for £5/day.',
     'baggage',
     NULL,
     CURRENT_DATE - INTERVAL '2 days',
     TRUE, 92.80,
     'https://railops.local/baggage/allowance-update-2026',
     NULL),

    ('60000000-0000-0000-0000-000000000011',
     'edinburgh-scenic-route-launch',
     'New Edinburgh Scenic Route Launched for Summer Season',
     'RailOps is pleased to announce the launch of a new scenic route connecting Edinburgh Waverley to the Scottish Highlands. The service operates on weekends from May through September, with departures at 08:15 and 14:30. The route features panoramic viewing carriages and an onboard commentary service. Tickets start at £45 for Economy and £89 for First Class. Early booking is strongly recommended as capacity is limited.',
     'general',
     '10000000-0000-0000-0000-000000000004',
     CURRENT_DATE - INTERVAL '7 days',
     TRUE, 90.50,
     'https://railops.local/news/edinburgh-scenic-launch',
     'Edinburgh'),

    ('60000000-0000-0000-0000-000000000012',
     'sheffield-station-delays-weather',
     'Sheffield Station Weather-Related Service Disruptions',
     'Heavy rainfall has caused flooding on tracks south of Sheffield station, affecting Midlands West services. Trains between Sheffield and Birmingham are currently suspended with replacement bus services in operation. Services are expected to resume by Wednesday evening. Passengers with confirmed bookings on affected services are entitled to a full refund or free rebooking to alternative dates. Check the RailOps disruptions page for live updates.',
     'delays',
     '10000000-0000-0000-0000-000000000002',
     CURRENT_DATE,
     TRUE, 88.00,
     'https://railops.local/delays/sheffield-weather',
     'Sheffield')
ON CONFLICT (slug) DO NOTHING;

-- Tags for new content pages
INSERT INTO content_tags (content_id, tag) VALUES
    ('60000000-0000-0000-0000-000000000006', 'station'),
    ('60000000-0000-0000-0000-000000000006', 'accessibility'),
    ('60000000-0000-0000-0000-000000000006', 'london'),
    ('60000000-0000-0000-0000-000000000007', 'fares'),
    ('60000000-0000-0000-0000-000000000007', 'cardiff'),
    ('60000000-0000-0000-0000-000000000007', 'weekend'),
    ('60000000-0000-0000-0000-000000000008', 'delays'),
    ('60000000-0000-0000-0000-000000000008', 'birmingham'),
    ('60000000-0000-0000-0000-000000000008', 'signal'),
    ('60000000-0000-0000-0000-000000000009', 'accessibility'),
    ('60000000-0000-0000-0000-000000000009', 'wheelchair'),
    ('60000000-0000-0000-0000-000000000009', 'booking'),
    ('60000000-0000-0000-0000-000000000010', 'baggage'),
    ('60000000-0000-0000-0000-000000000010', 'policy'),
    ('60000000-0000-0000-0000-000000000010', 'luggage'),
    ('60000000-0000-0000-0000-000000000011', 'edinburgh'),
    ('60000000-0000-0000-0000-000000000011', 'scenic'),
    ('60000000-0000-0000-0000-000000000011', 'new-service'),
    ('60000000-0000-0000-0000-000000000012', 'delays'),
    ('60000000-0000-0000-0000-000000000012', 'sheffield'),
    ('60000000-0000-0000-0000-000000000012', 'weather'),
    ('60000000-0000-0000-0000-000000000012', 'disruption')
ON CONFLICT DO NOTHING;

-- Update city on existing content pages based on associated routes
UPDATE content_pages cp
SET city = CASE
    WHEN r.origin_city ILIKE '%London%' OR r.destination_city ILIKE '%London%' THEN 'London'
    WHEN r.origin_city ILIKE '%Cardiff%' OR r.destination_city ILIKE '%Cardiff%' THEN 'Cardiff'
    WHEN r.origin_city ILIKE '%Birmingham%' OR r.destination_city ILIKE '%Birmingham%' THEN 'Birmingham'
    WHEN r.origin_city ILIKE '%Edinburgh%' OR r.destination_city ILIKE '%Edinburgh%' THEN 'Edinburgh'
    WHEN r.origin_city ILIKE '%Sheffield%' OR r.destination_city ILIKE '%Sheffield%' THEN 'Sheffield'
    ELSE r.origin_city
END
FROM routes r
WHERE cp.route_id = r.id AND cp.city IS NULL;

-- ── Contractor-side subscription seed data ───────────────────────────────────
INSERT INTO subscriptions (id, subscriber_type, subscriber_id, target_type, target_id)
VALUES
    ('e0000000-0000-0000-0000-000000000003',
     'contractor', '70000000-0000-0000-0000-000000000001',
     'shift', '80000000-0000-0000-0000-000000000001'),
    ('e0000000-0000-0000-0000-000000000004',
     'contractor', '70000000-0000-0000-0000-000000000002',
     'route', '10000000-0000-0000-0000-000000000001')
ON CONFLICT (subscriber_type, subscriber_id, target_type, target_id) DO NOTHING;

-- Create index on city for kiosk filtering
CREATE INDEX IF NOT EXISTS idx_content_city ON content_pages(city);
