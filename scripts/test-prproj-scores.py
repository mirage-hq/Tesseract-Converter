#!/usr/bin/env python3
"""Offline score-state tests; no GPU, Adobe, network, or private Assets."""

from fractions import Fraction
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout

import pytest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("prproj_scores", HERE / "check-prproj-scores.py")
score = importlib.util.module_from_spec(spec)
spec.loader.exec_module(score)


class PremiereSimilarityPolicyTests(unittest.TestCase):
    def setUp(self):
        self.meta = {"width": 1920, "height": 1080, "fps": Fraction(30), "duration_secs": 5.0}
        self.result = {"score": {"similarity": (0.99 + 0.97 + 1.0) / 3}, "compared_frames": 3,
                       "min_frame_similarity": 0.97,
                       "frame_results": [{"time_secs": time, "score": {"similarity": value}}
                                         for time, value in ((0, 0.99), (1, 0.97), (2, 1.0))]}
        self.policy = {"min_mean_similarity": 0.98, "min_frame_similarity": 0.95}

    def test_shortened_export_never_passes_due_to_min_duration_comparison(self):
        for altered in ({"duration_secs": 4.9}, {"width": 720}, {"fps": Fraction(24)}):
            with self.subTest(altered=altered), self.assertRaises(score.ScoreError):
                score.check_alignment(self.meta, {**self.meta, **altered})
        score.check_alignment(self.meta, {**self.meta, "duration_secs": 5 + 1 / 30})

    def test_requires_multiple_finite_frames_and_reports_worst_timestamp(self):
        self.assertEqual(score.evaluate(self.result, self.policy)["worst_time_secs"], 1)
        for change in ({"compared_frames": 1, "frame_results": self.result["frame_results"][:1]},
                       {"min_frame_similarity": float("nan")},
                       {"frame_results": []}):
            with self.subTest(change=change), self.assertRaises(score.ScoreError):
                score.evaluate({**self.result, **change}, self.policy)

    def test_conflicting_or_out_of_range_frame_data_cannot_pass(self):
        for change in ({"score": {"similarity": 0.9999}},
                       {"min_frame_similarity": 0.99},
                       {"frame_results": [{"time_secs": 0, "score": {"similarity": 1.2}},
                                          *self.result["frame_results"][1:]]}):
            with self.subTest(change=change), self.assertRaisesRegex(score.ScoreError, "invalid video comparison"):
                score.evaluate({**self.result, **change}, self.policy)

    def test_score_policy_checks_worst_frame_not_only_mean(self):
        policy = {**self.policy, "min_frame_similarity": 0.98}
        with self.assertRaisesRegex(score.ScoreError, "below configured"):
            score.evaluate(self.result, policy)
        self.assertEqual(score.evaluate(self.result)["min_frame_similarity"], 0.97)

    def test_references_select_every_scored_proof_case_and_include_bezier_reference(self):
        data = score.fixtures.load_manifest(score.fixtures.DEFAULT_MANIFEST)
        expected = [case["id"] for case in data["cases"]
                    if case["proof"] == "video_reference" and "score_policy" in case]
        selected = []
        binaries = []
        exporters = []
        encoders = []

        def fake_score(case, _cache, binary, _validation, tsrct, **kwargs):
            selected.append(case["id"])
            binaries.append(binary)
            exporters.append(tsrct)
            encoders.append(kwargs["ffmpeg"])
            return {"case_id": case["id"], "status": "scored"}

        stdout = io.StringIO()
        with patch.object(score.fixtures, "load_manifest", return_value=data), \
             patch.object(score, "score_case", side_effect=fake_score), \
             patch.object(sys, "argv", ["check-prproj-scores.py", "--references",
                                        "--ffmpeg", sys.executable]), \
             redirect_stdout(stdout):
            self.assertEqual(score.main(), 0)
        report = json.loads(stdout.getvalue())
        self.assertEqual(selected, expected)
        self.assertEqual(len(selected), 41)
        self.assertEqual(binaries, [score.REPO / "target/debug/tsrct-conv"] * 41)
        self.assertEqual(exporters, [score.REPO.parents[1] / "target/debug/tsrct"] * 41)
        self.assertEqual(encoders, [Path(sys.executable)] * 41)
        self.assertIn("premiere_isolated_variable_speed_ramp", selected)
        self.assertIn("premiere_isolated_motion_static_transform", selected)
        self.assertIn("premiere_isolated_text_point", selected)
        self.assertIn("premiere_isolated_hidden_nest_26_5", selected)
        self.assertIn("premiere_isolated_graphic_shapes_26_5", selected)
        self.assertIn("premiere_isolated_gradient_fills_26_5", selected)
        self.assertIn("premiere_isolated_graphic_bezier_position_rotation_26_5", selected)
        self.assertIn("premiere_isolated_transform_track_matte_26_5", selected)
        self.assertIn("premiere_multi_text_source_keys_import_20260930", selected)
        self.assertIn("premiere_motion_anchor_scale_width_probe_20260930", selected)
        self.assertIn("premiere_isolated_motion_scale_bezier_approximation", selected)
        self.assertIn("premiere_isolated_motion_scale_bezier_asymmetric_aligned", selected)
        # Enrolled once its Tint converts (JRB-2001).
        self.assertIn("premiere_isolated_effect_stack", selected)
        self.assertNotIn("premiere_isolated_motion_scale_bezier_asymmetric", selected)
        # The Premiere 26.5.1 save of the vertical canvas is gated; the
        # XML-derived vertical case stays structural.
        self.assertIn("premiere_isolated_vertical_canvas_26_5", selected)
        self.assertNotIn("premiere_isolated_vertical_canvas", selected)
        # Every-frame sampling shows a one-source-frame lag after the 24 fps cut.
        self.assertNotIn("premiere_isolated_rate_24_cut", selected)
        self.assertEqual(report["mode"], "strict")
        self.assertEqual(report["scored"], len(expected))
        self.assertNotIn("approved_scored", report)

    def test_strict_reference_uses_committed_video_without_reference_hash(self):
        cases = score.fixtures.load_manifest(score.fixtures.DEFAULT_MANIFEST)["cases"]
        case = next(case for case in cases if case["proof"] == "video_reference")
        with tempfile.TemporaryDirectory() as temp, \
             patch.object(score.fixtures, "verify_bytes", side_effect=AssertionError(
                 "committed reference must not require a hash")), \
             patch.object(score.fixtures, "stage_case", side_effect=score.ScoreError(
                 "stopped after local reference selection")) as stage:
            result = score.score_case(case, Path(temp), "tsrct-conv", "validation", "tsrct")
        self.assertEqual(result["status"], "failed")
        self.assertIn("stopped after local reference selection", result["reason"])
        stage.assert_called_once()

    def test_failed_visual_score_still_preserves_same_run_video_and_frame_data(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            reference = root / "reference.mp4"
            reference.write_bytes(b"Adobe")
            case = {"id": "pinned", "proof": "video_reference", "sequence": "uid",
                    "project": "project.prproj", "files": [], "reference_video": {
                        "repo_path": "tests/references/premiere/videos/premiere_two_video_tracks.mp4"},
                    "score_policy": {"min_mean_similarity": 0.99,
                                     "min_frame_similarity": 0.98,
                                     "sample_interval_secs": 0.5}}
            frames = [{"time_secs": i * 0.5, "score": {"similarity": value}}
                      for i, value in enumerate((1.0, 1.0, 0.5, 1.0, 1.0))]
            comparison = {"compared_frames": 5, "frame_results": frames,
                          "score": {"similarity": 0.9}, "min_frame_similarity": 0.5}
            case["files"] = [{"path": "fonts/Arial-BoldMT.ttf"}]
            validation_calls = []
            imported_fonts = []

            def fake_run(args, _timeout, *, env=None):
                output = Path(args[args.index("--output") + 1]) if "--output" in args else None
                if args[0] == "tsrct-conv":
                    self.assertEqual(args[1], "convert")
                    self.assertEqual(args[2], root / "cases/pinned/project.prproj")
                    self.assertEqual(args[3:7], ["--to", "tesseract", "--sequence", "uid"])
                    self.assertEqual(args[7], "--output")
                    output.mkdir()
                    (output / "project.tsrct").write_bytes(b"converted")
                    return ""
                if args[:3] == ["tsrct", "project", "import-font"]:
                    imported_fonts.append(Path(args[args.index("--file") + 1]))
                    self.assertEqual(Path(args[args.index("--project") + 1]).read_bytes(), b"converted")
                    return ""
                if args[:2] == ["tsrct", "export"]:
                    self.assertEqual(imported_fonts, [root / "cases/pinned/fonts/Arial-BoldMT.ttf"])
                    self.assertEqual(args[-4:], ["--encoder-backend", "external-ffmpeg-command",
                                                 "--ffmpeg-path", "ffmpeg"])
                    self.assertEqual(env["JERBOA_FORCE_SOFTWARE_DECODE"], "1")
                    output.write_bytes(b"rendered")
                    return ""
                if args[0] == "validation":
                    validation_calls.append(args)
                    return json.dumps(comparison)
                raise AssertionError(f"unexpected command: {args}")

            def fake_stage(_case, output, *, reuse):
                output.mkdir(parents=True)

            artifacts = root / "artifacts"
            with patch.object(score.fixtures, "stage_case", side_effect=fake_stage), \
                 patch.object(score, "run", side_effect=fake_run), \
                 patch.object(score, "metadata", return_value={
                     "width": 1920, "height": 1080, "fps": Fraction(30),
                     "duration_secs": 2.0}):
                result = score.score_case(case, root, "tsrct-conv", "validation", "tsrct",
                                          reference_file=reference, artifacts_dir=artifacts,
                                          ffmpeg="ffmpeg")
            self.assertEqual(result["status"], "failed")
            self.assertIn("below configured", result["reason"])
            self.assertIn("mean=0.900000, min=0.500000 at 1.000s", result["reason"])
            self.assertEqual(len(validation_calls), 1)
            self.assertIn("--canonical-rgb24", validation_calls[0])
            self.assertEqual((artifacts / "tesseract.mp4").read_bytes(), b"rendered")
            self.assertEqual(json.loads((artifacts / "comparison.json").read_text()), comparison)

    def test_export_renders_at_the_reference_rate_or_fails(self):
        for fps in (24, 30, 60):
            self.assertEqual(score.export_fps({"fps": Fraction(fps)}), fps)
        for fps in (Fraction(24000, 1001), Fraction(25), Fraction(30000, 1001)):
            with self.assertRaisesRegex(score.ScoreError, "cannot render"):
                score.export_fps({"fps": fps})

    def test_score_case_uses_the_converter_cli_and_the_reference_rate(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            reference = root / "reference.mp4"
            reference.write_bytes(b"Adobe")
            case = {"id": "pinned", "proof": "video_reference", "sequence": "uid",
                    "project": "project.prproj", "files": [], "reference_video": {
                        "repo_path": "tests/references/premiere/videos/premiere_two_video_tracks.mp4"},
                    "score_policy": {"min_mean_similarity": 0.99,
                                     "min_frame_similarity": 0.98,
                                     "sample_interval_secs": 0.5}}
            frames = [{"time_secs": i * 0.5, "score": {"similarity": 1.0}} for i in range(5)]
            comparison = {"compared_frames": 5, "frame_results": frames,
                          "score": {"similarity": 1.0}, "min_frame_similarity": 1.0}
            calls = {}
            environments = {}

            def fake_run(args, _timeout, *, env=None):
                calls[args[0]] = [str(arg) for arg in args]
                environments[args[0]] = env
                output = Path(args[args.index("--output") + 1]) if "--output" in args else None
                if args[0] == "tsrct-conv":
                    output.mkdir()
                    (output / "project.tsrct").write_bytes(b"converted")
                    return ""
                if args[0] == "tsrct":
                    output.write_bytes(b"rendered")
                    return ""
                return json.dumps(comparison)

            def fake_stage(_case, output, *, reuse):
                output.mkdir(parents=True)

            with patch.dict(os.environ, {"JERBOA_FORCE_SOFTWARE_DECODE": "0",
                                         "CONVERSION_TEST_ENV": "retained"}), \
                 patch.object(score.fixtures, "stage_case", side_effect=fake_stage), \
                 patch.object(score, "run", side_effect=fake_run), \
                 patch.object(score, "metadata", return_value={
                     "width": 1920, "height": 1080, "fps": Fraction(24),
                     "duration_secs": 2.0}):
                result = score.score_case(case, root, "tsrct-conv", "validation", "tsrct",
                                          reference_file=reference)
            self.assertEqual(result["status"], "scored", result)
            converter = calls["tsrct-conv"]
            self.assertEqual(converter[1], "convert")
            self.assertEqual(converter[2], str(root / "cases/pinned/project.prproj"))
            self.assertEqual(converter[3:7], ["--to", "tesseract", "--sequence", "uid"])
            self.assertEqual(converter[7], "--output")
            self.assertEqual(calls["tsrct"][-2:], ["--fps", "24"])
            self.assertIsNotNone(environments["tsrct"])
            self.assertEqual(environments["tsrct"]["JERBOA_FORCE_SOFTWARE_DECODE"], "1")
            self.assertEqual(environments["tsrct"]["CONVERSION_TEST_ENV"], "retained")
            self.assertIsNone(environments["tsrct-conv"])
            self.assertIsNone(environments["validation"])

    def test_run_sets_decode_mode_only_for_the_child(self):
        with patch.dict(os.environ, {"JERBOA_FORCE_SOFTWARE_DECODE": "0"}):
            child_env = {**os.environ, "JERBOA_FORCE_SOFTWARE_DECODE": "1"}
            output = score.run([sys.executable, "-c",
                                "import os; print(os.environ['JERBOA_FORCE_SOFTWARE_DECODE'])"],
                               10, env=child_env)
            self.assertEqual(output.strip(), "1")
            self.assertEqual(os.environ["JERBOA_FORCE_SOFTWARE_DECODE"], "0")

    def test_missing_reference_mp4_cannot_silently_leave_the_gpu_suite(self):
        with tempfile.TemporaryDirectory() as temp:
            reference_dir = Path(temp)
            (reference_dir / "one.mp4").write_bytes(b"first")
            cases = [{"id": name, "proof": "video_reference", "score_policy": {}}
                     for name in ("one", "two")]
            with patch.object(score.fixtures, "load_manifest", return_value={"cases": cases}), \
                 patch.object(sys, "argv", ["check-prproj-scores.py", "--references",
                                            "--reference-dir", str(reference_dir),
                                            "--ffmpeg", sys.executable]), \
                 redirect_stderr(io.StringIO()) as stderr, \
                 self.assertRaises(SystemExit) as exit_status:
                score.main()
            self.assertNotEqual(exit_status.exception.code, 0)
            self.assertIn("missing=['two']", stderr.getvalue())

    def test_known_conversion_blocker_is_unsupported_not_zero_or_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            cache = Path(temp)
            case = score.fixtures.load_manifest(score.fixtures.DEFAULT_MANIFEST)["cases"][0]
            with patch.object(score.fixtures, "stage_case"), \
                 patch.object(score, "run", side_effect=score.ScoreError(
                     "tsrct-conv exited 1: unsupported conversion: VideoTrackGroup:71: missing ComponentOwner")):
                outcome = score.score_case(case, cache, "tsrct-conv", "validation", "tsrct")
                self.assertEqual(outcome["status"], "unsupported")
                self.assertNotIn("approved", outcome)
                self.assertNotIn("mean_similarity", outcome)
            with patch.object(score.fixtures, "stage_case"), \
                 patch.object(score, "run", side_effect=score.ScoreError(
                     "tsrct-conv exited 1: invalid project format")):
                self.assertEqual(score.score_case(case, cache, "tsrct-conv", "validation", "tsrct")["status"],
                                 "failed")


@pytest.mark.parametrize("duration,interval,expected", [
    (2.5, 1.0, [0.0, 1.0, 2.0]),
    (2.0, 1.0, [0.0, 1.0, 2.0]),
    (0.3, 0.1, [0.0, 0.1, 0.2]),  # match floating multiplication in validation
])
def test_expected_sampling_schedule(duration, interval, expected):
    assert score.expected_sample_times(duration, interval, 10) == expected


def test_incomplete_or_mistimed_comparison_fails_before_scoring():
    expected = score.expected_sample_times(2.5, 1.0, 10)

    def comparison(times):
        return {"compared_frames": len(times), "frame_results": [
            {"time_secs": time, "score": {"similarity": 1.0}} for time in times],
            "score": {"similarity": 1.0}, "min_frame_similarity": 1.0}

    assert score.evaluate(comparison(expected), expected_times=expected)["sampled_frames"] == 3
    for times in ([0.0, 1.0], [0.0, 2.0], [0.0, 1.0, 2.1], [0.1, 1.0, 2.0]):
        with pytest.raises(score.ScoreError, match="sampling schedule"):
            score.evaluate(comparison(times), expected_times=expected)
    with pytest.raises(score.ScoreError, match="max-samples"):
        score.expected_sample_times(2.5, 1.0, 2)


if __name__ == "__main__":
    unittest.main()
