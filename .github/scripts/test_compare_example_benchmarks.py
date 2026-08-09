import unittest

from compare_example_benchmarks import format_delta, render_report


class CompareExampleBenchmarksTests(unittest.TestCase):
    def test_lower_values_are_improvements(self):
        self.assertEqual(format_delta(90, 100), "✅ -10.00%")
        self.assertEqual(format_delta(110, 100), "❌ +10.00%")
        self.assertEqual(format_delta(100, 100), "~0%")

    def test_report_supports_build_only_examples(self):
        current = {
            "schema_version": 1,
            "commit": "candidate123456789",
            "benchmarks": [
                {"name": "fibonacci", "cycles": 90, "mast_size": 200},
                {"name": "basic-wallet", "mast_size": 300},
            ],
        }
        baseline = {
            "schema_version": 1,
            "commit": "baseline123456789",
            "benchmarks": [
                {"name": "fibonacci", "cycles": 100, "mast_size": 180},
                {"name": "basic-wallet", "mast_size": 300},
            ],
        }

        report = render_report(current, baseline)

        self.assertIn("| fibonacci | 90 (✅ -10.00%) | 200B (❌ +11.11%) |", report)
        self.assertIn("| basic-wallet | n/a | 300B (~0%) |", report)
        self.assertIn("`next` `baseline1234`", report)


if __name__ == "__main__":
    unittest.main()
