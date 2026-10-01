#!/usr/bin/env python3
"""Offline stdlib tests for the bounded local AEP scoring runner."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import time
import unittest

import aep_test


class FakeHarness:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.source = root / "fixtures/native.aep"
        self.reference = root / "reference.mp4"
        self.source.parent.mkdir(parents=True)
        self.source.write_bytes(b"independent adobe source")
        self.reference.write_bytes(b"independent adobe render")
        self.tools: dict[str, Path] = {}
        for name in ("tsrct-conv", "tsrct", "validation_cli", "ffprobe"):
            path = root / "bin" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes((name + "\n").encode())
            path.chmod(path.stat().st_mode | stat.S_IXUSR)
            self.tools[name] = path
        self.case = {
            "case_id": "aep-fixtures-native-c1",
            "direction": "import",
            "feature": "animated native control retains editable values",
            "requirement": "required",
            "supported_by": [],
            "source_path": "fixtures/native.aep",
            "composition_id": 1,
            "critical_frames": ["0", "14/30", "1/2", "29/30", "1", "59/30"],
            "tests": [
                {
                    "path": "crate/tests.rs",
                    "symbol": "native_control_is_editable",
                    "assertions": [{"description": "concrete editable values", "source_anchor": "ORACLE"}],
                }
            ],
            "execution": {"status": "UNRUN"},
            "visual": {"status": "unmeasured"},
            "limitations": ["RGB is not alpha proof."],
            "pending_reason": "External execution is intentionally separate.",
        }
        self.source_record = {
            "source_path": self.case["source_path"],
            "source_sha256": self._sha(self.source),
            "source_bytes": self.source.stat().st_size,
            "media_dependencies": [],
        }
        self.composition = {
            "composition_id": 1,
            "composition_name": "NativeControl",
            "width": 320,
            "height": 180,
            "fps_numerator": 24,
            "fps_denominator": 1,
            "duration_numerator": 2,
            "duration_denominator": 1,
            "frame_count": 48,
            "display_start_numerator": 0,
            "display_start_denominator": 1,
            "pixel_aspect_numerator": 1,
            "pixel_aspect_denominator": 1,
            "status": "verified",
            "expected_frame_count": 60,
            "reference": {
                "path": "reference.mp4",
                "fps": 30,
                "frame_count": 60,
                "duration_seconds": 2.0,
                "width": 320,
                "height": 180,
                "pixel_format": "yuv420p",
                "color_space": "bt709",
                "color_transfer": "bt709",
                "color_primaries": "bt709",
                "audio_streams": [],
                "render_log_sha256": "a" * 64,
            },
            "verification": {
                "native_render": True,
                "full_decode": True,
                "visual_inspection": "Feature-critical samples inspected.",
                "fx_render_comparison": "not_run",
                "alpha_fidelity": "unverified",
                "audio_fidelity": "unverified",
            },
        }
        self.settings = {
            "renderer": "Adobe After Effects aerender",
            "adobe_version": "26.5x89",
            "output_fps": 30,
            "render_settings": "Use this frame rate: 30; Quality: Best; Resolution: Full",
            "output_module_template": "H.264",
            "range_policy": "Entire composition",
            "source_fps_unchanged": True,
            "retention": "long_term",
            "color_policy": "RGB only",
        }
        self.calls: list[tuple[list[str], int]] = []
        self.mode = "success"
        self.reference_fps = "30/1"
        self.actual_fps = "30/1"
        self.reference_color_space = "bt709"
        self.comparison = self.valid_comparison()

    @staticmethod
    def _sha(path: Path) -> str:
        return hashlib.sha256(path.read_bytes()).hexdigest()

    @staticmethod
    def valid_comparison() -> dict:
        samples = [
            {
                "index": index,
                "time_secs": index / 30,
                "score": {"similarity": 0.9 + index / 6000},
            }
            for index in range(61)
        ]
        samples[-1]["score"]["similarity"] = samples[-2]["score"]["similarity"]
        scores = [sample["score"]["similarity"] for sample in samples]
        return {
            "compared_frames": 61,
            "score": {"similarity": sum(scores) / len(scores)},
            "min_frame_similarity": min(scores),
            "frame_results": samples,
        }

    def tool_map(self) -> dict[str, Path]:
        return {
            "tesseract_conv": self.tools["tsrct-conv"],
            "tsrct": self.tools["tsrct"],
            "validation": self.tools["validation_cli"],
            "ffprobe": self.tools["ffprobe"],
        }

    def enable_expression_samples(self, source_sha256: str | None = None) -> Path:
        path = self.root / "fixtures/expression_samples.json"
        path.write_text(
            json.dumps(
                {
                    "version": 2,
                    "source_sha256": source_sha256 or self.source_record["source_sha256"],
                    "sample_interval_ms": 1,
                    "capture_scope": {
                        "mode": "selected_composition",
                        "root_composition_id": self.composition["composition_id"],
                    },
                    "properties": [],
                    "errors": [],
                }
            )
        )
        self.case["expression_samples"] = {
            "path": "fixtures/expression_samples.json",
            "bytes": path.stat().st_size,
            "sha256": self._sha(path),
        }
        return path

    def executor(self, command: list[str], timeout: int) -> aep_test.CompletedCommand:
        self.calls.append((command, timeout))
        executable = Path(command[0]).name
        if executable == "ffprobe":
            is_reference = Path(command[-1]).name == "reference.mp4"
            fps = self.reference_fps if is_reference else self.actual_fps
            value = {
                "streams": [
                    {
                        "codec_type": "video",
                        "width": 320,
                        "height": 180,
                        "avg_frame_rate": fps,
                        "nb_frames": "60",
                        "nb_read_frames": "60",
                        "pix_fmt": "yuv420p",
                        "color_space": self.reference_color_space if is_reference else "bt709",
                        "color_transfer": "bt709",
                        "color_primaries": "bt709",
                    }
                ],
                "format": {"duration": "2.000000"},
            }
            return aep_test.CompletedCommand(0, json.dumps(value), "")
        if executable == "tsrct-conv":
            if self.mode == "timeout":
                raise subprocess.TimeoutExpired(command, timeout)
            if self.mode == "cancelled":
                raise KeyboardInterrupt
            if self.mode == "unsupported":
                return aep_test.CompletedCommand(1, "", "unsupported conversion: native control")
            if self.mode != "partial_import":
                output = Path(command[command.index("--output") + 1])
                output.mkdir(parents=True)
                (output / "project.tsrct").write_bytes(b"fresh editable project")
            return aep_test.CompletedCommand(0, "imported", "warning: bounded approximation")
        if executable == "tsrct":
            if command[1:3] == ["project", "import-font"]:
                if self.mode == "font_failure":
                    return aep_test.CompletedCommand(1, "", "invalid font")
                return aep_test.CompletedCommand(0, '{"font":"Example-Regular"}', "")
            if self.mode == "missing_font" and not any(
                call[1:3] == ["project", "import-font"] for call, _ in self.calls
            ):
                return aep_test.CompletedCommand(1, "", '{"error":"missing_fonts"}')
            if self.mode == "render_failure":
                return aep_test.CompletedCommand(2, "", "render failed")
            output = Path(command[command.index("--output") + 1])
            output.write_bytes(b"fresh tesseract video")
            return aep_test.CompletedCommand(0, "rendered", "")
        if executable == "validation_cli":
            return aep_test.CompletedCommand(0, json.dumps(self.comparison), "")
        if executable == "git":
            return aep_test.CompletedCommand(0, "1" * 40 + "\n", "")
        raise AssertionError(command)

    def run(self, **kwargs) -> dict:
        return aep_test.score_case(
            self.root,
            self.case,
            (self.source_record, self.composition),
            self.settings,
            self.root / "run" / self.case["case_id"],
            self.tool_map(),
            self.root / "cache",
            self.reference,
            executor=self.executor,
            **kwargs,
        )


class AepScoringTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.harness = FakeHarness(self.root)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_aep_dispatch_is_composition_selected_and_full_size_rgb24(self) -> None:
        result = self.harness.run()
        self.assertEqual(result["status"], "scored")
        commands = [command for command, _timeout in self.harness.calls]
        conversion = next(command for command in commands if Path(command[0]).name == "tsrct-conv")
        case_dir = self.root / "run" / self.harness.case["case_id"]
        self.assertEqual(
            conversion,
            [
                str(self.harness.tools["tsrct-conv"].resolve()),
                "convert",
                str(case_dir / "source.aep"),
                "--to", "tesseract",
                "--composition", "1",
                "--output", str(case_dir / "conversion"),
            ],
        )
        self.assertNotIn("--sequence", conversion)
        rendering = next(command for command in commands if Path(command[0]).name == "tsrct")
        self.assertEqual(rendering[rendering.index("--fps") + 1], "30")
        comparison = next(command for command in commands if Path(command[0]).name == "validation_cli")
        self.assertIn("--canonical-rgb24", comparison)
        self.assertEqual(comparison[comparison.index("--max-dimension") + 1], "320")
        self.assertNotIn("pass", result)
        self.assertEqual(result["cpu_test_execution"]["registry_historical_status"], "UNRUN")

    def test_expression_samples_are_staged_passed_and_recorded(self) -> None:
        original = self.harness.enable_expression_samples()
        original_bytes = original.read_bytes()
        result = self.harness.run()
        self.assertEqual(result["status"], "scored")
        conversion = next(
            command
            for command, _timeout in self.harness.calls
            if Path(command[0]).name == "tsrct-conv"
        )
        staged = Path(conversion[conversion.index("--expression-samples") + 1])
        self.assertEqual(staged.name, "expression_samples.json")
        self.assertNotEqual(staged, original)
        self.assertEqual(staged.read_bytes(), original_bytes)
        identity = result["expression_samples"]
        self.assertEqual(identity["source"]["sha256"], self.harness._sha(original))
        self.assertEqual(identity["staged"]["sha256"], self.harness._sha(staged))
        self.assertEqual(
            identity["bound_aep_source_sha256"],
            self.harness.source_record["source_sha256"],
        )
        self.assertEqual(
            identity["capture_scope"],
            {"mode": "selected_composition", "root_composition_id": 1},
        )
        self.assertEqual(original.read_bytes(), original_bytes)

    def test_expression_samples_identity_mismatches_fail_before_conversion(self) -> None:
        for mismatch in ("bytes", "sha256"):
            with self.subTest(mismatch=mismatch), tempfile.TemporaryDirectory() as directory:
                harness = FakeHarness(Path(directory))
                harness.enable_expression_samples()
                if mismatch == "bytes":
                    harness.case["expression_samples"]["bytes"] += 1
                else:
                    harness.case["expression_samples"]["sha256"] = "f" * 64
                result = harness.run()
                self.assertEqual(result["status"], "identity_rejected")
                self.assertIn("expression samples", result["reason"])
                self.assertEqual(harness.calls, [])

    def test_expression_samples_must_bind_to_registered_source(self) -> None:
        self.harness.enable_expression_samples("f" * 64)
        result = self.harness.run()
        self.assertEqual(result["status"], "identity_rejected")
        self.assertIn("source_sha256", result["reason"])
        self.assertEqual(self.harness.calls, [])

    def test_expression_samples_must_bind_to_registered_root(self) -> None:
        path = self.harness.enable_expression_samples()
        payload = json.loads(path.read_text())
        payload["capture_scope"]["root_composition_id"] = 99
        path.write_text(json.dumps(payload))
        self.harness.case["expression_samples"].update(
            bytes=path.stat().st_size,
            sha256=self.harness._sha(path),
        )
        result = self.harness.run()
        self.assertEqual(result["status"], "identity_rejected")
        self.assertIn("capture root", result["reason"])
        self.assertEqual(self.harness.calls, [])

    def test_mutated_staged_expression_samples_are_rejected_without_original_writes(self) -> None:
        original = self.harness.enable_expression_samples()
        original_bytes = original.read_bytes()
        original_executor = self.harness.executor

        def executor(command: list[str], timeout: int) -> aep_test.CompletedCommand:
            result = original_executor(command, timeout)
            if Path(command[0]).name == "tsrct-conv":
                staged = Path(command[command.index("--expression-samples") + 1])
                staged.chmod(0o644)
                staged.write_bytes(b"mutated scratch sidecar")
            return result

        self.harness.executor = executor
        result = self.harness.run()
        self.assertEqual(result["status"], "identity_rejected")
        self.assertIn("staged expression samples", result["reason"])
        self.assertEqual(original.read_bytes(), original_bytes)
        self.assertNotIn("comparison", result)

    def test_missing_critical_schedule_still_compares_every_native_frame(self) -> None:
        self.harness.case["critical_frames"] = []
        result = self.harness.run()
        self.assertEqual(result["status"], "scored")
        comparison = result["comparison"]
        self.assertEqual(comparison["critical_frame_schedule_status"], "not_declared")
        self.assertEqual(comparison["critical_frames"], [])
        self.assertEqual(comparison["half_open_native_frames"]["unique_frame_count"], 60)
        self.assertEqual(len(comparison["frame_results"]), 61)
        self.assertTrue(any(Path(command[0]).name == "validation_cli" for command, _ in self.harness.calls))

    def test_missing_critical_schedule_does_not_hide_render_failure(self) -> None:
        self.harness.case["critical_frames"] = []
        self.harness.mode = "render_failure"
        result = self.harness.run()
        self.assertEqual(result["status"], "render_failed")
        self.assertNotIn("comparison", result)
        self.assertTrue(any(Path(command[0]).name == "tsrct" for command, _ in self.harness.calls))

    def test_explicit_fonts_are_snapshotted_and_packaged_before_render(self) -> None:
        fonts = [self.harness.root / name for name in ("Font One.ttf", "Collection.ttc")]
        for font in fonts:
            font.write_bytes(font.name.encode())
        self.harness.mode = "missing_font"
        result = self.harness.run(font_files=fonts)
        self.assertEqual(result["status"], "scored")
        stages = [command["stage"] for command in result["commands"]]
        self.assertEqual(stages.count("font-import"), 2)
        self.assertLess(stages.index("import"), stages.index("font-import"))
        self.assertLess(stages.index("font-import"), stages.index("render"))
        for font, identity in zip(fonts, result["font_inputs"], strict=True):
            staged = self.harness.root / "run" / self.harness.case["case_id"] / identity["staged_path"]
            self.assertNotEqual(staged, font)
            self.assertEqual(staged.read_bytes(), font.read_bytes())
            self.assertEqual(identity["sha256"], self.harness._sha(font))
            self.assertEqual(identity["bytes"], font.stat().st_size)
            self.assertEqual(identity["reference_font_identity"], "unverified")
            self.assertTrue(any(str(staged) in call for call, _ in self.harness.calls))

    def test_font_import_failure_is_recorded_without_rendering(self) -> None:
        font = self.harness.root / "Invalid.ttf"
        font.write_bytes(b"invalid")
        self.harness.mode = "font_failure"
        result = self.harness.run(font_files=[font])
        self.assertEqual(result["status"], "font_import_failed")
        self.assertIn("invalid font", result["reason"])
        self.assertNotIn("render", [command["stage"] for command in result["commands"]])
        self.assertNotIn("comparison", result)

    def test_no_explicit_font_does_not_silently_substitute(self) -> None:
        self.harness.mode = "missing_font"
        result = self.harness.run()
        self.assertEqual(result["status"], "render_failed")
        self.assertNotIn("font-import", [command["stage"] for command in result["commands"]])

    def test_font_snapshot_mutation_is_rejected_without_modifying_original(self) -> None:
        font = self.harness.root / "Example.ttf"
        font.write_bytes(b"original font")
        original_executor = self.harness.executor

        def executor(command: list[str], timeout: int) -> aep_test.CompletedCommand:
            result = original_executor(command, timeout)
            if command[1:3] == ["project", "import-font"]:
                staged = Path(command[command.index("--file") + 1])
                staged.chmod(0o644)
                staged.write_bytes(b"tampered")
            return result

        self.harness.executor = executor
        result = self.harness.run(font_files=[font])
        self.assertEqual(result["status"], "identity_rejected")
        self.assertEqual(font.read_bytes(), b"original font")
        self.assertNotIn("comparison", result)

    def test_font_failure_does_not_prevent_the_next_case_comparison(self) -> None:
        font = self.root / "Example.ttf"
        font.write_bytes(b"local font")
        second = {**self.harness.case, "case_id": "aep-fixtures-second-c1"}
        original_executor = self.harness.executor
        self.harness.mode = "font_failure"

        def executor(command: list[str], timeout: int) -> aep_test.CompletedCommand:
            result = original_executor(command, timeout)
            if command[1:3] == ["project", "import-font"]:
                self.harness.mode = "success"
            return result

        report, code = aep_test.run_selected(
            self.root,
            [self.harness.case, second],
            {(self.harness.case["source_path"], 1): (self.harness.source_record, self.harness.composition)},
            self.harness.settings,
            self.root / "multi-run",
            self.harness.tool_map(),
            self.root / "cache",
            {case["case_id"]: self.harness.reference for case in (self.harness.case, second)},
            font_files=[font],
            executor=executor,
        )
        self.assertEqual(code, 1)
        self.assertEqual([case["status"] for case in report["cases"]], ["font_import_failed", "scored"])
        self.assertEqual(report["counts"], {"font_import_failed": 1, "scored": 1})
        self.assertEqual(len(report["cases"][1]["font_inputs"]), 1)

    def test_missing_explicit_font_is_a_classified_font_failure(self) -> None:
        result = self.harness.run(font_files=[self.root / "absent.ttf"])
        self.assertEqual(result["status"], "font_import_failed")
        self.assertNotIn("render", [command["stage"] for command in result["commands"]])

    def test_unexpandable_font_path_finalizes_every_case(self) -> None:
        class UnexpandableFont:
            def expanduser(self) -> Path:
                raise RuntimeError("Could not determine home directory.")

        second = {**self.harness.case, "case_id": "aep-fixtures-second-c1"}
        run_dir = self.root / "invalid-font-run"
        report, code = aep_test.run_selected(
            self.root,
            [self.harness.case, second],
            {(self.harness.case["source_path"], 1): (self.harness.source_record, self.harness.composition)},
            self.harness.settings,
            run_dir,
            self.harness.tool_map(),
            self.root / "cache",
            {case["case_id"]: self.harness.reference for case in (self.harness.case, second)},
            font_files=[UnexpandableFont()],
            executor=self.harness.executor,
        )
        self.assertEqual(code, 1)
        self.assertEqual(report["counts"], {"font_import_failed": 2})
        self.assertTrue(report["finished_at"])
        self.assertEqual(json.loads((run_dir / "report.json").read_text()), report)
        for case in report["cases"]:
            self.assertIn("Could not determine home directory", case["reason"])
            self.assertEqual(json.loads((run_dir / case["case_id"] / "result.json").read_text()), case)
            self.assertNotIn("render", [command["stage"] for command in case["commands"]])

    def test_cli_accepts_repeated_explicit_font_paths(self) -> None:
        args = aep_test.build_parser().parse_args([
            "run", "--case-id", "example", "--tsrct", "tsrct",
            "--font", "Font One.ttf", "--font", "Collection.ttc",
        ])
        self.assertEqual(args.font, [Path("Font One.ttf"), Path("Collection.ttc")])

    def test_source_hash_mismatch_is_identity_rejection_not_score_zero(self) -> None:
        self.harness.source.write_bytes(b"changed source")
        result = self.harness.run()
        self.assertEqual(result["status"], "identity_rejected")
        self.assertNotIn("comparison", result)
        self.assertNotIn("score", result)
        self.assertEqual(self.harness.calls, [])

    def test_reference_metadata_mismatch_is_rejected_before_import(self) -> None:
        self.harness.reference_fps = "24/1"
        result = self.harness.run()
        self.assertEqual(result["status"], "identity_rejected")
        self.assertIn("not exactly 30fps", result["reason"])
        self.assertFalse(any(Path(command[0]).name == "tsrct-conv" for command, _ in self.harness.calls))
        self.assertNotIn("comparison", result)

    def test_partial_import_output_is_an_import_failure(self) -> None:
        self.harness.mode = "partial_import"
        result = self.harness.run()
        self.assertEqual(result["status"], "import_failed")
        self.assertIn("project.tsrct", result["reason"])
        self.assertNotIn("comparison", result)

    def test_missing_dependency_is_distinct_and_never_scored(self) -> None:
        self.harness.tools["validation_cli"].unlink()
        result = self.harness.run()
        self.assertEqual(result["status"], "missing_dependency")
        self.assertNotIn("comparison", result)
        self.assertNotIn("score", result)

    def test_timeout_is_distinct_and_never_scored(self) -> None:
        self.harness.mode = "timeout"
        result = self.harness.run()
        self.assertEqual(result["status"], "timeout")
        self.assertNotIn("comparison", result)
        self.assertNotIn("score", result)
        self.assertEqual(result["commands"][-1]["status"], "timeout")

    def test_cancellation_is_distinct_and_never_scored(self) -> None:
        self.harness.mode = "cancelled"
        result = self.harness.run()
        self.assertEqual(result["status"], "cancelled")
        self.assertNotIn("comparison", result)
        self.assertNotIn("score", result)
        self.assertEqual(result["commands"][-1]["status"], "cancelled")

    def test_human_diagnostic_text_does_not_infer_unsupported_semantics(self) -> None:
        self.harness.mode = "unsupported"
        result = self.harness.run()
        self.assertEqual(result["status"], "import_failed")
        self.assertNotIn("comparison", result)

    def test_structured_unsupported_failure_remains_distinct(self) -> None:
        def executor(command: list[str], timeout: int) -> aep_test.CompletedCommand:
            if Path(command[0]).name == "tsrct-conv":
                raise aep_test.UnsupportedConversion("structured unsupported result")
            return self.harness.executor(command, timeout)

        result = aep_test.score_case(
            self.root,
            self.harness.case,
            (self.harness.source_record, self.harness.composition),
            self.harness.settings,
            self.root / "run" / "structured-unsupported",
            self.harness.tool_map(),
            self.root / "cache",
            self.harness.reference,
            executor=executor,
        )
        self.assertEqual(result["status"], "unsupported")
        self.assertNotIn("comparison", result)

    def test_low_similarity_is_preserved_as_descriptive_score_not_failure(self) -> None:
        self.harness.comparison = self.harness.valid_comparison()
        for sample in self.harness.comparison["frame_results"]:
            sample["score"]["similarity"] = 0.0
        self.harness.comparison["score"]["similarity"] = 0.0
        self.harness.comparison["min_frame_similarity"] = 0.0
        result = self.harness.run()
        self.assertEqual(result["status"], "scored")
        self.assertEqual(result["comparison"]["comparator_inclusive"]["mean_similarity"], 0.0)
        self.assertEqual(result["comparison"]["quality_verdict"], "not_defined_descriptive_only")

    def test_rendered_video_metadata_failure_is_render_failed_not_identity_rejected(self) -> None:
        self.harness.actual_fps = "24/1"
        result = self.harness.run()
        self.assertEqual(result["status"], "render_failed")
        self.assertIn("not exactly 30fps", result["reason"])
        self.assertNotIn("comparison", result)

    def test_reference_color_metadata_must_match_inventory(self) -> None:
        self.harness.reference_color_space = "smpte170m"
        result = self.harness.run()
        self.assertEqual(result["status"], "identity_rejected")
        self.assertIn("color_space", result["reason"])
        self.assertFalse(any(Path(command[0]).name == "tsrct-conv" for command, _ in self.harness.calls))

    def test_invalid_or_incomplete_samples_are_malformed_not_scored(self) -> None:
        self.harness.comparison = self.harness.valid_comparison()
        self.harness.comparison["frame_results"].pop()
        self.harness.comparison["compared_frames"] = 60
        result = self.harness.run()
        self.assertEqual(result["status"], "malformed")
        self.assertNotIn("comparison", result)
        self.assertNotIn("score", result)

    def test_nonduplicate_terminal_probe_is_malformed_not_a_sixty_first_frame(self) -> None:
        self.harness.comparison["frame_results"][-1]["score"]["similarity"] = 0.1
        scores = [sample["score"]["similarity"] for sample in self.harness.comparison["frame_results"]]
        self.harness.comparison["score"]["similarity"] = sum(scores) / len(scores)
        self.harness.comparison["min_frame_similarity"] = min(scores)
        result = self.harness.run()
        self.assertEqual(result["status"], "malformed")
        self.assertIn("terminal duration probe", result["reason"])

    def test_off_grid_critical_sample_is_an_honest_blocker(self) -> None:
        self.harness.case["critical_frames"] = ["1/100"]
        result = self.harness.run()
        self.assertEqual(result["status"], "blocked")
        self.assertIn("exact 30fps comparator grid", result["reason"])
        self.assertEqual(self.harness.calls, [])

    def test_explicit_local_reference_is_accepted_without_asset_hash_identity(self) -> None:
        local_reference = self.root / "local-reference.mp4"
        local_reference.write_bytes(b"arbitrary movie")
        result = aep_test.score_case(
            self.root,
            self.harness.case,
            (self.harness.source_record, self.harness.composition),
            self.harness.settings,
            self.root / "run" / "local-reference",
            self.harness.tool_map(),
            self.root / "cache",
            local_reference,
            executor=self.harness.executor,
        )
        self.assertEqual(result["status"], "scored")
        self.assertEqual(result["reference"]["origin"], "explicit_local_file")

    def test_committed_local_reference_is_resolved_from_manifest_path(self) -> None:
        result = aep_test.score_case(
            self.root.resolve(),
            self.harness.case,
            (self.harness.source_record, self.harness.composition),
            self.harness.settings,
            self.root / "run" / "committed-reference",
            self.harness.tool_map(),
            self.root / "cache",
            executor=self.harness.executor,
        )
        self.assertEqual(result["status"], "scored")
        self.assertEqual(result["reference"]["path"], "reference.mp4")
        self.assertEqual(result["reference"]["origin"], "committed_local_reference")

    def test_external_tools_receive_scratch_copies_not_immutable_inputs(self) -> None:
        result = self.harness.run()
        self.assertEqual(result["status"], "scored")
        commands = [command for command, _timeout in self.harness.calls]
        conversion = next(command for command in commands if Path(command[0]).name == "tsrct-conv")
        comparison = next(command for command in commands if Path(command[0]).name == "validation_cli")
        self.assertNotEqual(Path(conversion[2]), self.harness.source)
        self.assertEqual(Path(conversion[2]).name, "source.aep")
        self.assertNotEqual(Path(comparison[comparison.index("--left") + 1]), self.harness.reference)
        self.assertEqual(Path(comparison[comparison.index("--left") + 1]).name, "reference.mp4")
        self.assertEqual(self.harness.source.read_bytes(), b"independent adobe source")
        self.assertEqual(self.harness.reference.read_bytes(), b"independent adobe render")

    def test_mutated_staged_source_is_identity_rejected_without_touching_fixture(self) -> None:
        original_executor = self.harness.executor

        def executor(command: list[str], timeout: int) -> aep_test.CompletedCommand:
            result = original_executor(command, timeout)
            if Path(command[0]).name == "tsrct-conv":
                Path(command[2]).chmod(0o644)
                Path(command[2]).write_bytes(b"mutated scratch copy")
            return result

        result = aep_test.score_case(
            self.root,
            self.harness.case,
            (self.harness.source_record, self.harness.composition),
            self.harness.settings,
            self.root / "run" / "mutated-staged-source",
            self.harness.tool_map(),
            self.root / "cache",
            self.harness.reference,
            executor=executor,
        )
        self.assertEqual(result["status"], "identity_rejected")
        self.assertEqual(self.harness.source.read_bytes(), b"independent adobe source")

    def test_terminal_probe_is_not_claimed_as_a_sixty_first_unique_frame(self) -> None:
        result = self.harness.run()
        comparison = result["comparison"]
        self.assertEqual(comparison["comparator_inclusive"]["sample_count"], 61)
        self.assertEqual(comparison["half_open_native_frames"]["unique_frame_count"], 60)
        self.assertFalse(comparison["terminal_probe"]["unique_native_frame"])
        self.assertEqual(comparison["terminal_probe"]["time_seconds"], 2.0)
        self.assertEqual(
            [sample["sample_index"] for sample in comparison["critical_frames"]],
            [0, 14, 15, 29, 30, 59],
        )
        self.assertEqual(comparison["critical_frame_schedule_status"], "declared")

    def test_historical_unrun_registry_is_valid_and_case_filterable(self) -> None:
        rust = self.root / "crate/tests.rs"
        rust.parent.mkdir(parents=True)
        rust.write_text("#[test]\nfn native_control_is_editable() { let _ = \"ORACLE\"; }\n")
        registry = {"schema_version": 1, "scope": "Import-only historical checkpoint metadata.", "cases": [self.harness.case]}
        manifest = {
            "schema_version": 1,
            "scope": "Independent native reference inventory.",
            "reference_settings": self.harness.settings,
            "sources": [{**self.harness.source_record, "compositions": [self.harness.composition]}],
        }
        registry_path = self.root / "registry.json"
        manifest_path = self.root / "references.json"
        registry_path.write_text(json.dumps(registry))
        manifest_path.write_text(json.dumps(manifest))
        cases, targets, _references = aep_test.load_catalog(self.root, registry_path, manifest_path)
        selected = aep_test.select_cases(cases, [self.harness.case["case_id"]])
        self.assertEqual(selected[0]["execution"], {"status": "UNRUN"})
        self.assertIn((self.harness.case["source_path"], 1), targets)

    def test_pinned_native_source_resolves_from_jerboa_and_converter_roots(self) -> None:
        manifest = json.loads(aep_test.DEFAULT_REFERENCES.read_text())
        source = manifest["sources"][0]
        workspaces = [aep_test.REPO]
        enclosing = aep_test.REPO.parents[1]
        if (enclosing / "opensource/conv").resolve() == aep_test.REPO:
            workspaces.append(enclosing)
        for workspace in workspaces:
            path = aep_test.proof.resolve_repo_path(
                aep_test.conversion_workspace(workspace), source["source_path"], must_exist=True
            )
            self.assertEqual(path.stat().st_size, source["source_bytes"])
            self.assertEqual(aep_test.sha256_file(path), source["source_sha256"])

    @unittest.skipUnless(os.name == "posix", "process-group ownership uses POSIX sessions")
    def test_timeout_kills_owned_descendants(self) -> None:
        sentinel = self.root / "orphaned-child"
        child = f"import pathlib,time; time.sleep(0.5); pathlib.Path({str(sentinel)!r}).write_text('orphan')"
        parent = (
            "import subprocess,sys,time; "
            f"subprocess.Popen([sys.executable, '-c', {child!r}]); "
            "time.sleep(10)"
        )
        with self.assertRaises(subprocess.TimeoutExpired):
            aep_test.default_executor([sys.executable, "-c", parent], 0.1)
        time.sleep(0.7)
        self.assertFalse(sentinel.exists())

    def test_fixture_storage_is_rejected_for_outputs_and_cache(self) -> None:
        fixtures = self.root / "opensource/conv/crates/aftereffects_file/tests/fixtures"
        with self.assertRaises(aep_test.BlockedCase):
            aep_test.score_case(
                self.root,
                self.harness.case,
                (self.harness.source_record, self.harness.composition),
                self.harness.settings,
                fixtures / "score-output",
                self.harness.tool_map(),
                self.root / "cache",
                self.harness.reference,
                executor=self.harness.executor,
            )
        with self.assertRaises(aep_test.BlockedCase):
            aep_test.score_case(
                self.root,
                self.harness.case,
                (self.harness.source_record, self.harness.composition),
                self.harness.settings,
                self.root / "safe-output",
                self.harness.tool_map(),
                fixtures / "cache",
                self.harness.reference,
                executor=self.harness.executor,
            )

    def test_case_selection_requires_explicit_unique_known_ids(self) -> None:
        cases = {self.harness.case["case_id"]: self.harness.case}
        with self.assertRaises(aep_test.BlockedCase):
            aep_test.select_cases(cases, [])
        with self.assertRaises(aep_test.BlockedCase):
            aep_test.select_cases(cases, ["missing"])
        with self.assertRaises(aep_test.BlockedCase):
            aep_test.select_cases(cases, [self.harness.case["case_id"]] * 2)


if __name__ == "__main__":
    unittest.main()
