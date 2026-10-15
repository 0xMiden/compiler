import argparse
import importlib.util
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location("compiler_coverage", Path(__file__).with_name("coverage.py"))
coverage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(coverage)


class CoverageTests(unittest.TestCase):
    def test_environment_preserves_spaces_and_encoded_flags(self):
        parsed = coverage.parse_environment(
            "export LLVM_PROFILE_FILE='/a path/compiler-%p-%18m.profraw'\n"
            "export FLAGS='-C\x1finstrument-coverage'\n"
            "export EMPTY=''\n"
        )
        self.assertEqual(parsed["LLVM_PROFILE_FILE"], "/a path/compiler-%p-%18m.profraw")
        self.assertEqual(parsed["FLAGS"], "-C\x1finstrument-coverage")
        self.assertEqual(parsed["EMPTY"], "")

    def test_environment_rejects_unexpected_shell_commands(self):
        with self.assertRaises(ValueError):
            coverage.parse_environment("echo untrusted shell command")

    def collect(self, *, failure=None, skip_rust=False, skip_lit=False, package=None):
        calls = []
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "coverage.xml"
            output.write_text("stale report")
            args = argparse.Namespace(
                output_path=output,
                coverage_dir=root / "isolated",
                package=package,
                nextest_filter=None,
                lit_filter=None,
                skip_rust=skip_rust,
                skip_lit=skip_lit,
            )

            def run(command, environment, **kwargs):
                calls.append((command, environment.copy()))
                self.assertFalse(output.exists(), "the stale report must be removed before running")
                return SimpleNamespace(returncode=7 if failure and failure in command else 0)

            variables = (
                f"export LLVM_PROFILE_FILE='{root}/isolated/target/compiler-%p-%18m.profraw'\n"
                "export RUSTC_WRAPPER='/tools/cargo-llvm-cov'\n"
                "export __CARGO_LLVM_COV_RUSTC_WRAPPER_COVERAGE_TARGET='host-triple'\n"
            )
            with (
                mock.patch.object(coverage, "run", side_effect=run),
                mock.patch.object(coverage.subprocess, "check_output", return_value=variables) as show,
                mock.patch.object(coverage, "active_toolchain", return_value="nightly-test"),
            ):
                status = coverage.collect(args)
            self.assertIn("--coverage-host-only", show.call_args.args[0])
        return status, calls

    def test_builds_all_tools_and_merges_both_test_suites(self):
        status, calls = self.collect()
        self.assertEqual(status, 0)
        commands = [command for command, _ in calls]
        self.assertEqual(commands[0], ["cargo", "llvm-cov", "clean", "--workspace"])
        build = commands[1]
        for package in coverage.TOOL_PACKAGES:
            self.assertIn(package, build)
        self.assertIn("--artifact-dir", build)
        self.assertIn("nextest", commands[2])
        self.assertIn("--workspace", commands[2])
        self.assertEqual(commands[3][:3], ["litcheck", "lit", "run"])
        self.assertEqual(commands[4][:3], ["cargo", "llvm-cov", "report"])
        self.assertIn("--cobertura", commands[4])
        profiles = {environment["LLVM_PROFILE_FILE"] for _, environment in calls}
        self.assertEqual(len(profiles), 1)
        for _, environment in calls[1:]:
            self.assertIn("isolated", environment["CARGO_TARGET_DIR"])
            self.assertIn("isolated", environment["MIDENC_BIN_DIR"])
            self.assertEqual(environment["RUSTC_WRAPPER"], "/tools/cargo-llvm-cov")
            self.assertEqual(environment["RUSTUP_TOOLCHAIN"], "nightly-test")
            self.assertEqual(environment["__CARGO_LLVM_COV_RUSTC_WRAPPER_COVERAGE_TARGET"], "host-triple")

    def test_failed_rust_tests_still_run_lit_and_report_but_fail_the_task(self):
        status, calls = self.collect(failure="nextest")
        self.assertEqual(status, 7)
        self.assertTrue(any(command[0] == "litcheck" for command, _ in calls))
        self.assertIn("report", calls[-1][0])

    def test_failed_lit_still_generates_a_partial_report(self):
        status, calls = self.collect(failure="litcheck")
        self.assertEqual(status, 7)
        self.assertIn("report", calls[-1][0])

    def test_failed_report_fails_the_task(self):
        status, _ = self.collect(failure="report")
        self.assertEqual(status, 7)

    def test_failed_build_skips_tests_but_attempts_a_report(self):
        status, calls = self.collect(failure="build")
        self.assertEqual(status, 7)
        self.assertFalse(any("nextest" in command or "litcheck" in command for command, _ in calls))
        self.assertIn("report", calls[-1][0])

    def test_package_selection_and_single_suite_collection(self):
        status, calls = self.collect(skip_lit=True, package=["midenc-hir"])
        self.assertEqual(status, 0)
        test_command = next(command for command, _ in calls if "nextest" in command)
        self.assertIn("midenc-hir", test_command)
        self.assertNotIn("--workspace", test_command)
        self.assertFalse(any(command[0] == "litcheck" for command, _ in calls))
        status, calls = self.collect(skip_rust=True)
        self.assertEqual(status, 0)
        self.assertFalse(any("nextest" in command for command, _ in calls))


if __name__ == "__main__":
    unittest.main()
