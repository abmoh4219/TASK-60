# RailOps — Architecture & Security Design

## Overview

RailOps is a multi-tenant railway operations platform. It consists of:

- **Backend** — Actix-web 4 (Rust), async, stateless
- **Frontend** — Yew 0.21 (Rust → WASM), HashRouter, single-page app
- **Database** — PostgreSQL 16 with pg_trgm, tsvector full-text search
- **Transport** — TLS 1.3 (self-signed cert generated at first boot via rcgen)

All components are packaged as a single Docker Compose stack.

---

## Repository Layout

```
repo/
├── Cargo.toml              Workspace (members: backend, frontend, shared)
├── Dockerfile              5-stage multi-stage build
├── docker-compose.yml      db + app services
├── migrations/             Flyway-compatible SQL migrations (numbered prefix)
│   ├── 1__initial_schema.sql
│   ├── 2__extended_seed.sql
│   ├── 3__business_rules.sql
│   └── 4__contractor_user_mapping.sql
├── backend/src/
│   ├── main.rs             Server startup, route registration, background tasks
│   ├── config.rs           AppConfig::from_env()
│   ├── error.rs            AppError / AppResult (thiserror + ResponseError)
│   ├── crypto.rs           Argon2id, AES-256-GCM, HMAC-SHA-256
│   ├── auth/               Session middleware, RBAC extractors
│   ├── domain/             Bounded-context repositories (rail, orders, content, …)
│   ├── ops/                Ops console handlers (schedules, orders)
│   ├── kiosk/              Public API handlers (no auth)
│   ├── staffing/           Staffing handlers + match engine
│   ├── credentials/        Credential upload, review, e-sign, download
│   ├── crawl/              Crawl engine, pipeline, quality scorer
│   └── rules/              Business rules engine + CRUD handlers
├── frontend/src/
│   ├── api/                Typed API client modules (ops, kiosk, credentials, …)
│   ├── auth.rs             AuthContext (Yew context)
│   ├── app.rs              Route enum, top-level component, HashRouter
│   └── pages/              Page components (kiosk/*, ops/*)
└── shared/src/lib.rs       Shared types (UserRole, QualityScore, rules constants)
```

---

## Request Authentication

All non-public endpoints use a three-header scheme:

```
Authorization: Bearer <session_token>
X-RailOps-Ts:  <unix_seconds>
X-RailOps-Sig: HMAC-SHA256(session_token, "METHOD\nPATH_WITH_QUERY\nTIMESTAMP")
```

### Signature construction

```
message = METHOD + "\n" + PATH_WITH_QUERY + "\n" + TIMESTAMP
sig     = hex(HMAC-SHA256(key=session_token, data=message))
```

`PATH_WITH_QUERY` includes the query string when present:
- Plain path: `/api/v1/ops/schedules`
- With query: `/api/v1/ops/orders?page=2&per_page=20`

The backend rejects requests where:
- `|server_time - X-RailOps-Ts| > 300 seconds` (replay protection)
- HMAC signature does not match (tamper detection)

Session tokens are 32 random bytes (base64-encoded), stored in `sessions` table, scoped to one user and one IP address (recorded at login).

---

## Role-Based Access Control (RBAC)

Roles are defined in `shared::UserRole`:

| Role | Capabilities |
|------|-------------|
| `admin` | Full access to all endpoints |
| `ops_agent` | View/edit schedules and orders; upload/view credentials |
| `dispatcher` | View/edit staffing; approve credentials; manage subscriptions |
| `cs_agent` | Read-only order/passenger search |

RBAC is enforced in backend extractors (`RequireRole`, `RequireAdmin`, etc.) — the frontend mirrors these checks for UX only and is not the security boundary.

---

## Security Controls

### Authentication
- **Password hashing**: Argon2id (m=19456, t=2, p=1) via `argon2` 0.5
- **Account lockout**: 5 consecutive failures locks account for 15 minutes
- **Rate limiting**: In-memory DashMap; 5 failed logins per IP per 60 s → `429`; TTL eviction runs every 120 s in a background tokio task

### Transport
- TLS 1.3; certificate generated at startup by `rcgen` if absent
- All cookies (if used) would be `Secure; SameSite=Strict`

### Data at Rest
- **PII**: passenger names, emails, phone numbers are AES-256-GCM encrypted; decrypted only when returned via authenticated API
- **Credential files**: stored as AES-256-GCM ciphertext under `$UPLOAD_DIR/contractors/{contractor_id}/{fp8}_{filename}`
- **Key**: `AES_KEY` environment variable (32-byte hex); rotatable without DB migration (files must be re-encrypted)

### File Upload Validation
Credentials are validated at two layers:
1. **MIME type**: only `application/pdf`, `image/jpeg`, `image/png` accepted
2. **Magic bytes**: file content inspected to confirm the declared MIME type:
   - PDF: starts with `%PDF`
   - JPEG: starts with `FF D8 FF`
   - PNG: starts with `89 50 4E 47 0D 0A 1A 0A`

Duplicate detection uses SHA-256 fingerprint with a `UNIQUE` constraint on `credentials.fingerprint`.

### Credential Download & Watermarking
The `GET /api/v1/credentials/{id}/download` endpoint:
- Decrypts ciphertext in-memory (never written to disk decrypted)
- Appends `%%RailOps-Watermark: <username> at <timestamp>` to PDF byte streams
- Sets `X-Watermark: <username>/<timestamp>` response header for all types
- Inserts an immutable `"downloaded"` audit log entry

### Audit Trail
- `audit_logs` table: insert-only (PostgreSQL `RULE` blocks UPDATE/DELETE)
- `credential_audit_log` table: insert-only (same mechanism)
- Every mutating API call appends an audit entry with actor, action, target, and a JSON `data` payload

### SQL Injection Prevention
All queries use parameterized `sqlx::query_as()` / `sqlx::query()` calls. No string interpolation in SQL. Full-text search uses `plainto_tsquery` (safe against injection).

---

## Content Quality Pipeline

The crawl engine scores each ingested article on three axes:

| Axis | Weight | Signals |
|------|--------|---------|
| Completeness | 50% | 6 required fields: title, body, category, source_url, publish_date, route_id |
| Accuracy | 30% | Body length ≥ 200 chars, title ≥ 10 chars, source URL scheme valid; minus anomaly penalty |
| Timeliness | 20% | Article age: <7d = full score, <30d = 75%, <90d = 50%, older = 25% |

**Anomaly checks** (applied during accuracy scoring):

| Pattern | Penalty | Issue code |
|---------|---------|------------|
| `"lorem ipsum"` in body | −80 | `placeholder_content` |
| `"[placeholder]"` / `"[tbd]"` | −60 | `incomplete_placeholder_content` |
| `"test article"` in title | −50 | `test_content_detected` |
| Negative fare language in fares content | −40 | `negative_fare_detected` |
| Delay article older than 30 days | −25 | `stale_disruption_notice` |
| Fare article with no source URL | −15 | `fare_article_missing_source` |

Articles scoring below `crawl.quality_threshold` (default 60) are quarantined and not published.  
Articles with pg_trgm similarity ≥ `crawl.quarantine_similarity` (default 0.92) against existing content are quarantined as near-duplicates.

---

## Staffing Match Engine

`MatchEngine::rank_candidates(shift)` scores each active contractor:

```
score = quality_score + region_bonus + tag_breadth_bonus
```

- **quality_score** (0–60): `rating / 5.0 × 60.0`
- **region_bonus** (+25): contractor tags include the shift's required region tag
- **tag_breadth_bonus** (0–15): +5 per additional matching tag, max 3 extras

Each scored candidate carries three human-readable `match_reasons` strings stored as JSONB.

---

## Business Rules Engine

All policy thresholds are stored in the `business_rules` table and loaded per-request. The `rules::engine::RulesEngine` struct is stateless; it fetches values from the DB with fallback to `shared::rules` constants:

```
refund.full_minus_fee_hours  (default: 24)
refund.partial_hours         (default:  2)
refund.service_fee_pct       (default: 0.10)
hold.ttl_minutes             (default: 30)
crawl.quality_threshold      (default: 60.0)
crawl.quarantine_similarity  (default: 0.92)
```

`evaluate_refund()` returns a `RefundDecision { outcome, max_amount, reason }`:
- `FullMinusFee`: departure > 24 h away
- `HalfFare`: departure 2–24 h away
- `ServiceDisruption`: disruption flag set → full refund
- `RefundBlocked`: departure < 2 h away

---

## Database

PostgreSQL 16 with extensions: `uuid-ossp`, `pgcrypto`, `pg_trgm`.

Key constraints:
- `audit_logs` and `credential_audit_log` are insert-only (PostgreSQL RULEs)
- `credentials.fingerprint` is UNIQUE (duplicate detection)
- `contractors.user_id` UNIQUE WHERE NOT NULL (one user ↔ one contractor profile)
- `order_number_seq` sequence for human-readable order numbers

Migrations are embedded at compile time via `sqlx::migrate!("../migrations")` and applied automatically on startup.

---

## Background Tasks

Two tokio background tasks run after server startup:

1. **Rate-limiter eviction** — sweeps expired IP entries from the in-memory `DashMap` every 120 s
2. **Crawl engine** — wakes every 60 s or on trigger signal; processes one due source per tick

---

## Build

The Dockerfile uses a 5-stage multi-stage build:

1. `chef` — installs cargo-chef for dependency caching
2. `planner` — computes the dependency recipe
3. `backend-builder` — compiles the Rust backend (release)
4. `frontend-builder` — installs trunk + wasm-pack; builds WASM frontend
5. `runtime` — debian-slim; copies binaries + static assets; no build toolchain

```bash
docker compose up --build   # builds and starts the full stack
```
