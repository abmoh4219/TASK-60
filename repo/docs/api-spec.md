# RailOps API Specification

**Base URL:** `https://localhost:8443`
**Protocol:** HTTPS (TLS 1.2+, self-signed cert in development)
**Content-Type:** `application/json`

---

## Authentication

All endpoints except `/health` and `/api/v1/kiosk/*` require three headers:

| Header              | Value                                                |
|---------------------|------------------------------------------------------|
| `Authorization`     | `Bearer <token>` — raw session token from login      |
| `X-RailOps-Ts`      | Current UNIX timestamp (seconds, `i64`)              |
| `X-RailOps-Sig`     | HMAC-SHA-256(`token`, `"METHOD\nPATH\nTS"`) hex      |

The timestamp must be within ±120 seconds of server time.

**Signing example (pseudo-code):**
```
message = "GET\n/api/v1/ops/routes\n1700000000"
sig     = HMAC-SHA256(key=token, message=message)
header  = hex_encode(sig)
```

---

## Common Response Codes

| Code | Meaning                                          |
|------|--------------------------------------------------|
| 200  | Success                                          |
| 201  | Resource created                                 |
| 204  | Success, no body (logout)                        |
| 400  | Bad request / validation error                   |
| 401  | Missing or invalid credentials / signature       |
| 403  | Authenticated but insufficient role              |
| 404  | Resource not found                               |
| 409  | Conflict (duplicate)                             |
| 429  | Rate limit exceeded (60 req/min per session)     |
| 500  | Internal server error                            |

---

## Health

### `GET /health`

No authentication required.

**Response 200:**
```json
{
  "status":  "ok",
  "service": "railops-backend",
  "version": "0.1.0"
}
```

---

## Auth

### `POST /api/v1/auth/login`

**Request:**
```json
{
  "username": "admin",
  "password": "AdminRailOps2024!"
}
```

**Response 200:**
```json
{
  "token":      "<64-char hex token>",
  "expires_at": "2024-11-15T12:30:00Z",
  "user": {
    "id":        "00000000-0000-0000-0000-000000000001",
    "username":  "admin",
    "role":      "admin",
    "full_name": "System Administrator"
  }
}
```

---

### `DELETE /api/v1/auth/logout`

Invalidates the current session.

**Response 204** — No body.

---

### `GET /api/v1/auth/me`

Returns the current authenticated user.

**Response 200:**
```json
{
  "id":        "00000000-0000-0000-0000-000000000001",
  "username":  "admin",
  "role":      "admin",
  "full_name": "System Administrator"
}
```

---

## Kiosk (Public — No Auth Required)

### `GET /api/v1/kiosk/content`

Full-text search over published content.

**Query params:**

| Param      | Type   | Description                              |
|------------|--------|------------------------------------------|
| `q`        | string | Search query (FTS + pg_trgm fallback)    |
| `category` | string | Filter: `fares`, `delays`, `baggage`, etc.|
| `tag`      | string | Filter by tag                            |
| `page`     | int    | Page number (default 1)                  |
| `per_page` | int    | Items per page (default 20, max 100)     |

**Response 200:**
```json
{
  "items": [
    {
      "id":           "uuid",
      "slug":         "train-delays-update",
      "title":        "Train Delays Update",
      "summary":      "...",
      "category":     "delays",
      "tags":         ["trains", "schedule"],
      "published_at": "2024-01-15",
      "quality_score": "92.50"
    }
  ],
  "total":       42,
  "page":        1,
  "per_page":    20,
  "total_pages": 3,
  "fts":         true
}
```

---

### `GET /api/v1/kiosk/content/{slug}`

Get a full article with related content.

**Response 200:**
```json
{
  "article": {
    "id":           "uuid",
    "slug":         "train-delays-update",
    "title":        "Train Delays Update",
    "body":         "Full article text...",
    "category":     "delays",
    "tags":         ["trains"],
    "published_at": "2024-01-15",
    "quality_score": "92.50",
    "route_id":     "EW-001",
    "source_url":   "https://example.com/article",
    "author":       "Jane Smith"
  },
  "related": [ /* array of ContentSummary */ ]
}
```

---

### `GET /api/v1/kiosk/archive`

Archive index or month articles.

**Query params:**

| Param      | Type   | Description                              |
|------------|--------|------------------------------------------|
| `year`     | int    | Year filter (required for month view)    |
| `month`    | int    | Month 1–12 (required for month view)     |
| `category` | string | Optional category filter                 |
| `page`     | int    | Page (for month view)                    |

Without `year`+`month` → returns index: `{ "entries": [{ "year", "month", "count" }] }`
With `year`+`month` → returns paginated articles for that month.

---

### `GET /api/v1/kiosk/categories`

**Response 200:** Array of `{ "category": "delays", "count": 5 }`

---

### `GET /api/v1/kiosk/tags`

**Response 200:** Array of `{ "tag": "trains", "count": 12 }`

---

## Operations Console

All `/api/v1/ops/*` endpoints require authentication.

### Reference Data

#### `GET /api/v1/ops/routes`

Requires: `ViewSchedules`

**Response 200:** Array of route objects:
```json
[
  {
    "id":          "uuid",
    "code":        "EW-001",
    "name":        "East-West Express",
    "origin":      "London Paddington",
    "destination": "Cardiff Central",
    "distance_km": 245
  }
]
```

---

#### `GET /api/v1/ops/seat-classes`

Requires: `ViewSchedules`

**Response 200:** Array of seat class objects:
```json
[
  { "id": "uuid", "code": "ECO", "name": "Economy", "base_multiplier": "1.00" }
]
```

---

### Schedules

#### `GET /api/v1/ops/schedules`

Requires: `ViewSchedules`

**Query params:** `route_id`, `status`, `departure_after`, `page`, `per_page`

**Response 200:** Paginated `{ items, total, page, per_page, total_pages }`

---

#### `GET /api/v1/ops/schedules/{id}`

Returns full schedule detail with inventory breakdown.

---

#### `PATCH /api/v1/ops/schedules/{id}/status`

Requires: `ManageSchedules`

**Request:** `{ "status": "delayed" }`

---

#### `POST /api/v1/ops/schedules/{id}/inventory`

Requires: `ManageSchedules`

**Request:**
```json
{
  "seat_class_id":   "uuid",
  "total_seats":     100,
  "available_seats": 87
}
```

---

### Passengers

#### `GET /api/v1/ops/passengers`

Requires: `ViewOrders`
**Query params:** `q` (name/email search), `page`, `per_page`

**Response 200:** Paginated list. PII is masked in list view.

---

#### `POST /api/v1/ops/passengers`

Requires: `ManageOrders`

**Request:**
```json
{
  "full_name": "Jane Doe",
  "email":     "jane@example.com",
  "phone":     "+44 7700 900000"
}
```

**Response 201:** Created passenger object.

---

#### `POST /api/v1/ops/passengers/{id}/pii-purge`

Requires: `ManageOrders`
Schedules PII deletion. Writes audit entry.

---

### Orders

#### `GET /api/v1/ops/orders`

Requires: `ViewOrders`
**Query params:** `passenger_id`, `schedule_id`, `status`, `page`, `per_page`

---

#### `GET /api/v1/ops/orders/by-number/{num}`

Requires: `ViewOrders`
Lookup by human-readable order number (e.g. `RL-000001`).

---

#### `GET /api/v1/ops/orders/{id}`

Requires: `ViewOrders`
Returns full order detail with passenger info and seat class.

---

#### `POST /api/v1/ops/orders`

Requires: `ManageOrders`

**Request:**
```json
{
  "passenger_id":  "uuid",
  "schedule_id":   "uuid",
  "seat_class_id": "uuid",
  "seat_number":   "12A",
  "fare_amount":   "89.50"
}
```

**Response 201:** Created order object.

---

#### `POST /api/v1/ops/orders/{id}/hold`

Requires: `ManageOrders`
Moves order to `held` status. Expiry set from `hold.ttl_minutes` rule.

---

#### `POST /api/v1/ops/orders/{id}/confirm`

Requires: `ManageOrders`
Moves order from `held` or `pending` → `confirmed`.

---

#### `POST /api/v1/ops/orders/{id}/cancel`

Requires: `ManageOrders`

**Request:**
```json
{
  "reason":          "Passenger requested cancellation",
  "disruption_flag": false,
  "refund_amount":   "45.00"
}
```

---

#### `POST /api/v1/ops/orders/{id}/refund`

Requires: `ProcessRefunds`

The backend evaluates the `RulesEngine` to compute `max_amount`.  The submitted
`amount` must not exceed `max_amount`.

**Request:** `{ "amount": "45.00" }`

**Response 200:**
```json
{
  "order_id": "uuid",
  "outcome":  "full_minus_fee",
  "amount":   "45.00",
  "reason":   "More than 24 hours before departure"
}
```

---

#### `POST /api/v1/ops/orders/{id}/fee-override`

Requires: `OverrideFees`

**Request:** `{ "override_amount": "20.00", "reason": "Goodwill gesture" }`

---

#### `POST /api/v1/ops/orders/{id}/disruption`

Requires: `ManageOrders`
Flags the order's associated schedule as disrupted.

---

#### `GET /api/v1/ops/orders/{id}/events`

Requires: `ViewOrders`
Returns the event timeline for an order.

---

## Business Rules

All `/api/v1/rules/*` endpoints require authentication.

### `GET /api/v1/rules`

Returns all business rules.  Read: any authenticated user.

**Response 200:** Array of:
```json
[
  {
    "key":         "refund.processing_fee_usd",
    "value":       "5.00",
    "value_type":  "decimal",
    "description": "Processing fee deducted from full refunds",
    "updated_at":  "2024-01-01T00:00:00Z",
    "updated_by":  "admin"
  }
]
```

---

### `GET /api/v1/rules/{key}`

Returns a single rule by key.

---

### `PATCH /api/v1/rules/{key}`

Requires: `Admin` role only.

**Request:** `{ "value": "7.50" }`

Value is validated against the rule's `value_type` (integer, decimal, boolean, duration).
Old and new values are written to `audit_logs`.

---

## Staffing

All `/api/v1/staffing/*` endpoints require authentication.

### Contractors

#### `GET /api/v1/staffing/contractors`

**Query params:** `q` (name search), `region`, `active`, `page`, `per_page`

---

#### `POST /api/v1/staffing/contractors`

**Request:**
```json
{
  "name":   "John Smith",
  "email":  "john@example.com",
  "region": "London",
  "tags":   ["driver", "conductor"]
}
```

---

#### `GET /api/v1/staffing/contractors/{id}`

Returns contractor with availability windows and active assignments.

---

#### `PATCH /api/v1/staffing/contractors/{id}/active`

**Request:** `{ "active": false }`

---

#### `POST /api/v1/staffing/contractors/{id}/availability`

**Request:**
```json
{
  "start_at": "2024-11-20T06:00:00Z",
  "end_at":   "2024-11-20T14:00:00Z"
}
```

---

### Shifts

#### `GET /api/v1/staffing/shifts`

**Query params:** `status`, `region`, `from`, `to`, `page`, `per_page`

---

#### `POST /api/v1/staffing/shifts`

Requires: `ManageStaffing`

**Request:**
```json
{
  "title":     "Platform 3 Supervisor",
  "region":    "London Paddington",
  "start_at":  "2024-11-20T06:00:00Z",
  "end_at":    "2024-11-20T14:00:00Z",
  "required_tags": ["supervisor"]
}
```

---

#### `PATCH /api/v1/staffing/shifts/{id}/status`

**Request:** `{ "status": "cancelled" }`

---

#### `GET /api/v1/staffing/shifts/{id}/candidates`

Returns ranked contractor candidates using availability overlap + tag matching.

---

#### `POST /api/v1/staffing/shifts/{id}/propose`

**Request:** `{ "contractor_id": "uuid" }`
Creates an assignment in `proposed` status; contractor must respond.

---

#### `PATCH /api/v1/staffing/assignments/{id}/respond`

**Request:** `{ "status": "accepted" }` or `{ "status": "rejected" }`

---

### Subscriptions

#### `GET /api/v1/staffing/subscriptions`

Returns subscriptions for the authenticated user.

---

#### `POST /api/v1/staffing/subscriptions`

**Request:** `{ "contractor_id": "uuid" }`

---

#### `DELETE /api/v1/staffing/subscriptions`

**Request:** `{ "contractor_id": "uuid" }`

---

## Credentials

All `/api/v1/credentials/*` endpoints require authentication.

### `GET /api/v1/credentials`

**Query params:** `contractor_id`, `status`, `expiring_before`, `page`, `per_page`

---

### `POST /api/v1/credentials`

Multipart form upload.

**Fields:** `contractor_id` (UUID), `doc_type` (string), `expires_on` (date), `file` (binary)

**Response 201:** Created credential object with `fingerprint` and `status: pending`.

---

### `GET /api/v1/credentials/{id}`

Returns credential detail with download URL.

---

### `PATCH /api/v1/credentials/{id}/review`

Requires: `Admin` or `OpsAgent`

**Request:** `{ "status": "approved", "notes": "Verified against register" }`

---

### `GET /api/v1/credentials/{id}/audit`

Returns audit chain for the credential.

---

### `POST /api/v1/credentials/{id}/esign`

**Request:** `{ "signature_data": "<base64 SVG path>" }`

---

### `POST /api/v1/credentials/expire`

Requires: `Admin`
Marks all credentials past their `expires_on` date as `expired`.
Returns `{ "expired_count": N }`.

---

## E-Signatures

### `POST /api/v1/esignatures`

**Request:**
```json
{
  "entity_type": "credential",
  "entity_id":   "uuid",
  "signer_name": "John Smith",
  "signature_data": "<base64>"
}
```

---

### `GET /api/v1/esignatures/{entity_type}/{entity_id}`

Returns all e-signatures for a given entity.

---

## Crawl Pipeline

All `/api/v1/crawl/*` endpoints require `Admin` role.

### `GET /api/v1/crawl/sources`
### `POST /api/v1/crawl/sources`
### `GET /api/v1/crawl/sources/{id}`
### `GET /api/v1/crawl/sources/{id}/tasks`
### `POST /api/v1/crawl/sources/{id}/tasks`
### `POST /api/v1/crawl/tasks/{id}/run`
### `GET /api/v1/crawl/tasks/{id}/runs`
### `GET /api/v1/crawl/runs/{id}`
### `GET /api/v1/crawl/runs/{id}/quality`
### `GET /api/v1/crawl/quality/quarantined`

Trigger a crawl run:
```
POST /api/v1/crawl/tasks/{id}/run
```
Returns 202 Accepted. The crawl engine picks up the task asynchronously.
