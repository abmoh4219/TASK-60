# RailOps API Specification

Base URL: `https://localhost:8443`

All **authenticated** endpoints require three headers:

```
Authorization: Bearer <session_token>
X-RailOps-Ts:  <unix_seconds>
X-RailOps-Sig: HMAC-SHA256(token, "METHOD\nPATH_WITH_QUERY\nTIMESTAMP")
```

`PATH_WITH_QUERY` includes the query string when present, e.g.
`/api/v1/ops/orders?page=1&per_page=20`.  
Timestamps outside ±120 s of server time are rejected with `401`.

---

## Auth

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST   | `/api/v1/auth/login` | — | Obtain session token |
| DELETE | `/api/v1/auth/logout` | Bearer | Revoke session |
| GET    | `/api/v1/auth/me`    | Bearer | Current user info |

### POST /api/v1/auth/login

**Body**
```json
{ "username": "admin", "password": "AdminRailOps2024!" }
```

**Response 200**
```json
{ "token": "<session_token>", "role": "admin", "user_id": "<uuid>" }
```

**Rate limiting**: 5 failures per IP per minute → `429 Too Many Requests`.  
**Lockout**: 5 consecutive failures locks the account for 15 minutes.

---

## Public Kiosk (no auth)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/kiosk/content` | Search / list published articles |
| GET | `/api/v1/kiosk/content/{slug}` | Full article + related |
| GET | `/api/v1/kiosk/archive` | Archive index or month articles |
| GET | `/api/v1/kiosk/categories` | Category counts |
| GET | `/api/v1/kiosk/tags` | Tag counts |

### GET /api/v1/kiosk/content

| Param | Type | Description |
|-------|------|-------------|
| `q` | string | Full-text / fuzzy search query |
| `category` | string | Filter: `fares` \| `delays` \| `baggage` \| `accessibility` |
| `tag` | string | Filter by tag |
| `departure_from` | ISO-8601 datetime | Only articles linked to routes with departures ≥ this time |
| `departure_to` | ISO-8601 datetime | Only articles linked to routes with departures ≤ this time |
| `page` | int | 1-based page (default 1) |
| `per_page` | int | Items per page (default 20, max 50) |

**Response 200**
```json
{
  "items": [{ "id": "...", "slug": "...", "title": "...", "category": "...",
              "route_id": null, "is_published": true,
              "quality_score": "92.50", "publish_date": "2026-01-15",
              "updated_at": "2026-01-15T10:00:00Z" }],
  "total": 42, "page": 1, "per_page": 20, "total_pages": 3,
  "search_type": "fts"
}
```

### GET /api/v1/kiosk/content/{slug}

**Response 200**
```json
{
  "article": { "id": "...", "slug": "...", "title": "...", "body": "...",
               "category": "...", "tags": ["rail", "discount"],
               "source_url": "https://...", "quality_score": "88.00",
               "publish_date": "2026-01-10", "is_published": true,
               "created_at": "...", "updated_at": "..." },
  "related": [{ /* ContentSummary */ }]
}
```

### GET /api/v1/kiosk/archive

| Param | Description |
|-------|-------------|
| `year` | Filter to a specific year |
| `month` | With `year`: return paginated articles for that month |
| `category` | Optional category filter |
| `page` | Page number (when returning articles) |

---

## Ops — Schedules

Roles: Admin, OpsAgent, Dispatcher (read); Admin, OpsAgent (write).

| Method | Path | Description |
|--------|------|-------------|
| GET   | `/api/v1/ops/routes` | List routes |
| GET   | `/api/v1/ops/seat-classes` | List seat classes |
| GET   | `/api/v1/ops/schedules` | List schedules (filtered) |
| GET   | `/api/v1/ops/schedules/{id}` | Schedule detail + inventory |
| PATCH | `/api/v1/ops/schedules/{id}/status` | Update schedule status |
| POST  | `/api/v1/ops/schedules/{id}/inventory` | Correct inventory |

---

## Ops — Orders & Passengers

Roles: Admin, OpsAgent, Dispatcher, CsAgent (read); Admin, OpsAgent, CsAgent (write — CsAgent can manage orders, process refunds, and apply fee overrides with mandatory reason).

| Method | Path | Description |
|--------|------|-------------|
| GET    | `/api/v1/ops/passengers` | Search passengers (pg_trgm) |
| POST   | `/api/v1/ops/passengers` | Create passenger |
| POST   | `/api/v1/ops/passengers/{id}/pii-purge` | Request PII purge (Admin only) |
| GET    | `/api/v1/ops/orders` | List orders (filtered) |
| GET    | `/api/v1/ops/orders/by-number/{number}` | Find by order number |
| GET    | `/api/v1/ops/orders/{id}` | Order detail + events |
| POST   | `/api/v1/ops/orders` | Create order |
| POST   | `/api/v1/ops/orders/{id}/confirm` | Confirm order |
| POST   | `/api/v1/ops/orders/{id}/cancel` | Cancel order |
| POST   | `/api/v1/ops/orders/{id}/hold` | Place order on hold |
| POST   | `/api/v1/ops/orders/{id}/refund` | Process refund |
| POST   | `/api/v1/ops/orders/{id}/fee-override` | Apply fee override |
| POST   | `/api/v1/ops/orders/{id}/disruption` | Flag service disruption |
| POST   | `/api/v1/ops/orders/{id}/rebook` | Rebook onto new schedule |
| GET    | `/api/v1/ops/orders/{id}/events` | Order event timeline |

`GET /api/v1/ops/orders` query params:

| Param | Type | Description |
|-------|------|-------------|
| `passenger_id` | UUID | Filter by exact passenger ID |
| `schedule_id` | UUID | Filter by schedule |
| `status` | string | Filter by status |
| `passenger_name` | string | Free-text passenger name search (ILIKE) |
| `passenger_phone` | string | Partial phone last-4 search |
| `page` / `per_page` | int | Pagination |

---

## Ops — Business Rules

Roles: Admin only.

| Method | Path | Description |
|--------|------|-------------|
| GET   | `/api/v1/rules` | List all business rules |
| GET   | `/api/v1/rules/{key}` | Get single rule |
| PATCH | `/api/v1/rules/{key}` | Update rule value |

Rule keys and defaults (field name is `rule_key`):

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `refund_full_hours` | float | 24 | Hours before departure for full-minus-fee refund |
| `refund_partial_hours` | float | 2 | Hours before departure for partial (50%) refund |
| `refund_processing_fee_usd` | float | 5.00 | Flat processing fee for full-minus-fee tier |
| `order_hold_ttl_minutes` | integer | 15 | Minutes before a held order expires |
| `quality_publish_threshold` | float | 85 | Minimum quality score for publication |
| `similarity_quarantine` | float | 0.92 | pg_trgm similarity threshold for quarantine |
| `max_failed_logins` | integer | 5 | Failed logins before account lockout |
| `lockout_minutes` | integer | 15 | Account lockout duration in minutes |
| `rate_limit_rpm` | integer | 60 | Requests per minute per session token |
| `session_idle_minutes` | integer | 30 | Idle timeout before session expires |

---

## Staffing

Roles: Admin, Dispatcher (write); Admin, Dispatcher, OpsAgent (read).

| Method | Path | Description |
|--------|------|-------------|
| GET    | `/api/v1/staffing/contractors` | List contractors |
| POST   | `/api/v1/staffing/contractors` | Create contractor |
| GET    | `/api/v1/staffing/contractors/{id}` | Contractor detail |
| PATCH  | `/api/v1/staffing/contractors/{id}/active` | Toggle active flag |
| POST   | `/api/v1/staffing/contractors/{id}/availability` | Add availability window |
| GET    | `/api/v1/staffing/shifts` | List shifts |
| POST   | `/api/v1/staffing/shifts` | Create shift |
| GET    | `/api/v1/staffing/shifts/{id}` | Shift detail |
| PATCH  | `/api/v1/staffing/shifts/{id}/status` | Update shift status |
| GET    | `/api/v1/staffing/shifts/{id}/candidates` | Run match engine |
| POST   | `/api/v1/staffing/shifts/{id}/propose` | Propose assignment |
| PATCH  | `/api/v1/staffing/assignments/{id}/respond` | Accept / reject assignment |
| GET    | `/api/v1/staffing/subscriptions` | List subscriptions |
| POST   | `/api/v1/staffing/subscriptions` | Subscribe to shift |
| DELETE | `/api/v1/staffing/subscriptions` | Unsubscribe (body: `{ "shift_id": "<uuid>" }`) |

### Match scoring

`GET /api/v1/staffing/shifts/{id}/candidates` returns candidates ranked by:

- **Quality** (0–60 pts): `contractor.rating / 5 × 60`
- **Region match** (+25 pts): contractor tags include the shift's required region tag
- **Tag breadth** (up to +15 pts): 5 pts per extra matching tag (max 3)

---

## Credentials

Roles: Admin, Dispatcher, OpsAgent (view); Admin, Dispatcher (approve/e-sign).

| Method | Path | Description |
|--------|------|-------------|
| GET    | `/api/v1/credentials` | List credentials |
| POST   | `/api/v1/credentials` | Upload credential (multipart) |
| POST   | `/api/v1/credentials/expire` | Run expiry sweep |
| GET    | `/api/v1/credentials/{id}` | Credential detail (logs "viewed") |
| GET    | `/api/v1/credentials/{id}/download` | Download decrypted file (with watermark) |
| PATCH  | `/api/v1/credentials/{id}/review` | Approve / reject |
| GET    | `/api/v1/credentials/{id}/audit` | Credential audit log |
| POST   | `/api/v1/credentials/{id}/esign` | Attach e-signature to credential |

Upload accepts `multipart/form-data` with fields:
- `contractor_id` (UUID)
- `document_type` (string)
- `expires_at` (YYYY-MM-DD, optional)
- `file` (binary; PDF/JPEG/PNG validated by MIME type AND magic bytes; max 10 MB)

The download endpoint:
- Decrypts AES-256-GCM stored ciphertext
- Appends `%%RailOps-Watermark: <viewer> at <timestamp>` to PDF bodies
- Sets `X-Watermark` response header for all file types
- Records a `"downloaded"` entry in the credential audit log

## E-Signatures

Roles: Admin, Dispatcher (write); Admin, Dispatcher, OpsAgent (read).

| Method | Path | Description |
|--------|------|-------------|
| POST   | `/api/v1/esignatures` | Create e-signature for any entity |
| GET    | `/api/v1/esignatures/{entity_type}/{entity_id}` | List e-signatures for an entity |

`entity_type` must be one of: `credential`, `order`, `assignment`.

**POST body:**
```json
{
  "entity_type": "credential",
  "entity_id":   "<uuid>",
  "signer_name": "Jane Smith",
  "signed_date": "2026-01-15",
  "signature_data": "<optional SVG string>"
}
```

---

## Crawl Engine

Roles: Admin, OpsAgent.

| Method | Path | Description |
|--------|------|-------------|
| GET    | `/api/v1/crawl/sources` | List active crawl sources |
| POST   | `/api/v1/crawl/sources` | Create source |
| GET    | `/api/v1/crawl/sources/{id}` | Source detail |
| GET    | `/api/v1/crawl/sources/{id}/tasks` | List tasks for a source |
| POST   | `/api/v1/crawl/sources/{id}/tasks` | Create task for a source |
| POST   | `/api/v1/crawl/tasks/{id}/run` | Trigger immediate run |
| GET    | `/api/v1/crawl/tasks/{id}/runs` | List runs for a task |
| GET    | `/api/v1/crawl/runs/{id}` | Run detail |
| GET    | `/api/v1/crawl/runs/{id}/quality` | Quality logs for a run |
| GET    | `/api/v1/crawl/quality/quarantined` | All quarantined items |

**Task pagination_rules** (optional JSON object):
```json
{ "max_pages": 10, "max_items": 500 }
```
These controls are enforced during execution: `max_pages` stops the page loop after N pages; `max_items` stops after N items are ingested. Source `city` and `keywords` are applied as content filters before ingest.

**Source types:**
- `local_package` — reads JSON files from `base_path` (content articles)
- `internal_mirror` — reads JSON files from `base_path/mirror/` (content articles)
- `schedule_feed` — reads JSON files from `base_path` containing schedule/inventory data; dispatches to the schedule aggregation pipeline which upserts into `schedules` and `inventory_snapshots` tables with validation and audit logging

---

## Error Format

All errors return JSON:

```json
{ "error": { "code": "NOT_FOUND", "message": "Resource not found" } }
```

Common status codes:

| Status | Meaning |
|--------|---------|
| 400 | Validation error |
| 401 | Missing / invalid / expired auth |
| 403 | Insufficient role |
| 404 | Resource not found |
| 409 | Conflict (duplicate) |
| 429 | Rate limited |
| 500 | Internal server error |
