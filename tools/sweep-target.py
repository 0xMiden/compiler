#!/usr/bin/env python3
"""Garbage-collect Cargo target directories with a mark-and-sweep pass.

Cargo keys intermediate artifacts by a metadata hash and never deletes them:
when an edit to the dependency graph gives a unit a new hash, the old
artifacts stay on disk forever. No local record separates a dead unit from a
live one — Cargo refreshes its `invoked.timestamp` bookkeeping only when it
compiles a unit, and grouping units by their fingerprint configuration cannot
work either, because one crate legitimately keeps several live variants whose
fingerprints are identical (each top-level invocation unifies transitive
features differently).

The one reliable source for the live set is Cargo itself: with
`--message-format=json` it emits an artifact message for every unit of an
invocation, including the fresh ones. This tool therefore:

1. MARKS: replays the workspace's build and test invocations in JSON mode and
   collects every artifact hash they report. On a warm tree — right after a
   test run — this compiles nothing and takes seconds.
2. PROTECTS, with one of two age guards per profile directory:
   - A directory that holds marked units belongs to the replayed graphs, so
     an unmarked unit there is almost always dead; it survives only `--age`
     days (default 1 — with agent-driven editing the dead units can double a
     target directory in a single day). The exceptions this guard covers are
     the checker layers of IDEs and clippy, which Cargo rebuilds cheaply
     when one is swept.
   - A directory with no marked units (the nested caches only test runs
     populate, the release profile) cannot be told apart from garbage at
     all, and its live units cannot refresh their timestamps without a
     recompile. A short age would force a full rebuild of such a cache per
     window, so these keep `--cache-age` days (default 14).
3. SWEEPS: deletes the `deps/`, `build/`, and `.fingerprint/` entries of
   every unit that is neither marked nor protected. Incremental caches and
   uplifted final artifacts are never touched.

A profile directory whose Cargo lock is held by a running build is skipped.

Usage: sweep-target.py [--dry-run] [--age DAYS] [--cache-age DAYS] ROOT [ROOT...]
"""

import fcntl
import json
import os
import re
import shutil
import subprocess
import sys
import time

# The workspace invocations whose unit graphs define the live set of the
# workspace's own profile directories. These are the graphs `cargo make`
# builds and tests with; each runs with `--message-format=json` appended.
MARK_INVOCATIONS = (
    ["cargo", "build", "-p", "midenc"],
    ["cargo", "build", "-p", "cargo-miden"],
    ["cargo", "build", "-p", "midenc-hir-opt"],
    ["cargo", "build", "-p", "miden-objtool"],
    ["cargo", "test", "--no-run", "--workspace"],
)

# Subdirectories of a profile directory that hold build output. The scan must
# not descend into them when it looks for more profile directories.
OUTPUT_DIRS = (".fingerprint", "deps", "build", "incremental", "examples", "tmp")

HASH_RE = re.compile(r"-([0-9a-f]{16,20})$")


def unit_hash(name):
    """Return the metadata hash carried by an artifact or directory name."""
    match = HASH_RE.search(name.split(".", 1)[0])
    return match.group(1) if match else None


def mark_live_units(workspace_root):
    """Replay the workspace invocations and collect the live unit hashes.

    Returns the set of hashes, or None when an invocation fails — the caller
    must not sweep with an incomplete live set.
    """
    live = set()
    for invocation in MARK_INVOCATIONS:
        result = subprocess.run(
            invocation + ["--message-format=json"],
            cwd=workspace_root,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        if result.returncode != 0:
            print(f"mark failed, not sweeping: {' '.join(invocation)}", file=sys.stderr)
            return None
        for line in result.stdout.splitlines():
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                continue
            # Harvest the hash from every path component: a library carries it
            # in the file name, but a build-script executable is reported as a
            # hashless `.../build/<crate>-<hash>/build-script-build`, and a
            # build-script run only as its `.../build/<crate>-<hash>/out`.
            paths = list(message.get("filenames", []))
            if message.get("out_dir"):
                paths.append(message["out_dir"])
            for path in paths:
                for component in path.split(os.sep):
                    marked = unit_hash(component)
                    if marked:
                        live.add(marked)
    return live


def profile_dirs(root):
    """Yield every directory under `root` that holds a `.fingerprint` table."""
    for dirpath, dirnames, _ in os.walk(root):
        if ".fingerprint" in dirnames:
            yield dirpath
        dirnames[:] = [d for d in dirnames if d not in OUTPUT_DIRS]


def try_lock(profile_dir):
    """Take the profile directory's Cargo lock without blocking.

    Returns the open file object that holds the lock, or None when a running
    build holds it. The caller must keep the object alive while it deletes.
    """
    for name in (".cargo-lock", ".cargo-build-lock"):
        path = os.path.join(profile_dir, name)
        if os.path.exists(path):
            handle = open(path)
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError:
                handle.close()
                return None
            return handle
    return open(os.devnull)


def tree_size(path):
    """Return the total apparent size of `path` in bytes."""
    if not os.path.isdir(path):
        return os.lstat(path).st_size
    total = 0
    for dirpath, _, filenames in os.walk(path):
        for name in filenames:
            try:
                total += os.lstat(os.path.join(dirpath, name)).st_size
            except OSError:
                pass
    return total


def remove(path, dry_run):
    """Delete `path` and return the bytes it held."""
    size = tree_size(path)
    if not dry_run:
        if os.path.isdir(path):
            shutil.rmtree(path, ignore_errors=True)
        else:
            os.unlink(path)
    return size


def dead_units(profile_dir, root, live, marked_cutoff, cache_cutoff):
    """Return (dead hash set, cutoff) for one profile directory.

    The directory's tier decides which age applies. The marked tier — the
    short `marked_cutoff` — requires both signals: the profile directory
    sits directly under the swept root (the workspace's own profile
    directories, the only ones whose graphs the marks replay), and the
    marks cover a meaningful share of its units. Everything nested (the
    shared build cache, per-test target directories) is cache tier with
    `cache_cutoff`: its live units are not enumerable, they cannot refresh
    their timestamps without a recompile, and hash overlap with the
    workspace graphs is common there (fixture builds share path
    dependencies) without implying the marks cover the directory.
    """
    fingerprint_dir = os.path.join(profile_dir, ".fingerprint")
    unmarked = []
    marked_count = 0
    total = 0
    for unit in os.listdir(fingerprint_dir):
        found = unit_hash(unit)
        if not found:
            continue
        total += 1
        if found in live:
            marked_count += 1
            continue
        unit_dir = os.path.join(fingerprint_dir, unit)
        newest = os.lstat(unit_dir).st_mtime
        stamp = os.path.join(unit_dir, "invoked.timestamp")
        if os.path.exists(stamp):
            newest = max(newest, os.lstat(stamp).st_mtime)
        unmarked.append((found, newest))
    at_root = os.path.dirname(os.path.abspath(profile_dir)) == os.path.abspath(root)
    covered = at_root and marked_count >= max(8, total // 20)
    cutoff = marked_cutoff if covered else cache_cutoff
    return {found for found, newest in unmarked if newest < cutoff}, cutoff


def sweep_profile_dir(profile_dir, root, live, marked_cutoff, cache_cutoff, dry_run):
    """Sweep one profile directory. Returns (bytes, unit count)."""
    dead, cutoff = dead_units(profile_dir, root, live, marked_cutoff, cache_cutoff)
    freed = 0
    for subdir in ("deps", "build", ".fingerprint"):
        path = os.path.join(profile_dir, subdir)
        if not dead:
            break
        if not os.path.isdir(path):
            continue
        for name in os.listdir(path):
            if unit_hash(name) in dead:
                freed += remove(os.path.join(path, name), dry_run)
    # Incremental caches are keyed separately from the unit hashes, so they are
    # swept by age alone, with the same cutoff as the directory's tier: a cache
    # nobody compiled with since the cutoff only saves time for a unit that is
    # gone or dormant, and deleting one costs a single non-incremental rebuild.
    incremental = os.path.join(profile_dir, "incremental")
    count = len(dead)
    if os.path.isdir(incremental):
        for name in os.listdir(incremental):
            path = os.path.join(incremental, name)
            newest = 0
            for dirpath, _, filenames in os.walk(path):
                for filename in filenames:
                    try:
                        newest = max(newest, os.lstat(os.path.join(dirpath, filename)).st_mtime)
                    except OSError:
                        pass
            if newest < cutoff:
                freed += remove(path, dry_run)
                count += 1
    return freed, count


def main():
    args = sys.argv[1:]
    dry_run = "--dry-run" in args
    args = [a for a in args if a != "--dry-run"]

    def take_days(flag, default):
        if flag in args:
            index = args.index(flag)
            value = float(args[index + 1])
            del args[index:index + 2]
            return value
        return default

    age_days = take_days("--age", 1.0)
    cache_age_days = take_days("--cache-age", 14.0)
    if not args:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    roots = args
    now = time.time()
    marked_cutoff = now - age_days * 86400.0
    cache_cutoff = now - cache_age_days * 86400.0
    live = mark_live_units(os.path.dirname(os.path.abspath(roots[0])))
    if live is None:
        return 1
    print(f"marked {len(live)} live units")
    total = units = skipped = 0
    for root in roots:
        for profile_dir in profile_dirs(root):
            lock = try_lock(profile_dir)
            if lock is None:
                print(f"skipped (build in progress): {profile_dir}")
                skipped += 1
                continue
            with lock:
                freed, count = sweep_profile_dir(
                    profile_dir, root, live, marked_cutoff, cache_cutoff, dry_run)
            if count:
                print(f"{freed / 1e9:6.2f} GB  {count:4d} units  {profile_dir}")
            total += freed
            units += count
    verb = "would free" if dry_run else "freed"
    print(f"{verb} {total / 1e9:.2f} GB from {units} dead units"
          + (f" ({skipped} directories skipped)" if skipped else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
