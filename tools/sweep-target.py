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


class UnresolvedExecutableError(Exception):
    """Raised when a Cargo executable cannot be mapped to its hashed cache unit."""


def unhashed_artifact_name(name):
    """Remove Cargo's unit hash from an artifact file name."""
    stem, separator, suffix = name.partition(".")
    match = HASH_RE.search(stem)
    if not match:
        return name
    name = stem[:match.start()]
    return f"{name}{separator}{suffix}" if separator else name


def cargo_path_components(path, target_dir=None, build_dir=None):
    """Return path components below Cargo's target or build directory."""
    if target_dir is None or build_dir is None:
        return tuple(path.split(os.sep))

    absolute = os.path.abspath(path)
    matching_roots = []
    for root in {os.path.abspath(target_dir), os.path.abspath(build_dir)}:
        try:
            if os.path.commonpath((absolute, root)) == root:
                matching_roots.append(root)
        except ValueError:
            continue
    if not matching_roots:
        return ()

    # Prefer the most specific root when one is nested under the other.
    root = max(matching_roots, key=len)
    relative = os.path.relpath(absolute, root)
    return tuple(relative.split(os.sep)) if relative != os.curdir else ()


def artifact_hashes(path, target_dir=None, build_dir=None):
    """Return unit hashes encoded in a Cargo artifact path.

    Normal artifacts carry the hash in their basename. Build-script artifacts
    instead use a hashless basename below `build/<crate>-<hash>`. Restricting
    the search to those Cargo-owned positions avoids mistaking a workspace or
    target-directory component for a live unit hash.
    """
    components = cargo_path_components(path, target_dir, build_dir)
    if not components:
        return set()

    found = set()
    marked = unit_hash(components[-1])
    if marked:
        found.add(marked)
    for index, component in enumerate(components[:-1]):
        if component == "build":
            marked = unit_hash(components[index + 1])
            if marked:
                found.add(marked)
    return found


def files_equal(first, second):
    """Return whether two regular files are byte-identical."""
    if os.path.samefile(first, second):
        return True
    if os.path.getsize(first) != os.path.getsize(second):
        return False
    with open(first, "rb") as lhs, open(second, "rb") as rhs:
        while True:
            lhs_chunk = lhs.read(1024 * 1024)
            rhs_chunk = rhs.read(1024 * 1024)
            if lhs_chunk != rhs_chunk:
                return False
            if not lhs_chunk:
                return True


def uplifted_executable_hashes(message, target_dir, build_dir):
    """Resolve an uplifted executable to its byte-identical hashed cache unit.

    Cargo's JSON messages name ordinary binaries only by their final, unhashed
    path (for example, `target/debug/midenc`). The build cache keeps the same
    executable as `target/debug/deps/midenc-<hash>`. Compare the uplifted file
    with same-named cache candidates so the live unit hash is not swept.

    Multiple byte-identical candidates are all live for sweep purposes. That
    deliberately favors retaining a duplicate over guessing which one Cargo
    uplifted and deleting a live unit.
    """
    executable = message["executable"]
    target = message.get("target", {})
    crate_name = target.get("name")
    if not crate_name:
        raise UnresolvedExecutableError(
            f"Cargo did not report a target name for uplifted executable: {executable}"
        )

    # Cargo normalizes hyphens in package target names to underscores in the
    # rustc crate name used for cached artifacts. The uplifted executable keeps
    # the original target name (for example, `cargo-miden` versus
    # `deps/cargo_miden-<hash>`).
    executable_suffix = os.path.splitext(executable)[1]
    cached_name = f"{crate_name.replace('-', '_')}{executable_suffix}"

    profile_dir = os.path.dirname(os.path.abspath(executable))
    cache_subdir = "deps"
    if "example" in target.get("kind", []):
        cache_subdir = "examples"
        if os.path.basename(profile_dir) == "examples":
            profile_dir = os.path.dirname(profile_dir)
    try:
        relative_profile = os.path.relpath(profile_dir, os.path.abspath(target_dir))
    except ValueError as err:
        raise UnresolvedExecutableError(
            f"uplifted executable is outside Cargo's target directory: {executable}"
        ) from err
    if relative_profile == os.pardir or relative_profile.startswith(os.pardir + os.sep):
        raise UnresolvedExecutableError(
            f"uplifted executable is outside Cargo's target directory: {executable}"
        )
    cache_dir = os.path.join(os.path.abspath(build_dir), relative_profile, cache_subdir)
    try:
        candidates = os.scandir(cache_dir)
    except OSError as err:
        raise UnresolvedExecutableError(
            f"cannot inspect cache for uplifted executable {executable}: {err}"
        ) from err

    resolved = set()
    try:
        with candidates:
            for entry in candidates:
                marked = unit_hash(entry.name)
                if marked is None or unhashed_artifact_name(entry.name) != cached_name:
                    continue
                try:
                    matches = entry.is_file(follow_symlinks=False) and files_equal(
                        executable, entry.path
                    )
                except OSError as err:
                    raise UnresolvedExecutableError(
                        f"cannot compare uplifted executable {executable} with {entry.path}: {err}"
                    ) from err
                if matches:
                    resolved.add(marked)
    except OSError as err:
        raise UnresolvedExecutableError(
            f"cannot inspect cache for uplifted executable {executable}: {err}"
        ) from err

    if not resolved:
        raise UnresolvedExecutableError(
            f"cannot resolve uplifted executable to a hashed cache unit: {executable}"
        )
    return resolved


def live_hashes_from_message(message, target_dir=None, build_dir=None):
    """Return every live unit hash represented by one Cargo JSON message."""
    live = set()
    paths = list(message.get("filenames", []))
    if message.get("out_dir"):
        paths.append(message["out_dir"])
    for path in paths:
        live.update(artifact_hashes(path, target_dir, build_dir))

    executable = message.get("executable")
    executable_hashes = (
        artifact_hashes(executable, target_dir, build_dir) if executable else set()
    )
    live.update(executable_hashes)
    if executable and not executable_hashes:
        if target_dir is None or build_dir is None:
            raise UnresolvedExecutableError(
                "Cargo target and build directories are required to resolve "
                f"uplifted executable: {executable}"
            )
        live.update(uplifted_executable_hashes(message, target_dir, build_dir))
    return live


def cargo_output_directories(workspace_root):
    """Return Cargo's final-artifact and cache roots for the workspace."""
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version=1"],
        cwd=workspace_root,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    if result.returncode != 0:
        return None
    try:
        metadata = json.loads(result.stdout)
        target_dir = metadata["target_directory"]
        # `build_directory` was added after `target_directory`. On older Cargo
        # versions they are necessarily the same directory.
        build_dir = metadata.get("build_directory", target_dir)
    except (json.JSONDecodeError, KeyError, TypeError):
        return None
    return target_dir, build_dir


def mark_live_units(workspace_root):
    """Replay the workspace invocations and collect the live unit hashes.

    Returns the set of hashes, or None when an invocation fails — the caller
    must not sweep with an incomplete live set.
    """
    output_dirs = cargo_output_directories(workspace_root)
    if output_dirs is None:
        print("mark failed, not sweeping: cargo metadata failed", file=sys.stderr)
        return None
    target_dir, build_dir = output_dirs

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
            # Harvest hashes from every path component: libraries carry one in
            # the file name, while build scripts carry one in a parent directory.
            # Ordinary binaries require resolving their unhashed uplifted copy
            # back to the hashed cache artifact.
            try:
                live.update(live_hashes_from_message(message, target_dir, build_dir))
            except UnresolvedExecutableError as err:
                print(f"mark failed, not sweeping: {err}", file=sys.stderr)
                return None
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
