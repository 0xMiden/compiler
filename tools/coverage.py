#!/usr/bin/env python3
"""Merge coverage from Rust tests and the compiler tools executed by lit."""

import argparse
import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parents[1]
TOOL_PACKAGES = ("midenc", "cargo-miden", "midenc-hir-opt", "miden-objtool")


def parse_environment(output):
    environment = {}
    for line in output.splitlines():
        if not line.strip():
            continue
        assignments = shlex.split(line.removeprefix("export "))
        if len(assignments) != 1 or "=" not in assignments[0]:
            raise ValueError(f"unexpected cargo llvm-cov environment assignment: {line!r}")
        name, value = assignments[0].split("=", 1)
        environment[name] = value
    return environment


def run(command, environment, *, stdout=None):
    print("+ " + shlex.join(str(argument) for argument in command), flush=True)
    return subprocess.run(command, cwd=WORKSPACE, env=environment, stdout=stdout, check=False)


def collect(args):
    output = args.output_path.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.unlink(missing_ok=True)
    coverage_dir = args.coverage_dir.resolve()
    target_dir = coverage_dir / "target"
    store_dir = target_dir / "nextest"
    bin_dir = coverage_dir / "bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    toolchain = active_toolchain()

    nextest_config_path = store_dir / "coverage-nextest.toml"
    nextest_config_path.parent.mkdir(parents=True, exist_ok=True)
    nextest_config_path.write_text(
        f'[store]\ndir = {json.dumps(str(store_dir), ensure_ascii=False)}\n',
        encoding="utf-8",
    )
    environment["RUSTUP_TOOLCHAIN"] = toolchain
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    environment["CARGO_LLVM_COV_TARGET_DIR"] = str(target_dir)
    environment["CARGO_BUILD_BUILD_DIR"] = str(target_dir)
    environment["CARGO_LLVM_COV_BUILD_DIR"] = str(target_dir)
    environment["RUST_MIN_STACK"] = environment.get("RUST_MIN_STACK", "16777216")
    variables = subprocess.check_output(
        ["cargo", "llvm-cov", "show-env", "--sh", "--coverage-host-only"],
        cwd=WORKSPACE,
        env=environment,
        text=True,
    )
    environment.update(parse_environment(variables))
    profile_pattern = environment["LLVM_PROFILE_FILE"]
    if "%p" not in profile_pattern or not re.search(r"%\d*m", profile_pattern):
        raise ValueError("coverage profiles must have a process-unique filename")
    clean = run(["cargo", "llvm-cov", "clean", "--workspace"], environment)
    if clean.returncode:
        return clean.returncode

    environment["MIDENC_BIN_DIR"] = str(bin_dir)
    environment["PATH"] = str(bin_dir) + os.pathsep + environment.get("PATH", "")
    build = ["cargo", "-Z", "unstable-options", "build", "--locked", "--bins"]
    for package in TOOL_PACKAGES:
        build.extend(["-p", package])
    build.extend(["--artifact-dir", str(bin_dir)])
    status = run(build, environment).returncode
    if not status:
        if not args.skip_rust:
            tests = ["cargo", "nextest", "run", "--locked", "--all-targets", "--no-fail-fast", "--tool-config-file", f"coverage:{nextest_config_path}"]
            if args.package:
                for package in args.package:
                    tests.extend(["-p", package])
            else:
                tests.append("--workspace")
            if args.nextest_filter:
                tests.extend(["-E", args.nextest_filter])
            status = run(tests, environment).returncode
        if not args.skip_lit:
            lit = ["litcheck", "lit", "run", "--verbose", "--path", str(bin_dir)]
            if args.lit_filter:
                lit.extend(["--filter", args.lit_filter])
            lit.append(str(WORKSPACE / "tests" / "lit"))
            environment["CARGO_MAKE_RUSTUP_TOOLCHAIN_NAME"] = toolchain
            lit_status = run(lit, environment).returncode
            status = status or lit_status

    report = run(
        ["cargo", "llvm-cov", "report", "--cobertura", "--output-path", str(output)],
        environment,
    )
    return status or report.returncode


def active_toolchain():
    return subprocess.check_output(
        ["rustup", "show", "active-toolchain"], cwd=WORKSPACE, text=True
    ).split()[0]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--coverage-dir", type=Path, default=WORKSPACE / "target" / "compiler-coverage")
    parser.add_argument("--output-path", type=Path, default=WORKSPACE / "rust-coverage.xml")
    parser.add_argument("--package", action="append", help="Limit Rust tests to a package; repeatable")
    parser.add_argument("--nextest-filter", help="Limit Rust tests with a nextest filter expression")
    parser.add_argument("--lit-filter", help="Limit lit tests with a regular expression")
    parser.add_argument("--skip-rust", action="store_true", help="Collect only lit coverage")
    parser.add_argument("--skip-lit", action="store_true", help="Collect only Rust test coverage")
    args = parser.parse_args(argv)
    if args.skip_rust and args.skip_lit:
        parser.error("at least one test suite must run")
    try:
        return collect(args)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"coverage: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
