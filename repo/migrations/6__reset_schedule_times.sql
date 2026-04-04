-- Reset schedule departure/arrival times to future intervals so integration
-- tests remain in valid refund-policy windows regardless of when the DB
-- was first initialised.

UPDATE schedules SET
    departure_time = NOW() + INTERVAL  '6 hours',
    arrival_time   = NOW() + INTERVAL '20 hours'
WHERE id = '30000000-0000-0000-0000-000000000001';

UPDATE schedules SET
    departure_time = NOW() + INTERVAL '30 hours',
    arrival_time   = NOW() + INTERVAL '44 hours'
WHERE id = '30000000-0000-0000-0000-000000000002';

UPDATE schedules SET
    departure_time = NOW() + INTERVAL  '4 hours',
    arrival_time   = NOW() + INTERVAL '12 hours'
WHERE id = '30000000-0000-0000-0000-000000000003';

UPDATE schedules SET
    departure_time = NOW() + INTERVAL '28 hours',
    arrival_time   = NOW() + INTERVAL '36 hours'
WHERE id = '30000000-0000-0000-0000-000000000004';

UPDATE schedules SET
    departure_time = NOW() + INTERVAL  '8 hours',
    arrival_time   = NOW() + INTERVAL '22 hours'
WHERE id = '30000000-0000-0000-0000-000000000005';

UPDATE schedules SET
    departure_time = NOW() + INTERVAL '10 hours',
    arrival_time   = NOW() + INTERVAL '18 hours'
WHERE id = '30000000-0000-0000-0000-000000000006';

UPDATE schedules SET
    departure_time = NOW() + INTERVAL '2 hours',
    arrival_time   = NOW() + INTERVAL  '5 hours'
WHERE id = '30000000-0000-0000-0000-000000000007';
