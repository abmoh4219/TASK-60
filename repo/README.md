# RailOps

Railway operations platform — public kiosk, ops console, staffing dispatch,
credential management, content crawl pipeline, and configurable business rules.

---

## Start the stack

```bash
docker compose up --build
```

The app starts at **https://localhost:8443** (self-signed TLS cert is generated
automatically on first boot).

---

## Credentials

| Username     | Password            | Role       |
|--------------|---------------------|------------|
| `admin`      | `AdminRailOps2024!` | Admin      |
| `ops_agent1` | `AdminRailOps2024!` | OpsAgent   |
| `cs_agent1`  | `AdminRailOps2024!` | CsAgent    |
| `dispatch1`  | `AdminRailOps2024!` | Dispatcher |

Passwords are hashed with Argon2id on first boot from the `ADMIN_SEED_PASSWORD`
environment variable.  Change the admin password after the first login in
production.

---

## Frontend

Open **https://localhost:8443** in your browser.

- `#/` → Kiosk (public article search, archive, categories)
- `#/login` → Staff login
- `#/ops/schedules` → Operations console — schedules (default after login)
- `#/ops/orders` → Operations console — orders & passengers
- `#/ops/rules` → Business rules (Admin only)
- `#/ops/staffing` → Staffing dispatch
- `#/ops/credentials` → Credential management

---

## API

All authenticated requests require:

```
Authorization: Bearer <token>
X-RailOps-Ts:  <unix_timestamp_seconds>
X-RailOps-Sig: <hmac-sha256(token, "METHOD\nPATH_WITH_QUERY\nTS")>
```

See [`docs/api-spec.md`](docs/api-spec.md) for full endpoint documentation.
See [`docs/design.md`](docs/design.md) for architecture and security details.

---

## Run tests

Only Docker is required — no Rust, Node, or other host tooling needed.

**Step 1 — start the stack** (skip if already running):

```bash
docker compose up --build -d
```

**Step 2 — run the test suite** (streams output directly to your terminal):

```bash
docker compose --profile test run --rm tester
```

This runs all three test suites inside the Docker tester container against the live PostgreSQL database:

- **Backend unit tests** — crypto, crawl pipeline (no DB required)
- **Shared utility tests** — `QualityScore`, `PaginationParams`, `mask_phone`
- **API integration tests** — live HTTP against the real PostgreSQL database

All test output is printed to stdout (`--nocapture`).

A convenience wrapper `run_test.sh` is also available — it starts the stack, waits for the health endpoint, then executes the same `docker compose --profile test run --rm tester` command.

---

## Verification steps

```bash
# Health check
curl -sk https://localhost:8443/health | python3 -m json.tool

# Login as admin
curl -sk -X POST https://localhost:8443/api/v1/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"AdminRailOps2024!"}' | python3 -m json.tool

# Public kiosk content (no auth)
curl -sk https://localhost:8443/api/v1/kiosk/content | python3 -m json.tool
```
