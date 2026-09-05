#!/usr/bin/env python3
"""Append a reproducible wall-time record for one command (without invoking a shell)."""

import argparse
import datetime
import json
import os
import platform
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", required=True, help="Cache/edit state, e.g. warm-unchanged-1")
    parser.add_argument("--output", type=Path, required=True, help="Append-only JSONL results")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        parser.error("a command is required after --")

    def version(command):
        return subprocess.check_output(command, text=True).strip()

    record = {
        "label": args.label,
        "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "command": command,
        "cwd": os.getcwd(),
        "platform": platform.platform(),
        "revision": version(["git", "rev-parse", "HEAD"]),
        "dirty": bool(version(["git", "status", "--porcelain"])),
        "rustc": version(["rustc", "--version"]),
        "environment": {key: os.environ[key] for key in (
            "CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "CARGO_BUILD_JOBS",
            "CARGO_MANIFEST_DIR", "RUSTUP_TOOLCHAIN", "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS", "NEXTEST_PROFILE", "NEXTEST_TEST_THREADS",
            "MIDENC_TEST_TIMINGS",
        ) if key in os.environ},
    }
    started = time.perf_counter()
    result = subprocess.run(command, check=False)
    record.update(elapsed_seconds=time.perf_counter() - started, exit_code=result.returncode)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("a", encoding="utf-8") as output:
        output.write(json.dumps(record) + "\n")
    print(json.dumps(record), flush=True)
    raise SystemExit(result.returncode if result.returncode >= 0 else 128 - result.returncode)


if __name__ == "__main__":
    main()
