#!/usr/bin/env bash
# =============================================================================
# run-test.sh — RailOps Docker-first test runner
#
# Usage (from repo/):
#   ./run-test.sh
#
# Prerequisites:
#   • Docker (and docker compose) must be installed and running.
#   • No Rust, Node, npm, or other host tooling required.
#
# What this script does:
#   1. Starts the full stack with `docker compose up -d --build`
#      (db + app; skipped automatically if already running).
#   2. Polls https://localhost:8443/health until the backend is healthy.
#   3. Runs the three test suites inside the Docker tester container:
#        [1/3] Backend unit tests    (cargo test --bin backend -p backend)
#        [2/3] Shared utility tests  (cargo test -p shared)
#        [3/3] API integration tests (cargo test --test api_integration_test)
#      The tester container connects to the app via the internal Docker network
#      (https://app:8443), so no host-level routing is needed for the tests.
# =============================================================================

set -euo pipefail

HEALTH_URL="https://localhost:8443/health"
MAX_WAIT=180      # max seconds to wait for the backend to be ready
WAIT_INTERVAL=5   # poll interval in seconds

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── colour helpers ─────────────────────────────────────────────────────────────
GREEN="\033[0;32m"
RED="\033[0;31m"
YELLOW="\033[0;33m"
BOLD="\033[1m"
RESET="\033[0m"

ok()   { echo -e "${GREEN}${BOLD}[PASS]${RESET} $*"; }
fail() { echo -e "${RED}${BOLD}[FAIL]${RESET} $*"; }
info() { echo -e "${YELLOW}[info]${RESET} $*"; }

echo -e "${BOLD}============================================================${RESET}"
echo -e "${BOLD} RailOps — Docker Test Suite${RESET}"
echo -e "${BOLD}============================================================${RESET}"
echo ""

# ── pre-flight: require docker ─────────────────────────────────────────────────
if ! command -v docker &>/dev/null; then
    fail "docker not found in PATH"
    echo "  Install Docker: https://docs.docker.com/get-docker/"
    exit 1
fi
info "docker $(docker --version)"

# ── 1. Start services ──────────────────────────────────────────────────────────
echo ""
info "Starting Docker services (docker compose up -d --build) ..."
cd "${REPO_DIR}"
docker compose up -d --build

# ── 2. Wait for backend health ─────────────────────────────────────────────────
echo ""
info "Waiting for backend at ${HEALTH_URL}  (max ${MAX_WAIT}s) ..."
elapsed=0
until curl -sk "${HEALTH_URL}" 2>/dev/null | grep -q '"status":"ok"'; do
    if [ "${elapsed}" -ge "${MAX_WAIT}" ]; then
        fail "Backend did not become healthy within ${MAX_WAIT}s"
        echo ""
        echo "  Check service status:  docker compose ps"
        echo "  View logs:             docker compose logs app"
        exit 1
    fi
    info "  Not ready yet — retrying in ${WAIT_INTERVAL}s  (${elapsed}/${MAX_WAIT}s elapsed)"
    sleep "${WAIT_INTERVAL}"
    elapsed=$((elapsed + WAIT_INTERVAL))
done
ok "Backend is healthy"

# ── 3. Run the full test suite inside Docker ───────────────────────────────────
echo ""
echo -e "${BOLD}------------------------------------------------------------${RESET}"
echo -e "${BOLD} Running all tests inside Docker tester container${RESET}"
echo -e "${BOLD}------------------------------------------------------------${RESET}"
echo ""

if docker compose --profile test run --rm tester; then
    echo ""
    echo -e "${BOLD}============================================================${RESET}"
    ok "All tests PASSED"
    echo -e "${BOLD}============================================================${RESET}"
else
    TESTER_EXIT=$?
    echo ""
    echo -e "${BOLD}============================================================${RESET}"
    fail "Tests FAILED (exit ${TESTER_EXIT})"
    echo -e "${BOLD}============================================================${RESET}"
    exit "${TESTER_EXIT}"
fi
