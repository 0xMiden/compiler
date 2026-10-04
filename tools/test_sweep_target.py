import fcntl
import importlib.util
import json
import os
import tempfile
import time
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("sweep-target.py")
SPEC = importlib.util.spec_from_file_location("sweep_target", SCRIPT)
sweep_target = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sweep_target)


class LiveExecutableMarkingTests(unittest.TestCase):
    def test_hash_suffixed_root_still_marks_all_matching_cached_units(self):
        with tempfile.TemporaryDirectory() as temp:
            target_dir = Path(temp) / "cargo-target-deadbeefdeadbeef"
            build_dir = Path(temp) / "build"
            profile = target_dir / "debug"
            deps = build_dir / "debug" / "deps"
            deps.mkdir(parents=True)
            profile.mkdir(parents=True)

            executable = profile / "demo-tool"
            executable.write_bytes(b"live executable")
            (deps / "demo_tool-1111111111111111").write_bytes(b"stale executable")
            (deps / "demo_tool-2222222222222222").write_bytes(executable.read_bytes())
            (deps / "demo_tool-3333333333333333").write_bytes(executable.read_bytes())

            message = {
                "reason": "compiler-artifact",
                "filenames": [str(executable)],
                "executable": str(executable),
                "target": {"name": "demo-tool", "kind": ["bin"]},
            }

            self.assertEqual(
                sweep_target.live_hashes_from_message(message, target_dir, build_dir),
                {"2222222222222222", "3333333333333333"},
            )

    def test_build_script_hash_is_read_from_cache_ancestor(self):
        message = {
            "filenames": [
                "target/debug/build/build-helper-4444444444444444/build-script-build"
            ],
            "executable": None,
        }

        self.assertEqual(
            sweep_target.live_hashes_from_message(message),
            {"4444444444444444"},
        )

    def test_unresolved_uplifted_executable_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            target_dir = Path(temp) / "target"
            build_dir = Path(temp) / "build"
            profile = target_dir / "debug"
            (build_dir / "debug" / "deps").mkdir(parents=True)
            profile.mkdir(parents=True)
            executable = profile / "midenc"
            executable.write_bytes(b"live executable")

            message = {
                "reason": "compiler-artifact",
                "filenames": [str(executable)],
                "executable": str(executable),
                "target": {"name": "midenc", "kind": ["bin"]},
            }

            with self.assertRaises(sweep_target.UnresolvedExecutableError):
                sweep_target.live_hashes_from_message(message, target_dir, build_dir)

    def test_hashed_executable_is_marked_directly(self):
        message = json.loads(
            '{"filenames":["target/debug/deps/tool-3333333333333333"],'
            '"executable":"target/debug/deps/tool-3333333333333333"}'
        )
        self.assertEqual(
            sweep_target.live_hashes_from_message(message),
            {"3333333333333333"},
        )


class PerUnitLayoutMarkingTests(unittest.TestCase):
    """Cargo's per-unit layout: `build/<package>/<hash>/{fingerprint,out}`."""

    def test_the_unit_directory_names_the_unit_whatever_the_file_is_called(self):
        hashed = "target/debug/build/demo/1111111111111111/out/libdemo-1111111111111111.rlib"
        build_script = "target/debug/build/demo/2222222222222222/out/build_script_build"
        out_dir = "target/debug/build/demo/3333333333333333/out"
        message = {"filenames": [hashed, build_script], "out_dir": out_dir, "executable": None}

        self.assertEqual(
            sweep_target.live_hashes_from_message(message),
            {"1111111111111111", "2222222222222222", "3333333333333333"},
        )

    def test_a_package_directory_is_not_taken_for_a_unit(self):
        # `build/<package>` is followed by the unit; a package whose own name
        # is sixteen hex digits must not be marked as one.
        path = "target/debug/build/abcdefabcdefabcd/4444444444444444/out/libx-4444444444444444.rlib"
        self.assertEqual(sweep_target.artifact_hashes(path), {"4444444444444444"})

    def test_uplifted_executable_resolves_to_its_unit_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            target_dir = Path(temp) / "target"
            profile = target_dir / "debug"
            units = profile / "build" / "demo-tool"
            stale = units / "1111111111111111" / "out"
            live = units / "2222222222222222" / "out"
            harness = units / "3333333333333333" / "out"
            for directory in (stale, live, harness):
                directory.mkdir(parents=True)

            executable = profile / "demo-tool"
            executable.write_bytes(b"live executable")
            (stale / "demo_tool").write_bytes(b"stale executable")
            (live / "demo_tool").write_bytes(executable.read_bytes())
            # The test harness of the same crate: hashed name, other bytes
            (harness / "demo_tool-3333333333333333").write_bytes(b"test harness")

            message = {
                "reason": "compiler-artifact",
                "filenames": [str(executable)],
                "executable": str(executable),
                "target": {"name": "demo-tool", "kind": ["bin"]},
            }

            self.assertEqual(
                sweep_target.live_hashes_from_message(message, target_dir, target_dir),
                {"2222222222222222"},
            )

    def test_unresolved_uplifted_executable_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            target_dir = Path(temp) / "target"
            profile = target_dir / "debug"
            out = profile / "build" / "midenc" / "1111111111111111" / "out"
            out.mkdir(parents=True)
            (out / "midenc").write_bytes(b"another build")
            executable = profile / "midenc"
            executable.write_bytes(b"live executable")

            message = {
                "reason": "compiler-artifact",
                "filenames": [str(executable)],
                "executable": str(executable),
                "target": {"name": "midenc", "kind": ["bin"]},
            }

            with self.assertRaises(sweep_target.UnresolvedExecutableError):
                sweep_target.live_hashes_from_message(message, target_dir, target_dir)


def make_unit(profile, package, unit_hash, age_days, now):
    """Create a per-unit-layout unit `age_days` old and return its directory."""
    unit = profile / "build" / package / unit_hash
    fingerprint = unit / "fingerprint"
    out = unit / "out"
    fingerprint.mkdir(parents=True)
    out.mkdir()
    (out / f"lib{package}-{unit_hash}.rlib").write_bytes(b"0123456789")
    stamp = fingerprint / "invoked.timestamp"
    stamp.write_text("")
    then = now - age_days * 86400.0
    for path in (stamp, fingerprint, out, unit):
        os.utime(path, (then, then))
    return unit


class PerUnitLayoutSweepTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "target"
        self.now = time.time()
        self.marked_cutoff = self.now - 1 * 86400.0
        self.cache_cutoff = self.now - 14 * 86400.0

    def profile(self, *parts):
        profile = self.root.joinpath(*parts)
        profile.mkdir(parents=True)
        (profile / ".cargo-build-lock").write_text("")
        return profile

    def sweep(self, profile, live, dry_run=False):
        return sweep_target.sweep_profile_dir(
            str(profile), str(self.root), live, self.marked_cutoff, self.cache_cutoff, dry_run
        )

    def test_profile_directories_of_both_layouts_are_found(self):
        per_unit = self.profile("debug")
        (per_unit / "build").mkdir()
        nested = self.profile("cache", "wasm32-wasip2", "release")
        (nested / "build").mkdir()
        older = self.root / "legacy" / "debug"
        (older / ".fingerprint").mkdir(parents=True)
        # A `build` directory without a Cargo lock beside it is not a profile
        (self.root / "docs" / "build").mkdir(parents=True)

        self.assertEqual(
            sorted(sweep_target.profile_dirs(str(self.root))),
            sorted(str(p) for p in (per_unit, nested, older)),
        )

    def test_dead_units_go_and_live_and_young_ones_stay(self):
        profile = self.profile("debug")
        live = {f"{n:016x}" for n in range(1, 9)}
        live_units = [make_unit(profile, "kept", h, 30, self.now) for h in sorted(live)]
        dead = make_unit(profile, "gone", "aaaaaaaaaaaaaaaa", 3, self.now)
        dead_sibling = make_unit(profile, "kept", "bbbbbbbbbbbbbbbb", 3, self.now)
        young = make_unit(profile, "young", "cccccccccccccccc", 0.1, self.now)

        freed, count = self.sweep(profile, live)

        self.assertEqual(count, 2)
        self.assertEqual(freed, 20)
        self.assertTrue(all(unit.is_dir() for unit in live_units), "live units are kept")
        self.assertTrue(young.is_dir(), "an unmarked unit younger than --age is kept")
        self.assertFalse(dead.exists())
        self.assertFalse(dead_sibling.exists())
        self.assertFalse(dead.parent.exists(), "a package directory left empty goes too")
        self.assertTrue(dead_sibling.parent.is_dir())

    def test_a_dry_run_reports_and_deletes_nothing(self):
        profile = self.profile("debug")
        live = {f"{n:016x}" for n in range(1, 9)}
        for h in live:
            make_unit(profile, "kept", h, 30, self.now)
        dead = make_unit(profile, "gone", "aaaaaaaaaaaaaaaa", 3, self.now)

        freed, count = self.sweep(profile, live, dry_run=True)

        self.assertEqual((freed, count), (10, 1))
        self.assertTrue(dead.is_dir())

    def test_a_nested_cache_keeps_unmarked_units_for_the_cache_age(self):
        # No unit of a nested cache is marked, so only the long guard applies
        profile = self.profile("cache", "wasm32-wasip2", "release")
        recent = make_unit(profile, "dep", "aaaaaaaaaaaaaaaa", 3, self.now)
        ancient = make_unit(profile, "dep", "bbbbbbbbbbbbbbbb", 30, self.now)

        freed, count = self.sweep(profile, set())

        self.assertEqual(count, 1)
        self.assertTrue(recent.is_dir())
        self.assertFalse(ancient.exists())

    def test_a_directory_whose_build_lock_is_held_is_not_locked(self):
        profile = self.profile("debug")
        with sweep_target.try_lock(str(profile)) as held:
            self.assertIsNotNone(held)
            # A second taker stands for a running Cargo build
            self.assertIsNone(sweep_target.try_lock(str(profile)))
        with open(profile / ".cargo-build-lock") as build:
            fcntl.flock(build, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertIsNone(sweep_target.try_lock(str(profile)))
        released = sweep_target.try_lock(str(profile))
        self.assertIsNotNone(released)
        released.close()


if __name__ == "__main__":
    unittest.main()
