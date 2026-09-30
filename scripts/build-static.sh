#!/usr/bin/env bash
# Build the Vite SPA into static/, which the server hands out at GET /.
#
# Used when running message-crate-server on the host (./scripts/run-dev.sh
# or cargo run … serve). `ln -s web/dist static` works too. Not needed for the
# release image, which builds web/ in its own stage, or for Tauri, which builds
# it via beforeBuildCommand.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "Building Vite SPA in $REPO_ROOT/web…"
(cd "$REPO_ROOT/web" && npm run build)

echo "Copying dist/ to $REPO_ROOT/static/…"
rm -rf "$REPO_ROOT/static"
cp -r "$REPO_ROOT/web/dist" "$REPO_ROOT/static"

echo "Done. Served at http://127.0.0.1:8080/ by"
echo "./scripts/run-dev.sh or cargo run -p message-crate-server -- serve"
