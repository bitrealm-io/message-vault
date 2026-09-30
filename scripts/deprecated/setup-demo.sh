#!/usr/bin/env bash
# setup-demo.sh — first-time demo bootstrap without Docker
#
# Deprecated: use ./scripts/run-vault-dev.sh --reset-demo from the repo root.
#
# Usage:
#   ./scripts/deprecated/setup-demo.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

cd "${REPO_ROOT}"

echo "Building message-crate-server (release)…"
cargo build --release -p message-crate-server

echo "Regenerating demo bundle, importing, and processing assets…"
cargo run --release -p message-crate-server -- reset-demo

echo "Demo ready. Start the UI: cd web && npm run dev"
