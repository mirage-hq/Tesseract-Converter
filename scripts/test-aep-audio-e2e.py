#!/usr/bin/env python3
"""Offline orchestration checks; no renderer, Adobe app, or network is invoked."""
import array
import copy
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent))
import aep_audio_e2e as e2e
import aep_audio_reference as native_reference
from aep_audio_test import AudioTestError, sha256


class RunnerTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.source = self.root / "source.aep"
        self.source.write_bytes(b"native-source")
        self.primary = self.root / "sound.wav"
        self.primary.write_bytes(b"media")
        self.reference = self.root / "tests/references/aep/audio/wave-static.mp4"
        self.reference.parent.mkdir(parents=True)
        self.reference.write_bytes(b"independent-reference")
        self.fx = self.root / "explicit.tsrct"
        self.fx.write_bytes(b"editable-fx")
        (self.root / "fx").mkdir()
        (self.root / "fx/wave-static.json").write_text("{}")
        self.policy = {"sample_rate": 48000, "channels": 2, "duration_seconds": 6,
                       "duration_tolerance_seconds": 0.01, "relative_rms_error_max": 0.05,
                       "window_rms_error_max": 0.003, "silence_reference_rms_max": 0.0001,
                       "silence_leak_rms_max": 0.0005}
        self.case = {"id": "wave-static", "directions": ["import", "export"],
                     "reference_expectation": {"audible_windows": [[1.1, 1.4]],
                         "silent_windows": [[0, 0.9]], "minimum_rms": 0.005,
                         "maximum_silent_rms": 0.0001},
                     "source": {"path": self.source.name, "sha256": sha256(self.source),
                                "composition_id": 42, "composition_name": "wave-static"},
                     "primary": [{"path": self.primary.name, "sha256": sha256(self.primary)}],
                     "fx_input": {"document_sha256": sha256(self.root / "fx/wave-static.json"),
                                  "sha256": sha256(self.fx), "path": self.fx.name},
                     "reference": {"path": "tests/references/aep/audio/wave-static.mp4"},
                     "expected_diagnostics": {"import": [], "export": []},
                     "native_expected": {"import": [{"layer": "sound", "field": "audioEnabled", "value": True}],
                                         "export": [{"layer": "sound", "field": "audioEnabled", "value": True}]}}
        self.manifest = self.root / "manifest.json"
        self.write_manifest()
        for tool in ("converter", "tsrct", "preparer"):
            (self.root / tool).write_bytes(tool.encode())
        self.args = SimpleNamespace(manifest=self.manifest, case="wave-static", direction="both",
                                    work=self.root / "job", converter=str(self.root / "converter"),
                                    tsrct=str(self.root / "tsrct"), preparer=str(self.root / "preparer"), timeout=90)
        self.commands = []

    def write_manifest(self):
        self.manifest.write_text(json.dumps({"version": 1, "policy": self.policy, "cases": [self.case]}))

    def fake_run(self, command, log, timeout):
        self.commands.append(command)
        log.write_text("mock command log")
        if command[1] == "stage-source":
            Path(command[-1]).write_bytes(self.source.read_bytes())
        elif command[1] == "prepare":
            target = Path(command[-1]); target.mkdir()
            (target / "project.tsrct").write_bytes(self.fx.read_bytes())
            (target / "document.json").write_text("{}")
        elif command[1] in ("inspect-import", "inspect-export"):
            Path(command[-1]).write_text(json.dumps({"case_id": "wave-static", "assertion": "passed",
                "audio": ["sound.wav"], "audio_count": 1}))
        elif command[0] == self.args.converter:
            target = Path(command[-1]); target.mkdir()
            (target / ("project.tsrct" if "tesseract" in command else "project.aep")).write_bytes(b"converted")
        elif command[0] == self.args.tsrct:
            Path(command[command.index("--output") + 1]).write_bytes(b"actual")
        return "mock command log"

    def run_mocked(self, score=True):
        with (patch.object(e2e, "_run", side_effect=self.fake_run),
              patch.object(e2e, "_video_contract", return_value={"sha256": "mock-video"}),
              patch.object(e2e, "_reference_windows", return_value={"passed": True}),
              patch.object(e2e, "compare", return_value={"passed": score, "channel_results": []}),
              patch("subprocess.Popen", side_effect=AssertionError("no real processes"))):
            return e2e.run(self.args)

    def test_both_paths_offline_and_roundtrip(self):
        result = self.run_mocked()
        self.assertEqual(result["status"], "scored_local_reference")
        commands = self.commands
        self.assertEqual(commands[1][commands[1].index("--composition") + 1], "42")
        self.assertIn([self.args.preparer, "inspect-export", "wave-static",
                       str(self.args.work / "export/conversion/project.aep"),
                       str(self.args.work / "export/export-inspection.json")], commands)
        reimport = [cmd for cmd in commands if cmd[0] == self.args.converter and
                    "reimport" in cmd[-1]]
        self.assertEqual(len(reimport), 1)
        self.assertEqual(reimport[0][reimport[0].index("--composition") + 1], "1")
        self.assertIn("roundtrip_fx_vs_edited_fx", result["directions"]["export"])
        self.assertIn("not Adobe acceptance", result["provenance"])
        self.assertFalse(any("aerender" in str(cmd).lower() for cmd in commands))

    def test_missing_reference_path_fails_before_any_command(self):
        self.case["reference"] = None
        self.write_manifest()
        with self.assertRaisesRegex(AudioTestError, "repository-relative path"):
            self.run_mocked()
        self.assertFalse(self.args.work.exists())
        self.assertEqual(self.commands, [])

    def test_missing_committed_reference_fails_before_any_command(self):
        self.reference.unlink()
        with self.assertRaisesRegex(AudioTestError, "missing committed"):
            self.run_mocked()
        self.assertFalse(self.args.work.exists())

    def test_failed_score_persisted_and_other_direction_attempted(self):
        with self.assertRaisesRegex(AudioTestError, "attempts failed"):
            self.run_mocked(score=False)
        report = json.loads((self.args.work / "result.json").read_text())
        self.assertEqual(report["directions"]["import"]["comparison"]["status"], "failed_score")
        self.assertEqual(report["directions"]["export"]["comparison"]["status"], "failed_score")

    def test_timeout_continues_export(self):
        original = self.fake_run
        def timeout_import(command, log, timeout):
            if command[0] == self.args.converter and "tesseract" in command and "reimport" not in command[-1]:
                raise subprocess.TimeoutExpired(command, timeout)
            return original(command, log, timeout)
        with patch.object(self, "fake_run", side_effect=timeout_import):
            with self.assertRaisesRegex(AudioTestError, "attempts failed"):
                self.run_mocked()
        report = json.loads((self.args.work / "result.json").read_text())
        self.assertEqual(report["directions"]["import"]["fresh_import"]["status"], "blocked")
        self.assertEqual(report["directions"]["export"]["comparison"]["status"], "passed")

    def test_reference_silence_rejected(self):
        with patch.object(e2e, "decode", return_value=(8, 2, array.array('f', [0] * 96))):
            with self.assertRaisesRegex(AudioTestError, "audible/silent"):
                e2e._reference_windows(self.reference, self.case["reference_expectation"],
                                       {**self.policy, "sample_rate": 8})

    def test_rejects_remote_and_escaping_reference_fields(self):
        for reference in ({"url": "https://example.org/reference.mp4"},
                          {"path": "../audio/wave-static.mp4"},
                          {"path": "tests/references/aep/audio/wave-static.mp4", "sha256": "0" * 64}):
            self.case["reference"] = reference
            self.write_manifest()
            with self.assertRaises(AudioTestError):
                self.run_mocked()

    def test_native_audio_reference_records_a_local_file_without_asset_id(self):
        case = copy.deepcopy(self.case)
        case["id"] = "new-case"
        case["reference"] = None
        self.manifest.write_text(json.dumps({"cases": [case]}))
        work = self.root / "native-work"
        work.mkdir()
        video = work / "reference.mp4"
        video.write_bytes(b"Adobe audio")
        (work / "validated.json").write_text(json.dumps({
            "case_id": case["id"], "source": case["source"],
            "reference": {"sha256": sha256(video), "bytes": video.stat().st_size},
        }))
        with patch.object(native_reference, "ROOT", self.root), \
             patch.object(native_reference, "FIXTURE", self.root), \
             patch.object(native_reference, "MANIFEST", self.manifest):
            native_reference.publish(case, work)
            with self.assertRaisesRegex(RuntimeError, "already has a committed reference"):
                native_reference.publish(case, work)
        recorded = json.loads(self.manifest.read_text())["cases"][0]["reference"]
        self.assertEqual(recorded, {"path": "tests/references/aep/audio/new-case.mp4"})
        self.assertEqual((self.root / recorded["path"]).read_bytes(), video.read_bytes())

    def test_native_source_path_rejects_traversal_and_symlink(self):
        for invalid in ("../source.aep", str(self.source)):
            with self.subTest(invalid=invalid), self.assertRaises(AudioTestError):
                e2e._path(self.root, {"path": invalid, "sha256": sha256(self.source)}, "source")
        alias = self.root / "source-alias.aep"
        alias.symlink_to(self.source)
        with self.assertRaises(AudioTestError):
            e2e._path(self.root, {"path": alias.name, "sha256": sha256(self.source)}, "source")

    def test_normal_entrypoint_rejects_retired_flags(self):
        spec = importlib.util.spec_from_file_location("aep_test_entry", Path(__file__).with_name("aep-test.py"))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with patch.object(module, "run_e2e", side_effect=AssertionError("must reject flags")):
            for flag in ("--allow-adobe", "--create-reference", "--aerender"):
                with self.assertRaises(SystemExit):
                    module.main(["--case", "wave-static", "--direction", "both",
                                 "--work", str(self.root / "job"), flag])


if __name__ == "__main__":
    unittest.main()
