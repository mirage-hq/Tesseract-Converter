"""Offline regressions for recorded native evidence, not fresh Adobe proof."""
import copy
import json
from pathlib import Path
import unittest
from unittest import mock

import adobe_export_test as adapter
import adobe_vector_controls as controls

ROOT = Path(__file__).resolve().parents[1]


class VectorNativeControlsTests(unittest.TestCase):
    def setUp(self):
        self.entry = json.loads((ROOT / adapter.VECTOR_CONTROL_EVIDENCE).read_text())["cases"][0]
        self.expected = json.loads((ROOT / adapter.VECTOR_FIXTURES / "vector-rect-size.expected.json").read_text())
        self.case = next(c for c in adapter.load_cases(ROOT) if c.control_kind == "vector")
        self.identity = {
            "input_sha256": self.entry["input"]["sha256"],
            "native_aep_sha256": self.entry["generated_aep"]["sha256"],
            "artifacts": {"expected_json": {"sha256": self.entry["oracle_sha256"]}},
            "vector_inspector_sha256": self.entry["inspector_sha256"],
            "independent_source_declared": {"sha256": self.entry["source_sha256"]},
            "vector_control_oracle": self.expected,
        }

    def compare(self, exported):
        return controls.compare(self.case.name, self.expected, exported, self.entry["source_readback"])

    def test_recorded_native_controls_and_critical_samples_match(self):
        self.assertEqual(controls.compare(self.case.name, self.expected, self.entry["source_readback"]), [])
        self.assertEqual(self.compare(self.entry["export_readback"]), [])
        phase = adapter._native_control_evidence(self.case, self.identity, ("vector", self.entry), None)
        self.assertEqual(phase["status"], "success")
        self.assertFalse(phase["fresh_adobe_inspection"])
        self.assertEqual(phase["provenance"], "reused_by_exact_hash")

    def test_native_quantized_sample_not_ideal_linear_value_is_the_oracle(self):
        exported = copy.deepcopy(self.entry["export_readback"])
        sample = exported["controls"][0]["samples"][2]
        sample["value"] = [80 + 70 * 29 / 30, 40 + 30 * 29 / 30]
        self.assertTrue(any("valueAtTime" in f for f in self.compare(exported)))

    def test_missing_duplicate_changed_or_unfinished_controls_fail(self):
        mutations = [
            lambda r: r.update(completed=False),
            lambda r: r.update(ownedProjectClosed=False),
            lambda r: r["controls"].pop(),
            lambda r: r["controls"].append(copy.deepcopy(r["controls"][0])),
            lambda r: r["controls"][0]["keys"][0].update(inCode="6613"),
            lambda r: r["controls"][0]["keys"][1].update(time=0.9),
            lambda r: r["controls"][0]["keys"][1].update(value=[149, 70]),
            lambda r: r["controls"][0]["samples"].pop(),
            lambda r: r["controls"][0]["samples"][1].update(value=[float("nan"), 55]),
        ]
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                exported = copy.deepcopy(self.entry["export_readback"])
                mutate(exported)
                self.assertTrue(self.compare(exported))

    def test_source_oracle_inspector_and_aep_hash_drift_fail_closed(self):
        for field in ("source_sha256", "oracle_sha256", "inspector_sha256", "generated_aep", "input"):
            with self.subTest(field=field):
                entry = copy.deepcopy(self.entry)
                if isinstance(entry[field], dict):
                    entry[field]["sha256"] = "0" * 64
                else:
                    entry[field] = "0" * 64
                phase = adapter._native_control_evidence(self.case, self.identity, ("vector", entry), None)
                self.assertEqual(phase["status"], "failure")
                self.assertEqual(phase["provenance"], "unverified")

    def test_recorded_success_cannot_hide_a_control_mismatch(self):
        entry = copy.deepcopy(self.entry)
        entry["comparison"] = {"static_key_values_and_interpolation": "passed"}
        entry["export_readback"]["controls"][0]["keys"][1]["value"] = [99, 70]
        phase = adapter._native_control_evidence(self.case, self.identity, ("vector", entry), None)
        self.assertEqual(phase["status"], "failure")

    def test_missing_vector_evidence_does_not_discard_63_existing_cases(self):
        with mock.patch.object(adapter, "VECTOR_CONTROL_EVIDENCE", Path("missing-vector-evidence.json")):
            inventory, problem = adapter._control_evidence_inventory(ROOT, adapter.load_cases(ROOT))
        self.assertIsNone(problem)
        self.assertEqual(len(inventory), 63)
        self.assertNotIn(self.case.name, inventory)
        phase = adapter._native_control_evidence(self.case, self.identity, None, problem)
        self.assertEqual(phase["status"], "failure")


if __name__ == "__main__":
    unittest.main()
