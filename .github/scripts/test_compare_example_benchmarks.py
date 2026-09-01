import unittest

from compare_example_benchmarks import format_delta, has_metric_changes, render_report


class CompareExampleBenchmarksTests(unittest.TestCase):
    def test_lower_values_are_improvements(self):
        self.assertEqual(format_delta(90, 100), "✅ -10.00%")
        self.assertEqual(format_delta(110, 100), "❌ +10.00%")
        self.assertEqual(format_delta(100, 100), "~0%")

    def test_report_renders_one_table_for_all_examples(self):
        current = {
            "schema_version": 3,
            "commit": "candidate123456789",
            "benchmarks": [
                {"name": "fibonacci", "cycles": 90, "mast_size": 200},
                {"name": "basic-wallet", "cycles": 500, "mast_size": 300},
            ],
        }
        baseline = {
            "schema_version": 3,
            "commit": "baseline123456789",
            "benchmarks": [
                {"name": "fibonacci", "cycles": 100, "mast_size": 180},
                {"name": "basic-wallet", "cycles": 550, "mast_size": 300},
            ],
        }

        report = render_report(current, baseline)

        self.assertIn("| fibonacci | 90 (✅ -10.00%) | 200B (❌ +11.11%) |", report)
        self.assertIn("| basic-wallet | 500 (✅ -9.09%) | 300B (~0%) |", report)
        self.assertEqual(report.count("| example | VM cycles (vs next) |"), 1)
        self.assertNotIn("MockChain", report)
        self.assertIn("`next` `baseline1234`", report)

    def test_metric_change_detection(self):
        baseline = {
            "benchmarks": [
                {"name": "fibonacci", "cycles": 100, "mast_size": 200},
                {"name": "wallet", "cycles": 500, "mast_size": 300},
            ]
        }
        unchanged = {
            "benchmarks": [
                {"name": "fibonacci", "cycles": 100, "mast_size": 200},
                {"name": "wallet", "cycles": 500, "mast_size": 300},
            ]
        }
        changed = {
            "benchmarks": [
                {"name": "fibonacci", "cycles": 99, "mast_size": 200},
                {"name": "wallet", "cycles": 500, "mast_size": 300},
            ]
        }

        self.assertFalse(has_metric_changes(unchanged, baseline))
        self.assertTrue(has_metric_changes(changed, baseline))

    def test_added_or_removed_benchmark_is_a_change(self):
        baseline = {
            "benchmarks": [{"name": "fibonacci", "cycles": 100, "mast_size": 200}]
        }
        current = {"benchmarks": []}

        self.assertTrue(has_metric_changes(current, baseline))


if __name__ == "__main__":
    unittest.main()
