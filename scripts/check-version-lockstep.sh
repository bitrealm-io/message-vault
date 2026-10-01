#!/usr/bin/env bash
# Assert every file that carries the product version agrees, and on a release
# tag that the tag agrees with them.
#
#   ./scripts/check-version-lockstep.sh            # files agree with each other
#   ./scripts/check-version-lockstep.sh v0.8.3     # ...and with this tag
#
# Read-only. The product version lives in four files that nothing else ties
# together (AGENTS.md, "Product version files"), plus the lockfiles that
# record them. A tag pushed against stale files ships a Docker image and
# installers named for the tag whose contents report the old number. In CI
# the tag argument comes from GITHUB_REF_NAME on a v* ref. Runs in CI and in
# check-all.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

failures=0
declare -A found

# The four files the release process edits by hand.
found["src-tauri/Cargo.toml"]="$(sed -n 's/^version = "\(.*\)"$/\1/p' src-tauri/Cargo.toml | head -1)"
found["crates/server/server/Cargo.toml"]="$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/server/server/Cargo.toml | head -1)"
found["src-tauri/tauri.conf.json"]="$(sed -n 's/^  "version": "\(.*\)",$/\1/p' src-tauri/tauri.conf.json | head -1)"
found["web/package.json"]="$(sed -n 's/^  "version": "\(.*\)",$/\1/p' web/package.json | head -1)"

# The lockfiles that record those manifests. `npm ci` refuses a package-lock
# that disagrees with package.json, but `cargo build` rewrites a stale
# Cargo.lock without a word, so the Cargo entries are the ones worth checking.
found["Cargo.lock (message-crate-server)"]="$(awk '/^name = "message-crate-server"$/{getline; sub(/^version = "/, ""); sub(/"$/, ""); print; exit}' Cargo.lock)"
found["src-tauri/Cargo.lock (message-crate-desktop)"]="$(awk '/^name = "message-crate-desktop"$/{getline; sub(/^version = "/, ""); sub(/"$/, ""); print; exit}' src-tauri/Cargo.lock)"
found["web/package-lock.json"]="$(sed -n 's/^  "version": "\(.*\)",$/\1/p' web/package-lock.json | head -1)"

expected="${found["src-tauri/Cargo.toml"]}"
if [[ -z "${expected}" ]]; then
  echo "src-tauri/Cargo.toml: could not read a version" >&2
  exit 1
fi

for file in "${!found[@]}"; do
  if [[ -z "${found[$file]}" ]]; then
    echo "${file}: could not read a version" >&2
    failures=$((failures + 1))
  elif [[ "${found[$file]}" != "${expected}" ]]; then
    echo "${file}: version is ${found[$file]}, src-tauri/Cargo.toml says ${expected}" >&2
    failures=$((failures + 1))
  fi
done

# On a release tag, the tag is the number the artifacts are named for, so it
# has to be the number the files carry, and the changelog has to have a
# heading for it (AGENTS.md, "Ship a release", step 2).
tag="${1:-}"
if [[ -n "${tag}" ]]; then
  if [[ "${tag}" != v* ]]; then
    echo "tag ${tag} does not start with v" >&2
    failures=$((failures + 1))
  elif [[ "${tag#v}" != "${expected}" ]]; then
    echo "tag ${tag} does not match the product version ${expected} in the files" >&2
    failures=$((failures + 1))
  fi
  if ! grep -q "^## \[${tag#v}\] - [0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}$" CHANGELOG.md; then
    echo "CHANGELOG.md has no '## [${tag#v}] - YYYY-MM-DD' heading for ${tag}" >&2
    failures=$((failures + 1))
  fi
fi

if [[ "${failures}" -gt 0 ]]; then
  echo "version lockstep check failed (${failures} problem(s))" >&2
  exit 1
fi

if [[ -n "${tag}" ]]; then
  echo "product version ${expected} agrees across all files and with tag ${tag}"
else
  echo "product version ${expected} agrees across all files"
fi
