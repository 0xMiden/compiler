#!/usr/bin/env python3
"""Reduce cargo-llvm-cov JSON to a fuzza-oriented Markdown coverage report.

Usage:
    cov.py <llvm-cov-json-path> <workspace-root> > report.md

The output is designed to be handed to the fuzza case-generation agent: it
highlights compiler functions that the current fuzz corpus has not yet
exercised (or has covered poorly), ignoring crates that aren't "compiler"
code (sdk, tests, examples, dev tools, etc.) and skipping trivial impls.
"""
from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

# Workspace-relative directory prefixes that contain the compiler code we want
# the fuzzer to grow coverage in. Everything else is ignored.
COMPILER_PREFIXES: tuple[str, ...] = (
    "codegen/",
    "dialects/",
    "eval/",
    "frontend/",
    "hir/",
    "hir-analysis/",
    "hir-macros/",
    "hir-symbol/",
    "hir-transform/",
    "midenc/",
    "midenc-compile/",
    "midenc-driver/",
    "midenc-log/",
    "midenc-session/",
    "tools/cargo-miden/",
)

# Function-name patterns that are uninteresting targets (boilerplate traits,
# diagnostic formatting, etc.). Matched against the demangled name.
BORING_NAME_RE = re.compile(
    r"""
    ::fmt$
    |::clone$
    |::clone_from$
    |::default$
    |::drop$
    |::deserialize$
    |::serialize$
    |::eq$
    |::ne$
    |::hash$
    |::partial_cmp$
    |::cmp$
    |^<.*\ as\ core::convert::(From|Into)<
    """,
    re.VERBOSE,
)

TOP_N = 30


def classify(path: str, workspace: Path) -> str | None:
    """Return the workspace-relative path if `path` is inside a compiler crate, else None."""
    try:
        rel = Path(path).resolve().relative_to(workspace)
    except (ValueError, OSError):
        return None
    s = rel.as_posix()
    return s if any(s.startswith(p) for p in COMPILER_PREFIXES) else None


def pct(a: int, b: int) -> str:
    return f"{100.0 * a / b:.1f}%" if b else "n/a"


def demangle_all(names: list[str]) -> list[str]:
    """Batch-demangle Rust symbols via `rustfilt` if available, else return as-is."""
    if not names or shutil.which("rustfilt") is None:
        return names
    proc = subprocess.run(
        ["rustfilt"],
        input="\n".join(names),
        capture_output=True,
        text=True,
        check=True,
    )
    out = proc.stdout.rstrip("\n").split("\n")
    return out if len(out) == len(names) else names


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit("usage: cov.py <llvm-cov-json> <workspace-root>")
    json_path = Path(sys.argv[1])
    workspace = Path(sys.argv[2]).resolve()

    doc = json.loads(json_path.read_text())
    exports = doc.get("data") or []
    if not exports:
        sys.exit("cov.py: no coverage data in JSON")

    # First pass: keep only entries from compiler files (names still mangled).
    prelim = []
    for raw in exports[0].get("functions", []):
        filenames = raw.get("filenames") or []
        if not filenames:
            continue
        rel = classify(filenames[0], workspace)
        if rel is None:
            continue
        regions = raw.get("regions", [])
        # llvm-cov region tuple: [line_start, col_start, line_end, col_end,
        #                        exec_count, file_id, expanded_file_id, kind]
        covered = sum(1 for r in regions if r[4] > 0)
        prelim.append({
            "name": raw.get("name", ""),
            "file": rel,
            "line": regions[0][0] if regions else 0,
            "count": raw.get("count", 0),
            "regions": len(regions),
            "covered": covered,
        })

    # Second pass: batch-demangle and drop boring names.
    demangled = demangle_all([f["name"] for f in prelim])
    funcs = [
        {**f, "name": dm}
        for f, dm in zip(prelim, demangled)
        if not BORING_NAME_RE.search(dm)
    ]

    total_fns = len(funcs)
    hit_fns = sum(1 for f in funcs if f["count"] > 0)
    total_regions = sum(f["regions"] for f in funcs)
    covered_regions = sum(f["covered"] for f in funcs)

    out = sys.stdout
    print("# fuzza coverage report (compiler crates)\n", file=out)
    print(
        f"- Functions touched: **{hit_fns}/{total_fns}** ({pct(hit_fns, total_fns)})",
        file=out,
    )
    print(
        f"- Regions covered:   **{covered_regions}/{total_regions}** "
        f"({pct(covered_regions, total_regions)})",
        file=out,
    )
    print(file=out)

    untouched = sorted(
        (f for f in funcs if f["count"] == 0),
        key=lambda f: (-f["regions"], f["file"], f["line"]),
    )
    print(f"## Top untouched functions (by size, up to {TOP_N})\n", file=out)
    print("| Regions | File:line | Function |", file=out)
    print("| --- | --- | --- |", file=out)
    for f in untouched[:TOP_N]:
        print(f"| {f['regions']} | `{f['file']}:{f['line']}` | `{f['name']}` |", file=out)
    if not untouched:
        print("| _none_ | | |", file=out)
    print(file=out)

    cold = sorted(
        (
            f for f in funcs
            if f["count"] > 0 and f["regions"] > 4 and f["covered"] * 2 < f["regions"]
        ),
        key=lambda f: (-(f["regions"] - f["covered"]), f["file"], f["line"]),
    )
    print(
        f"## Partially-covered functions — <50% regions hit (up to {TOP_N})\n",
        file=out,
    )
    print("| Uncovered | Regions | File:line | Function |", file=out)
    print("| --- | --- | --- | --- |", file=out)
    for f in cold[:TOP_N]:
        uncov = f["regions"] - f["covered"]
        print(
            f"| {uncov} | {f['regions']} | `{f['file']}:{f['line']}` | `{f['name']}` |",
            file=out,
        )
    if not cold:
        print("| _none_ | | | |", file=out)


if __name__ == "__main__":
    main()
