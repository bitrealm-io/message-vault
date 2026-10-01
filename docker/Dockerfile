# Multi-stage release image built from the current checkout.
# Usage (from the repository root):
#   docker build -f docker/Dockerfile -t message-crate-server .
# Or:
#   docker compose -f docker/compose.release.yml up --build
#
# BUILD_METADATA is the part of the Build after the "+", normally the commit:
#   docker build --build-arg BUILD_METADATA="$(git rev-parse --short=8 HEAD)" ...
# .git is not in the build context, so neither stage can ask git for it. Left
# out, the server and the website report 0.9.0+unknown. Passed empty, as the
# release job does, they report the Product Version alone.

# -----------------------------------------------------------------------------
# Stage 1: Vite SPA frontend
# -----------------------------------------------------------------------------
FROM node:22-bookworm-slim AS frontend-builder

WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
ARG BUILD_METADATA=unknown
ENV MESSAGE_CRATE_BUILD_METADATA=$BUILD_METADATA
RUN npm run build

# -----------------------------------------------------------------------------
# Stage 2: Rust binary
# -----------------------------------------------------------------------------
# rust-toolchain.toml is the one place the compiler version is chosen. The
# image copies it and lets rustup install that exact toolchain before the
# build, so the published server compiles on the Rust that CI tested (#427).
# The base tag only picks the Debian release and pre-installs the same
# minor: once Docker Hub carries the patch release the install is a no-op,
# and scripts/check-docker-context.sh fails when the two minors disagree.
FROM rust:1.98-bookworm AS rust-builder

WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
RUN rustup toolchain install
COPY crates ./crates
# Required at compile time by crates/server/server/src/db/schema.rs include_str!(…).
COPY schema ./schema
# [patch.crates-io] sqlx-sqlite lives here (see Cargo.toml). Without this
# copy, cargo build fails: failed to read /src/vendor/sqlx-sqlite/Cargo.toml.
COPY vendor ./vendor

ARG BUILD_METADATA=unknown
ENV MESSAGE_CRATE_BUILD_METADATA=$BUILD_METADATA
# The server compiles in crates/server/demo-seed/data/ (corpus + names) and
# generates Demo Data from it on a first start. Do not ignore that path.
RUN cargo build --release -p message-crate-server

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
COPY --from=rust-builder /src/target/release/message-crate-server /usr/local/bin/message-crate-server

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
    MC_DB=/app/data/messagecrate.db \
    MC_DATA_DIR=/app/data

EXPOSE 8080
VOLUME ["/app/data"]

ENTRYPOINT ["tini", "--", "/usr/local/bin/docker-entrypoint-release.sh"]
