#!/usr/bin/env bash
# Assert every place the repo states its license agrees on the Fair Core License.
#
#   ./scripts/check-license.sh
#
# Read-only. Checks LICENSE.md, every tracked Cargo.toml, web/package.json (and
# its lockfile), and the committed OpenAPI spec. Runs in CI and in check-pr.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

EXPECTED_CARGO_LICENSE="LicenseRef-FCL-1.0-ALv2"
# The three crates that are deliberately not FCL, and the licence each
# carries. The helper program links GPL code, so it is GPL; the protocol crate
# is linked by both the GPL helper and the FCL app, and the chat.db fixture by
# the tests of both, so those two are permissive. Every other crate is FCL.
# Why: docs/agents/licences.md.
declare -A LICENCE_EXCEPTIONS=(
  ["crates/helpers/imessage-reader/Cargo.toml"]="GPL-3.0-or-later"
  ["crates/helpers/imessage-reader-protocol/Cargo.toml"]="MIT OR Apache-2.0"
  ["crates/helpers/chat-db-fixture/Cargo.toml"]="MIT OR Apache-2.0"
)
EXPECTED_WEB_LICENSE="SEE LICENSE IN ../LICENSE.md"
EXPECTED_SPEC_LICENSE="Fair Core License 1.0 (ALv2 future)"

failures=0

# LICENSE.md itself is the Fair Core License.
first_line="$(head -1 LICENSE.md)"
if [[ "${first_line}" != "# Fair Core License, Version 1.0, ALv2 Future License" ]]; then
  echo "LICENSE.md does not start with the Fair Core License header: ${first_line}" >&2
  failures=$((failures + 1))
fi

# Every tracked Cargo.toml package declares the FCL expression (or the one
# exception listed for it above), so Cargo metadata cannot silently drift to
# another license. The workspace-root
# manifest has no [package] section and therefore no license field. The
# vendored `vendor/sqlx-sqlite/` fork is third-party code and keeps upstream's
# own license (MIT OR Apache-2.0), so it is excluded from the FCL check. The
# AGPL sweep in the loop below skips vendor/ the same way: a future vendored
# crate whose text mentions AGPL would pass silently. That is intentional —
# vendor/ is third-party code, and its license is not the repo's — but keep
# the exclusion in mind when the sweep seems to miss a manifest.
while IFS= read -r manifest; do
  if grep -q '^\[package\]' "${manifest}"; then
    expected="${LICENCE_EXCEPTIONS[${manifest}]:-${EXPECTED_CARGO_LICENSE}}"
    if ! grep -q "^license = \"${expected}\"$" "${manifest}"; then
      echo "${manifest}: license must be \"${expected}\"" >&2
      failures=$((failures + 1))
    fi
  fi
  if grep -q 'AGPL' "${manifest}"; then
    echo "${manifest}: still mentions AGPL" >&2
    failures=$((failures + 1))
  fi
done < <(git ls-files '*Cargo.toml' | grep -v '^vendor/' | sort)

# web/package.json and its lockfile mirror the same license.
for file in web/package.json web/package-lock.json; do
  if ! grep -qF "\"license\": \"${EXPECTED_WEB_LICENSE}\"" "${file}"; then
    echo "${file}: license must be \"${EXPECTED_WEB_LICENSE}\"" >&2
    failures=$((failures + 1))
  fi
done

# The committed OpenAPI spec advertises the Fair Core License, not AGPL.
if ! grep -qF "${EXPECTED_SPEC_LICENSE}" docs/src/assets/openapi.json; then
  echo "docs/src/assets/openapi.json: info.license must name \"${EXPECTED_SPEC_LICENSE}\"" >&2
  failures=$((failures + 1))
fi
if grep -qF 'AGPL' docs/src/assets/openapi.json; then
  echo "docs/src/assets/openapi.json: spec still mentions AGPL" >&2
  failures=$((failures + 1))
fi

if [[ ${failures} -gt 0 ]]; then
  echo "License metadata is inconsistent (${failures} failure(s))." >&2
  exit 1
fi
