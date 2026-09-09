#!/usr/bin/env bash
# Full local check: everything CI runs, in one command.
#
#   ./scripts/check-all.sh
#
# Stops on the first failure. Starts with ./scripts/check-pr.sh (format and
# lint), then builds and tests the workspace and src-tauri, and runs the
# license, Docker-context, version-lockstep and generated-API-type checks, cargo-deny
# (advisories, licences and bans on the workspace, licences and bans on src-tauri), and
# the web and docs test/build/audit steps. The workspace always tests on
# SQLite; export MV_TEST_POSTGRES_URL and the server crate runs a second
# time on Postgres, the way CI does. CI runs the same set in
# parallel; this exists so nobody types nine commands by hand. Why the
# split from check-pr.sh: docs/adr/0007-ci-is-the-only-gate.md.
# Runs npm ci in web/ and docs/ only when that tree has no node_modules yet.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

"${SCRIPT_DIR}/check-pr.sh"

echo "==> license consistency"
"${SCRIPT_DIR}/check-license.sh"

echo "==> docker rust-builder copies patched crates"
"${SCRIPT_DIR}/check-docker-context.sh"

echo "==> product version lockstep"
"${SCRIPT_DIR}/check-version-lockstep.sh"

echo "==> cargo deny check advisories licenses bans"
if cargo deny --version >/dev/null 2>&1; then
  cargo deny check advisories licenses bans
  cargo deny --manifest-path src-tauri/Cargo.toml --config deny.toml check licenses bans
else
  echo "cargo-deny not installed; skipping advisory and licence checks (CI enforces them)" >&2
fi

echo "==> cargo build --workspace"
cargo build --workspace

echo "==> cargo test --workspace (SQLite)"
env -u MV_TEST_POSTGRES_URL cargo test --workspace

if [[ -n "${MV_TEST_POSTGRES_URL:-}" ]]; then
  echo "==> cargo test -p message-vault-server (Postgres)"
  cargo test -p message-vault-server
fi

echo "==> cargo test src-tauri"
cargo test --manifest-path src-tauri/Cargo.toml

echo "==> generated vault API types match the OpenAPI document"
"${SCRIPT_DIR}/check-generated-api-types.sh"

echo "==> web test"
(cd web && npm test)
echo "==> web audit"
(cd web && npm audit --audit-level=high)
echo "==> web build (type-check + bundle)"
(cd web && npm run build)

if [[ ! -d docs/node_modules ]]; then
  echo "==> npm ci (docs)"
  (cd docs && npm ci)
fi
echo "==> docs check"
(cd docs && npm run check)
echo "==> docs build"
(cd docs && npm run build)
echo "==> docs audit"
(cd docs && npm audit --audit-level=high)

echo "All checks passed."
