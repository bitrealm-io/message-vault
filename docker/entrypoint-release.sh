#!/usr/bin/env bash
# Release entrypoint: write the container config, then run the server.
#
# Nothing is seeded here. `message-crate-server serve` adds the Demo Account
# itself when the database does not exist yet, so a Message Crate started by
# Docker and one started any other way begin the same.
#
# Arguments run another server command in place of `serve`, with the stack
# stopped: docker compose run --rm server reset-demo --size large
set -euo pipefail

cd /app

CONFIG_DOCKER="config/config.docker.toml"
CONFIG="config/config.toml"

export MC_DB="${MC_DB:-/app/data/messagecrate.db}"
export MC_DATA_DIR="${MC_DATA_DIR:-/app/data}"

mkdir -p config data
cp "${CONFIG_DOCKER}" "${CONFIG}"

if [[ $# -gt 0 ]]; then
  exec message-crate-server "$@"
fi

echo "Starting message-crate-server (API + static files)…"
exec message-crate-server serve --config "${CONFIG}"
