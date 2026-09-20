#!/usr/bin/env bash
# Host vault for day-to-day work from a git checkout.
#
#   ./scripts/run-vault-dev.sh                 # keep existing data/; empty vault if none
#   ./scripts/run-vault-dev.sh --reset         # wipe data/, start empty
#   ./scripts/run-vault-dev.sh --reset --owner # wipe data/, claim the vault as admin/admin
#   ./scripts/run-vault-dev.sh --reset-demo    # wipe data/, seed sample inbox
#   ./scripts/run-vault-dev.sh --sqlweb        # SQLite browser on http://127.0.0.1:8081
#   ./scripts/run-vault-dev.sh --release       # optimized binary (combine with any flag above)
#
# Website (separate terminal):
#   cd web && npm run dev          # http://localhost:5173, proxies /v1 here
#   cargo tauri dev                # desktop window, same Vite
#
# Debug profile by default so server-crate edits recompile quickly; --release
# builds the optimized binary for seeding and serving. Restart this process
# after Rust changes.
#
# Does not overwrite an existing config/config.toml except after reset-demo,
# which replaces that file with a demo config that has no [server] section.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

CONFIG="config/config.toml"
CONFIG_EXAMPLE="config/config.toml.example"
DEMO=0
RESET=0
SQLWEB=0
SQLWEB_PID=""
RELEASE=0
OWNER=0

usage() {
  cat <<EOF
Usage: $(basename "$0") [--reset | --reset-demo] [--owner] [--sqlweb] [--release]

  --reset       Wipe data/ and start with an empty vault
  --reset-demo  Wipe data/ and seed the sample inbox
  --owner       Claim the vault as admin/admin. Combine with --reset for an
                empty claimed vault; without it, --reset leaves the vault
                unclaimed so the Create Vault Owner screen is reachable.
                Rejected with --reset-demo, which claims the vault itself.
  --sqlweb      Start sqlite-web on http://127.0.0.1:8081 (needs sqlite_web on PATH)
  --release     Build and run the optimized binary (seed and serve)
  -h, --help

Examples:
  ./scripts/$(basename "$0")
      Keep data/ as it is and serve
  ./scripts/$(basename "$0") --reset
      Empty, unclaimed vault: the web UI opens on Create Vault Owner
  ./scripts/$(basename "$0") --reset --owner
      Empty vault, log in as admin / admin
  ./scripts/$(basename "$0") --owner
      Claim the existing vault as admin / admin (warns and carries on if it
      is already claimed)
  ./scripts/$(basename "$0") --reset-demo
      Sample inbox, log in as demo with an empty password
  ./scripts/$(basename "$0") --reset-demo --release --sqlweb
      Sample inbox on the optimized binary, with the SQLite browser on
      http://127.0.0.1:8081
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --reset) RESET=1 ;;
    --reset-demo) DEMO=1 ;;
    --owner) OWNER=1 ;;
    --release) RELEASE=1 ;;
    --sqlweb) SQLWEB=1 ;;
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

if [[ "${RESET}" -eq 1 && "${DEMO}" -eq 1 ]]; then
  echo "error: use either --reset or --reset-demo, not both" >&2
  exit 1
fi

if [[ "${OWNER}" -eq 1 && "${DEMO}" -eq 1 ]]; then
  echo "error: --reset-demo claims the vault itself; drop --owner" >&2
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
  # Loopback bind (no Docker port publish) and Vite/Tauri CORS.
  # Uncomments the single-line cors_origins array in the example. Keep that
  # array on one line or this substitution leaves invalid TOML.
  sed \
    -e 's/^# cors_origins =/cors_origins =/' \
    "${CONFIG_EXAMPLE}" >"${CONFIG}"
}

stop_sqlweb() {
  if [[ -n "${SQLWEB_PID}" ]]; then
    kill "${SQLWEB_PID}" 2>/dev/null || true
    wait "${SQLWEB_PID}" 2>/dev/null || true
    SQLWEB_PID=""
  fi
}

start_sqlweb() {
  require_cmd sqlite_web
  (
    echo "sqlite-web: waiting for data/vault.ready"
    while [[ ! -f data/vault.ready ]]; do
      sleep 1
    done
    echo "SQLite UI: http://127.0.0.1:8081"
    exec sqlite_web -H 127.0.0.1 -p 8081 -x data/vault.db
  ) &
  SQLWEB_PID=$!
  trap stop_sqlweb EXIT INT TERM
}

# cargo run with the chosen profile; arguments after -- go to the server binary.
vault() {
  cargo run "${CARGO_PROFILE[@]}" -p message-vault-server -- "$@"
}

run_server() {
  echo "Starting message-vault-server (${PROFILE_NAME}). Restart after server-crate edits."
  if [[ "${SQLWEB}" -eq 1 ]]; then
    vault serve --config "${CONFIG}"
  else
    exec cargo run "${CARGO_PROFILE[@]}" -p message-vault-server -- serve --config "${CONFIG}"
  fi
}

wipe_data() {
  echo "Removing ${REPO_ROOT}/data/…"
  rm -rf data
  mkdir -p data
}

require_cmd cargo

CARGO_PROFILE=()
PROFILE_NAME="debug"
if [[ "${RELEASE}" -eq 1 ]]; then
  CARGO_PROFILE=(--release)
  PROFILE_NAME="release"
fi

mkdir -p data

if [[ ! -f "${CONFIG}" ]]; then
  echo "Writing ${CONFIG} from ${CONFIG_EXAMPLE} (CORS for :5173 enabled)."
  write_host_dev_config
fi

if [[ "${RESET}" -eq 1 || "${DEMO}" -eq 1 ]]; then
  wipe_data
fi

if [[ "${DEMO}" -eq 1 ]]; then
  require_cmd ffmpeg
  require_cmd ffprobe
  echo "Seeding demo data…"
  vault reset-demo --config "${CONFIG}"
  write_host_dev_config
  echo "Converting demo media…"
  vault process-assets --config "${CONFIG}" \
    || echo "warning: process-assets failed; UI still works"
elif [[ "${RESET}" -eq 1 ]]; then
  echo "Empty data/ (claim the vault in the web UI, or pass --owner)."
elif [[ ! -f data/vault.db ]]; then
  echo "Empty data/ (pass --reset-demo to seed a sample inbox, or --owner to claim the vault)."
else
  echo "Vault DB present; leaving it in place."
fi

# Claiming is separate from seeding: --reset alone leaves the vault unclaimed,
# which is the only way to reach the Create Vault Owner screen in dev.
if [[ "${OWNER}" -eq 1 ]]; then
  echo "Claiming the vault as admin/admin…"
  vault create-owner --config "${CONFIG}" --username admin --password admin \
    || echo "warning: create-owner failed (already claimed?); leaving the vault as it is"
fi

echo
echo "Vault API:  http://127.0.0.1:8080"
echo "Website:    cd web && npm run dev     → http://localhost:5173"
echo "Desktop:    cargo tauri dev"
if [[ "${SQLWEB}" -eq 1 ]]; then
  echo "SQLite UI:  http://127.0.0.1:8081"
else
  echo "SQLite UI:  ./scripts/run-vault-dev.sh --sqlweb"
fi
echo

if [[ "${SQLWEB}" -eq 1 ]]; then
  start_sqlweb
fi

run_server
