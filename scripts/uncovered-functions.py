#!/usr/bin/env python3
"""List the functions no test calls, from cargo-llvm-cov's Cobertura report.

    uncovered-functions.py target/llvm-cov/cobertura.xml            # headline, per-file counts, full list
    uncovered-functions.py target/llvm-cov/cobertura.xml --summary  # headline and per-file counts only

scripts/coverage.sh runs this; it is not meant to be run on its own.

Function coverage is the number worth chasing in this repository, and what
somebody acts on is the list of functions that no test reaches, grouped by
file. cargo-llvm-cov's summary table gives a functions column per file but
never names the functions, and its JSON and lcov outputs carry mangled
symbol names. The Cobertura output is the one format it demangles, so this
reads that.

Each Cobertura method is one function with one line, whose hit count is the
number of times the function ran. The same function appears once per test
binary that linked it and once per generic instantiation, so hits are summed
by name. Closures ("{closure#0}") and shims are folded into the function
that contains them, and generic arguments are dropped, both the turbofish
("::<T>") and the ones on a self type or trait ("<Json<T> as
FromRequest<S>>"), so one source function is one entry. The count here is therefore smaller than the
functions column in the summary table, which counts every instantiation and
closure separately.
"""

import re
import sys
import xml.etree.ElementTree as ET
from collections import defaultdict

BRACE_COMPONENT = re.compile(r"::\{[^{}]*\}")


def strip_generic_args(name: str) -> str:
    """Remove every `::<...>` turbofish, honouring nested angle brackets."""
    out = []
    i = 0
    while i < len(name):
        if name.startswith("::<", i):
            depth = 0
            j = i + 2
            while j < len(name):
                if name[j] == "<":
                    depth += 1
                elif name[j] == ">" and name[j - 1] != "-":
                    # `->` in an `fn(..) -> T` argument is not a closer.
                    depth -= 1
                    if depth == 0:
                        break
                j += 1
            i = j + 1
        else:
            out.append(name[i])
            i += 1
    return "".join(out)


def strip_type_generics(name: str) -> str:
    """Remove the `<...>` after a type or trait name, keeping the `<Type as
    Trait>` wrapper of a qualified path: `<Json<T> as FromRequest<S>>::f`
    becomes `<Json as FromRequest>::f`, so one generic function is one entry
    however many types it was instantiated for."""
    out = []
    depth = 0
    for i, ch in enumerate(name):
        if depth:
            if ch == "<":
                depth += 1
            elif ch == ">" and name[i - 1] != "-":
                depth -= 1
            continue
        if ch == "<" and i > 0 and (name[i - 1].isalnum() or name[i - 1] == "_"):
            depth = 1
            continue
        out.append(ch)
    return "".join(out)


def function_key(name: str) -> str:
    name = strip_type_generics(strip_generic_args(name))
    while True:
        folded = BRACE_COMPONENT.sub("", name)
        if folded == name:
            return name
        name = folded


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    summary_only = "--summary" in sys.argv[2:]
    root = ET.parse(sys.argv[1]).getroot()

    # (file, function) -> [hits, first line]
    functions: dict[tuple[str, str], list[int]] = {}
    for cls in root.iter("class"):
        path = cls.get("filename", "")
        for method in cls.iter("method"):
            key = (path, function_key(method.get("name", "")))
            for line in method.iter("line"):
                hits = int(line.get("hits", "0"))
                number = int(line.get("number", "0"))
                entry = functions.setdefault(key, [0, number])
                entry[0] += hits
                entry[1] = min(entry[1], number)

    total_by_file: dict[str, int] = defaultdict(int)
    uncovered_by_file: dict[str, list[tuple[int, str]]] = defaultdict(list)
    for (path, name), (hits, line) in functions.items():
        total_by_file[path] += 1
        if hits == 0:
            uncovered_by_file[path].append((line, name))

    total = len(functions)
    uncovered = sum(len(v) for v in uncovered_by_file.values())
    called = total - uncovered
    pct = 100.0 * called / total if total else 0.0
    print(f"functions called by a test: {called} of {total} ({pct:.1f}%), {uncovered} never called")
    print()
    print("uncovered functions by file (uncovered / total):")
    ranked = sorted(uncovered_by_file.items(), key=lambda kv: (-len(kv[1]), kv[0]))
    width = max((len(p) for p, _ in ranked), default=0)
    for path, entries in ranked[: 20 if summary_only else len(ranked)]:
        print(f"  {path.ljust(width)}  {len(entries)} / {total_by_file[path]}")
    if summary_only:
        if len(ranked) > 20:
            print(f"  ... {len(ranked) - 20} more files; the full list is target/llvm-cov/uncovered-functions.txt")
        return 0

    print()
    for path, entries in sorted(uncovered_by_file.items()):
        print(path)
        for line, name in sorted(entries):
            print(f"  {line:>5}  {name}")
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
