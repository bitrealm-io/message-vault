# Multi-stage release image built from the current checkout.
# Usage (from the repository root):
#   docker build -f docker/Dockerfile -t message-vault-server .
# Or:
#   docker compose -f docker/compose.release.yml up --build

# -----------------------------------------------------------------------------
# Stage 1: Vite SPA frontend
# -----------------------------------------------------------------------------
FROM node:22-bookworm-slim AS frontend-builder

WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# -----------------------------------------------------------------------------
# Stage 2: Rust binary
# -----------------------------------------------------------------------------
# rust-toolchain.toml is the one place the compiler version is chosen. The
# image copies it and lets rustup install that exact toolchain before the
# build, so the published vault compiles on the Rust that CI tested (#427).
# The base tag only picks the Debian release and pre-installs the same
# minor: once Docker Hub carries the patch release the install is a no-op,
# and scripts/check-docker-context.sh fails when the two minors disagree.
FROM rust:1.98-bookworm AS rust-builder

WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
RUN rustup toolchain install
COPY crates ./crates
# Required at compile time by crates/vault/server/src/db/schema.rs include_str!(…).
COPY schema ./schema
# [patch.crates-io] sqlx-sqlite lives here (see Cargo.toml). Without this
# copy, cargo build fails: failed to read /src/vendor/sqlx-sqlite/Cargo.toml.
COPY vendor ./vendor

RUN cargo build --release -p message-vault-server
# Conversation JSONL is not in git. Write it here so the runtime image can seed.
# Needs crates/vault/demo-seed/data/ (corpus + names). Do not ignore that path.
RUN cargo run --release -p demo-seed

# -----------------------------------------------------------------------------
# Stage 3: slim runtime
# -----------------------------------------------------------------------------
FROM node:20-bookworm-slim AS runtime

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    ffmpeg \
    tini \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Rust CLI / serve binary
COPY --from=rust-builder /src/target/release/message-vault-server /usr/local/bin/message-vault-server

# Demo bundle (seeded on first start when DEMO_DATA is truthy)
COPY --from=rust-builder /src/crates/vault/demo-seed/staging ./crates/vault/demo-seed/staging
COPY --from=rust-builder /src/crates/vault/demo-seed/config ./crates/vault/demo-seed/config
COPY --from=rust-builder /src/crates/vault/demo-seed/README.md ./crates/vault/demo-seed/README.md

# Container config template
COPY config/config.docker.toml ./config/config.docker.toml

# Vite SPA (built in Stage 1)
COPY --from=frontend-builder /web/dist ./static

COPY docker/entrypoint-release.sh /usr/local/bin/docker-entrypoint-release.sh
RUN chmod +x /usr/local/bin/docker-entrypoint-release.sh \
  && mkdir -p /app/data \
  && chown -R node:node /app

USER node

ENV HOSTNAME=0.0.0.0 \
    VAULT_DB=/app/data/vault.db \
    VAULT_DATA_DIR=/app/data

EXPOSE 8080
VOLUME ["/app/data"]

ENTRYPOINT ["tini", "--", "/usr/local/bin/docker-entrypoint-release.sh"]
