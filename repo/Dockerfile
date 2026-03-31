# syntax=docker/dockerfile:1.4
# =============================================================================
# RailOps — Multi-stage Dockerfile
#
# Stages:
#   chef            — base toolchain with cargo-chef for layer caching
#   planner         — produce dependency recipe from workspace manifests
#   backend-builder — compile Actix-web binary (release)
#   frontend-builder— compile Yew → WASM via trunk (release)
#   runtime         — minimal Debian image with binary + static assets
# =============================================================================

ARG RUST_VERSION=1.82

# ── Stage 1: base toolchain ───────────────────────────────────────────────────
FROM rust:${RUST_VERSION}-slim-bookworm AS chef
# No extra apt packages needed:
#   pkg-config  — removed: only required to locate system libs; our tree is rustls/ring/RustCrypto
#                 (all pure Rust or self-contained C via the cc crate, never via pkg-config)
#   libssl-dev  — removed: we link rustls (actix-web `rustls-0_23`, sqlx `tls-rustls`),
#                 so openssl-sys is never pulled in and its headers are not needed
# gcc + libc6-dev are already present in rust:slim-bookworm and are sufficient for ring's
# bundled C/asm compilation.

# cargo-chef enables reproducible, cached dependency layers
RUN cargo install cargo-chef --locked

WORKDIR /src

# ── Stage 2: dependency planner ───────────────────────────────────────────────
FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: backend build ────────────────────────────────────────────────────
FROM chef AS backend-builder

COPY --from=planner /src/recipe.json recipe.json

# Build and cache all backend dependencies (this layer is reused on code-only changes)
RUN cargo chef cook --release -p backend --recipe-path recipe.json

# Copy full source and build the application binary
COPY . .
RUN cargo build --release -p backend

# ── Stage 4: frontend (WASM) build ────────────────────────────────────────────
FROM chef AS frontend-builder

# Add the WASM compilation target
RUN rustup target add wasm32-unknown-unknown

# Install trunk (downloads wasm-bindgen-cli automatically at build time)
RUN cargo install trunk --locked

COPY --from=planner /src/recipe.json recipe.json

# Cache frontend + shared dependencies for the WASM target
RUN cargo chef cook --release -p frontend --target wasm32-unknown-unknown --recipe-path recipe.json

# Copy full source and build the Yew app
COPY . .

WORKDIR /src/frontend
RUN trunk build --release

# ── Stage 5: minimal runtime image ────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
# No apt packages needed:
#   ca-certificates — removed: the app never makes outbound HTTPS connections
#                     (fully offline), generates its own TLS cert via rcgen,
#                     and connects to Postgres internally without CA verification.
# groupadd/useradd come from the `passwd` package already in debian:bookworm-slim.
RUN groupadd -r railops \
    && useradd -r -g railops -u 1001 railops

WORKDIR /app

# Backend binary
COPY --from=backend-builder  /src/target/release/backend   /app/railops

# Compiled Yew WASM + HTML served as static files
COPY --from=frontend-builder /src/frontend/dist            /app/static

# Create runtime directories with correct ownership
RUN mkdir -p /app/uploads /app/certs \
    && chown -R railops:railops /app

USER railops

EXPOSE 8443

# Healthcheck is handled by docker-compose (bash /dev/tcp probe — no extra packages needed).
HEALTHCHECK NONE

CMD ["/app/railops"]
