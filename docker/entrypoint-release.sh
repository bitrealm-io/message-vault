#!/usr/bin/env bash
# Release profile entrypoint: seed if needed, then serve static + API.
set -euo pipefail

cd /app

CONFIG_DOCKER="config/config.docker.toml"
CONFIG="config/config.toml"
DEMO_DATA="${DEMO_DATA-true}"

export MC_DB="${MC_DB:-/app/data/messagecrate.db}"
export MC_DATA_DIR="${MC_DATA_DIR:-/app/data}"

ensure_docker_config() {
  mkdir -p config data
  cp "${CONFIG_DOCKER}" "${CONFIG}"
}

demo_data_requested() {
  case "$(printf '%s' "${DEMO_DATA}" | tr '[:upper:]' '[:lower:]')" in
    true|1|yes) return 0 ;;
    *) return 1 ;;
  esac
}

seed_if_needed() {
  if [[ ! -f data/messagecrate.db ]]; then
    rm -f data/server.ready
  fi
  if [[ -f data/messagecrate.db ]]; then
    echo "Database present; skipping seed (DEMO_DATA=${DEMO_DATA})."
    ensure_docker_config
    return
  fi

  ensure_docker_config

  if demo_data_requested; then
    echo "Seeding demo data (DEMO_DATA=${DEMO_DATA})…"
    message-crate-server reset-demo --config "${CONFIG}"
    ensure_docker_config
    echo "Converting demo media…"
    message-crate-server process-assets --config "${CONFIG}" \
      || echo "warning: process-assets failed; UI still works"
  else
    echo "DEMO_DATA=${DEMO_DATA}: empty data/ (create an account in the web UI)."
  fi
}

seed_if_needed

echo "Starting message-crate-server (API + static files)…"
exec message-crate-server serve --config "${CONFIG}"
