"""Regression tests for complete, static allocation-trace accounting."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("allocator_histogram", Path(__file__).with_name("allocator-histogram.py"))
histogram = importlib.util.module_from_spec(spec)
spec.loader.exec_module(histogram)


def capture(identity="hir:module::f", tier="Stack", actions=1, cost=1, loads=0, stores=0):
    return f'''[TRACE codegen] physical-plan identity-utf8={identity.encode().hex()}
procedure example
segment b0
  unit s0 kind=1 source=0
    tier={tier} victims=[] actions={actions} cost={cost} loads={loads} stores={stores}
  control-preparation e0 tier=Stack victims=[] actions=0 cost=0 loads=0 stores=0
edge e0 edge-traffic actions=2 cost=7 loads=0 stores=1
frame size=4 source=0 scalar=1 spanning=0 workspace=0 padding=3
physical-plan-end
'''


class HistogramTests(unittest.TestCase):
    def test_distinct_preparations_and_noops(self):
        data = histogram.analyze(capture(tier="Aliases", actions=0, cost=0))
        group = data["standalone"]
        self.assertEqual(group["raw"]["unit"]["Aliases"]["zero_actions"], 1)
        self.assertEqual(group["raw"]["control"]["Stack"]["count"], 1)
        self.assertEqual(group["raw"]["edge"]["Transfer"]["stores"], 1)
        self.assertEqual(group["raw"]["edge"]["Transfer"]["cost"], 7)

    def test_identical_captures_are_deduplicated_within_workload(self):
        data = histogram.analyze(capture() + "[TRACE codegen] elapsed_ns=42\n" + capture())
        group = data["standalone"]
        self.assertEqual(group["raw"]["unit"]["Stack"]["count"], 2)
        self.assertEqual(group["distinct"]["unit"]["Stack"]["count"], 1)
        self.assertEqual(group["captures"][0]["encounters"], 2)

    def test_same_identity_with_changed_plan_is_a_distinct_variant(self):
        data = histogram.analyze(capture() + capture(actions=2, cost=2))
        self.assertEqual(len(data["standalone"]["captures"]), 2)
        self.assertEqual(data["standalone"]["distinct"]["unit"]["Stack"]["cost"], 3)

    def test_same_short_name_under_different_modules_is_distinct(self):
        data = histogram.analyze(capture("hir:a::f") + capture("hir:b::f"))
        self.assertEqual(len(data["standalone"]["captures"]), 2)

    def test_workloads_are_independent_and_cache_hits_are_visible(self):
        text = "PASS [1s] (1/3) crate test_one\n" + capture()
        text += "PASS [1s] (2/3) crate test_two\n" + capture()
        text += "PASS [1s] (3/3) crate cached_test\n"
        data = histogram.analyze(text)
        self.assertEqual(data["crate test_one"]["captures"][0]["encounters"], 1)
        self.assertEqual(data["crate test_two"]["captures"][0]["encounters"], 1)
        self.assertEqual(data["crate cached_test"]["captures"], [])

    def test_empty_procedure_and_log_indentation(self):
        text = '  [TRACE codegen] physical-plan identity-utf8=67656e6572617465643a303a5b5d\n    procedure empty\n    physical-plan-end\n'
        data = histogram.analyze(text)["standalone"]
        self.assertEqual(len(data["captures"]), 1)
        self.assertEqual(data["raw"], {})

    def test_unplanned_units_are_not_fabricated_preparations(self):
        text = capture().replace("frame size=", "unplanned-unit s9 kind=1 source=9\nframe size=")
        data = histogram.analyze(text)["standalone"]
        self.assertEqual(data["raw"]["unit"]["Stack"]["count"], 1)

    def test_actual_normalization_retains_traffic(self):
        data = histogram.analyze(capture(tier="Normalization", actions=4, cost=22, loads=2, stores=2))
        self.assertEqual(data["standalone"]["raw"]["unit"]["Normalization"],
                         {"count": 1, "zero_actions": 0, "actions": 4, "cost": 22, "loads": 2, "stores": 2})

    def test_nonprinting_identity_roundtrips_without_debug_string_escapes(self):
        identity = "hir:module::zero\x00width\u200b"
        text = capture(identity)
        self.assertEqual(histogram.analyze(text)["standalone"]["captures"][0]["identity"], identity)

    def test_truncated_reserved_records_are_errors(self):
        for record in ["edge e0 edge-traffic", "control-preparation", "unit", "unit sBAD kind=1", "tier", "unplanned-unit"]:
            text = 'physical-plan identity-utf8=66697874757265\n' + record + '\nphysical-plan-end\n'
            with self.subTest(record=record), self.assertRaises(ValueError):
                histogram.analyze(text)

    def test_malformed_or_partial_captures_are_rejected(self):
        for text in [capture().replace("physical-plan-end", ""),
                     capture().replace("actions=1", "actions=-1"),
                     capture(tier="Bogus"), capture(actions=0, stores=1),
                     capture().replace("tier=Stack victims=[] actions=1 cost=1 loads=0 stores=0", "tier=Stack"),
                     capture().replace("physical-plan-end", capture())]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                histogram.analyze(text)


if __name__ == "__main__":
    unittest.main()
