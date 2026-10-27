#!/usr/bin/env python3
"""Summarize static preparation choices from complete codegen physical-plan traces.

Use MIDENC_TRACE='codegen:^$=trace' and retain successful nextest output. Counts
are compiler encounters, never runtime frequencies. Identical complete planning
captures are deduplicated only within a workload; a hash is not proof that native
lowering bodies or final machine code are identical. Tests that reuse cached
packages are listed with no captures, rather than assigned a fabricated histogram.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

START = re.compile(r'physical-plan identity-utf8=([0-9a-f]+)$')
TEST = re.compile(r'\b(?:PASS|FAIL) \[.*?\]\s+\([^)]*\)\s+(.+)$')
METRICS = re.compile(r'actions=(\d+) cost=(\d+) loads=(\d+) stores=(\d+)$')
TIERS = {"Stack", "Loads", "Selective", "Normalization", "Aliases"}


def _add(totals, records):
    for domain, tier, actions, cost, loads, stores in records:
        entry = totals.setdefault(domain, {}).setdefault(tier, dict.fromkeys(
            ["count", "zero_actions", "actions", "cost", "loads", "stores"], 0))
        for key, amount in [("count", 1), ("zero_actions", int(actions == 0)),
                            ("actions", actions), ("cost", cost), ("loads", loads), ("stores", stores)]:
            entry[key] += amount


def _records(lines):
    records = []
    pending_unit = False
    for line in lines:
        if line.startswith("unplanned-unit"):
            if not re.fullmatch(r'unplanned-unit s\d+ kind=\d+ source=\d+', line):
                raise ValueError(f"invalid unplanned unit header: {line}")
            continue
        if line.startswith("unit"):
            if not re.fullmatch(r'unit s\d+ kind=\d+ source=\d+', line):
                raise ValueError(f"invalid unit header: {line}")
            if pending_unit:
                raise ValueError("unit missing preparation metrics")
            pending_unit = True
        domain = None
        if line.startswith("tier"):
            if not pending_unit:
                raise ValueError("unit preparation without unit")
            pending_unit = False
            domain = "unit"
        elif line.startswith("control-preparation"):
            domain = "control"
        elif line.startswith("edge"):
            domain = "edge"
        if domain is None:
            continue
        metrics = METRICS.search(line)
        if (domain == "control" and not re.match(r'control-preparation e\d+ tier=', line)) or (domain == "edge" and not re.match(r'edge e\d+ edge-traffic ', line)):
            raise ValueError(f"invalid {domain} record: {line}")
        if metrics is None:
            raise ValueError(f"invalid preparation metrics: {line}")
        actions, cost, loads, stores = map(int, metrics.groups())
        if loads + stores > actions or (actions == 0 and cost != 0):
            raise ValueError(f"inconsistent preparation metrics: {line}")
        if domain == "edge":
            tier = "Transfer"
        else:
            match = re.search(r'\btier=(\w+)\b', line)
            if match is None or match[1] not in TIERS:
                raise ValueError(f"unknown preparation class: {line}")
            tier = match[1]
            if domain == "control" and tier == "Aliases":
                raise ValueError("control cannot publish aliases")
        records.append((domain, tier, actions, cost, loads, stores))
    if pending_unit:
        raise ValueError("unit missing preparation metrics")
    return records


def analyze(text, workload="standalone"):
    """Return raw encounters and distinct complete captures per nextest test or caller label.

    Reject partial/new-format records rather than silently publishing undercounts.
    Timestamps and logger prefixes outside the delimited capture are not identity.
    """
    groups = {}
    active = None

    def group():
        return groups.setdefault(workload, {"raw": {}, "distinct": {}, "captures": []})

    for number, original in enumerate(text.splitlines(), 1):
        line = original.strip()
        test = TEST.search(line)
        start = START.search(line)
        if test and active is None:
            workload = test[1]
            group()
        if "physical-plan identity" in line and start is None:
            raise ValueError(f"invalid physical-plan identity at line {number}")
        if start:
            if active is not None:
                raise ValueError(f"nested physical plan at line {number}")
            active = (bytes.fromhex(start[1]).decode("utf-8"), [])
        elif line == "physical-plan-end":
            if active is None:
                raise ValueError(f"unmatched physical-plan-end at line {number}")
            identity, lines = active
            records = _records(lines)
            digest = hashlib.sha256(json.dumps([identity, lines], separators=(",", ":")).encode()).hexdigest()
            current = group()
            _add(current["raw"], records)
            capture = next((item for item in current["captures"] if item["sha256"] == digest), None)
            if capture is None:
                capture = {"identity": identity, "sha256": digest, "encounters": 0}
                current["captures"].append(capture)
                _add(current["distinct"], records)
            capture["encounters"] += 1
            active = None
        elif active is not None:
            active[1].append(line)
    if active is not None:
        raise ValueError("incomplete physical plan at end of input")
    return groups


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path, help="captured codegen log (successful output included)")
    parser.add_argument("--workload", default="standalone", help="label for a non-nextest capture")
    args = parser.parse_args()
    raw = args.trace.read_bytes()
    try:
        groups = analyze(raw.decode(), args.workload)
        if not any(group["captures"] for group in groups.values()):
            raise ValueError("no complete physical-plan captures; enable tracing and successful test output")
    except (ValueError, UnicodeError) as error:
        parser.error(str(error))
    print(json.dumps({"measurement": "static compiler encounters; not execution-weighted",
                      "trace_sha256": hashlib.sha256(raw).hexdigest(), "workloads": groups}, indent=2))


if __name__ == "__main__":
    main()
