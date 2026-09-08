#!/usr/bin/env bash
# Test coverage for the Rust workspace, measured by cargo-llvm-cov.
#
#   ./scripts/coverage.sh          # summary table on stdout, reports under target/llvm-cov/
#   ./scripts/coverage.sh --open   # the same, then open the HTML report in a browser
#
# Function coverage is the number worth chasing here, so the last thing
# printed is the count of functions no test calls and the files with the
# most of them; target/llvm-cov/uncovered-functions.txt names every one
# with its line. Line coverage is in the table and the HTML report but is
# not a target. Also written: target/llvm-cov/html/index.html (per-file,
# per-line view), target/llvm-cov/lcov.info (for editor plugins),
# target/llvm-cov/summary.txt (the table) and functions.txt (the headline).
#
# Needs cargo-llvm-cov (`cargo install cargo-llvm-cov --locked`), the
# llvm-tools rustup component, which rust-toolchain.toml installs, and
# python3 for scripts/uncovered-functions.py. The
# instrumented build lives under target/llvm-cov-target, apart from plain
# `cargo build` output, so the first run compiles the workspace from scratch.
# The workspace always runs on SQLite. Export MV_TEST_POSTGRES_URL and the
# server crate runs a second time on Postgres, so the Postgres-gated suites
# count too; both passes feed one report. ffmpeg on PATH matters for the
# same reason: the transcode and media tests skip themselves without it.
# src-tauri is not a workspace member and is not measured; its commands are
# thin wrappers over the exporter and push/pull crates, which are.
# Test code itself (tests/ directories and the <module>/tests.rs files) is
# left out of the numbers. Coverage is a report, never a gate:
# docs/adr/0007-ci-is-the-only-gate.md. The Coverage workflow runs this
# same script on every push to main and keeps the reports as artifacts.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo "cargo-llvm-cov is not installed: cargo install cargo-llvm-cov --locked" >&2
  exit 1
fi

OUT="target/llvm-cov"
IGNORE='(^|/)tests/|/tests\.rs$'
mkdir -p "${OUT}"

echo "==> cargo llvm-cov (workspace tests on SQLite, instrumented)"
env -u MV_TEST_POSTGRES_URL cargo llvm-cov --workspace --no-report --ignore-filename-regex "${IGNORE}"

if [[ -n "${MV_TEST_POSTGRES_URL:-}" ]]; then
  echo "==> cargo llvm-cov (server tests on Postgres, same profile)"
  cargo llvm-cov -p message-vault-server --no-report --ignore-filename-regex "${IGNORE}"
fi

echo "==> reports"
cargo llvm-cov report --ignore-filename-regex "${IGNORE}" --lcov --output-path "${OUT}/lcov.info"
cargo llvm-cov report --ignore-filename-regex "${IGNORE}" --html --output-dir "${OUT}"
cargo llvm-cov report --ignore-filename-regex "${IGNORE}" | tee "${OUT}/summary.txt"

# Cobertura is the one output cargo-llvm-cov demangles, which is what the
# uncovered-functions list needs; its JSON and lcov carry mangled names.
echo "==> functions no test calls"
cargo llvm-cov report --ignore-filename-regex "${IGNORE}" --cobertura --output-path "${OUT}/cobertura.xml"
python3 "${SCRIPT_DIR}/uncovered-functions.py" "${OUT}/cobertura.xml" > "${OUT}/uncovered-functions.txt"
python3 "${SCRIPT_DIR}/uncovered-functions.py" "${OUT}/cobertura.xml" --summary | tee "${OUT}/functions.txt"

echo "HTML report: ${OUT}/html/index.html"
echo "Uncovered functions: ${OUT}/uncovered-functions.txt"
if [[ "${1:-}" == "--open" ]]; then
  cargo llvm-cov report --ignore-filename-regex "${IGNORE}" --html --output-dir "${OUT}" --open
fi
