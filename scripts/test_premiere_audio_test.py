import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from aep_audio_test import AudioTestError, sha256
from premiere_audio_test import main, run


class PremiereAudioTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        pins = {}
        for name in ("native_project", "fx_project", "actual", "reference", "media"):
            path = self.root / name
            path.write_bytes(name.encode())
            pins[name] = {"path": name, "sha256": sha256(path)}
        self.case = {"version": 1, "id": "ducking", "direction": "import",
                     "sequence_uid": "04f1ae8b-7e92-4c35-b584-7c662280fe01",
                     **{key: pins[key] for key in ("native_project", "fx_project", "actual", "reference")},
                     "media": [pins["media"]],
                     "policy": {"sample_rate": 48000, "channels": 2, "duration_seconds": 8,
                                "duration_tolerance_seconds": 0.01, "relative_rms_error_max": 0.05,
                                "window_rms_error_max": 0.003, "silence_reference_rms_max": 0.0001,
                                "silence_leak_rms_max": 0.0005}}
        self.path = self.root / "case.json"
        self.output = self.root / "result"

    def save(self):
        self.path.write_text(json.dumps(self.case))

    def test_both_directions_keep_raw_scores_and_preserve_failed_results(self):
        # Scoring itself is covered by test-aep-audio-test.py. This tests the
        # Premiere adapter's direction labels, score preservation and exit value.
        for direction, passed in [("import", True), ("export", False)]:
            with self.subTest(direction=direction):
                self.case["direction"] = direction
                self.save()
                output = self.root / direction
                score = {"passed": passed, "channel_results": [{"error_rms": 0.01}]}
                with patch("premiere_audio_test.compare", return_value=score) as comparator:
                    self.assertEqual(run(self.path, output), passed)
                comparator.assert_called_once_with(self.root / "actual", self.root / "reference", self.case["policy"], report_duration_failure=True)
                self.assertEqual(json.loads((output / "comparison.json").read_text()), score)
                status = json.loads((output / "status.json").read_text())
                self.assertEqual(status["measurement"], "measured")
                self.assertEqual(status["status"], "passed" if passed else "failed_score")
                roles = json.loads((output / "inputs.json").read_text())["roles"]
                self.assertEqual(roles["actual"], "FX render" if direction == "import" else "Premiere export render")
                with patch("premiere_audio_test.compare", return_value=score):
                    self.assertEqual(main(["--case", str(self.path), "--output", str(self.root / (direction + "-cli"))]), 0 if passed else 1)
                with self.assertRaises(FileExistsError):
                    run(self.path, output)

    def test_changed_input_blocks_before_scoring(self):
        self.save()
        (self.root / "media").write_bytes(b"changed")
        with patch("premiere_audio_test.compare") as comparator:
            with self.assertRaisesRegex(AudioTestError, "SHA-256 mismatch"):
                run(self.path, self.output)
            comparator.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_hardlink_self_comparison_is_rejected(self):
        (self.root / "reference").unlink()
        os.link(self.root / "actual", self.root / "reference")
        self.case["reference"]["sha256"] = self.case["actual"]["sha256"]
        self.save()
        with self.assertRaisesRegex(AudioTestError, "separate artifacts"):
            run(self.path, self.output)

    def test_decode_failure_is_unmeasured_not_a_failed_score(self):
        self.save()
        with patch("premiere_audio_test.compare", side_effect=AudioTestError("channel count differs")):
            with self.assertRaises(AudioTestError):
                run(self.path, self.output)
        status = json.loads((self.output / "status.json").read_text())
        self.assertEqual(status["measurement"], "unmeasured")
        self.assertEqual(status["status"], "blocked")
        self.assertFalse((self.output / "comparison.json").exists())


if __name__ == "__main__":
    unittest.main()
