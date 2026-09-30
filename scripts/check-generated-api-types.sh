#!/usr/bin/env bash
# The web app's vault types are generated from docs/src/assets/openapi.json.
# That JSON is already pinned to the running server by a Rust test
# (crates/server/server/src/openapi.rs). This is the other half: it fails when
# the checked-in TypeScript no longer matches the JSON, so a route or field
# renamed on the vault cannot reach the web app as a silent runtime error.
#
#   ./scripts/check-generated-api-types.sh
#
# Regenerate with: (cd web && npm run gen:api)
#
# The generator runs through npx rather than as a web/ dependency: it declares
# a peer dependency on TypeScript 5 and this project is on TypeScript 7, so
# installing it into web/ fails to resolve. Running it in its own tree sidesteps
# that, and only its text output ever reaches the repository. Keep the pinned
# version here in step with web/package.json's gen:api script.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

GENERATED="web/src/lib/vaultApi.types.ts"
SPEC="docs/src/assets/openapi.json"

if [[ ! -f "${GENERATED}" ]]; then
  echo "missing ${GENERATED}; run: (cd web && npm run gen:api)" >&2
  exit 1
fi

tmp="$(mktemp -t vaultApi.types.XXXXXX.ts)"
trap 'rm -f "${tmp}"' EXIT

npx --yes openapi-typescript@7.13.0 "${SPEC}" -o "${tmp}" >/dev/null

if ! diff -u "${GENERATED}" "${tmp}"; then
  echo >&2
  echo "${GENERATED} is out of date with ${SPEC}." >&2
  echo "run: (cd web && npm run gen:api)" >&2
  exit 1
fi

echo "${GENERATED} matches ${SPEC}"
