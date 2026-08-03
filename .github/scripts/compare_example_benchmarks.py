#!/usr/bin/env python3
"""Compare compiler example benchmark results and render a PR report."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Any


def load_report(path: Path) -> dict[str, Any]:
    with path.open() as file:
        report = json.load(file)
    if report.get("schema_version") != 1:
        raise ValueError(f"unsupported benchmark schema in {path}")
    if not isinstance(report.get("benchmarks"), list):
        raise ValueError(f"benchmark list is missing from {path}")
    return report


def format_value(value: int | None, suffix: str = "") -> str:
    return "n/a" if value is None else f"{value:,}{suffix}"


def format_delta(current: int | None, baseline: int | None) -> str:
    if current is None or baseline is None:
        return "n/a"
    if baseline == 0:
        return "~0%" if current == 0 else "n/a"
    change = (current - baseline) / baseline * 100
    if round(change, 2) == 0:
        return "~0%"
    emoji = "✅" if change < 0 else "❌"
    return f"{emoji} {change:+.2f}%"


def format_measurement(current: int | None, baseline: int | None, suffix: str = "") -> str:
    value = format_value(current, suffix)
    if current is None:
        return value
    return f"{value} ({format_delta(current, baseline)})"


def render_report(current: dict[str, Any], baseline: dict[str, Any]) -> str:
    baseline_by_name = {
        benchmark["name"]: benchmark for benchmark in baseline["benchmarks"]
    }
    rows = []
    for benchmark in current["benchmarks"]:
        previous = baseline_by_name.get(benchmark["name"], {})
        rows.append(
            "| "
            + " | ".join(
                [
                    str(benchmark["name"]),
                    format_measurement(
                        benchmark.get("cycles"), previous.get("cycles")
                    ),
                    format_measurement(
                        benchmark.get("mast_size"),
                        previous.get("mast_size"),
                        "B",
                    ),
                ]
            )
            + " |"
        )

    current_commit = str(current.get("commit", "unknown"))[:12]
    baseline_commit = str(baseline.get("commit", "unknown"))[:12]
    return "\n".join(
        [
            "## Miden examples benchmark",
            "",
            f"Candidate `{current_commit}` compared with `next` `{baseline_commit}`. Lower is better.",
            "",
            "| example | VM cycles (vs next) | MAST size (vs next) |",
            "| --- | ---: | ---: |",
            *rows,
            "",
            "SVG flamegraphs and compiled packages are attached to the workflow run.",
            "",
        ]
    )


def append_step_summary(report: str) -> None:
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as file:
            file.write(report)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("current", type=Path)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    report = render_report(load_report(args.current), load_report(args.baseline))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(report)
    print(report, end="")
    append_step_summary(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
