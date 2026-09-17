#!/usr/bin/env bash
# Host vault against compose Postgres, from a git checkout.
#
#   ./scripts/run-vault-pg-dev.sh                 # start Postgres if needed; keep data
#   ./scripts/run-vault-pg-dev.sh --reset         # wipe volume + data/, empty vault
#   ./scripts/run-vault-pg-dev.sh --reset --owner # wipe, claim the vault as admin/admin
#   ./scripts/run-vault-pg-dev.sh --reset-demo    # wipe, seed sample inbox (demo / empty password)
#   ./scripts/run-vault-pg-dev.sh --release       # optimized binary (also with --reset / --reset-demo)
#
# Website (separate terminal):
#   cd web && npm run dev
#   cargo tauri dev
#
# Stops the Postgres container when this script exits (volume stays).
# Do not run at the same time as ./scripts/run-vault-dev.sh (both use :8080).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

CONFIG="config/config.toml"
CONFIG_EXAMPLE="config/config.toml.example"
COMPOSE=(docker compose -f docker-compose.pg.yml)
DB_URL="postgres://vault:vault@127.0.0.1:5432/vault"
DEMO=0
RESET=0
OWNER=0
RELEASE=0

usage() {
  cat <<EOF
Usage: $(basename "$0") [--reset | --reset-demo] [--owner] [--release]

  --reset       Wipe the Postgres volume and data/, start empty
  --reset-demo  Wipe the Postgres volume and data/, seed the sample inbox
  --owner       Claim the vault as admin/admin (rejected with --reset-demo,
                which claims the vault itself)
  --release     Build and run the optimized binary (seed and serve)
  -h, --help

Examples:
  ./scripts/$(basename "$0")
      Start Postgres if needed, keep its data, and serve
  ./scripts/$(basename "$0") --reset
      Empty, unclaimed vault: the web UI opens on Create Vault Owner
  ./scripts/$(basename "$0") --reset --owner
      Empty vault, sign in as admin / admin
  ./scripts/$(basename "$0") --owner
      Claim the existing vault as admin / admin (warns and carries on if it
      is already claimed)
  ./scripts/$(basename "$0") --reset-demo
      Sample inbox, sign in as demo with an empty password
  ./scripts/$(basename "$0") --reset-demo --release
      Sample inbox on the optimized binary
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --reset) RESET=1 ;;
    --reset-demo) DEMO=1 ;;
    --owner) OWNER=1 ;;
    --release) RELEASE=1 ;;
    --sqlweb)
      echo "error: --sqlweb is only for ./scripts/run-vault-dev.sh (SQLite)" >&2
      exit 1
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

if [[ "${OWNER}" -eq 1 && "${DEMO}" -eq 1 ]]; then
  echo "error: --reset-demo claims the vault itself; drop --owner" >&2
  exit 1
fi

if [[ "${RESET}" -eq 1 && "${DEMO}" -eq 1 ]]; then
  echo "error: use either --reset or --reset-demo, not both" >&2
  exit 1
fi

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: '$1' not found on PATH" >&2
    exit 1
  fi
}

write_host_dev_config() {
  mkdir -p config data
  if [[ ! -f "${CONFIG_EXAMPLE}" ]]; then
    echo "error: missing ${CONFIG_EXAMPLE}" >&2
    exit 1
  fi
  sed \
    -e 's/^# cors_origins =/cors_origins =/' \
    "${CONFIG_EXAMPLE}" >"${CONFIG}"
}

stop_postgres() {
  "${COMPOSE[@]}" down
}

# This file used to inherit the directory project name `message-vault`,
# the same project as leftover `sqlite-web` from the deleted
# compose-dev.yml. Stop that old Postgres so the new `message-vault-pg`
# project can bind 5432. Does not remove sqlite-web.
stop_legacy_postgres() {
  if docker ps -aq --filter name=message-vault-postgres-1 \
    --filter label=com.docker.compose.project=message-vault | grep -q .; then
    echo "Stopping leftover Postgres from the old message-vault compose project…"
    docker compose -p message-vault -f docker-compose.pg.yml down
  fi
}

wait_postgres() {
  local i
  for i in $(seq 1 30); do
    if "${COMPOSE[@]}" exec -T postgres pg_isready -U vault >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  echo "error: Postgres did not become ready on 127.0.0.1:5432" >&2
  exit 1
}

require_cmd cargo
require_cmd docker

CARGO_RUN=(cargo run -p message-vault-server)
if [[ "${RELEASE}" -eq 1 ]]; then
  CARGO_RUN=(cargo run --release -p message-vault-server)
fi

mkdir -p data

if [[ ! -f "${CONFIG}" ]]; then
  echo "Writing ${CONFIG} from ${CONFIG_EXAMPLE} (CORS for :5173 enabled)."
  write_host_dev_config
fi

stop_legacy_postgres

if [[ "${RESET}" -eq 1 || "${DEMO}" -eq 1 ]]; then
  echo "Removing Postgres volume vault_pg_data and ${REPO_ROOT}/data/…"
  "${COMPOSE[@]}" down -v
  rm -rf data
  mkdir -p data
fi

trap stop_postgres EXIT INT TERM

echo "Starting Postgres (docker compose -f docker-compose.pg.yml)…"
"${COMPOSE[@]}" up -d
wait_postgres

if [[ "${DEMO}" -eq 1 ]]; then
  require_cmd ffmpeg
  require_cmd ffprobe
  echo "Seeding demo data into Postgres…"
  "${CARGO_RUN[@]}" -- reset-demo --config "${CONFIG}" --db-url "${DB_URL}"
  write_host_dev_config
elif [[ "${RESET}" -eq 1 ]]; then
  echo "Empty Postgres (claim the vault in the web UI, or pass --owner)."
else
  echo "Postgres volume present; leaving it in place."
fi

# Claiming is separate from seeding: --reset alone leaves the vault unclaimed,
# which is the only way to reach the Create Vault Owner screen in dev.
if [[ "${OWNER}" -eq 1 ]]; then
  echo "Claiming the vault as admin/admin…"
  "${CARGO_RUN[@]}" -- create-owner --config "${CONFIG}" --db-url "${DB_URL}" \
    --username admin --password admin \
    || echo "warning: create-owner failed (already claimed?); leaving the vault as it is"
fi

echo
echo "Vault API:  http://127.0.0.1:8080  (Postgres postgres://127.0.0.1:5432/vault)"
echo "Website:    cd web && npm run dev     → http://localhost:5173"
echo "Desktop:    cargo tauri dev"
echo "Stop:       Ctrl+C also stops the Postgres container (volume kept)."
echo

if [[ "${RELEASE}" -eq 1 ]]; then
  echo "Starting message-vault-server (release). First compile can take several minutes."
else
  echo "Starting message-vault-server (debug). Restart after server-crate edits."
fi
"${CARGO_RUN[@]}" -- serve --config "${CONFIG}" --db-url "${DB_URL}"
