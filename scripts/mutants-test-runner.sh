#!/usr/bin/env bash
# Runs one test binary under a memory cap. scripts/mutants.sh makes this
# Cargo's target runner, so nextest starts every test process through it;
# rustc and the linker are not limited.
#
# A mutant can turn a loop that grows a Vec into one that never ends (`i += 1`
# becomes `i *= 1` in a byte scanner). It fills the machine's memory long
# before the timeout stops it, and on a CI runner that kills the runner
# itself, losing every result in the shard. With the cap the allocation
# fails, the test aborts, and cargo-mutants counts the mutant as caught.
#
# The cap is on the data segment (heap and anonymous mmap), not on address
# space, so reserved but unused memory such as thread stacks does not count.
# Four tests run at once on a 16 GB runner; 3 GiB each leaves room for the
# build. MUTANTS_TEST_MEMORY_KB overrides it.
set -euo pipefail

ulimit -d "${MUTANTS_TEST_MEMORY_KB:-3145728}"
exec "$@"
