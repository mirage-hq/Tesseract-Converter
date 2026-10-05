#!/usr/bin/env python3
"""Unified, fail-continuing Adobe import assertion and RGB scoring runner.

The default run selects every validated AEP registry target.  ``--case-id`` is
only a diagnostic subset selector and is recorded as such.  Scores are
strictly descriptive: this runner defines no quality threshold.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from fractions import Fraction
from functools import partial
import hashlib
import importlib.util
import json
import math
import os
import shutil
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import threading
from typing import Any, Callable, Sequence

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
sys.path.insert(0, str(HERE))

import aep_test  # noqa: E402
import adobe_audio_cases  # noqa: E402
from aep_adjustment_cases import ADJUSTMENT_EXPORT_CASE_IDS  # noqa: E402

DEFAULT_SCRATCH = REPO / "tmp/adobe-test-runs"
DEFAULT_CPU_COMMAND = ("make", "adobe-test-cpu")
CPU_RECORD_SCHEMA_VERSION = 1
REPORT_SCHEMA_VERSION = 1
CPU_LOG_LIMIT_BYTES = 1_000_000
CPU_RECORD_LIMIT_BYTES = 32 * 1024 * 1024
_REQUIRED_RECORD_KEYS = {
    "schema_version",
    "case_id",
    "source_path",
    "source_sha256",
    "composition_id",
    "direction",
    "test_symbol",
    "test_binary",
    "status",
    "attempted",
    "assertions_executed",
    "error",
}


class AdobeTestError(RuntimeError):
    """A runner configuration or input error."""


@dataclass(frozen=True)
class CpuExecution:
    exit_code: int | None
    timed_out: bool
    cancelled: bool
    stdout_log: Path | None
    stderr_log: Path | None
    stdout_bytes: int = 0
    stderr_bytes: int = 0


CpuRunner = Callable[[Path, Path, Sequence[str], int, int, list[str]], CpuExecution]
ScoreRunner = Callable[..., dict[str, Any]]
ExportRunner = Callable[..., list[dict[str, Any]]]
AudioRunner = Callable[..., list[dict[str, Any]]]


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _atomic_json(path: Path, value: dict[str, Any]) -> None:
    aep_test.atomic_json(path, value)


def _repository_identity(workspace: Path) -> dict[str, Any]:
    values: dict[str, Any] = {}
    for key, arguments in (
        ("commit", ["rev-parse", "--verify", "HEAD"]),
        ("worktree_status", ["status", "--porcelain=v1", "--untracked-files=all"]),
    ):
        try:
            process = subprocess.run(
                ["git", *arguments], cwd=workspace, capture_output=True, timeout=10, check=False,
            )
            if process.returncode:
                return {"available": False, "reason": "Git repository identity unavailable"}
            values[key] = process.stdout.decode("utf-8", errors="replace").strip()
        except (OSError, subprocess.TimeoutExpired) as exc:
            return {"available": False, "reason": str(exc)}
    values["available"] = True
    values["dirty"] = bool(values["worktree_status"])
    return values


def _safe_case_filename(case_id: str) -> str:
    if re.fullmatch(r"[A-Za-z0-9._-]+", case_id):
        return f"{case_id}.json"
    return f"case-{hashlib.sha256(case_id.encode()).hexdigest()}.json"


def expected_test_symbol(case: dict[str, Any]) -> str:
    """Return the exact Rust test-thread name required in the CPU JSONL record."""
    tests = case.get("tests")
    if not isinstance(tests, list) or len(tests) != 1 or not isinstance(tests[0], dict):
        raise AdobeTestError(f"{case.get('case_id', '<unknown>')}: exactly one declared CPU test is required")
    test = tests[0]
    path_value = test.get("path")
    symbol = test.get("symbol")
    prefix = "crates/aftereffects_file/src/"
    if not isinstance(path_value, str) or not path_value.startswith(prefix) or not path_value.endswith(".rs"):
        raise AdobeTestError(f"{case['case_id']}: unsupported CPU test source path {path_value!r}")
    if not isinstance(symbol, str) or not symbol:
        raise AdobeTestError(f"{case['case_id']}: missing CPU test symbol")
    module_path = path_value[len(prefix) : -3].replace("/", "::")
    return f"{module_path}::{symbol}"


def _case_spec(
    case: dict[str, Any],
    target: tuple[dict[str, Any], dict[str, Any]] | None,
) -> dict[str, Any]:
    if target is None:
        raise AdobeTestError(f"{case['case_id']}: source/composition is absent from the reference inventory")
    source, composition = target
    if source.get("source_path") != case.get("source_path"):
        raise AdobeTestError(f"{case['case_id']}: registry/reference source identity differs")
    if composition.get("composition_id") != case.get("composition_id"):
        raise AdobeTestError(f"{case['case_id']}: registry/reference composition identity differs")
    return {
        "case_id": case["case_id"],
        "direction": "import",
        "source_path": source["source_path"],
        "source_sha256": source["source_sha256"],
        "composition_id": composition["composition_id"],
        "test_symbol": expected_test_symbol(case),
        "reference": {
            "path": composition["reference"]["path"],
            "expected_frame_count": composition["expected_frame_count"],
        },
    }


def select_cases(cases: dict[str, dict[str, Any]], case_ids: list[str]) -> list[dict[str, Any]]:
    """Select all cases by default; explicit IDs are a recorded debug subset."""
    if not case_ids:
        return list(cases.values())
    if len(case_ids) != len(set(case_ids)):
        raise AdobeTestError("duplicate --case-id values are not allowed")
    unknown = [case_id for case_id in case_ids if case_id not in cases]
    if unknown:
        raise AdobeTestError(f"unknown case ID(s): {', '.join(unknown)}")
    return [cases[case_id] for case_id in case_ids]


def select_case_dispatch(
    cases: dict[str, dict[str, Any]],
    case_ids: list[str],
    adjustment_export_ids: set[str],
    effects_export_ids: set[str],
) -> tuple[list[dict[str, Any]], list[str], list[str]]:
    """Partition an exact import/Adjustment/Effects export selection.

    With no explicit IDs the caller preserves all-import and all-Effects defaults.
    """
    if not case_ids:
        return list(cases.values()), [], []
    if len(case_ids) != len(set(case_ids)):
        raise AdobeTestError("duplicate --case-id values are not allowed")
    known = set(cases) | adjustment_export_ids | effects_export_ids
    unknown = [case_id for case_id in case_ids if case_id not in known]
    if unknown:
        raise AdobeTestError(f"unknown case ID(s): {', '.join(unknown)}")
    selected_imports = [cases[case_id] for case_id in case_ids if case_id in cases]
    adjustments = [case_id for case_id in case_ids if case_id in adjustment_export_ids]
    effects = [case_id for case_id in case_ids if case_id in effects_export_ids]
    if adjustments and effects:
        raise AdobeTestError("select Adjustment and Effects exports in separate runs")
    return selected_imports, adjustments, effects


def _full_frame_case(case: dict[str, Any], composition: dict[str, Any]) -> dict[str, Any]:
    """Replace incomplete critical lists with every unique 30fps reference frame."""
    frame_count = composition["expected_frame_count"]
    exhaustive = dict(case)
    exhaustive["critical_frames"] = [str(Fraction(index, aep_test.SAMPLE_FPS)) for index in range(frame_count)]
    return exhaustive


def required_max_samples(
    selected: list[dict[str, Any]],
    targets: dict[tuple[str, int], tuple[dict[str, Any], dict[str, Any]]],
) -> int:
    required = 0
    for case in selected:
        target = targets.get((case["source_path"], case["composition_id"]))
        if target is None:
            raise AdobeTestError(f"{case['case_id']}: source/composition is absent from references")
        required = max(required, int(target[1]["expected_frame_count"]) + 1)
    return required


def _record_problem(record: dict[str, Any], spec: dict[str, Any]) -> str | None:
    missing = sorted(_REQUIRED_RECORD_KEYS - set(record))
    extra = sorted(set(record) - _REQUIRED_RECORD_KEYS)
    if missing:
        return f"CPU record missing field(s): {', '.join(missing)}"
    if extra:
        return f"CPU record has unexpected field(s): {', '.join(extra)}"
    if record["schema_version"] != CPU_RECORD_SCHEMA_VERSION:
        return f"CPU record schema_version must be {CPU_RECORD_SCHEMA_VERSION}"
    for key in ("case_id", "source_path", "source_sha256", "direction", "test_symbol", "status"):
        if not isinstance(record[key], str):
            return f"CPU record {key} must be a string"
    if not isinstance(record["composition_id"], int) or isinstance(record["composition_id"], bool):
        return "CPU record composition_id must be an integer"
    if record["attempted"] is not True:
        return "CPU record attempted must be true for a completed callback"
    if not isinstance(record["assertions_executed"], bool):
        return "CPU record assertions_executed must be a boolean"
    if record["status"] not in {"success", "failure"}:
        return "CPU record status must be success or failure"
    if record["status"] == "success" and record["assertions_executed"] is not True:
        return "successful CPU record requires assertions_executed=true"
    if record["status"] == "success" and record["error"] is not None:
        return "successful CPU record error must be null"
    if record["status"] == "failure" and (not isinstance(record["error"], str) or not record["error"].strip()):
        return "failed CPU record error must be a nonempty string"
    binary = record["test_binary"]
    if (
        not isinstance(binary, dict)
        or not isinstance(binary.get("path"), str)
        or not binary["path"]
        or not isinstance(binary.get("sha256"), str)
        or re.fullmatch(r"[0-9a-f]{64}", binary["sha256"]) is None
        or type(binary.get("bytes")) is not int
        or binary["bytes"] <= 0
    ):
        return "CPU record lacks a valid test executable identity"
    for key in ("case_id", "source_path", "source_sha256", "composition_id", "direction", "test_symbol"):
        if record[key] != spec[key]:
            return f"CPU record {key} mismatch: expected {spec[key]!r}, got {record[key]!r}"
    return None


def _load_record_bytes(path: Path) -> bytes:
    try:
        size = path.stat().st_size
        if size > CPU_RECORD_LIMIT_BYTES:
            raise AdobeTestError(
                f"CPU records exceed the explicit {CPU_RECORD_LIMIT_BYTES}-byte input bound: {path}"
            )
        return path.read_bytes()
    except OSError as exc:
        raise AdobeTestError(f"cannot read CPU records {path}: {exc}") from exc


def parse_cpu_records(
    data: bytes,
    specs: dict[str, dict[str, Any]],
    known_case_ids: set[str],
) -> tuple[dict[str, dict[str, Any]], list[str], int]:
    """Strictly merge one completed callback record per selected case."""
    buckets: dict[str, list[tuple[int, dict[str, Any]]]] = {case_id: [] for case_id in specs}
    suite_failures: list[str] = []
    ignored_known = 0
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as exc:
        text = ""
        suite_failures.append(f"CPU records are not UTF-8: {exc}")
    for line_number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            suite_failures.append(f"CPU records line {line_number} is blank")
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            suite_failures.append(f"CPU records line {line_number} is invalid JSON: {exc.msg}")
            continue
        if not isinstance(value, dict):
            suite_failures.append(f"CPU records line {line_number} is not an object")
            continue
        case_id = value.get("case_id")
        if not isinstance(case_id, str):
            suite_failures.append(f"CPU records line {line_number} has no string case_id")
        elif case_id in specs:
            buckets[case_id].append((line_number, value))
        elif case_id in known_case_ids:
            ignored_known += 1
        else:
            suite_failures.append(f"CPU records line {line_number} has unknown case_id {case_id!r}")

    phases: dict[str, dict[str, Any]] = {}
    for case_id, spec in specs.items():
        entries = buckets[case_id]
        if not entries:
            phases[case_id] = {
                "attempted": False,
                "callback_completed": False,
                "status": "failure",
                "reason": "missing CPU completion record",
                "record_valid": False,
                "record_line": None,
            }
            continue
        if len(entries) != 1:
            lines = ", ".join(str(line) for line, _ in entries)
            phases[case_id] = {
                "attempted": any(value.get("assertions_executed") is True for _, value in entries),
                "callback_completed": any(value.get("attempted") is True for _, value in entries),
                "status": "failure",
                "reason": f"duplicate CPU completion records on lines {lines}",
                "record_valid": False,
                "record_line": None,
            }
            continue
        line_number, record = entries[0]
        problem = _record_problem(record, spec)
        if problem is not None:
            phases[case_id] = {
                "attempted": record.get("assertions_executed") is True,
                "callback_completed": record.get("attempted") is True,
                "status": "failure",
                "reason": problem,
                "record_valid": False,
                "record_line": line_number,
            }
        elif record["status"] == "failure":
            phases[case_id] = {
                "attempted": record["assertions_executed"],
                "callback_completed": True,
                "status": "failure",
                "reason": f"CPU assertion failed: {record['error']}",
                "record_valid": True,
                "test_binary": record["test_binary"],
                "record_line": line_number,
            }
        else:
            phases[case_id] = {
                "attempted": True,
                "callback_completed": True,
                "status": "success",
                "reason": None,
                "record_valid": True,
                "test_binary": record["test_binary"],
                "record_line": line_number,
            }
    return phases, suite_failures, ignored_known


class _BoundedLogCapture:
    def __init__(self, stream: Any, path: Path, limit: int) -> None:
        self.stream = stream
        self.path = path
        self.limit = limit
        self.total = 0
        self.kept = 0
        self.thread = threading.Thread(target=self._drain, daemon=True)

    def _drain(self) -> None:
        marker = b"\n[adobe-test: log truncated at configured byte bound]\n"
        with self.path.open("wb") as output:
            while True:
                chunk = self.stream.read(65536)
                if not chunk:
                    break
                self.total += len(chunk)
                allowance = max(0, self.limit - len(marker) - self.kept)
                if allowance:
                    kept = chunk[:allowance]
                    output.write(kept)
                    self.kept += len(kept)
            if self.total > self.kept:
                output.write(marker)


def _kill_process_group(process: subprocess.Popen[bytes]) -> None:
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    else:
        process.kill()
    process.wait()


def run_cpu_command(
    workspace: Path,
    records_path: Path,
    command: Sequence[str],
    timeout: int,
    log_limit: int,
    selected_case_ids: list[str],
) -> CpuExecution:
    """Run selected CaseBatch callbacks and retain bounded logs and artifacts."""
    stdout_log = records_path.parent / "cpu.stdout.log"
    stderr_log = records_path.parent / "cpu.stderr.log"
    # Rust's compile-time channel is fixed inside this converter workspace.
    # The shared Rust queue serializes commands; retain each run's artifacts
    # before the next command clears the channel.
    channel = workspace / "target" / "adobe-test"
    if channel.exists():
        shutil.rmtree(channel)
    channel.mkdir(parents=True)
    channel_records = channel / "adobe-test-records.jsonl"
    channel_exports = channel / "adobe-export-records.jsonl"
    export_root = channel / "fx_exports"
    channel_records.touch()
    channel_exports.touch()
    export_root.mkdir()
    # The selector only scopes this runner's subprocess; a stale file would make
    # later direct Rust test runs silently skip unselected cases.
    selection = channel / "selected-case-ids.json"
    selection.write_text(json.dumps(selected_case_ids))
    timed_out = False
    cancelled = False
    try:
        process = subprocess.Popen(
            list(command),
            cwd=workspace,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=os.name == "posix",
        )
        assert process.stdout is not None and process.stderr is not None
        stdout = _BoundedLogCapture(process.stdout, stdout_log, log_limit)
        stderr = _BoundedLogCapture(process.stderr, stderr_log, log_limit)
        stdout.thread.start()
        stderr.thread.start()
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            _kill_process_group(process)
        except KeyboardInterrupt:
            cancelled = True
            _kill_process_group(process)
        stdout.thread.join()
        stderr.thread.join()
    finally:
        selection.unlink(missing_ok=True)
    retained_exports = records_path.parent / "fx_exports"
    original_exports = str(export_root.resolve())
    shutil.move(str(export_root), retained_exports)
    retained_path = str(retained_exports.resolve())
    records_path.write_text(channel_records.read_text().replace(original_exports, retained_path))
    export_records = records_path.parent / "adobe-export-records.jsonl"
    export_records.write_text(channel_exports.read_text().replace(original_exports, retained_path))
    return CpuExecution(
        exit_code=None if cancelled else process.returncode,
        timed_out=timed_out,
        cancelled=cancelled,
        stdout_log=stdout_log,
        stderr_log=stderr_log,
        stdout_bytes=stdout.total,
        stderr_bytes=stderr.total,
    )


def _cpu_execution_dict(execution: CpuExecution, run_dir: Path) -> dict[str, Any]:
    def relative(path: Path | None) -> str | None:
        return str(path.relative_to(run_dir)) if path is not None and path.is_relative_to(run_dir) else (
            str(path) if path is not None else None
        )

    return {
        "exit_code": execution.exit_code,
        "timed_out": execution.timed_out,
        "cancelled": execution.cancelled,
        "stdout_log": relative(execution.stdout_log),
        "stderr_log": relative(execution.stderr_log),
        "stdout_bytes": execution.stdout_bytes,
        "stderr_bytes": execution.stderr_bytes,
        "log_limit_bytes_each": CPU_LOG_LIMIT_BYTES,
    }


def _default_score_runner(**kwargs: Any) -> dict[str, Any]:
    return aep_test.score_case(**kwargs)


def _score_phase(result: dict[str, Any]) -> tuple[dict[str, Any], float | None, str | None, bool]:
    status = result.get("status")
    if status != "scored":
        reason = result.get("reason")
        if not isinstance(reason, str) or not reason:
            reason = f"scoring returned status {status!r} without a reason"
        return ({"attempted": True, "status": "failure", "reason": reason, "raw_status": status}, None, reason, status == "cancelled")
    try:
        score = float(result["comparison"]["half_open_native_frames"]["min_frame_similarity"])
    except (KeyError, TypeError, ValueError) as exc:
        reason = f"scoring result lacks a valid half-open minimum similarity: {exc}"
        return ({"attempted": True, "status": "failure", "reason": reason, "raw_status": status}, None, reason, False)
    if not math.isfinite(score) or not 0 <= score <= 1:
        reason = f"scoring result similarity is outside [0,1]: {score!r}"
        return ({"attempted": True, "status": "failure", "reason": reason, "raw_status": status}, None, reason, False)
    phase = {
        "attempted": True,
        "status": "success",
        "reason": None,
        "raw_status": status,
        "metric": "canonical_rgb24_minimum_similarity_half_open_unique_frames",
        "quality_threshold": None,
        "quality_verdict": "not_defined_descriptive_only",
    }
    return phase, score, None, False


def _write_case(run_dir: Path, row: dict[str, Any]) -> None:
    _atomic_json(
        run_dir / "rows" / _safe_case_filename(row["case_id"]),
        {"schema_version": REPORT_SCHEMA_VERSION, "case": row},
    )


def _refresh_row(row: dict[str, Any], cpu_only: bool) -> None:
    assertions_ok = row["assertions"]["status"] == "success"
    scoring_ok = cpu_only or row["scoring"]["status"] == "success"
    row["attempted"] = {
        "assertions": row["assertions"]["attempted"],
        "scoring": row["scoring"]["attempted"],
    }
    row["status"] = "success" if assertions_ok and scoring_ok else "failure"


def run_unified(
    *,
    workspace: Path,
    cases: dict[str, dict[str, Any]],
    targets: dict[tuple[str, int], tuple[dict[str, Any], dict[str, Any]]],
    reference_settings: dict[str, Any],
    selected: list[dict[str, Any]],
    run_dir: Path,
    tools: dict[str, Path | str],
    cache: Path,
    local_references: dict[str, Path],
    max_samples: int,
    cpu_timeout: int,
    cpu_only: bool,
    score_timeouts: dict[str, int],
    records_path: Path | None = None,
    cpu_exit_code: int | None = None,
    cpu_runner: CpuRunner = run_cpu_command,
    score_runner: ScoreRunner = _default_score_runner,
    cpu_command: Sequence[str] = DEFAULT_CPU_COMMAND,
    export_runner: ExportRunner | None = None,
    export_records_path: Path | None = None,
    export_local_references: dict[str, Path] | None = None,
    expected_export_ids: Sequence[str] = (),
    export_registry_count: int | None = None,
    audio_cases: dict[str, dict[str, Any]] | None = None,
    selected_audio: list[dict[str, Any]] | None = None,
    audio_runner: AudioRunner | None = None,
) -> tuple[dict[str, Any], int]:
    """Execute every selected row, preserving failures and partial cancellation state."""
    workspace = workspace.resolve()
    aep_test._reject_fixture_output_path(workspace, run_dir, "unified scratch")
    aep_test._reject_fixture_output_path(workspace, cache, "unified cache")
    run_dir = run_dir.resolve()
    if run_dir.exists():
        if run_dir.is_symlink() or not run_dir.is_dir() or any(run_dir.iterdir()):
            raise AdobeTestError(f"run directory is not a fresh empty directory: {run_dir}")
    else:
        run_dir.mkdir(parents=True)
    required_samples = required_max_samples(selected, targets)
    if max_samples < required_samples:
        raise AdobeTestError(
            f"--max-samples={max_samples} cannot cover the selected exhaustive inclusive schedule; "
            f"at least {required_samples} is required"
        )

    selected_ids = [case["case_id"] for case in selected]
    audio_cases = audio_cases or {}
    selected_audio = selected_audio or []
    audio_ids = [case["case_id"] for case in selected_audio]
    if (len(audio_ids) != len(set(audio_ids)) or any(audio_cases.get(case_id) != case
        for case_id, case in zip(audio_ids, selected_audio))
            or set(audio_ids) & (set(cases) | set(expected_export_ids))):
        raise AdobeTestError("invalid or duplicate selected audio case identity")
    specs = {
        case["case_id"]: _case_spec(case, targets.get((case["source_path"], case["composition_id"])))
        for case in selected
    }
    selected_compositions = [
        targets[(case["source_path"], case["composition_id"])][1] for case in selected
    ]
    report_path = run_dir / "report.json"
    # A standalone conv checkout has no private Adobe Adjustment adapter. Only
    # hash it when actually available; selected Adjustment cases still fail
    # closed in main() if the private adapter cannot be imported.
    adjustment_spec = importlib.util.find_spec("adobe_adjustment_test")
    adjustment_adapter_path = (
        Path(adjustment_spec.origin)
        if adjustment_spec is not None and adjustment_spec.origin is not None
        else None
    )
    report: dict[str, Any] = {
        "schema_version": REPORT_SCHEMA_VERSION,
        "runner": "unified_adobe_import_assertions_and_rgb_scoring",
        "state": "running",
        "started_at": aep_test.utc_now(),
        "workspace": str(workspace),
        "selection": {
            "mode": (
                "cpu_only_import_records"
                if cpu_only
                else ("full_registry" if len(selected) == len(cases) and set(selected_ids) == set(cases)
                      and export_runner is not None
                      and export_registry_count is not None
                      and len(set(expected_export_ids)) == export_registry_count
                      and (not audio_cases or set(audio_ids) == set(audio_cases)) else "debug_case_filter")
            ),
            "selected_count": len(selected),
            "registry_count": len(cases) + (export_registry_count or 0),
            "selected_case_ids": selected_ids,
            "full_coverage_claimed": False,
        },
        "direction_scope": ["import"] if selected else [],
        "audio_inventory": {
            "selected_count": len(audio_ids), "registry_count": len(audio_cases),
            "import_selected": sum(case["direction"] == "import" for case in selected_audio),
            "export_selected": sum(case["direction"] == "export" for case in selected_audio),
            "metric": "stereo_pcm_audio_policy_not_rgb", "execution_at_start": "UNRUN",
        },
        "inventory": {
            "target_count": len(selected_compositions),
            "unique_30fps_frame_count": sum(
                int(composition["expected_frame_count"]) for composition in selected_compositions
            ),
            "inclusive_comparator_sample_count": sum(
                int(composition["expected_frame_count"]) + 1 for composition in selected_compositions
            ),
            "reference_bytes": sum(
                int(composition["reference"].get("bytes", 0)) for composition in selected_compositions
            ),
        },
        "cpu_only": cpu_only,
        "validation_claim": (
            "CPU assertions only; scoring was not requested and full Adobe validation is not claimed."
            if cpu_only
            else "Fresh import RGB measurements plus CPU assertions; scores are descriptive, not quality passes."
        ),
        "sampling_policy": {
            "fps": aep_test.SAMPLE_FPS,
            "unique_range": "all frames in [0,duration)",
            "critical_frame_policy": "exhaustive_all_unique_reference_frames_no_preselected_feature_critical_proof",
            "terminal_duration_probe": "included for comparator validation and excluded from unique-frame score",
            "downsampling": False,
            "max_samples": max_samples,
            "required_max_samples": required_samples,
            "quality_threshold": None,
        },
        "producer_code": {
            "runner_path": str(Path(__file__).resolve()),
            "runner_sha256": aep_test.sha256_file(Path(__file__).resolve()),
            "scorer_path": str(Path(aep_test.__file__).resolve()),
            "scorer_sha256": aep_test.sha256_file(Path(aep_test.__file__).resolve()),
            "export_adapter_sha256": aep_test.sha256_file(HERE / "adobe_export_test.py"),
            "adjustment_export_adapter_sha256": (
                aep_test.sha256_file(adjustment_adapter_path)
                if adjustment_adapter_path is not None and adjustment_adapter_path.is_file()
                else None
            ),
        },
        "repository": _repository_identity(workspace),
        "cpu": {},
        "suite_failures": [],
        "cases": [],
    }
    _atomic_json(report_path, report)

    live_records = run_dir / "cpu-records.jsonl"
    if not selected and not expected_export_ids and records_path is None:
        live_records.write_bytes(b"")
        execution = CpuExecution(0, False, False, None, None)
        report["cpu"]["mode"] = "not_applicable_audio_only"
        record_bytes = b""
    elif records_path is not None:
        if cpu_exit_code is None:
            raise AdobeTestError("--cpu-exit-code is required with --records")
        record_bytes = _load_record_bytes(records_path)
        live_records.write_bytes(record_bytes)
        execution = CpuExecution(cpu_exit_code, False, False, None, None)
        report["cpu"]["mode"] = "offline_records"
        report["cpu"]["input_path"] = str(records_path.resolve())
    else:
        execution = cpu_runner(
            workspace,
            live_records,
            cpu_command,
            cpu_timeout,
            CPU_LOG_LIMIT_BYTES,
            selected_ids,
        )
        record_bytes = _load_record_bytes(live_records) if live_records.is_file() else b""
        report["cpu"]["mode"] = "parent_owned_case_batch"
        report["cpu"]["command"] = list(cpu_command)
        report["cpu"]["timeout_seconds"] = cpu_timeout
        report["cpu"]["export_records_path"] = "adobe-export-records.jsonl"
        report["cpu"]["export_artifacts_dir"] = "fx_exports"
    report["cpu"].update(_cpu_execution_dict(execution, run_dir))
    report["cpu"]["records"] = {
        "path": str(live_records.relative_to(run_dir)),
        "bytes": len(record_bytes),
        "sha256": _sha256_bytes(record_bytes),
    }
    phases, record_failures, ignored_known = parse_cpu_records(record_bytes, specs, set(cases))
    report["cpu"]["ignored_known_unselected_records"] = ignored_known
    report["suite_failures"].extend(record_failures)
    if execution.timed_out:
        report["suite_failures"].append(f"CPU suite timed out after {cpu_timeout} seconds")
    if execution.exit_code not in (None, 0):
        report["suite_failures"].append(
            f"CPU suite exited {execution.exit_code}; valid individual records do not hide additional suite failure"
        )

    for case in selected:
        spec = specs[case["case_id"]]
        row = {
            "case_id": case["case_id"],
            "direction": "import",
            "identity": spec,
            "assertions": phases[case["case_id"]],
            "scoring": {
                "attempted": False,
                "status": "not_requested" if cpu_only else "pending",
                "reason": "scoring not requested by --cpu-only" if cpu_only else "scoring has not started",
            },
            "score": None,
            "score_reason": "scoring not requested by --cpu-only" if cpu_only else "scoring has not started",
            "status": "success" if cpu_only and phases[case["case_id"]]["status"] == "success" else "failure",
        }
        _refresh_row(row, cpu_only)
        report["cases"].append(row)
        _write_case(run_dir, row)
    _atomic_json(report_path, report)

    cancelled = execution.cancelled
    if cancelled:
        report["suite_failures"].append("CPU suite cancelled by operator")
        if not cpu_only:
            for row in report["cases"]:
                row["scoring"] = {
                    "attempted": False,
                    "status": "failure",
                    "reason": "CPU suite cancelled before scoring started",
                    "raw_status": "not_started",
                }
                row["score"] = None
                row["score_reason"] = "CPU suite cancelled before scoring started"
                _refresh_row(row, False)
                _write_case(run_dir, row)
    elif not cpu_only:
        (run_dir / "scoring").mkdir()
        for index, (case, row) in enumerate(zip(selected, report["cases"])):
            target = targets[(case["source_path"], case["composition_id"])]
            exhaustive_case = _full_frame_case(case, target[1])
            result = None
            try:
                result = score_runner(
                    workspace=workspace,
                    case=exhaustive_case,
                    target=target,
                    reference_settings=reference_settings,
                    case_dir=run_dir / "scoring" / case["case_id"],
                    tools=tools,
                    cache=cache,
                    local_reference=local_references.get(case["case_id"]),
                    import_timeout=score_timeouts["import"],
                    render_timeout=score_timeouts["render"],
                    compare_timeout=score_timeouts["compare"],
                    metadata_timeout=score_timeouts["metadata"],
                    max_samples=max_samples,
                )
                phase, score, reason, case_cancelled = _score_phase(result)
            except KeyboardInterrupt:
                phase = {
                    "attempted": True,
                    "status": "failure",
                    "reason": "cancelled by operator during scoring",
                    "raw_status": "cancelled",
                }
                score, reason, case_cancelled = None, phase["reason"], True
            except Exception as exc:  # preserve the rest of a broad validation run
                phase = {
                    "attempted": True,
                    "status": "failure",
                    "reason": f"unhandled scoring failure: {type(exc).__name__}: {exc}",
                    "raw_status": "runner_exception",
                }
                score, reason, case_cancelled = None, phase["reason"], False
            row["scoring"] = phase
            row["score"] = score
            row["score_reason"] = reason
            if result is not None:
                bound_result = run_dir / "scoring-results" / f"{case['case_id']}.json"
                _atomic_json(bound_result, result)
                row["scoring_result_path"] = str(bound_result.relative_to(run_dir))
                row["scoring_result_sha256"] = aep_test.sha256_file(bound_result)
            _refresh_row(row, False)
            _write_case(run_dir, row)
            _atomic_json(report_path, report)
            if case_cancelled:
                cancelled = True
                for remaining in report["cases"][index + 1 :]:
                    remaining["scoring"] = {
                        "attempted": False,
                        "status": "failure",
                        "reason": "run cancelled before scoring started",
                        "raw_status": "not_started",
                    }
                    remaining["score"] = None
                    remaining["score_reason"] = "run cancelled before scoring started"
                    _refresh_row(remaining, False)
                    _write_case(run_dir, remaining)
                break

    if export_runner is not None and not cpu_only:
        emitted_export_rows: list[dict[str, Any]] = []
        expected_exports = set(expected_export_ids)
        if not expected_exports or len(expected_exports) != len(expected_export_ids):
            raise AdobeTestError("export validation requires distinct expected export case IDs")
        adapter_problem = "export adapter omitted the target's result"

        def on_export_row(row: dict[str, Any]) -> None:
            nonlocal cancelled
            if not isinstance(row, dict):
                raise AdobeTestError("export adapter emitted a non-object row")
            if row.get("cancelled") is True:
                cancelled = True
            case_id = row.get("case_id")
            if not isinstance(case_id, str) or case_id not in expected_exports:
                raise AdobeTestError(f"export adapter emitted invalid case_id {case_id!r}")
            if row.get("direction") != "export" or row.get("status") not in {"success", "failure"}:
                raise AdobeTestError(f"{case_id}: export row has invalid direction or status")
            if any(existing["case_id"] == case_id for existing in report["cases"]):
                raise AdobeTestError(f"duplicate unified case ID {case_id}")
            scoring = row.get("scoring")
            if row.get("score") is None and isinstance(scoring, dict) and scoring.get("status") == "not_available":
                reason = row.get("score_reason")
                if not isinstance(reason, str) or not reason:
                    reason = "independent native render reference is not available for this export case"
                    row["score_reason"] = reason
                row["status"] = "failure"
                scoring["reason"] = reason
            report["cases"].append(row)
            emitted_export_rows.append(row)
            _write_case(run_dir, row)
            _atomic_json(report_path, report)

        try:
            returned_rows = export_runner(
                workspace=workspace,
                run_dir=run_dir,
                records_path=(export_records_path or (run_dir / "adobe-export-records.jsonl")),
                cache_dir=cache,
                local_references=export_local_references or {},
                tools={
                    "ffprobe": tools["ffprobe"],
                    "validation": tools["validation"],
                },
                timeouts={
                    "render": score_timeouts["render"],
                    "metadata": score_timeouts["metadata"],
                    "compare": score_timeouts["compare"],
                },
                on_row=on_export_row,
                cancelled=lambda: cancelled,
            )
            if [row.get("case_id") for row in returned_rows] != [
                row.get("case_id") for row in emitted_export_rows
            ]:
                report["suite_failures"].append(
                    "export adapter return value differs from rows emitted through its persistence hook"
                )
        except KeyboardInterrupt:
            cancelled = True
            adapter_problem = "export validation cancelled by operator"
            report["suite_failures"].append(adapter_problem)
        except Exception as exc:
            adapter_problem = f"export adapter failed: {type(exc).__name__}: {exc}"
            report["suite_failures"].append(adapter_problem)
        emitted_count = len(emitted_export_rows)
        if emitted_count != len(expected_exports):
            report["suite_failures"].append(
                f"export adapter emitted {emitted_count} rows; exactly {len(expected_exports)} are required"
            )
        reported_ids = {row["case_id"] for row in emitted_export_rows}
        for case_id in expected_export_ids:
            if case_id not in reported_ids:
                on_export_row({
                    "case_id": case_id,
                    "direction": "export",
                    "status": "failure",
                    "score": None,
                    "score_reason": adapter_problem,
                    "identity": {"case_id": case_id},
                    "assertions": {"attempted": False, "status": "failure", "reason": adapter_problem},
                    "native_acceptance": {"attempted": False, "status": "failure", "reason": adapter_problem},
                    "scoring": {"attempted": False, "status": "failure", "reason": adapter_problem},
                })
        report["selection"]["import_target_count"] = len(selected)
        report["selection"]["export_target_count"] = len(emitted_export_rows)
        report["selection"]["selected_count"] = len(report["cases"])
        report["selection"]["registry_count"] = len(cases) + (
            export_registry_count if export_registry_count is not None else len(expected_exports)
        )
        report["selection"]["selected_case_ids"] = selected_ids + list(expected_export_ids)
        report["selection"]["export_reported_count"] = emitted_count
        report["direction_scope"] = (["import"] if selected else []) + ["export"]
        if not selected:
            report["validation_claim"] = (
                "One explicit export selection: CPU assertions, native acceptance and "
                "independent reference comparison; missing proof remains failure. "
                "No import or whole-converter coverage is claimed."
            )

    if selected_audio:
        emitted_audio: list[dict[str, Any]] = []
        audio_problem = "audio adapter omitted the selected case/direction"

        def on_audio_row(row: dict[str, Any]) -> None:
            nonlocal cancelled
            if not isinstance(row, dict) or row.get("case_id") not in audio_ids:
                raise AdobeTestError("audio adapter emitted an unknown case ID")
            case_id = row["case_id"]
            expected = audio_cases[case_id]
            if (row.get("direction") != expected["direction"]
                    or row.get("identity", {}).get("source") != expected["source"]
                    or row.get("identity", {}).get("reference") != expected["reference"]
                    or row.get("status") not in {"success", "failure"}
                    or any(existing["case_id"] == case_id for existing in report["cases"])):
                raise AdobeTestError(f"{case_id}: invalid or duplicate audio evidence identity")
            if row.get("cancelled") is True:
                cancelled = True
            report["cases"].append(row)
            emitted_audio.append(row)
            _write_case(run_dir, row)
            _atomic_json(report_path, report)

        if audio_runner is not None and not cpu_only:
            try:
                returned = audio_runner(selected=selected_audio, run_dir=run_dir,
                    tools=tools, timeout=score_timeouts["render"], on_row=on_audio_row,
                    cancelled=lambda: cancelled)
                if [row.get("case_id") for row in returned] != [row["case_id"] for row in emitted_audio]:
                    report["suite_failures"].append("audio adapter return differs from persisted rows")
            except KeyboardInterrupt:
                cancelled = True
                audio_problem = "audio adapter cancelled by operator"
                report["suite_failures"].append(audio_problem)
            except Exception as exc:
                audio_problem = f"audio adapter failed: {type(exc).__name__}: {exc}"
                report["suite_failures"].append(audio_problem)
        else:
            audio_problem = "audio execution not requested by --cpu-only or adapter unavailable"
        emitted_count = len(emitted_audio)
        if emitted_count != len(audio_ids) and audio_runner is not None and not cpu_only:
            report["suite_failures"].append(
                f"audio adapter emitted {emitted_count} rows; exactly {len(audio_ids)} are required")
        for case in selected_audio:
            if case["case_id"] in {row["case_id"] for row in emitted_audio}:
                continue
            reason = "cancelled before audio execution" if cancelled else audio_problem
            on_audio_row({
                "case_id": case["case_id"], "direction": case["direction"],
                "identity": {key: case[key] for key in ("case_id", "slug", "direction", "source", "primary", "reference", "fx_input", "native_expected", "expected_diagnostics", "reference_expectation")},
                "status": "failure", "score": None, "score_reason": reason,
                "audio_metric": "pinned_stereo_pcm_audio_policy", "execution": "UNRUN", "measurement": "unmeasured",
                "assertions": {"attempted": False, "status": "failure", "reason": reason},
                "scoring": {"attempted": False, "status": "failure", "reason": reason},
                "native_acceptance": {"attempted": False, "status": "failure", "reason": reason}
                    if case["direction"] == "export" else {"attempted": False, "status": "not_applicable"},
            })
        report["audio_inventory"]["reported_count"] = emitted_count
        report["selection"]["selected_count"] = len(report["cases"])
        report["selection"]["registry_count"] += len(audio_cases)
        report["selection"]["selected_case_ids"] += audio_ids
        report["direction_scope"] = sorted(set(report["direction_scope"]) | {case["direction"] for case in selected_audio})
        report["validation_claim"] += " Audio comparisons are separate PCM measurements; export needs a fresh Adobe native render."

    report["selection"]["full_coverage_claimed"] = (
        not cpu_only
        and not cancelled
        and report["selection"]["mode"] == "full_registry"
        and export_runner is not None
        and export_registry_count is not None
        and len(set(expected_export_ids)) == export_registry_count
        and (not audio_cases or set(audio_ids) == set(audio_cases))
        and all(
            (row["assertions"].get("record_valid") is True
             and row["assertions"]["attempted"] and row["scoring"]["attempted"])
            if row["case_id"] in cases
            else (row["assertions"]["attempted"] and row["native_acceptance"]["attempted"])
            if row["case_id"] in expected_export_ids
            else (row["status"] == "success" and row["measurement"] == "measured"
                  and row["assertions"]["attempted"] and row["scoring"]["attempted"]
                  and (row["direction"] == "import" or row["native_acceptance"]["status"] == "success"))
            for row in report["cases"]
        )
    )
    report["state"] = "cancelled" if cancelled else "completed"
    report["finished_at"] = aep_test.utc_now()
    report["counts"] = {
        "selected": len(report["cases"]),
        "success": sum(row["status"] == "success" for row in report["cases"]),
        "failure": sum(row["status"] == "failure" for row in report["cases"]),
        "scored": sum(row["score"] is not None for row in report["cases"]),
        "audio_measured": sum(row.get("measurement") == "measured" for row in report["cases"]),
        "audio_unmeasured": sum(row.get("audio_metric") is not None and row.get("measurement") != "measured"
                                for row in report["cases"]),
        "assertions_attempted": sum(row["assertions"]["attempted"] for row in report["cases"]),
        "scoring_attempted": sum(row["scoring"]["attempted"] for row in report["cases"]),
        "suite_failures": len(report["suite_failures"]),
    }
    _atomic_json(report_path, report)
    if cancelled:
        return report, 130
    failed = report["counts"]["failure"] > 0 or bool(report["suite_failures"])
    return report, int(failed)


def _new_run_dir(scratch: Path) -> Path:
    scratch.mkdir(parents=True, exist_ok=True)
    return Path(tempfile.mkdtemp(prefix="run-", dir=scratch))


def _parse_reference_map(
    path: Path | None,
    selected_ids: set[str],
    known_ids: set[str],
) -> dict[str, Path]:
    if path is None:
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise AdobeTestError(f"cannot load --reference-map {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise AdobeTestError("--reference-map must contain a JSON object of CASE_ID to local MP4 path")
    result: dict[str, Path] = {}
    for case_id, local_path in value.items():
        if not isinstance(case_id, str) or not isinstance(local_path, str) or not local_path:
            raise AdobeTestError("--reference-map keys and values must be nonempty strings")
        if case_id not in known_ids:
            raise AdobeTestError(f"--reference-map contains unknown case ID {case_id!r}")
        if case_id in selected_ids:
            result[case_id] = Path(local_path).expanduser().resolve()
    return result


def _parse_references(
    values: list[str],
    reference_map: Path | None,
    selected_ids: set[str],
    known_ids: set[str],
) -> dict[str, Path]:
    mapped = _parse_reference_map(reference_map, selected_ids, known_ids)
    explicit = aep_test._parse_local_references(values, selected_ids)
    overlap = sorted(mapped.keys() & explicit.keys())
    if overlap:
        raise AdobeTestError(
            "local references are specified by both --reference-map and --reference for: "
            + ", ".join(overlap)
        )
    mapped.update(explicit)
    return mapped


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, default=REPO)
    parser.add_argument("--registry", type=Path, default=aep_test.DEFAULT_REGISTRY)
    parser.add_argument("--references", type=Path, default=aep_test.DEFAULT_REFERENCES)
    parser.add_argument(
        "--case-id", action="append", default=[],
        help="exact import, Effects or Adjustment export subset; repeatable",
    )
    parser.add_argument("--export-case-id", help="run one declared Effects export and its exact CPU test; no imports")
    parser.add_argument("--audio-case-id", action="append", default=[],
                        help="select an audio case/direction ID; repeatable, no visual cases")
    parser.add_argument("--records", type=Path, help="offline import CPU JSONL; requires --cpu-exit-code")
    parser.add_argument("--export-records", type=Path, help="offline export CPU JSONL used with --records")
    parser.add_argument("--cpu-exit-code", type=int)
    parser.add_argument("--cpu-timeout", type=int, default=120)
    parser.add_argument("--cpu-only", action="store_true", help="validate CPU records only; not full Adobe validation")
    parser.add_argument("--tsrct-conv", "--tesseract-conv", dest="tesseract_conv", type=Path, default=REPO / "target/debug/tsrct-conv")
    parser.add_argument("--audio-preparer", type=Path, default=REPO / "target/debug/examples/audio_e2e")
    parser.add_argument("--tsrct", type=Path, default=REPO.parents[1] / "target/debug/tsrct")
    parser.add_argument("--validation", type=Path, default=REPO.parents[1] / "target/debug/validation_cli")
    parser.add_argument(
        "--aerender",
        type=Path,
        default=None,
        help="Deprecated compatibility option; never executed. Configure HEADLESS_ADOBE_COMMAND.",
    )
    parser.add_argument("--ffprobe", default="ffprobe")
    parser.add_argument("--cache-dir", type=Path, default=aep_test.DEFAULT_CACHE)
    parser.add_argument("--scratch-dir", type=Path, default=DEFAULT_SCRATCH)
    parser.add_argument("--reference", action="append", default=[], metavar="CASE_ID=MP4")
    parser.add_argument(
        "--reference-map",
        type=Path,
        help="JSON object mapping case IDs to local MP4s; each file is still inventory-hash verified",
    )
    parser.add_argument("--import-timeout", type=int, default=300)
    parser.add_argument("--export-timeout", type=int, default=300)
    parser.add_argument("--readback-timeout", type=int, default=180)
    parser.add_argument("--render-timeout", type=int, default=900)
    parser.add_argument("--compare-timeout", type=int, default=900)
    parser.add_argument("--metadata-timeout", type=int, default=60)
    parser.add_argument("--max-samples", type=int, default=601)
    return parser


def _print_summary(report: dict[str, Any], report_path: Path) -> None:
    for row in report["cases"]:
        score = "null" if row["score"] is None else f"{row['score']:.9f}"
        print(
            f"{row['status'].upper()}\t{row['direction']}\t{row['case_id']}\t"
            f"score={score}\tassertions={row['assertions']['status']}\t"
            f"scoring={row['scoring']['status']}",
            file=sys.stderr,
        )
    counts = report["counts"]
    print(
        f"summary: selected={counts['selected']} success={counts['success']} failure={counts['failure']} "
        f"scored={counts['scored']} suite_failures={counts['suite_failures']}",
        file=sys.stderr,
    )
    print(f"report: {report_path}", file=sys.stderr)


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    positive = (
        "cpu_timeout",
        "import_timeout",
        "export_timeout",
        "readback_timeout",
        "render_timeout",
        "compare_timeout",
        "metadata_timeout",
        "max_samples",
    )
    for name in positive:
        if getattr(args, name) <= 0:
            parser.error(f"--{name.replace('_', '-')} must be positive")
    if args.export_case_id is not None and (args.case_id or args.cpu_only or args.audio_case_id):
        parser.error("--export-case-id cannot be combined with --case-id, --audio-case-id or --cpu-only")
    if args.audio_case_id and args.case_id:
        parser.error("--audio-case-id cannot be combined with --case-id")
    if (args.records is None) != (args.cpu_exit_code is None):
        parser.error("--records and --cpu-exit-code must be supplied together")
    if args.audio_case_id and args.records is not None:
        parser.error("--audio-case-id does not accept import CPU records")

    try:
        workspace = args.workspace.resolve()
        try:
            from adobe_export_test import load_cases as load_export_cases, run_exports
        except ImportError as exc:
            raise AdobeTestError(f"explicit export adapter is unavailable: {exc}") from exc
        cases, targets, references = aep_test.load_catalog(workspace, args.registry, args.references)
        export_cases = {case.case_id: case for case in load_export_cases(workspace)}
        effects_export_ids = set(export_cases)
        adjustment_export_ids = ADJUSTMENT_EXPORT_CASE_IDS
        audio_cases = adobe_audio_cases.load_inventory()
        if set(audio_cases) & (set(cases) | effects_export_ids | adjustment_export_ids):
            raise AdobeTestError("audio case IDs collide with visual cases")
        if len(args.audio_case_id) != len(set(args.audio_case_id)):
            raise AdobeTestError("duplicate --audio-case-id values are not allowed")
        unknown_audio = set(args.audio_case_id) - set(audio_cases)
        if unknown_audio:
            raise AdobeTestError(f"unknown audio case ID(s): {', '.join(sorted(unknown_audio))}")
        selected_audio = [audio_cases[case_id] for case_id in (args.audio_case_id or sorted(audio_cases))]
        requested_ids = args.case_id
        cpu_command = DEFAULT_CPU_COMMAND
        if args.export_case_id is not None:
            if args.export_case_id not in export_cases:
                raise AdobeTestError(f"unknown export case ID: {args.export_case_id}")
            requested_ids = [args.export_case_id]
            cpu_command = (
                "make", "adobe-test-cpu",
                f"filter={export_cases[args.export_case_id].test_symbol}",
            )
        selected, selected_adjustment_exports, selected_effects_exports = select_case_dispatch(
            cases, requested_ids, adjustment_export_ids, effects_export_ids
        )
        if args.cpu_only and selected_adjustment_exports:
            raise AdobeTestError("Adjustment export cases cannot be selected with --cpu-only")
        if requested_ids:
            selected_audio = []
        if args.audio_case_id:
            selected = []
        selected_ids = {case["case_id"] for case in selected}
        if not requested_ids and not args.audio_case_id:
            selected_ids.update(effects_export_ids)
        else:
            selected_ids.update(selected_adjustment_exports)
            selected_ids.update(selected_effects_exports)
        local_references = _parse_references(
            args.reference,
            args.reference_map,
            selected_ids,
            set(cases) | effects_export_ids | adjustment_export_ids,
        )
        aep_test._reject_fixture_output_path(workspace, args.scratch_dir, "unified scratch")
        aep_test._reject_fixture_output_path(workspace, args.cache_dir, "unified cache")
        run_dir = _new_run_dir(args.scratch_dir.resolve())
        export_runner: ExportRunner | None
        expected_export_ids: list[str]
        if selected_adjustment_exports:
            try:
                from adobe_adjustment_test import run_exports as run_adjustment_exports
            except ImportError as exc:
                raise AdobeTestError(
                    f"selected Adjustment export requires the private Adobe adapter: {exc}"
                ) from exc
            expected_export_ids = selected_adjustment_exports

            def export_runner(**kwargs: Any) -> list[dict[str, Any]]:
                adjustment_kwargs = dict(kwargs)
                adjustment_tools = {
                    **adjustment_kwargs.pop("tools"),
                    "tesseract_conv": args.tesseract_conv,
                }
                adjustment_timeouts = {
                    **adjustment_kwargs.pop("timeouts"),
                    "export": args.export_timeout,
                    "readback": args.readback_timeout,
                }
                return run_adjustment_exports(
                    **adjustment_kwargs,
                    selected_case_ids=selected_adjustment_exports,
                    tools=adjustment_tools,
                    timeouts=adjustment_timeouts,
                )

        elif selected_effects_exports:
            expected_export_ids = selected_effects_exports

            def export_runner(**kwargs: Any) -> list[dict[str, Any]]:
                return run_exports(**kwargs, selected_case_ids=selected_effects_exports)

        elif not requested_ids and not args.audio_case_id:
            export_runner = run_exports
            expected_export_ids = sorted(effects_export_ids)
        else:
            export_runner = None
            expected_export_ids = []

        report, code = run_unified(
            workspace=workspace,
            cases=cases,
            targets=targets,
            reference_settings=references["reference_settings"],
            selected=selected,
            run_dir=run_dir,
            tools={
                "tesseract_conv": args.tesseract_conv,
                "audio_preparer": args.audio_preparer,
                "tsrct": args.tsrct,
                "validation": args.validation,
                "ffprobe": args.ffprobe,
                "aerender": args.aerender,
            },
            cache=args.cache_dir.resolve(),
            local_references=local_references,
            max_samples=args.max_samples,
            cpu_timeout=args.cpu_timeout,
            cpu_only=args.cpu_only,
            score_timeouts={
                "import": args.import_timeout,
                "render": args.render_timeout,
                "compare": args.compare_timeout,
                "metadata": args.metadata_timeout,
            },
            records_path=args.records,
            cpu_exit_code=args.cpu_exit_code,
            cpu_command=cpu_command,
            export_runner=export_runner,
            expected_export_ids=expected_export_ids,
            export_registry_count=len(effects_export_ids),
            export_records_path=args.export_records,
            export_local_references={
                case_id: path
                for case_id, path in local_references.items()
                if case_id.startswith("fx-export-")
            },
            audio_cases=audio_cases,
            selected_audio=selected_audio,
            audio_runner=adobe_audio_cases.run_cases,
        )
        report_path = run_dir / "report.json"
        _print_summary(report, report_path)
        machine = {
            "schema_version": REPORT_SCHEMA_VERSION,
            "status": "cancelled" if code == 130 else ("success" if code == 0 else "failure"),
            "exit_code": code,
            "report": str(report_path),
            "selection": report["selection"],
            "counts": report["counts"],
        }
        print(json.dumps(machine, sort_keys=True))
        return code
    except (AdobeTestError, aep_test.AepTestError) as exc:
        parser.error(str(exc))
    return 2


if __name__ == "__main__":
    sys.exit(main())
