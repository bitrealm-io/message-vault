#!/usr/bin/env python3
"""Summarise one or more cargo-mutants runs as Markdown.

    mutants-summary.py target/mutants/mutants.out                 # headline, per-file table, every missed mutant
    mutants-summary.py shard-*/mutants.out --summary              # headline and per-file table only

scripts/mutants.sh and the Mutants workflow run this; it is not meant to be
run on its own.

A missed mutant is a change to the code that every test still passes: a
`<` turned into `<=`, a function made to return `Default::default()`, a
`&&` turned into `||`. That is what somebody acts on, so the report ends
with each missed mutant, grouped by file, in the form cargo-mutants prints
it (`file:line:col: replace X with Y in function`).

Several directories are merged into one report, which is how the workflow
joins its shards: each shard ran a disjoint slice of the same mutants, so
their outcomes add up. Every shard also runs the unmutated baseline; those
are not mutants and are left out.

The score is caught / (caught + missed + timeout). Unviable mutants, the
ones that do not compile, say nothing about the tests and are left out of
it. A timeout usually means the mutant made a loop run forever, which a
test run did notice, but it is listed apart so a slow test is not mistaken
for a strong one.
"""

import json
import sys
from collections import defaultdict
from pathlib import Path

KINDS = {
    "CaughtMutant": "caught",
    "MissedMutant": "missed",
    "Timeout": "timeout",
    "Unviable": "unviable",
}


def load(dirs):
    """Every mutant outcome in `dirs` as (file, kind, name)."""
    rows = []
    for d in dirs:
        path = Path(d) / "outcomes.json"
        if not path.is_file():
            print(f"warning: no outcomes.json in {d}", file=sys.stderr)
            continue
        for o in json.loads(path.read_text())["outcomes"]:
            scenario = o["scenario"]
            if not isinstance(scenario, dict) or "Mutant" not in scenario:
                continue
            m = scenario["Mutant"]
            kind = KINDS.get(o["summary"], "other")
            rows.append((m["file"], kind, m["name"]))
    return rows


def score(counts):
    tested = counts["caught"] + counts["missed"] + counts["timeout"]
    return f"{100 * counts['caught'] / tested:.0f}%" if tested else "–"


def main(argv):
    summary_only = "--summary" in argv
    dirs = [a for a in argv if a != "--summary"]
    if not dirs:
        print(__doc__, file=sys.stderr)
        return 1
    rows = load(dirs)
    if not rows:
        print("No mutant outcomes found.")
        return 0

    total = defaultdict(int)
    by_file = defaultdict(lambda: defaultdict(int))
    missed = defaultdict(list)
    timeouts = defaultdict(list)
    for file, kind, name in rows:
        total[kind] += 1
        by_file[file][kind] += 1
        if kind == "missed":
            missed[file].append(name)
        elif kind == "timeout":
            timeouts[file].append(name)

    print(
        f"**{score(total)}** of the mutants that compiled were caught: "
        f"{total['caught']} caught, {total['missed']} missed, "
        f"{total['timeout']} timed out, {total['unviable']} unviable, "
        f"{len(rows)} in all."
    )
    print()
    print("| File | Caught | Missed | Timeout | Unviable | Score |")
    print("|---|---:|---:|---:|---:|---:|")
    for file in sorted(by_file, key=lambda f: (-by_file[f]["missed"], f)):
        c = by_file[file]
        print(
            f"| `{file}` | {c['caught']} | {c['missed']} | {c['timeout']} "
            f"| {c['unviable']} | {score(c)} |"
        )

    if summary_only:
        return 0
    for title, groups in (("Missed mutants", missed), ("Timed out", timeouts)):
        if not groups:
            continue
        print()
        print(f"## {title}")
        for file in sorted(groups):
            print()
            print(f"### `{file}`")
            print()
            for name in sorted(groups[file], key=line_of):
                print(f"- `{name.removeprefix(file + ':')}`")
    return 0


def line_of(name):
    """Sort key: the line and column in `file:line:col: ...`."""
    parts = name.split(":")
    try:
        return (int(parts[1]), int(parts[2]))
    except (IndexError, ValueError):
        return (0, 0)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
