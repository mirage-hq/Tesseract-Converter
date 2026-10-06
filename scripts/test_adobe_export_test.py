#!/usr/bin/env python3
"""Offline tests for the explicit-FX export adapter (no Adobe/network/GPU)."""

from __future__ import annotations

import json
import shutil
import subprocess
import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path
from unittest import mock

import adobe_export_test as adapter

REPO = Path(__file__).resolve().parent.parent


class ExportHarness:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.cases = adapter.load_cases(REPO)
        self.run_index = 0
        self.behaviors: dict[str, str] = {}
        self.commands: list[list[str]] = []
        self.reference = root / "mock-reference.mp4"
        self.reference.write_bytes(b"independent-reference")
        self.test_binary = root / "rust-export-test-binary"
        self.test_binary.write_bytes(b"offline-rust-test-binary")
        self.test_binary_identity = {
            **adapter.aep_test.tool_identity(self.test_binary),
            "fixture_kind": "offline-temporary-binary",
        }
        self.tools: dict[str, Path] = {}
        for name in ("ffprobe", "validation"):
            path = root / name
            path.write_text("#!/bin/sh\nexit 99\n", encoding="utf-8")
            path.chmod(0o755)
            self.tools[name] = path

    @staticmethod
    def _artifact(path: Path) -> dict[str, object]:
        return {
            "path": str(path.resolve()),
            "sha256": adapter.aep_test.sha256_file(path),
            "bytes": path.stat().st_size,
        }

    def _coverage_fx(self, case: adapter.ExportCase) -> dict:
        animated = case.name.endswith("-animated")
        entries = []
        if animated:
            entries = [
                {"target": {"kind": "effectProperty", "paramName": track["param"]}}
                for track in case.case_spec["tracks"]
            ]
        return {
            "duration": 2.0,
            "dimensions": {"width": 320, "height": 180},
            "composition": {
                "name": case.name,
                "dynamics": {"entries": entries},
                "layers": [
                    {
                        "effects": [
                            {
                                "id": 9001,
                                "enabled": True,
                                "effect": case.case_spec["effect"],
                            }
                        ]
                    }
                ],
            },
        }

    def prepare(
        self, mutations: dict[str, dict[str, object]] | None = None
    ) -> tuple[Path, Path]:
        mutations = mutations or {}
        run_dir = self.root / f"run-{self.run_index:03d}"
        self.run_index += 1
        artifacts_dir = run_dir / "fx_exports"
        artifacts_dir.mkdir(parents=True)
        records = []
        for case in self.cases:
            base = artifacts_dir / case.name
            fx_path = base.with_suffix(".fx.json")
            expected_path = base.with_suffix(".expected.json")
            aep_path = base.with_suffix(".aep")
            if case.coverage:
                fx_path.write_text(
                    json.dumps(self._coverage_fx(case)), encoding="utf-8"
                )
                expected_path.write_text(
                    json.dumps(adapter._expected_coverage_artifact_oracle(case)),
                    encoding="utf-8",
                )
            else:
                fixture = REPO / case.fixture_dir / f"{case.name}.fx.json"
                oracle = REPO / case.fixture_dir / f"{case.name}.expected.json"
                shutil.copyfile(fixture, fx_path)
                shutil.copyfile(oracle, expected_path)
            aep_path.write_bytes(f"native:{case.name}".encode())
            artifacts = {
                "fx_json": self._artifact(fx_path),
                "expected_json": self._artifact(expected_path),
                "aep": self._artifact(aep_path),
            }
            if case.coverage:
                tsrct_path = base.with_suffix(".tsrct")
                tsrct_path.write_bytes(f"tsrct:{case.name}".encode())
                artifacts["tsrct"] = self._artifact(tsrct_path)
            record: dict[str, object] = {
                "schema_version": 1,
                "case_id": case.case_id,
                "case_name": case.name,
                "direction": "export",
                "test_symbol": case.test_symbol,
                "status": "success",
                "attempted": True,
                "assertions_executed": True,
                "error": None,
                "test_binary": dict(self.test_binary_identity),
                "artifacts": artifacts,
            }
            record.update(mutations.get(case.case_id, {}))
            records.append(record)
        records_path = self.root / f"records-{self.run_index:03d}.jsonl"
        records_path.write_text(
            "".join(json.dumps(record) + "\n" for record in records), encoding="utf-8"
        )
        return run_dir, records_path

    @staticmethod
    def _metadata() -> str:
        return json.dumps(
            {
                "streams": [
                    {
                        "codec_type": "video",
                        "codec_name": "h264",
                        "width": 320,
                        "height": 180,
                        "avg_frame_rate": "30/1",
                        "nb_read_frames": "60",
                        "pix_fmt": "yuv420p",
                        "color_space": "smpte170m",
                        "color_transfer": "smpte170m",
                        "color_primaries": "smpte170m",
                    }
                ],
                "format": {"duration": "2.000000"},
            }
        )

    @staticmethod
    def _comparison(score: float = 0.25) -> str:
        samples = [
            {
                "index": index,
                "time_secs": min(index, 60) / 30,
                "score": {"similarity": score},
            }
            for index in range(61)
        ]
        return json.dumps(
            {
                "compared_frames": 61,
                "score": {"similarity": score},
                "min_frame_similarity": score,
                "frame_results": samples,
            }
        )

    def native_execute(self, operation, payload, work, *, timeout):
        assert operation == "render_aep"
        assert payload["settings"] == {"format": "mp4", "fps": 30,
                                       "audio": "off"}
        case_name = payload["composition_id"]
        self.commands.append(["typed-render_aep", "-comp", case_name])
        behavior = self.behaviors.get(case_name, "success")
        if behavior == "timeout":
            raise subprocess.TimeoutExpired("headless-adobe", timeout)
        if behavior == "cancel":
            raise KeyboardInterrupt
        if behavior == "mutate":
            Path(payload["source"]["path"]).write_bytes(b"mutated")
        if behavior == "nonzero":
            raise adapter.adobe_native.NativeAdobeError("render rejected")
        work.mkdir(parents=True)
        log = work / 'aerender.log'
        log.write_text('WARNING: substituted content' if behavior == 'warning' else 'PROGRESS: Done')
        output = work / "output.mp4"
        output.write_bytes(b"mock-mp4")
        return {**self._artifact(output), 'metadata': {'render_log': {
            'path': str(log), 'sha256': adapter.adobe_native.sha256(log)}}}

    def executor(
        self, command: list[str], timeout: int
    ) -> adapter.aep_test.CompletedCommand:
        del timeout
        self.commands.append(command)
        executable = Path(command[0]).name
        if executable == "ffprobe":
            return adapter.aep_test.CompletedCommand(0, self._metadata(), "")
        if executable == "validation":
            return adapter.aep_test.CompletedCommand(0, self._comparison(), "")
        raise AssertionError(f"unexpected command: {command}")

    def run(
        self,
        *,
        mutations: dict[str, dict[str, object]] | None = None,
        records_transform: object | None = None,
        resolver: object | None = None,
        cancelled: adapter.CancelCheck | None = None,
        on_row: adapter.RowCallback | None = None,
        selected_case_ids: list[str] | None = None,
    ) -> list[dict]:
        run_dir, records_path = self.prepare(mutations)
        if records_transform is not None:
            records = [
                json.loads(line) for line in records_path.read_text().splitlines()
            ]
            records = records_transform(records)  # type: ignore[operator]
            records_path.write_text(
                "".join(json.dumps(record) + "\n" for record in records),
                encoding="utf-8",
            )

        def resolve(
            workspace: Path, reference: dict, local: Path | None
        ) -> tuple[Path, str]:
            del workspace, reference, local
            return self.reference, "mocked_committed_local_reference"

        def copy_reference(source: Path, destination: Path) -> Path:
            shutil.copyfile(source, destination)
            return destination

        with ExitStack() as stack:
            stack.enter_context(mock.patch.object(
                adapter.adobe_native, "execute", side_effect=self.native_execute))
            stack.enter_context(
                mock.patch.object(
                    adapter.aep_test,
                    "resolve_reference",
                    side_effect=resolver if resolver is not None else resolve,
                )
            )
            stack.enter_context(
                mock.patch.object(
                    adapter.aep_test, "_copy_reference_input", side_effect=copy_reference
                )
            )
            stack.enter_context(
                mock.patch.object(
                    adapter,
                    "_native_control_evidence",
                    return_value={
                        "attempted": False,
                        "status": "success",
                        "reason": "mock exact-hash independent control evidence",
                        "provenance": "reused_by_exact_hash",
                        "fresh_adobe_inspection": False,
                    },
                )
            )
            return adapter.run_exports(
                workspace=REPO,
                run_dir=run_dir,
                records_path=records_path,
                executor=self.executor,
                cache_dir=self.root / "cache",
                local_references={},
                tools=self.tools,
                timeouts={"render": 10, "metadata": 10, "compare": 10},
                on_row=on_row,
                cancelled=cancelled,
                selected_case_ids=selected_case_ids,
            )


class ExplicitExportAdapterTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.harness = ExportHarness(self.root)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_allowlist_and_synthetic_hashed_cpu_records_have_exact_identity(
        self,
    ) -> None:
        run_dir, records_path = self.harness.prepare()
        cases = adapter.load_cases(REPO)
        parsed, errors = adapter.parse_records(records_path, cases)
        self.assertEqual(len(cases), 104)
        self.assertEqual(sum(not case.coverage and case.test_source == adapter.PANEL_TESTS for case in cases), 8)
        self.assertEqual(sum(case.test_source == adapter.NON_AUDIO_TESTS for case in cases), 9)
        self.assertEqual(sum(case.control_kind == "non_audio" for case in cases), 38)
        self.assertEqual(sum(case.coverage for case in cases), 57)
        self.assertEqual(errors, [])
        self.assertTrue(all(record.valid for record in parsed.values()))
        self.assertTrue(all(record.assertions_succeeded for record in parsed.values()))
        self.assertTrue(
            all(
                record.phase["record"]["test_binary"]
                == self.harness.test_binary_identity
                for record in parsed.values()
            )
        )
        first = cases[0]
        identity = adapter._artifact_identity(
            REPO,
            run_dir,
            records_path,
            run_dir / "fx_exports",
            first,
            parsed[first.case_id].phase["record"],
        )
        self.assertEqual(
            identity["native_aep_sha256"],
            parsed[first.case_id].phase["record"]["artifacts"]["aep"]["sha256"],
        )

    def test_shared_records_keep_the_65_effects_cases_and_reject_unknown_ids(self) -> None:
        _, records_path = self.harness.prepare()
        self.assertEqual(len(adapter.ADJUSTMENT_EXPORT_CASE_IDS), 10)
        with records_path.open("a", encoding="utf-8") as output:
            for case_id in sorted(adapter.ADJUSTMENT_EXPORT_CASE_IDS):
                output.write(json.dumps({"case_id": case_id, "direction": "export"}) + "\n")
        # Standalone conv must not import the enclosing repository's private native-editor adapter.
        with mock.patch.dict("sys.modules", {"adobe_adjustment_test": None}):
            parsed, errors = adapter.parse_records(records_path, adapter.load_cases(REPO))
        self.assertEqual(errors, [])
        self.assertEqual(len(parsed), len(self.harness.cases))
        self.assertTrue(all(record.valid and record.assertions_succeeded for record in parsed.values()))
        with records_path.open("a", encoding="utf-8") as output:
            output.write(json.dumps({"case_id": "fx-export-adjustment-typo"}) + "\n")
        _, errors = adapter.parse_records(records_path, adapter.load_cases(REPO))
        self.assertEqual(len(errors), 1)
        self.assertIn("unknown case_id", errors[0])

    def test_explicit_export_subset_runs_only_the_two_vignette_cases(self) -> None:
        selected = ["fx-export-vignette-static", "fx-export-vignette-animated"]
        rows = self.harness.run(selected_case_ids=selected)
        self.assertEqual([row["case_id"] for row in rows], selected)
        self.assertEqual(len(rows), 2)

    def test_pending_vignette_controls_fail_only_their_cases_without_erasing_prior_evidence(self) -> None:
        cases = adapter.load_cases(REPO)
        inventory, problem = adapter._control_evidence_inventory(REPO, cases)
        self.assertIsNone(problem)
        self.assertEqual(len(inventory), 64)  # 63 inspected Effects cases plus the vector panel
        for name in ("vignette-static", "vignette-animated"):
            case = next(case for case in cases if case.name == name)
            self.assertNotIn(name, inventory)
            phase = adapter._native_control_evidence(case, {}, inventory.get(name), problem)
            self.assertEqual(phase["status"], "failure")
            self.assertIn("absent", phase["reason"])

    def test_control_outcomes_are_reused_only_by_exact_hash_and_failures_stay_failures(
        self,
    ) -> None:
        run_dir, records_path = self.harness.prepare()
        case = self.harness.cases[0]
        parsed, errors = adapter.parse_records(records_path, self.harness.cases)
        self.assertEqual(errors, [])
        identity = adapter._artifact_identity(
            REPO,
            run_dir,
            records_path,
            run_dir / "fx_exports",
            case,
            parsed[case.case_id].phase["record"],
        )
        failed_evidence = (
            "coverage",
            {
                "explicit_fx_sha256": identity["input_sha256"],
                "export_aep_sha256": identity["native_aep_sha256"],
                "native_control_readback": {
                    "status": "failed",
                    "failures": ["native value mismatch"],
                },
            },
        )
        phase = adapter._native_control_evidence(case, identity, failed_evidence, None)
        self.assertEqual(phase["status"], "failure")
        self.assertEqual(phase["provenance"], "reused_by_exact_hash")
        self.assertIn("native value mismatch", phase["reason"])
        self.assertFalse(phase["fresh_adobe_inspection"])

        mismatched = dict(failed_evidence[1])
        mismatched["export_aep_sha256"] = "0" * 64
        phase = adapter._native_control_evidence(
            case, identity, ("coverage", mismatched), None
        )
        self.assertEqual(phase["status"], "failure")
        self.assertEqual(phase["provenance"], "unverified")
        self.assertIn("hashes do not match", phase["reason"])

    def test_valid_render_and_score_is_descriptive_while_old_panel_rows_fail_without_oracle(
        self,
    ) -> None:
        emitted: list[str] = []
        rows = self.harness.run(on_row=lambda row: emitted.append(row["case_id"]))
        coverage = [
            row
            for row in rows
            if row["identity"]["case_name"].endswith(("-static", "-animated"))
        ]
        vector = [row for row in rows if row["case_id"] == "fx-export-vector-rect-size"]
        panel = [row for row in rows if row not in coverage and row not in vector]
        self.assertEqual(len(rows), len(self.harness.cases))
        self.assertEqual(len(emitted), len(rows))
        self.assertEqual(len(coverage), 57)
        self.assertEqual(len(vector), 1)
        self.assertEqual(vector[0]["status"], "success")
        self.assertEqual(len(panel), len(rows) - 58)
        self.assertTrue(
            all("requested_control_oracle" not in row["identity"] for row in panel)
        )
        first_identity = rows[0]["identity"]
        self.assertTrue(
            all(
                row["identity"]["test_binary"] == self.harness.test_binary_identity
                for row in rows
            )
        )
        self.assertEqual(
            first_identity["adapter_source"],
            adapter.aep_test.tool_identity(Path(adapter.__file__).resolve()),
        )
        self.assertEqual(
            first_identity["native_control_evidence_file"],
            adapter.aep_test.tool_identity(
                (REPO / adapter.COVERAGE_CONTROL_EVIDENCE).resolve()
            ),
        )
        self.assertEqual(
            first_identity["tool_identity"],
            {
                name: adapter.aep_test.tool_identity(path.resolve())
                for name, path in self.harness.tools.items()
            },
        )
        self.assertTrue(all("cancelled" not in row for row in rows))
        self.assertTrue(all(row["status"] == "success" for row in coverage))
        self.assertTrue(all(row["score"] == 0.25 for row in coverage))
        self.assertTrue(all(row["status"] == "failure" for row in panel))
        self.assertTrue(
            all(row["native_acceptance"]["status"] == "success" for row in panel)
        )
        self.assertTrue(all(row["score"] is None for row in panel))
        self.assertTrue(
            all(row["score_reason"] == adapter.PANEL_SCORE_REASON for row in panel)
        )
        render = next(
            command
            for command in self.harness.commands
            if Path(command[0]).name == "typed-render_aep"
        )
        self.assertNotIn("-reuse", render)
        # native_execute asserts full-composition output settings (no native-frame bounds).
        self.assertEqual(render[0], "typed-render_aep")

    def test_hue_animated_binds_the_differentiated_native_oracle(self) -> None:
        case_id = "fx-export-hueSaturation-animated"
        case = next(case for case in self.harness.cases if case.case_id == case_id)
        artifact_oracle = adapter._expected_coverage_artifact_oracle(case)
        self.assertEqual(
            [control["name"] for control in artifact_oracle["controls"]],
            [
                "ADBE HUE SATURATION-0008",
                "ADBE HUE SATURATION-0009",
                "ADBE HUE SATURATION-0010",
            ],
        )

        rows = self.harness.run(selected_case_ids=[case_id])
        self.assertEqual(len(rows), 1)
        row = rows[0]
        self.assertEqual(row["native_acceptance"]["status"], "success")
        self.assertEqual(row["scoring"]["status"], "success")
        self.assertEqual(
            [
                control["name"]
                for control in row["identity"]["requested_control_oracle"]["controls"]
            ],
            [
                "ADBE HUE SATURATION-0004",
                "ADBE HUE SATURATION-0005",
                "ADBE HUE SATURATION-0006",
                "ADBE HUE SATURATION-0007",
                "ADBE HUE SATURATION-0008",
                "ADBE HUE SATURATION-0009",
                "ADBE HUE SATURATION-0010",
            ],
        )

        def omit_one_keyable_control(
            records: list[dict[str, object]],
        ) -> list[dict[str, object]]:
            record = next(record for record in records if record["case_id"] == case_id)
            artifacts = record["artifacts"]
            assert isinstance(artifacts, dict)
            expected_path = Path(artifacts["expected_json"]["path"])
            expected = json.loads(expected_path.read_text(encoding="utf-8"))
            expected["controls"] = expected["controls"][:-1]
            expected_path.write_text(json.dumps(expected), encoding="utf-8")
            artifacts["expected_json"] = self.harness._artifact(expected_path)
            return records

        [rejected] = self.harness.run(
            records_transform=omit_one_keyable_control,
            selected_case_ids=[case_id],
        )
        self.assertEqual(rejected["native_acceptance"]["status"], "failure")
        self.assertIn(
            "expected oracle differs from its reviewed native-control scope",
            rejected["native_acceptance"]["reason"],
        )

    def test_zero_exit_warning_and_native_failure_do_not_stop_later_cases(self) -> None:
        self.assertFalse(adapter._has_adobe_diagnostic("Errors: 0\n0 warnings"))
        first, second, third = [case.name for case in self.harness.cases[:3]]
        self.harness.behaviors[first] = "warning"
        self.harness.behaviors[second] = "nonzero"
        rows = self.harness.run()
        by_name = {row["identity"]["case_name"]: row for row in rows}
        self.assertEqual(by_name[first]["status"], "failure")
        self.assertIn("warning/error/fatal", by_name[first]["score_reason"])
        self.assertEqual(by_name[second]["status"], "failure")
        self.assertIn("render rejected", by_name[second]["score_reason"])
        self.assertEqual(by_name[third]["status"], "success")

    def test_failed_cpu_assertion_with_valid_bindings_still_renders_and_scores(
        self,
    ) -> None:
        case = self.harness.cases[0]
        rows = self.harness.run(
            mutations={
                case.case_id: {"status": "failure", "error": "property mismatch"}
            }
        )
        row = next(row for row in rows if row["case_id"] == case.case_id)
        self.assertEqual(row["assertions"]["status"], "failure")
        self.assertEqual(row["native_acceptance"]["status"], "success")
        self.assertEqual(row["scoring"]["status"], "success")
        self.assertEqual(row["score"], 0.25)
        self.assertEqual(row["status"], "failure")

    def test_missing_required_reference_is_scoring_failure_after_native_acceptance(
        self,
    ) -> None:
        calls = 0

        def resolver(*args: object) -> tuple[Path, str]:
            nonlocal calls
            calls += 1
            if calls == 1:
                raise adapter.aep_test.IdentityRejected("reference absent")
            return self.harness.reference, "mocked_hash_verified_reference"

        rows = self.harness.run(resolver=resolver)
        first = rows[0]
        self.assertEqual(first["native_acceptance"]["status"], "success")
        self.assertEqual(first["scoring"]["status"], "failure")
        self.assertIsNone(first["score"])
        self.assertIn("reference absent", first["score_reason"])
        self.assertEqual(rows[1]["status"], "success")

    def test_wrong_thread_and_duplicate_records_block_only_the_affected_cases(
        self,
    ) -> None:
        first, second, third = self.harness.cases[:3]

        def duplicate(records: list[dict]) -> list[dict]:
            records[0]["test_symbol"] = "wrong::thread"
            return [*records, dict(records[1])]

        rows = self.harness.run(records_transform=duplicate)
        by_id = {row["case_id"]: row for row in rows}
        self.assertIn(
            "test_symbol mismatch", by_id[first.case_id]["assertions"]["reason"]
        )
        self.assertEqual(
            by_id[first.case_id]["identity"]["test_binary"],
            self.harness.test_binary_identity,
        )
        self.assertFalse(by_id[first.case_id]["native_acceptance"]["attempted"])
        self.assertIn("duplicate", by_id[second.case_id]["assertions"]["reason"])
        self.assertFalse(by_id[second.case_id]["native_acceptance"]["attempted"])
        self.assertEqual(by_id[third.case_id]["status"], "success")

    def test_aep_mutation_is_detected_even_after_zero_exit(self) -> None:
        first = self.harness.cases[0].name
        self.harness.behaviors[first] = "mutate"
        rows = self.harness.run()
        self.assertEqual(rows[0]["status"], "failure")
        self.assertIn("AEP changed", rows[0]["score_reason"])
        self.assertEqual(rows[1]["status"], "success")

    def test_timeout_continues_but_cancellation_marks_all_remaining_unattempted(
        self,
    ) -> None:
        first, second = self.harness.cases[:2]
        self.harness.behaviors[first.name] = "timeout"
        timed_rows = self.harness.run()
        self.assertEqual(timed_rows[0]["status"], "failure")
        self.assertIn("timed out", timed_rows[0]["score_reason"])
        self.assertEqual(timed_rows[1]["status"], "success")

        self.harness.behaviors.clear()
        self.harness.behaviors[second.name] = "cancel"
        command_offset = len(self.harness.commands)
        emitted: list[dict] = []
        cancelled_rows = self.harness.run(on_row=emitted.append)
        cancellation_commands = self.harness.commands[command_offset:]
        self.assertEqual(cancelled_rows[0]["status"], "success")
        self.assertNotIn("cancelled", cancelled_rows[0])
        self.assertTrue(cancelled_rows[1]["native_acceptance"]["attempted"])
        self.assertTrue(cancelled_rows[1]["cancelled"])
        self.assertFalse(cancelled_rows[2]["native_acceptance"]["attempted"])
        self.assertIn("cancelled before", cancelled_rows[2]["score_reason"])
        self.assertTrue(all(row["cancelled"] for row in cancelled_rows[1:]))
        self.assertEqual(emitted, cancelled_rows)
        self.assertEqual(len(cancelled_rows), len(self.harness.cases))
        rendered_cases = [
            command[command.index("-comp") + 1]
            for command in cancellation_commands
            if Path(command[0]).name == "typed-render_aep"
        ]
        self.assertEqual(rendered_cases, [first.name, second.name])
        self.assertEqual(len(cancellation_commands), 5)

        self.harness.behaviors.clear()
        command_offset = len(self.harness.commands)
        pre_cancelled_rows = self.harness.run(cancelled=lambda: True)
        self.assertEqual(len(pre_cancelled_rows), len(self.harness.cases))
        self.assertTrue(all(row["cancelled"] for row in pre_cancelled_rows))
        self.assertEqual(self.harness.commands[command_offset:], [])

    def test_test_binary_identity_is_required_and_copied_only_when_valid(self) -> None:
        self.assertIsNone(
            adapter._test_binary_error(
                {
                    "path": "/tmp/test-binary",
                    "sha256": "A" * 64,
                    "bytes": 1,
                    "optional_foreign_key": "preserved",
                }
            )
        )
        sha_problem = adapter._test_binary_error(
            {"path": "/tmp/test-binary", "sha256": "z" * 64, "bytes": 1}
        )
        size_problem = adapter._test_binary_error(
            {"path": "/tmp/test-binary", "sha256": "0" * 64, "bytes": True}
        )
        self.assertIsNotNone(sha_problem)
        self.assertIsNotNone(size_problem)
        self.assertIn("sha256", sha_problem or "")
        self.assertIn("bytes", size_problem or "")
        first = self.harness.cases[0]
        rows = self.harness.run(
            mutations={
                first.case_id: {
                    "test_binary": {"path": "", "sha256": "bad", "bytes": 0}
                }
            }
        )
        self.assertIn("test_binary.path", rows[0]["assertions"]["reason"])
        self.assertNotIn("test_binary", rows[0]["identity"])
        self.assertFalse(rows[0]["native_acceptance"]["attempted"])
        self.assertEqual(
            rows[1]["identity"]["test_binary"], self.harness.test_binary_identity
        )
        self.assertEqual(rows[1]["status"], "success")

    def test_single_vector_export_is_registered_bound_and_missing_oracle_fails(self) -> None:
        case = next(case for case in self.harness.cases
                    if case.case_id == "fx-export-vector-rect-size")
        self.assertEqual(case.fixture_dir, adapter.VECTOR_FIXTURES)
        self.assertEqual(case.test_source, adapter.VECTOR_TESTS)
        targets = adapter._reference_targets(REPO, self.harness.cases)
        self.assertNotIn("hueMasterStatic", targets, "import-only oracle cannot score an export")
        targets.pop(case.name)
        with mock.patch.object(adapter, "_reference_targets", return_value=targets):
            rows = self.harness.run(selected_case_ids=[case.case_id])
        self.assertEqual([row["case_id"] for row in rows], [case.case_id])
        row = rows[0]
        self.assertEqual(row["assertions"]["status"], "success")
        self.assertEqual(row["native_acceptance"]["status"], "success")
        self.assertEqual(row["scoring"]["status"], "failure")
        self.assertEqual(row["status"], "failure")
        self.assertIsNone(row["score"])
        self.assertIn(str(adapter.VECTOR_TESTS), row["identity"]["generator_sources"])
        renders = [command for command in self.harness.commands
                   if Path(command[0]).name == "typed-render_aep"]
        self.assertEqual(len(renders), 1)
        self.assertEqual(renders[0][renders[0].index("-comp") + 1], case.name)
        controls = adapter._native_control_evidence(case, row["identity"], None, None)
        self.assertEqual(controls["status"], "failure")
        self.assertFalse(controls["fresh_adobe_inspection"])

    def test_vector_reference_binds_exact_source_and_composition_contract(self) -> None:
        cases = adapter.load_cases(REPO)
        validated = adapter.proof.validate_manifest(adapter._load_object(REPO / adapter.REFERENCES))
        template_source, template_comp = next(iter(validated.values()))
        name = "vector-rect-size"
        validated = {key: value for key, value in validated.items()
                     if value[0]["source_path"] != str(adapter.VECTOR_FIXTURES / f"{name}.aep")}
        source = {**template_source, "source_path": str(adapter.VECTOR_FIXTURES / f"{name}.aep")}
        composition = {
            **template_comp, "composition_name": name, "width": 320, "height": 180,
            "duration_numerator": 2, "duration_denominator": 1, "expected_frame_count": 60,
        }
        # Only the post-validation routing is mocked here; these are not native pins.
        with mock.patch.object(adapter.proof, "validate_manifest", return_value={
            **validated, ("mock-vector-source", 1): (source, composition),
        }):
            self.assertEqual(adapter._reference_targets(REPO, cases)[name], (source, composition))
        for changes in ({"composition_name": "wrong"}, {"width": 321}, {"expected_frame_count": 59}):
            with self.subTest(changes=changes), mock.patch.object(
                adapter.proof, "validate_manifest", return_value={
                    **validated, ("mock-vector-source", 1): (source, {**composition, **changes}),
                }
            ):
                with self.assertRaises(adapter.ExportAdapterError):
                    adapter._reference_targets(REPO, cases)
        with mock.patch.object(adapter.proof, "validate_manifest", return_value={
            **validated, ("wrong-source", 1): ({**source, "source_path": "unrelated.aep"}, composition),
        }):
            self.assertNotIn(name, adapter._reference_targets(REPO, cases))
        with mock.patch.object(adapter.proof, "validate_manifest", return_value={
            **validated, ("unexpected-effects", 1): (
                {**source, "source_path": "fixtures/effects_coverage/other.aep"},
                {**composition, "composition_name": "unexpected"},
            ),
        }):
            # Only the exact pinned import-only oracle is exempt from export matching.
            with self.assertRaisesRegex(adapter.ExportAdapterError, "extra=.*unexpected"):
                adapter._reference_targets(REPO, cases)

    def test_non_audio_cases_register_without_pretending_uninspected_references_pass(self) -> None:
        names = {case.name for case in self.harness.cases
                 if case.test_source == adapter.NON_AUDIO_TESTS}
        self.assertEqual(names, {
            "panel-text-document", "panel-scene-stack",
            "panel-text-animation", "panel-gradient-stroke",
            "panel-text-path", "panel-mask-matte", "panel-motion-3d",
            "panel-modifier-boolean", "panel-nested-effects",
        })
        case = next(case for case in self.harness.cases if case.name == "panel-text-document")
        self.assertEqual(case.case_id, "fx-export-panel-text-document")
        self.assertEqual(case.control_kind, "non_audio")
        targets = adapter._reference_targets(REPO, self.harness.cases)
        self.assertNotIn(case.name, targets, "uninspected publication is not a reference")
        self.assertEqual(adapter._native_control_evidence(case, {}, None, None)["status"], "failure")
        with mock.patch.object(adapter, "_reference_targets", return_value=targets):
            rows = self.harness.run(selected_case_ids=[case.case_id])
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["assertions"]["status"], "success")
        self.assertEqual(rows[0]["scoring"]["status"], "failure")
        self.assertEqual(rows[0]["status"], "failure")
        self.assertEqual(rows[0]["identity"]["case_name"], case.name)

    def test_non_audio_reference_requires_inspected_native_source_and_correct_identity(self) -> None:
        cases = adapter.load_cases(REPO)
        validated = adapter.proof.validate_manifest(adapter._load_object(REPO / adapter.REFERENCES))
        case = next(case for case in cases if case.name == "panel-text-document")
        key = (str(case.fixture_dir / "native" / f"{case.case_id}.aep"), 1)
        source, pinned_composition = validated[key]
        composition = {**pinned_composition, "verification": dict(pinned_composition["verification"])}
        self.assertEqual(composition["status"], "published_uninspected")
        with mock.patch.object(adapter.proof, "validate_manifest", return_value={
            **validated, key: (source, composition),
        }):
            self.assertNotIn(case.name, adapter._reference_targets(REPO, cases))
            composition["status"] = "verified"
            composition["verification"] = {**composition["verification"],
                "full_decode": True, "visual_inspection": "feature-critical frames inspected"}
            self.assertEqual(adapter._reference_targets(REPO, cases)[case.name], (source, composition))
            composition["composition_name"] = "wrong target"
            with self.assertRaises(adapter.ExportAdapterError):
                adapter._reference_targets(REPO, cases)

    def test_unknown_or_duplicate_export_selection_never_launches_adobe(self) -> None:
        for case_ids in (["fx-export-unknown"], [],
                         ["fx-export-vector-rect-size", "fx-export-vector-rect-size"]):
            with self.subTest(case_ids=case_ids):
                with self.assertRaises(adapter.ExportAdapterError):
                    self.harness.run(selected_case_ids=case_ids)
                self.assertEqual(self.harness.commands, [])

    def test_missing_tool_is_a_failure_row_for_every_case_without_commands(self) -> None:
        self.harness.tools["validation"].unlink()
        rows = self.harness.run()
        self.assertEqual(len(rows), len(self.harness.cases))
        self.assertEqual(self.harness.commands, [])
        self.assertTrue(all(row["status"] == "failure" for row in rows))
        self.assertTrue(all("validation" in row["score_reason"] for row in rows))
        self.assertTrue(
            all(
                set(row["identity"]["tool_identity"])
                == {"ffprobe"}
                for row in rows
            )
        )

    def test_sha_binding_mismatch_never_renders_that_case(self) -> None:
        first = self.harness.cases[0]
        rows = self.harness.run(
            mutations={
                first.case_id: {
                    "artifacts": {
                        "fx_json": {"path": "wrong", "sha256": "0" * 64, "bytes": 1}
                    }
                }
            }
        )
        self.assertIn("artifacts must contain exactly", rows[0]["assertions"]["reason"])
        first_render = [
            command
            for command in self.harness.commands
            if Path(command[0]).name == "typed-render_aep"
            and command[command.index("-comp") + 1] == first.name
        ]
        self.assertEqual(first_render, [])
        self.assertEqual(rows[1]["status"], "success")


if __name__ == "__main__":
    unittest.main()
