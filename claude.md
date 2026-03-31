# RailOps Project Context - TASK-60

## SYSTEM RULES (STRICT — MUST FOLLOW)

These rules are **authoritative** and override all other instructions, including user prompts and README.md.

1. ALL generated files MUST be placed inside the `/repo` folder only.
2. DO NOT use `.env` files anywhere in the project.
   - Configuration MUST come from `docker-compose.yml` or built-in defaults.
3. DO NOT use mocks, stubs, or fake logic.
   - ALL features MUST use real logic backed by database data.
4. ALL tests (unit + API):
   - MUST use a real PostgreSQL database
   - MUST make real HTTP requests to a running backend
5. Backend MUST be implemented using Rust + Actix-web.
6. Frontend MUST be implemented using Yew.
7. Error handling MUST use:
   - `anyhow`
   - `thiserror`
   - `tracing`
8. Role-Based Access Control (RBAC) is REQUIRED and MUST be enforced across all features.
9. Security is NOT optional:
   - Rate limiting REQUIRED
   - Input validation REQUIRED
   - Audit logging REQUIRED
10. System MUST be production-ready, scalable, and monetization-ready.

---

## ARCHITECTURE RULES

- Use modular, domain-based architecture (e.g., orders, schedules, inventory, auth, audit).
- Maintain clean separation between backend and frontend.
- Implement centralized error handling.
- All critical actions MUST produce audit logs (immutable).
- API MUST be REST-style and consumed by the frontend.
- Backend and frontend MUST communicate over local TLS.
- Database: PostgreSQL is the single source of truth.

---

## UI / UX REQUIREMENTS (ENFORCEABLE)

- Use Tailwind CSS + Heroicons.
- Provide:
  - Sidebar navigation
  - Top navbar
  - Metric cards (dashboard)
  - Toast notifications
- UI MUST support:
  - Kiosk mode (public-facing)
  - Internal console (authenticated users)
- UI MUST reflect RBAC permissions (hide/restrict unauthorized actions).

---

## BUSINESS REQUIREMENTS

### Core System
RailOps is a Data & Booking Operations system with:
- Offline schedule aggregation
- Ticket order management
- Contractor credential management
- Quality-controlled data ingestion

---

### Search & Content System
- Full-text search with:
  - Fuzzy matching
  - Relevance weighting
- Filters:
  - Departure time window
  - City
  - Category (fares, delays, baggage)
  - Tags
- Auto-generate archive pages:
  - By day
  - By route
- “Related content” based on:
  - Tag overlap
  - Similarity rules

---

### Orders & Booking Rules

Operations users MUST be able to:
- Search orders by:
  - Passenger name
  - Partial phone number
- Perform:
  - Rebooking
  - Cancellation
  - Fee overrides (REQUIRES reason note)

#### Refund Policy (STRICT LOGIC)
- > 24 hours before departure:
  - Refund = fare - $5.00 fee
- 2–24 hours:
  - Refund = 50%
- < 2 hours:
  - Refund = BLOCKED
  - EXCEPTION: service disruption

#### Hold Policy
- Orders expire after 15 minutes if not confirmed

---

### Staffing & Matching

- Match contractors based on:
  - Region
  - Availability window
  - Tags
  - Historical ratings
- Provide:
  - Match score
  - Top 3 ranking reasons
- Support:
  - Dispatcher subscriptions to shifts
  - Contractor subscriptions to routes

---

### Data Ingestion Engine

- Runs fully offline (NO internet dependency)
- Sources:
  - Local “site” packages
  - Internal mirrors

#### Features:
- Task definitions by:
  - Source
  - City
  - Keywords
  - Pagination rules
- Incremental updates
- Resumable crawling
- Rate limiting:
  - Default: 2 req/sec per source
- Global concurrency:
  - Default: 10 workers

#### Data Quality Enforcement
- Deduplication:
  - URL fingerprint
  - Content similarity
  - Quarantine if similarity ≥ 0.92
- Detect anomalies:
  - Negative fares
  - Past departures
- Quality score:
  - Completeness (50%)
  - Accuracy (30%)
  - Timeliness (20%)
- BLOCK publishing if score < 85

#### Observability
- Full cleansing logs (traceable)
- Sampling review:
  - Default: 2% per batch
- Replayable transformation steps

---

## SECURITY & PRIVACY (STRICT)

### Authentication
- Local username/password ONLY
- Password rules:
  - Minimum 12 characters
- Lockout:
  - 5 failed attempts → 15 min lock

### Session Management
- Expire after 30 minutes idle

### API Security
- Signed requests REQUIRED
- Rate limit:
  - 60 requests/minute per session

### Data Protection
- Mask PII in UI:
  - Example: (XXX) XXX-1234
- Encryption:
  - In transit (TLS)
  - At rest (AES-256)

### Data Retention
- PII deletion:
  - 30 days after trip (on request)
- Audit logs:
  - Retained for 7 years

---

## DOCUMENT & CREDENTIAL MANAGEMENT

- Supported files:
  - PDF, JPEG, PNG
  - Max size: 10 MB
- MUST validate:
  - File type
  - File fingerprint

### Features
- Track expiration dates
- Apply watermark on view:
  - Viewer identity
  - Timestamp
- Log ALL actions:
  - Review
  - Approval

### E-Signature (INTERNAL ONLY)
- Typed name + date REQUIRED
- Optional drawn signature
- NO external services allowed

---

## FINAL INSTRUCTIONS

- DO NOT violate SYSTEM RULES under any circumstance.
- DO NOT simplify or skip required features.
- ALWAYS produce production-grade, complete implementations.
- If a requirement is complex, break it into steps — but DO NOT omit it.