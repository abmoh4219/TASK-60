# syntax=docker/dockerfile:1.4
# =============================================================================
# RailOps — Multi-stage Dockerfile
#
# Stages:
#   chef            — base Rust toolchain with cargo-chef for layer caching
#   planner         — produce dependency recipe from workspace manifests
#   backend-builder — compile Actix-web binary (release)
#   frontend-builder— compile Yew → WASM via trunk + bundle Tailwind CSS
#   tester          — test runner (unit + integration tests via cargo test)
#   runtime         — minimal Debian image with binary + static assets
# =============================================================================

ARG RUST_VERSION=1.88

# ── Stage 1: base toolchain ───────────────────────────────────────────────────
FROM rust:${RUST_VERSION}-slim-bookworm AS chef
# Pure-Rust TLS (rustls) + RustCrypto mean we need no system OpenSSL headers.
# gcc and libc6-dev are already present in rust:slim-bookworm and are
# sufficient for ring's bundled C/asm compilation.

RUN cargo install cargo-chef --locked
WORKDIR /src

# ── Stage 2: dependency planner ───────────────────────────────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: backend build ────────────────────────────────────────────────────
FROM chef AS backend-builder

COPY --from=planner /src/recipe.json recipe.json
# Pre-compile all backend dependencies (layer is reused on code-only changes).
RUN cargo chef cook --release -p backend --recipe-path recipe.json

COPY . .
RUN cargo build --release -p backend

# ── Stage 4: frontend (WASM) build ────────────────────────────────────────────
FROM chef AS frontend-builder

# WASM compilation target
RUN rustup target add wasm32-unknown-unknown

# Install curl + CA certificates to download prebuilt binaries.
# These are build-time-only dependencies; the runtime image has no internet access.
RUN apt-get update -qq \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# ── trunk: Yew asset bundler ──────────────────────────────────────────────────
# Download the prebuilt trunk binary to avoid OOM-killing from compiling it.
ARG TRUNK_VERSION=0.21.14
RUN set -eux; \
    ARCH=$(uname -m); \
    case "$ARCH" in \
        x86_64)  TRUNK_ARCH=x86_64-unknown-linux-gnu   ;; \
        aarch64) TRUNK_ARCH=aarch64-unknown-linux-gnu  ;; \
        *) echo "Unsupported CPU architecture: $ARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL \
        "https://github.com/thedodd/trunk/releases/download/v${TRUNK_VERSION}/trunk-${TRUNK_ARCH}.tar.gz" \
        | tar -xz -C /usr/local/bin/ \
    && chmod +x /usr/local/bin/trunk \
    && trunk --version

# ── Tailwind CSS standalone CLI ───────────────────────────────────────────────
# Single self-contained binary — no Node.js required.
# Scans ./src/**/*.rs and ./index.html for used utility classes, then generates
# a minified CSS bundle (tailwind.css) that trunk copies into the dist folder.
ARG TAILWIND_VERSION=3.4.1
RUN set -eux; \
    ARCH=$(uname -m); \
    case "$ARCH" in \
        x86_64)  TW_ARCH=x64   ;; \
        aarch64) TW_ARCH=arm64 ;; \
        *) echo "Unsupported CPU architecture: $ARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSLo /usr/local/bin/tailwindcss \
        "https://github.com/tailwindlabs/tailwindcss/releases/download/v${TAILWIND_VERSION}/tailwindcss-linux-${TW_ARCH}" \
    && chmod +x /usr/local/bin/tailwindcss \
    && test -x /usr/local/bin/tailwindcss

# ── Dependency pre-warm (frontend + shared) ───────────────────────────────────
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release -p frontend --target wasm32-unknown-unknown \
    --recipe-path recipe.json

# ── Full source copy and build ────────────────────────────────────────────────
COPY . .
WORKDIR /src/frontend

# 1. Generate the Tailwind CSS bundle from Rust source files + config.
#    Output: frontend/tailwind.css (picked up by trunk via <link data-trunk>)
RUN tailwindcss \
    --input  ./src/app.css \
    --output ./tailwind.css \
    --config ./tailwind.config.js \
    --minify

# 2. Compile Yew to WASM and assemble the static dist/ folder.
RUN trunk build --release

# ── Stage 5: test runner ──────────────────────────────────────────────────────
# Inherits the full Rust toolchain + all pre-compiled dependencies.
# Used exclusively by `docker compose --profile test run --rm tester`.
FROM backend-builder AS tester

# curl is needed for the in-container health-check wait loop.
RUN apt-get update -qq \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src

# Default command: full test suite (overridden by docker-compose).
CMD ["sh", "-c", \
    "cargo test --bin backend -p backend -- --nocapture && \
     cargo test -p shared -- --nocapture && \
     cargo test --test api_integration_test -p backend -- --nocapture"]

# ── Stage 6: minimal runtime image ────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
# curl is used by the Docker Compose healthcheck to probe /health over HTTPS.
# It is the only package needed at runtime; ca-certificates is not required
# because the app generates its own TLS cert (rcgen) and connects to Postgres
# only on the internal Docker network.
RUN apt-get update -qq \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r railops \
    && useradd -r -g railops -u 1001 railops

WORKDIR /app

# Backend binary
COPY --from=backend-builder  /src/target/release/backend   /app/railops

# Compiled Yew WASM + HTML + bundled Tailwind CSS
COPY --from=frontend-builder /src/frontend/dist            /app/static

RUN mkdir -p /app/uploads /app/certs \
    && chown -R railops:railops /app

USER railops
EXPOSE 8443

HEALTHCHECK NONE
CMD ["/app/railops"]
