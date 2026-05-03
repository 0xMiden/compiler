#!/usr/bin/env python3
"""Reduce cargo-llvm-cov JSON to a fuzza-oriented Markdown coverage report.

Usage:
    cov.py <json> <workspace-root> [--prev <json>] > report.md

When `--prev` is supplied (and points at an existing llvm-cov JSON), the
report includes a "Delta since previous run" section so the fuzz-case
agent can judge whether its last case actually added coverage.

The output is filtered to compiler crates only; sdk/, tests/, examples/,
dev tools, and trivial trait impls (`fmt`, `clone`, `drop`, …) are dropped.
"""
from __future__ import annotations

import argparse
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
DELTA_TOP_N = 15


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


def load_funcs(json_path: Path, workspace: Path) -> list[dict]:
    """Parse an llvm-cov JSON and return the filtered list of compiler-function records."""
    doc = json.loads(json_path.read_text())
    exports = doc.get("data") or []
    if not exports:
        return []
    prelim: list[dict] = []
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

    demangled = demangle_all([f["name"] for f in prelim])
    return [
        {**f, "name": dm}
        for f, dm in zip(prelim, demangled)
        if not BORING_NAME_RE.search(dm)
    ]


def emit_delta(current: list[dict], prev: list[dict], out) -> None:
    """Print the delta section comparing `current` against `prev`."""
    prev_by_key = {(f["file"], f["name"]): f for f in prev}

    prev_hit = sum(1 for f in prev if f["count"] > 0)
    prev_covered = sum(f["covered"] for f in prev)
    cur_hit = sum(1 for f in current if f["count"] > 0)
    cur_covered = sum(f["covered"] for f in current)

    newly_touched = sorted(
        (
            f for f in current
            if f["count"] > 0
            and prev_by_key.get((f["file"], f["name"]), {"count": 0})["count"] == 0
        ),
        key=lambda f: (-f["covered"], f["file"], f["line"]),
    )
    gained_regions = sorted(
        (
            {
                **f,
                "gain": f["covered"]
                - prev_by_key.get((f["file"], f["name"]), {"covered": 0})["covered"],
            }
            for f in current
        ),
        key=lambda f: -f["gain"],
    )
    gained_regions = [f for f in gained_regions if f["gain"] > 0]

    print("## Delta since previous run\n", file=out)
    print(f"- Functions touched: {cur_hit - prev_hit:+d} (now {cur_hit})", file=out)
    print(
        f"- Regions covered:   {cur_covered - prev_covered:+d} (now {cur_covered})",
        file=out,
    )
    print(
        f"- Newly-exercised functions: **{len(newly_touched)}**; "
        f"functions that gained regions: **{len(gained_regions)}**",
        file=out,
    )
    print(file=out)

    if newly_touched:
        print(f"### Newly-exercised functions (up to {DELTA_TOP_N})\n", file=out)
        print("| Regions hit | File:line | Function |", file=out)
        print("| --- | --- | --- |", file=out)
        for f in newly_touched[:DELTA_TOP_N]:
            print(
                f"| {f['covered']}/{f['regions']} | `{f['file']}:{f['line']}` | `{f['name']}` |",
                file=out,
            )
        print(file=out)

    if gained_regions:
        print(f"### Functions that gained new regions (up to {DELTA_TOP_N})\n", file=out)
        print("| Δ regions | Now hit | File:line | Function |", file=out)
        print("| --- | --- | --- | --- |", file=out)
        for f in gained_regions[:DELTA_TOP_N]:
            print(
                f"| +{f['gain']} | {f['covered']}/{f['regions']} | "
                f"`{f['file']}:{f['line']}` | `{f['name']}` |",
                file=out,
            )
        print(file=out)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("json", type=Path, help="current llvm-cov JSON")
    parser.add_argument("workspace", type=Path, help="workspace root")
    parser.add_argument(
        "--prev",
        type=Path,
        default=None,
        help="previous llvm-cov JSON; if present, emit a delta section",
    )
    args = parser.parse_args()

    workspace = args.workspace.resolve()
    current = load_funcs(args.json, workspace)
    if not current:
        sys.exit("cov.py: no coverage data for compiler crates")

    prev = (
        load_funcs(args.prev, workspace)
        if args.prev is not None and args.prev.is_file()
        else []
    )

    total_fns = len(current)
    hit_fns = sum(1 for f in current if f["count"] > 0)
    total_regions = sum(f["regions"] for f in current)
    covered_regions = sum(f["covered"] for f in current)

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

    if prev:
        emit_delta(current, prev, out)

    untouched = sorted(
        (f for f in current if f["count"] == 0),
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
            f for f in current
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
