import importlib.util
import json
import tempfile
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


if __name__ == "__main__":
    unittest.main()
