#!/usr/bin/env python3
"""Offline tests for the unified Adobe runner (no Adobe, network, or GPU)."""

from __future__ import annotations

import json
import sys
from contextlib import redirect_stderr
from io import StringIO
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import adobe_test


class RunnerHarness:
    def __init__(self, root: Path, count: int = 3) -> None:
        self.root = root
        self.workspace = root / "workspace"
        self.workspace.mkdir(parents=True)
        self.cases: dict[str, dict] = {}
        self.targets: dict[tuple[str, int], tuple[dict, dict]] = {}
        for index in range(count):
            case_id = f"case-{index:03d}"
            source_path = f"fixtures/source-{index:03d}.aep"
            test_path = "crates/aftereffects_file/src/structure_document/tests/native_general_cases/properties.rs"
            case = {
                "case_id": case_id,
                "direction": "import",
                "feature": f"feature {index}",
                "source_path": source_path,
                "composition_id": index + 1,
                "critical_frames": [],
                "tests": [
                    {
                        "path": test_path,
                        "symbol": f"feature_case_{index:03d}",
                        "assertions": [{"description": "editable value", "source_anchor": "CASE"}],
                    }
                ],
                "execution": {"status": "UNRUN"},
            }
            source = {
                "source_path": source_path,
                "source_sha256": f"{index + 1:064x}",
                "source_bytes": 100 + index,
            }
            composition = {
                "composition_id": index + 1,
                "expected_frame_count": 60 + (index % 3),
                "reference": {"path": f"tests/references/aep/sample-{index:03d}.mp4"},
            }
            self.cases[case_id] = case
            self.targets[(source_path, index + 1)] = (source, composition)
        self.records = root / "records.jsonl"
        self.score_calls: list[str] = []
        self.score_behaviors: dict[str, object] = {}

    def record(
        self,
        case_id: str,
        *,
        status: str = "success",
        error: str | None = None,
        assertions_executed: bool = True,
        **overrides: object,
    ) -> dict:
        case = self.cases[case_id]
        source, _ = self.targets[(case["source_path"], case["composition_id"])]
        value = {
            "schema_version": 1,
            "case_id": case_id,
            "source_path": case["source_path"],
            "source_sha256": source["source_sha256"],
            "composition_id": case["composition_id"],
            "direction": "import",
            "test_symbol": adobe_test.expected_test_symbol(case),
            "test_binary": {"path": "/mock/cpu-test", "sha256": "a" * 64, "bytes": 123},
            "status": status,
            "attempted": True,
            "assertions_executed": assertions_executed,
            "error": error,
        }
        value.update(overrides)
        return value

    def write_records(self, records: list[dict]) -> None:
        self.records.write_text("".join(json.dumps(record) + "\n" for record in records), encoding="utf-8")

    def successful_records(self) -> list[dict]:
        return [self.record(case_id) for case_id in self.cases]

    def scorer(self, **kwargs: object) -> dict:
        case = kwargs["case"]
        assert isinstance(case, dict)
        case_id = case["case_id"]
        self.score_calls.append(case_id)
        behavior = self.score_behaviors.get(case_id, 0.75)
        if behavior == "cancel":
            raise KeyboardInterrupt
        if isinstance(behavior, dict):
            return behavior
        return {
            "status": "scored",
            "comparison": {"half_open_native_frames": {"min_frame_similarity": behavior}},
        }

    def run(
        self,
        *,
        records: list[dict] | None = None,
        cpu_exit_code: int = 0,
        cpu_only: bool = False,
        cpu_runner: adobe_test.CpuRunner = adobe_test.run_cpu_command,
        live_cpu: bool = False,
        selected_ids: list[str] | None = None,
        export_runner: adobe_test.ExportRunner | None = None,
        run_dir_override: Path | None = None,
    ) -> tuple[dict, int]:
        if records is None:
            records = self.successful_records()
        self.write_records(records)
        selected = [self.cases[case_id] for case_id in (selected_ids or list(self.cases))]
        run_dir = self.root / f"run-{len(list(self.root.glob('run-*'))):03d}"
        return adobe_test.run_unified(
            workspace=self.workspace,
            cases=self.cases,
            targets=self.targets,
            reference_settings={},
            selected=selected,
            run_dir=run_dir_override or run_dir,
            tools={
                "tesseract_conv": "tsrct-conv",
                "tsrct": "tsrct",
                "validation": "validation_cli",
                "ffprobe": "ffprobe",
                "aerender": "aerender",
            },
            cache=self.root / "cache",
            local_references={},
            max_samples=601,
            cpu_timeout=120,
            cpu_only=cpu_only,
            score_timeouts={"import": 1, "render": 1, "compare": 1, "metadata": 1},
            records_path=None if live_cpu else self.records,
            cpu_exit_code=None if live_cpu else cpu_exit_code,
            cpu_runner=cpu_runner,
            score_runner=self.scorer,
            export_runner=export_runner,
            expected_export_ids=[f"fx-export-case-{index:02d}" for index in range(63)] if export_runner else (),
        )


class UnifiedAdobeRunnerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.harness = RunnerHarness(self.root)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_cpu_command_clears_fixed_channel_and_retains_artifacts(self) -> None:
        channel = self.harness.workspace / "target" / "adobe-test"
        channel.mkdir(parents=True)
        (channel / "stale.jsonl").write_text("stale")
        run_dir = self.harness.root / "channel-run"
        run_dir.mkdir()
        records = run_dir / "adobe-test-records.jsonl"
        script = (
            "from pathlib import Path; import json; "
            "p = Path('target/adobe-test'); "
            "assert not (p / 'stale.jsonl').exists(); "
            "assert json.loads((p / 'selected-case-ids.json').read_text()) == ['selected-case']; "
            "a = p / 'fx_exports' / 'case.aep'; a.write_bytes(b'native'); "
            "(p / 'adobe-test-records.jsonl').write_text("
            "json.dumps({'path': str(a.resolve())}) + '\\n'); "
            "(p / 'adobe-export-records.jsonl').write_text("
            "json.dumps({'path': str(a.resolve())}) + '\\n')"
        )
        result = adobe_test.run_cpu_command(
            self.harness.workspace, records, [sys.executable, "-c", script], 10, 1024, ['selected-case']
        )
        self.assertEqual(result.exit_code, 0)
        artifact = run_dir / "fx_exports" / "case.aep"
        self.assertEqual(artifact.read_bytes(), b"native")
        self.assertEqual(json.loads(records.read_text())["path"], str(artifact.resolve()))
        journal = json.loads((run_dir / "adobe-export-records.jsonl").read_text())
        self.assertEqual(journal["path"], str(artifact.resolve()))
        self.assertFalse((channel / "selected-case-ids.json").exists(), "selector must not outlive the run")

    def test_cpu_command_removes_selector_when_launch_fails(self) -> None:
        run_dir = self.harness.root / "failed-run"
        run_dir.mkdir()
        with self.assertRaises(OSError):
            adobe_test.run_cpu_command(
                self.harness.workspace,
                run_dir / "adobe-test-records.jsonl",
                [str(self.harness.root / "missing-binary")],
                10,
                1024,
                ["selected-case"],
            )
        selector = self.harness.workspace / "target" / "adobe-test" / "selected-case-ids.json"
        self.assertFalse(selector.exists())

    def test_cli_export_selector_routes_one_case_and_exact_cpu_symbol(self) -> None:
        import adobe_export_test

        case_id = "fx-export-vector-rect-size"
        case = next(case for case in adobe_export_test.load_cases(adobe_test.REPO)
                    if case.case_id == case_id)
        with mock.patch.object(adobe_test, "run_unified", return_value=(
            {"selection": {}, "counts": {}}, 1
        )) as run, mock.patch.object(adobe_export_test, "run_exports", return_value=[]) as exports, mock.patch.object(adobe_test, "_print_summary"), mock.patch("builtins.print"):
            code = adobe_test.main([
                "--export-case-id", case_id,
                "--scratch-dir", str(self.root / "selected-export"),
                "--cache-dir", str(self.root / "cache"),
            ])
        self.assertEqual(code, 1)
        options = run.call_args.kwargs
        self.assertEqual(options["selected"], [])
        self.assertEqual(options["expected_export_ids"], [case_id])
        self.assertEqual(options["cpu_command"], (
            "make", "adobe-test-cpu", f"filter={case.test_symbol}"
        ))
        options["export_runner"](workspace=adobe_test.REPO)
        exports.assert_called_once_with(workspace=adobe_test.REPO, selected_case_ids=[case_id])

    def test_cli_unknown_export_fails_before_creating_run_directory(self) -> None:
        scratch = self.root / "unknown-export"
        for invalid in ("fx-export-not-registered", ""):
            with self.subTest(case_id=invalid), mock.patch.object(
                adobe_test, "run_unified"
            ) as run, mock.patch("sys.stderr"):
                with self.assertRaises(SystemExit):
                    adobe_test.main([
                        "--export-case-id", invalid,
                        "--scratch-dir", str(scratch),
                    ])
            run.assert_not_called()
            self.assertFalse(scratch.exists())

    def test_import_case_spec_uses_committed_reference_path(self) -> None:
        case = self.harness.cases["case-000"]
        target = self.harness.targets[(case["source_path"], case["composition_id"])]
        self.assertEqual(adobe_test._case_spec(case, target)["reference"], {
            "path": "tests/references/aep/sample-000.mp4",
            "expected_frame_count": 60,
        })

    def test_full_rust_thread_symbol_is_derived_from_registry_path(self) -> None:
        case = self.harness.cases["case-000"]
        self.assertEqual(
            adobe_test.expected_test_symbol(case),
            "structure_document::tests::native_general_cases::properties::feature_case_000",
        )

    def test_mocked_success_uses_exhaustive_frames_and_low_score_is_not_quality_failure(self) -> None:
        self.harness.score_behaviors["case-000"] = 0.001
        original_scorer = self.harness.scorer

        def scorer(**kwargs: object) -> dict:
            case = kwargs["case"]
            assert isinstance(case, dict)
            target = kwargs["target"]
            assert isinstance(target, tuple)
            self.assertEqual(len(case["critical_frames"]), target[1]["expected_frame_count"])
            self.assertEqual(case["critical_frames"][0], "0")
            self.assertEqual(case["critical_frames"][-1], "59/30" if case["case_id"] == "case-000" else case["critical_frames"][-1])
            return original_scorer(**kwargs)

        self.harness.scorer = scorer  # type: ignore[method-assign]
        report, code = self.harness.run()
        self.assertEqual(code, 0)
        self.assertEqual(report["state"], "completed")
        self.assertEqual(report["cases"][0]["score"], 0.001)
        self.assertEqual(report["cases"][0]["status"], "success")
        self.assertIsNone(report["cases"][0]["score_reason"])
        self.assertIsNone(report["sampling_policy"]["quality_threshold"])

    def test_assertion_failure_keeps_genuine_score_and_later_cases_execute(self) -> None:
        records = self.harness.successful_records()
        records[0] = self.harness.record(
            "case-000", status="failure", error="editable value mismatch", assertions_executed=True
        )
        report, code = self.harness.run(records=records)
        self.assertEqual(code, 1)
        self.assertEqual(self.harness.score_calls, list(self.harness.cases))
        first = report["cases"][0]
        self.assertEqual(first["status"], "failure")
        self.assertEqual(first["score"], 0.75)
        self.assertTrue(first["assertions"]["attempted"])
        self.assertEqual(first["scoring"]["status"], "success")

    def test_conversion_and_scoring_failures_do_not_stop_later_cases(self) -> None:
        self.harness.score_behaviors["case-000"] = {
            "status": "import_failed",
            "reason": "converter rejected selected composition",
        }
        self.harness.score_behaviors["case-001"] = {
            "status": "comparison_failed",
            "reason": "canonical comparison exited 2",
        }
        report, code = self.harness.run()
        self.assertEqual(code, 1)
        self.assertEqual(self.harness.score_calls, ["case-000", "case-001", "case-002"])
        self.assertEqual(report["cases"][0]["score_reason"], "converter rejected selected composition")
        self.assertEqual(report["cases"][1]["score_reason"], "canonical comparison exited 2")
        self.assertEqual(report["cases"][2]["status"], "success")

    def test_missing_and_duplicate_records_are_per_case_failures(self) -> None:
        records = [self.harness.record("case-000"), self.harness.record("case-000"), self.harness.record("case-002")]
        report, code = self.harness.run(records=records)
        self.assertEqual(code, 1)
        self.assertIn("duplicate", report["cases"][0]["assertions"]["reason"])
        self.assertIn("missing", report["cases"][1]["assertions"]["reason"])
        self.assertEqual(self.harness.score_calls, list(self.harness.cases))

    def test_wrong_source_composition_and_test_symbol_are_rejected_precisely(self) -> None:
        mutations = (
            ("source_path", "wrong/source.aep"),
            ("source_sha256", "f" * 64),
            ("composition_id", 999),
            ("test_symbol", "wrong::test::symbol"),
        )
        for key, value in mutations:
            with self.subTest(key=key):
                harness = RunnerHarness(self.root / f"mismatch-{key}", count=1)
                record = harness.record("case-000", **{key: value})
                report, code = harness.run(records=[record])
                self.assertEqual(code, 1)
                self.assertIn(f"{key} mismatch", report["cases"][0]["assertions"]["reason"])

    def test_success_without_entering_feature_assertions_is_rejected(self) -> None:
        records = self.harness.successful_records()
        records[0] = self.harness.record("case-000", assertions_executed=False)
        report, code = self.harness.run(records=records)
        self.assertEqual(code, 1)
        phase = report["cases"][0]["assertions"]
        self.assertFalse(phase["attempted"])
        self.assertTrue(phase["callback_completed"])
        self.assertIn("assertions_executed=true", phase["reason"])

    def test_cpu_timeout_is_suite_failure_but_scoring_still_runs(self) -> None:
        records = self.harness.successful_records()

        def timed_out_runner(
            workspace: Path,
            records_path: Path,
            command: object,
            timeout: int,
            log_limit: int,
            selected_ids: list[str],
        ) -> adobe_test.CpuExecution:
            del workspace, command, timeout, log_limit, selected_ids
            records_path.write_text("".join(json.dumps(record) + "\n" for record in records))
            return adobe_test.CpuExecution(None, True, False, None, None)

        report, code = self.harness.run(live_cpu=True, cpu_runner=timed_out_runner)
        self.assertEqual(code, 1)
        self.assertEqual(self.harness.score_calls, list(self.harness.cases))
        self.assertTrue(any("timed out" in failure for failure in report["suite_failures"]))

    def test_cancellation_preserves_partial_results_and_marks_unfinished_rows(self) -> None:
        self.harness.score_behaviors["case-001"] = "cancel"
        report, code = self.harness.run()
        self.assertEqual(code, 130)
        self.assertEqual(report["state"], "cancelled")
        self.assertEqual(self.harness.score_calls, ["case-000", "case-001"])
        self.assertEqual(report["cases"][0]["status"], "success")
        self.assertTrue(report["cases"][1]["scoring"]["attempted"])
        self.assertFalse(report["cases"][2]["scoring"]["attempted"])
        self.assertIn("cancelled before", report["cases"][2]["score_reason"])

    def test_nonzero_cpu_exit_with_valid_rows_is_additional_suite_failure(self) -> None:
        report, code = self.harness.run(cpu_exit_code=101)
        self.assertEqual(code, 1)
        self.assertTrue(all(row["status"] == "success" for row in report["cases"]))
        self.assertTrue(any("exited 101" in failure for failure in report["suite_failures"]))

    def test_unified_default_level_can_append_63_exports_and_missing_oracles_fail(self) -> None:
        harness = RunnerHarness(self.root / "unified-663", count=600)

        def export_runner(**kwargs: object) -> list[dict]:
            on_row = kwargs["on_row"]
            assert callable(on_row)
            rows = []
            for index in range(63):
                has_reference = index < 55
                reason = None if has_reference else "independent native render reference is not available for this panel case"
                row = {
                    "case_id": f"fx-export-case-{index:02d}",
                    "direction": "export",
                    "status": "success",
                    "score": 0.01 if has_reference else None,
                    "score_reason": reason,
                    "identity": {"case_name": f"case-{index:02d}", "test_symbol": f"tests::case_{index:02d}"},
                    "assertions": {"attempted": True, "status": "success", "reason": None},
                    "native_acceptance": {"attempted": True, "status": "success", "reason": None},
                    "scoring": {
                        "attempted": has_reference,
                        "status": "success" if has_reference else "not_available",
                        "reason": reason,
                    },
                    "attempted": {"assertions": True, "native_acceptance": True, "scoring": has_reference},
                    "limitations": [],
                }
                rows.append(row)
                on_row(row)
            return rows

        report, code = harness.run(export_runner=export_runner)
        self.assertEqual(code, 1)
        self.assertEqual(report["counts"]["selected"], 663)
        self.assertEqual(report["selection"]["export_target_count"], 63)
        export_rows = [row for row in report["cases"] if row["direction"] == "export"]
        self.assertEqual(len(export_rows), 63)
        self.assertEqual(sum(row["status"] == "failure" for row in export_rows), 8)
        self.assertTrue(all(row["score"] is None for row in export_rows[55:]))
        self.assertEqual(report["direction_scope"], ["import", "export"])

    def test_export_adapter_exception_retains_every_expected_failure_row(self) -> None:
        def broken_adapter(**kwargs: object) -> list[dict]:
            raise RuntimeError("native tool setup failed")

        report, code = self.harness.run(export_runner=broken_adapter)
        self.assertEqual(code, 1)
        rows = [row for row in report["cases"] if row["direction"] == "export"]
        self.assertEqual(len(rows), 63)
        self.assertTrue(all(row["status"] == "failure" and row["score"] is None for row in rows))
        self.assertTrue(all("native tool setup failed" in row["score_reason"] for row in rows))
        self.assertFalse(report["selection"]["full_coverage_claimed"])

    def test_export_cancellation_is_latched_by_parent(self) -> None:
        def cancelled_adapter(**kwargs: object) -> list[dict]:
            row = {
                "case_id": "fx-export-case-00", "direction": "export", "status": "failure",
                "score": None, "score_reason": "operator cancellation", "cancelled": True,
                "assertions": {"attempted": True},
                "native_acceptance": {"attempted": True},
                "scoring": {"attempted": False},
            }
            kwargs["on_row"](row)
            return [row]

        report, code = self.harness.run(export_runner=cancelled_adapter)
        self.assertEqual(code, 130)
        self.assertEqual(report["state"], "cancelled")
        self.assertEqual(len(report["cases"]), 66)
        self.assertFalse(report["selection"]["full_coverage_claimed"])

    def test_fixture_output_is_rejected_before_creating_any_file(self) -> None:
        forbidden = self.harness.workspace / "opensource/conv/crates/aftereffects_file/tests/fixtures/generated-run"
        with self.assertRaisesRegex(adobe_test.aep_test.BlockedCase, "immutable fixture"):
            self.harness.run(cpu_only=True, run_dir_override=forbidden)
        self.assertFalse(forbidden.exists())

    def test_missing_cpu_build_identity_is_not_accepted(self) -> None:
        records = self.harness.successful_records()
        records[0]["test_binary"] = {"error": "executable unavailable"}
        report, code = self.harness.run(records=records)
        self.assertEqual(code, 1)
        self.assertFalse(report["selection"]["full_coverage_claimed"])
        self.assertIn("test executable identity", report["cases"][0]["assertions"]["reason"])

    def test_score_sidecar_is_bound_by_hash(self) -> None:
        report, _ = self.harness.run()
        row = report["cases"][0]
        result = self.root / "run-000" / row["scoring_result_path"]
        self.assertEqual(row["scoring_result_sha256"], adobe_test.aep_test.sha256_file(result))
        self.assertEqual(row["assertions"]["test_binary"]["sha256"], "a" * 64)

    def test_missing_record_never_claims_full_execution(self) -> None:
        report, _ = self.harness.run(records=self.harness.successful_records()[:-1])
        self.assertFalse(report["selection"]["full_coverage_claimed"])

    def test_cpu_only_accounts_for_all_600_without_claiming_scoring(self) -> None:
        harness = RunnerHarness(self.root / "all-600", count=600)
        report, code = harness.run(cpu_only=True)
        self.assertEqual(code, 0)
        self.assertEqual(report["selection"]["mode"], "cpu_only_import_records")
        self.assertFalse(report["selection"]["full_coverage_claimed"])
        self.assertEqual(report["counts"]["selected"], 600)
        self.assertEqual(report["counts"]["success"], 600)
        self.assertEqual(report["counts"]["scored"], 0)
        self.assertEqual(harness.score_calls, [])
        self.assertTrue(all(row["score"] is None for row in report["cases"]))
        self.assertTrue(all("not requested" in row["score_reason"] for row in report["cases"]))
        self.assertIn("not claimed", report["validation_claim"])

    def test_debug_filter_is_explicit_and_extra_known_records_are_ignored(self) -> None:
        report, code = self.harness.run(selected_ids=["case-001"])
        self.assertEqual(code, 0)
        self.assertEqual(report["selection"]["mode"], "debug_case_filter")
        self.assertFalse(report["selection"]["full_coverage_claimed"])
        self.assertEqual(report["cpu"]["ignored_known_unselected_records"], 2)
        self.assertEqual([row["case_id"] for row in report["cases"]], ["case-001"])

    def test_explicit_effects_dispatch_keeps_only_requested_export_ids(self) -> None:
        imports, adjustment, effects = adobe_test.select_case_dispatch(
            {"import-one": {"case_id": "import-one"}},
            ["fx-export-vignette-static", "fx-export-vignette-animated"],
            {"fx-export-adjustment-scope"},
            {"fx-export-vignette-static", "fx-export-vignette-animated"},
        )
        self.assertEqual(imports, [])
        self.assertEqual(adjustment, [])
        self.assertEqual(effects, ["fx-export-vignette-static", "fx-export-vignette-animated"])
        with self.assertRaisesRegex(adobe_test.AdobeTestError, "separate runs"):
            adobe_test.select_case_dispatch(
                {}, ["fx-export-adjustment-scope", "fx-export-vignette-static"],
                {"fx-export-adjustment-scope"}, {"fx-export-vignette-static"},
            )

    def test_selected_adjustment_fails_closed_without_private_adapter(self) -> None:
        with (
            mock.patch.dict("sys.modules", {"adobe_adjustment_test": None}),
            redirect_stderr(StringIO()) as errors,
            self.assertRaises(SystemExit) as failure,
        ):
            adobe_test.main([
                "--workspace", str(adobe_test.REPO),
                "--scratch-dir", str(self.root / "scratch"),
                "--case-id", "fx-export-adjustment-scope",
            ])
        self.assertEqual(failure.exception.code, 2)
        self.assertIn("selected Adjustment export requires the private Adobe adapter", errors.getvalue())

    def test_unknown_case_record_is_a_suite_failure_and_selected_case_is_missing(self) -> None:
        unknown = self.harness.record("case-000")
        unknown["case_id"] = "not-in-registry"
        report, code = self.harness.run(records=[unknown, *self.harness.successful_records()[1:]])
        self.assertEqual(code, 1)
        self.assertIn("unknown case_id", report["suite_failures"][0])
        self.assertIn("missing", report["cases"][0]["assertions"]["reason"])
        self.assertEqual(self.harness.score_calls, list(self.harness.cases))

    def test_reference_map_selects_known_rows_and_rejects_unknown_ids(self) -> None:
        reference_map = self.root / "references.json"
        reference_map.write_text(
            json.dumps({"case-000": "/tmp/zero.mp4", "case-001": "/tmp/one.mp4"})
        )
        selected = adobe_test._parse_reference_map(
            reference_map, {"case-001"}, set(self.harness.cases)
        )
        self.assertEqual(selected, {"case-001": Path("/tmp/one.mp4").resolve()})
        reference_map.write_text(json.dumps({"unknown": "/tmp/no.mp4"}))
        with self.assertRaisesRegex(adobe_test.AdobeTestError, "unknown case ID"):
            adobe_test._parse_reference_map(reference_map, set(self.harness.cases), set(self.harness.cases))

    def test_all_imports_without_exports_never_claims_full_registry(self) -> None:
        report, code = self.harness.run()
        self.assertEqual(code, 0)
        self.assertEqual(report["selection"]["selected_count"], len(self.harness.cases))
        self.assertEqual(report["selection"]["mode"], "debug_case_filter")
        self.assertFalse(report["selection"]["full_coverage_claimed"])
        self.assertEqual(report["direction_scope"], ["import"])

    def test_real_import_inventory_has_641_local_cases_and_full_frame_schedule(self) -> None:
        cases, targets, _ = adobe_test.aep_test.load_catalog(
            adobe_test.REPO,
            adobe_test.aep_test.DEFAULT_REGISTRY,
            adobe_test.aep_test.DEFAULT_REFERENCES,
        )
        selected = list(cases.values())
        self.assertEqual(len(selected), 641)
        self.assertEqual(
            {
                case_id
                for case_id in cases
                if case_id.startswith("aep-layer-styles-styles-static-adobe-")
            },
            {
                "aep-layer-styles-styles-static-adobe-c1",
                "aep-layer-styles-styles-static-adobe-c16",
                "aep-layer-styles-styles-static-adobe-c30",
                "aep-layer-styles-styles-static-adobe-c44",
                "aep-layer-styles-styles-static-adobe-c58",
                "aep-layer-styles-styles-static-adobe-c72",
                "aep-layer-styles-styles-static-adobe-c86",
                "aep-layer-styles-styles-static-adobe-c100",
                "aep-layer-styles-styles-static-adobe-c114",
            },
        )
        self.assertIn("aep-expression-samples-sampled-position-expression-c1", cases)
        self.assertIn("aep-effects-fill-isolated-c1", cases)
        self.assertEqual(sum(target[1]["expected_frame_count"] for target in targets.values()), 51_900)
        # The nested support composition is exercised via its parent, not rendered separately.
        self.assertEqual(sum("reference" not in target[1] for target in targets.values()), 1)
        self.assertEqual(sum("path" in target[1].get("reference", {})
                             for target in targets.values()), 678)
        self.assertEqual(adobe_test.required_max_samples(selected, targets), 301)

    def test_max_samples_must_cover_full_inclusive_duration(self) -> None:
        selected = list(self.harness.cases.values())
        with self.assertRaisesRegex(adobe_test.AdobeTestError, "at least 63"):
            adobe_test.run_unified(
                workspace=self.harness.workspace,
                cases=self.harness.cases,
                targets=self.harness.targets,
                reference_settings={},
                selected=selected,
                run_dir=self.root / "too-small",
                tools={},
                cache=self.root / "cache",
                local_references={},
                max_samples=62,
                cpu_timeout=120,
                cpu_only=True,
                score_timeouts={"import": 1, "render": 1, "compare": 1, "metadata": 1},
                records_path=self.harness.records,
                cpu_exit_code=0,
            )


if __name__ == "__main__":
    unittest.main()
