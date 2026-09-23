#!/usr/bin/env bash
# Mutation testing for the Rust workspace, run by cargo-mutants.
#
#   ./scripts/mutants.sh                                        # the whole workspace, as .cargo/mutants.toml sets it
#   ./scripts/mutants.sh --file crates/libs/phone/src/lib.rs    # one file
#   ./scripts/mutants.sh --shard 0/40 --sharding round-robin    # one slice, as the workflow runs it
#
# cargo-mutants changes the code one small way at a time and runs the tests
# for the package that holds it. A mutant every test still passes is
# "missed": a change in behaviour no test notices. That list is the finding,
# so the last thing printed is the per-file table, and
# target/mutants/summary.md names every missed mutant with its line.
# cargo-mutants' own output (a log and a diff per mutant) is under
# target/mutants/mutants.out/.
#
# What is left out is set in .cargo/mutants.toml, and why. Other arguments
# go to `cargo mutants` as they are; `--file` mutates only that file. A full
# run is about 8,750 mutants: seconds each in most library crates, about 75 s
# each on a CI runner in the server crate (about 3,450 of them), so about 100
# hours on one machine. Point it at the file you changed; the Mutants
# workflow, started by hand, splits the full run across 40 parallel shards.
#
# The server tests run on SQLite, so a mutant in a Postgres-only branch
# shows as missed. ffmpeg on PATH matters: without it the transcode and
# media tests skip locally (and fail when CI is set), so every mutant they
# would have caught shows as missed.
#
# Needs cargo-mutants and cargo-nextest (`cargo install cargo-mutants
# cargo-nextest --locked`; .cargo/mutants.toml says why nextest) and python3
# for scripts/mutants-summary.py. Missed mutants do not fail this script:
# mutation testing is a report, never a gate
# (docs/adr/0007-ci-is-the-only-gate.md). It fails only when cargo-mutants
# itself does, for example when the unmutated tests fail.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

if ! cargo mutants --version >/dev/null 2>&1; then
  echo "cargo-mutants is not installed: cargo install cargo-mutants --locked" >&2
  exit 1
fi
if ! cargo nextest --version >/dev/null 2>&1; then
  echo "cargo-nextest is not installed: cargo install cargo-nextest --locked" >&2
  exit 1
fi

OUT="target/mutants"
mkdir -p "${OUT}"

# A --file on the command line would be added to the config's list, not
# replace it; read the config's other settings from the command line instead.
CONFIG=()
for arg in "$@"; do
  if [[ "${arg}" == "--file" || "${arg}" == -f || "${arg}" == --file=* ]]; then
    CONFIG=(--no-config --gitignore true --timeout-multiplier 3 --test-tool nextest)
  fi
done

echo "==> cargo mutants"
status=0
env -u MV_TEST_POSTGRES_URL cargo mutants "${CONFIG[@]}" --output "${OUT}" "$@" || status=$?
# 0: every mutant caught. 2: some missed. 3: some timed out. All three are
# results to report; anything else means the run itself went wrong.
if [[ ${status} -ne 0 && ${status} -ne 2 && ${status} -ne 3 ]]; then
  echo "cargo mutants failed (exit ${status}); see ${OUT}/mutants.out/log/" >&2
  exit "${status}"
fi

echo "==> summary"
python3 "${SCRIPT_DIR}/mutants-summary.py" "${OUT}/mutants.out" > "${OUT}/summary.md"
python3 "${SCRIPT_DIR}/mutants-summary.py" "${OUT}/mutants.out" --summary
echo
echo "Missed mutants: ${OUT}/summary.md"
