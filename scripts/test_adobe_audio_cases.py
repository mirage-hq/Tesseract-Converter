"""Unexecuted offline specifications for unified audio routing and evidence."""

from __future__ import annotations

import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import adobe_audio_cases as audio
import adobe_test


class AudioInventoryTests(unittest.TestCase):
    def test_all_direction_ids_have_native_source_pins_and_unrun_status(self) -> None:
        cases = audio.load_inventory()
        imports = {key for key, item in cases.items() if item["direction"] == "import"}
        exports = {key for key, item in cases.items() if item["direction"] == "export"}
        self.assertEqual((len(imports), len(exports), len(cases)), (19, 25, 44))
        self.assertEqual({item["source"]["sha256"] for item in cases.values()}, {audio.SOURCE_SHA256})
        self.assertEqual({item["execution"] for item in cases.values()}, {"UNRUN"})
        self.assertEqual({item["measurement"] for item in cases.values()}, {"unmeasured"})
        self.assertTrue(all(item["reference"] == {
            "path": f"tests/references/aep/audio/{item['slug']}.mp4"
        } and item["native_expected"] for item in cases.values()))
        self.assertIn("aep-audio-import-wave-static", imports)
        self.assertIn("aep-audio-export-wave-static", exports)
        self.assertIn("aep-audio-import-expression", imports)
        self.assertNotIn("aep-audio-export-expression", exports)
        self.assertNotIn("aep-audio-import-all-zero-one-key", imports)

    def test_local_reference_path_drift_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = json.loads(audio.MANIFEST.read_text())
            manifest["cases"][0]["reference"]["path"] = "tests/references/aep/audio/wrong.mp4"
            path = Path(tmp) / "cases.json"
            path.write_text(json.dumps(manifest))
            with self.assertRaisesRegex(audio.AudioAdapterError, "local inventory"):
                audio.load_inventory(manifest=path)

    def test_native_export_requires_real_adobe_artifact_before_acceptance(self) -> None:
        case = audio.load_inventory()["aep-audio-export-wave-static"]
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(audio.AudioAdapterError, "fresh exported AEP missing"):
                audio._native_acceptance(case=case, work=Path(tmp), aerender="/missing/aerender", timeout=10)

    def test_audio_adapter_keeps_failed_direction_and_continues_to_next(self) -> None:
        cases = audio.load_inventory()
        selected = [cases["aep-audio-import-wave-static"], cases["aep-audio-export-wave-static"]]
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = Path(tmp)
            def unavailable(_args: object) -> dict:
                raise RuntimeError("tool unavailable")
            rows = audio.run_cases(selected=selected, run_dir=run_dir,
                tools={"tesseract_conv": "conv", "tsrct": "tsrct", "audio_preparer": "preparer", "aerender": "aerender"},
                timeout=1, on_row=lambda _: None, cancelled=lambda: False, e2e_runner=unavailable)
            self.assertEqual([row["case_id"] for row in rows], [case["case_id"] for case in selected])
            self.assertTrue(all(row["status"] == "failure" and row["score"] is None for row in rows))
            self.assertTrue(all(row["execution"] == "UNRUN" for row in rows))
            self.assertTrue(all(row["native_acceptance"]["attempted"] is False for row in rows))

    def test_cancellation_is_not_a_fidelity_result(self) -> None:
        selected = list(audio.load_inventory().values())
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch.object(audio.e2e, "run", side_effect=AssertionError("must not launch")):
                rows = audio.run_cases(selected=selected, run_dir=Path(tmp),
                    tools={}, timeout=1, on_row=lambda _: None, cancelled=lambda: True)
            self.assertEqual(len(rows), 44)
            self.assertTrue(all(row["cancelled"] and row["score"] is None
                                and row["measurement"] == "unmeasured" for row in rows))

    def test_unified_parent_preserves_44_audio_failures_when_adapter_raises(self) -> None:
        cases = audio.load_inventory()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            workspace.mkdir()
            def unavailable(**_kwargs: object) -> list[dict]:
                raise RuntimeError("audio runner unavailable")
            report, code = adobe_test.run_unified(
                workspace=workspace, cases={}, targets={}, reference_settings={}, selected=[],
                run_dir=root / "run", tools={}, cache=root / "cache", local_references={},
                max_samples=1, cpu_timeout=1, cpu_only=False,
                score_timeouts={"render": 1}, audio_cases=cases,
                selected_audio=list(cases.values()), audio_runner=unavailable)
            self.assertEqual(code, 1)
            self.assertEqual(report["counts"]["selected"], 44)
            self.assertEqual(report["counts"]["failure"], 44)
            self.assertEqual(report["selection"]["registry_count"], 44)
            self.assertEqual(report["audio_inventory"]["reported_count"], 0)
            self.assertEqual(report["direction_scope"], ["export", "import"])
            self.assertTrue(all(row["score"] is None and row["measurement"] == "unmeasured"
                                for row in report["cases"]))
            self.assertFalse(report["selection"]["full_coverage_claimed"])

    def test_unified_parent_latches_audio_cancellation_and_retains_remaining_ids(self) -> None:
        cases = audio.load_inventory()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "workspace").mkdir()
            def interrupted(**kwargs: object) -> list[dict]:
                raise KeyboardInterrupt
            report, code = adobe_test.run_unified(
                workspace=root / "workspace", cases={}, targets={}, reference_settings={}, selected=[],
                run_dir=root / "run", tools={}, cache=root / "cache", local_references={},
                max_samples=1, cpu_timeout=1, cpu_only=False,
                score_timeouts={"render": 1}, audio_cases=cases,
                selected_audio=list(cases.values()), audio_runner=interrupted)
            self.assertEqual(code, 130)
            self.assertEqual(report["state"], "cancelled")
            self.assertEqual(len(report["cases"]), 44)
            self.assertEqual({row["case_id"] for row in report["cases"]}, set(cases))
            self.assertTrue(all(row["status"] == "failure" for row in report["cases"]))

    def test_native_reference_never_replaced_by_fx_playback(self) -> None:
        case = audio.load_inventory()["aep-audio-export-wave-static"]
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            exported = work / "export/conversion/project.aep"
            exported.parent.mkdir(parents=True)
            exported.write_bytes(b"native candidate")
            reference = work / "reference.mp4"
            reference.write_bytes(b"independent source")
            with (mock.patch.object(audio.e2e, "_video_contract", return_value={"frames": 180}),
                  mock.patch.object(audio.e2e, "_acquire_reference", return_value=reference),
                  mock.patch.object(audio.e2e, "_reference_windows"),
                  mock.patch.object(audio, "compare", return_value={"passed": False}) as comparator):
                # Native rendering goes through the central headless-adobe
                # boundary; mock it so this offline test needs no worker. A mock
                # render still needs a distinct native artifact.
                def fake_copy(_artifact: object, output: Path) -> None:
                    output.write_bytes(b"fresh Adobe render")
                with (mock.patch.object(audio.adobe_native, "execute", return_value={}),
                      mock.patch.object(audio.adobe_native, "copy_artifact", side_effect=fake_copy),
                      mock.patch.object(audio.adobe_native, "read_render_log", return_value=b"")):
                    phase = audio._native_acceptance(case=case, work=work, aerender="aerender", timeout=1)
            self.assertEqual(phase["status"], "failure")
            self.assertEqual(comparator.call_args.args[0], work / "export/adobe-native.mp4")
            self.assertEqual(comparator.call_args.args[1], reference)


if __name__ == "__main__":
    unittest.main()
