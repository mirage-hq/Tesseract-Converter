#!/usr/bin/env python3
"""Bounded explicit-FX export validation for the unified Adobe result report.

The population is derived from checked-in Rust test declarations, never from
whatever happens to exist in scratch.  This module does not launch a GUI host or
run Adobe scripting; ``aerender`` acceptance and independent RGB comparison are
separate from the fresh Rust property assertions recorded by the parent run.
"""

from __future__ import annotations

import json
import math
import re
import subprocess
import sys
from collections.abc import Callable
from dataclasses import dataclass
from fractions import Fraction
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import aep_feature_proof as proof
import aep_test
import adobe_vector_controls
import adobe_native
from aep_adjustment_cases import ADJUSTMENT_EXPORT_CASE_IDS

COVERAGE_CASES = Path(
    "crates/aftereffects_file/tests/fixtures/effects_coverage/cases.json"
)
COVERAGE_TESTS = Path(
    "crates/aftereffects_file/src/export_document/tests/effects_native_coverage.rs"
)
PANEL_TESTS = Path(
    "crates/aftereffects_file/src/export_document/tests/effects_native_panel.rs"
)
ADOBE_TEST_SUPPORT = Path("crates/aftereffects_file/src/adobe_test_support.rs")
PANEL_FIXTURES = Path("crates/aftereffects_file/tests/fixtures/effects/fx_export_panel")
VECTOR_TESTS = Path(
    "crates/aftereffects_file/src/export_document/tests/vector_native_panel.rs"
)
VECTOR_FIXTURES = Path("crates/aftereffects_file/tests/fixtures/vectors/fx_export_panel")
NON_AUDIO_TESTS = Path("crates/aftereffects_file/src/export_document/tests/non_audio_native_panel.rs")
NON_AUDIO_FIXTURES = Path("crates/aftereffects_file/tests/fixtures/non_audio_native_panel")
NON_AUDIO_ROUTES = NON_AUDIO_FIXTURES / "reference-routes.json"
ADDITIONAL_PANELS = tuple(
    (Path(f"crates/aftereffects_file/src/export_document/tests/{name}.rs"),
     Path(f"crates/aftereffects_file/tests/fixtures/{name}"), name)
    for name in ("non_audio_native_panel", "core_native_panel", "media_native_panel", "text_controls_native_panel")
)
REFERENCES = Path("crates/aftereffects_file/tests/fixtures/aep_video_references.json")
COVERAGE_CONTROL_EVIDENCE = Path(
    "docs/after-effects-evidence/effects-coverage-results.json"
)
PANEL_CONTROL_EVIDENCE = Path("docs/after-effects-evidence/effects-native-panel.json")
VECTOR_CONTROL_EVIDENCE = Path("docs/after-effects-evidence/vector-native-panel.json")
NON_AUDIO_CONTROL_EVIDENCE = NON_AUDIO_FIXTURES / "native-control-evidence.json"
VECTOR_INSPECTOR = Path("scripts/aep-vector-native.jsx")
# New Vignette Adobe render scores are measured, but exact-hash control evidence
# for the scored AEPs is not pinned. Preserve all earlier inspected outcomes.
PENDING_CONTROL_EVIDENCE = frozenset({"vignette-static", "vignette-animated"})
HUE_ANIMATED_CASE = "hueSaturation-animated"
HUE_ANIMATED_REQUESTED_CONTROLS = (
    "ADBE HUE SATURATION-0004",
    "ADBE HUE SATURATION-0005",
    "ADBE HUE SATURATION-0006",
    "ADBE HUE SATURATION-0007",
    "ADBE HUE SATURATION-0008",
    "ADBE HUE SATURATION-0009",
    "ADBE HUE SATURATION-0010",
)
HUE_ANIMATED_NATIVE_ORACLE_CONTROLS = frozenset(
    {
        "ADBE HUE SATURATION-0008",
        "ADBE HUE SATURATION-0009",
        "ADBE HUE SATURATION-0010",
    }
)

RENDER_SETTINGS = "Use this frame rate: 30; Quality: Best; Resolution: Full"
OUTPUT_MODULE = "H.264 - Match Render Settings - 40 Mbps"
PANEL_SCORE_REASON = (
    "independent native render reference is not available for this panel case"
)
MAX_RECORD_BYTES = 2 * 1024 * 1024
EXPECTED_WIDTH = 320
EXPECTED_HEIGHT = 180
EXPECTED_FRAMES = 60
EXPECTED_DURATION = 2
MAX_SAMPLES = EXPECTED_FRAMES + 1
DIAGNOSTIC_RE = re.compile(
    r"\b(?:warning|warnings|error|errors|fatal)\b", re.IGNORECASE
)
ZERO_DIAGNOSTIC_RE = re.compile(
    r"(?:\b(?:no|0)\s+(?:warnings?|errors?)\b|\b(?:warnings?|errors?)\s*:\s*0\b)",
    re.IGNORECASE,
)

RowCallback = Callable[[dict[str, Any]], None]
CancelCheck = Callable[[], bool]


class ExportAdapterError(RuntimeError):
    """A local adapter contract or case validation failure."""


@dataclass(frozen=True)
class ExportCase:
    name: str
    test_symbol: str
    coverage: bool
    case_spec: dict[str, Any] | None
    fixture_dir: Path = PANEL_FIXTURES
    test_source: Path = PANEL_TESTS

    @property
    def control_kind(self) -> str:
        if any(self.test_source == source for source, _, _ in ADDITIONAL_PANELS):
            return "non_audio"
        return "vector" if self.test_source == VECTOR_TESTS else ("coverage" if self.coverage else "panel")

    @property
    def case_id(self) -> str:
        return f"fx-export-{self.name}"


@dataclass(frozen=True)
class ParsedRecord:
    phase: dict[str, Any]
    valid: bool
    assertions_succeeded: bool


def _load_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ExportAdapterError(f"cannot load {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise ExportAdapterError(f"{path} must contain a JSON object")
    return value


def _coverage_declarations(source: str) -> dict[tuple[str, bool], str]:
    pattern = re.compile(
        r"coverage_case!\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*,\s*"
        r"(?:(?:([A-Za-z_][A-Za-z0-9_]*)\s*,\s*))?\"([^\"]+)\"\s*\);",
        re.MULTILINE,
    )
    declarations: dict[tuple[str, bool], str] = {}
    for static_test, animated_test, kind in pattern.findall(source):
        keys = [((kind, False), static_test)]
        if animated_test:
            keys.append(((kind, True), animated_test))
        for key, test_name in keys:
            if key in declarations:
                raise ExportAdapterError(
                    f"duplicate Rust coverage declaration for {key}"
                )
            declarations[key] = test_name
    return declarations


def _panel_declarations(source: str) -> dict[str, str]:
    pattern = re.compile(
        r'panel_case!\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*,\s*"([^"]+)"\s*,\s*'
        r'include_str!\(\s*"([^"]+)"\s*\)\s*,\s*'
        r'include_str!\(\s*"([^"]+)"\s*\)\s*\);'
    )
    declarations: dict[str, str] = {}
    for test_name, case_name, input_path, expected_path in pattern.findall(source):
        if not input_path.endswith(f"/{case_name}.fx.json") or not expected_path.endswith(
            f"/{case_name}.expected.json"
        ):
            raise ExportAdapterError(f"panel fixture declaration differs from {case_name}")
        if case_name in declarations:
            raise ExportAdapterError(
                f"duplicate Rust panel declaration for {case_name}"
            )
        declarations[case_name] = test_name
    return declarations


def load_cases(workspace: Path) -> list[ExportCase]:
    """Return declared export cases, preserving the 65 Effects cases."""
    workspace = workspace.resolve()
    coverage_data = _load_object(workspace / COVERAGE_CASES)
    raw_cases = coverage_data.get("cases")
    if coverage_data.get("version") != 1 or not isinstance(raw_cases, list):
        raise ExportAdapterError(
            "effects coverage registry must be version 1 with a cases array"
        )
    coverage_source = (workspace / COVERAGE_TESTS).read_text(encoding="utf-8")
    declarations = _coverage_declarations(coverage_source)
    cases: list[ExportCase] = []
    seen_kinds: set[str] = set()
    expected_declarations: set[tuple[str, bool]] = set()
    for raw in raw_cases:
        if not isinstance(raw, dict) or not isinstance(raw.get("id"), str):
            raise ExportAdapterError(
                "effects coverage registry contains an invalid case"
            )
        kind = raw["id"]
        if re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", kind) is None:
            raise ExportAdapterError(f"unsafe effects coverage case name: {kind!r}")
        if kind in seen_kinds:
            raise ExportAdapterError(f"duplicate effects coverage case: {kind}")
        seen_kinds.add(kind)
        tracks = raw.get("tracks")
        if not isinstance(tracks, list):
            raise ExportAdapterError(f"{kind}: tracks must be an array")
        variants = [False, *([True] if tracks else [])]
        for animated in variants:
            key = (kind, animated)
            expected_declarations.add(key)
            test_name = declarations.get(key)
            if test_name is None:
                raise ExportAdapterError(
                    f"{kind}: missing exact Rust {'animated' if animated else 'static'} test"
                )
            suffix = "animated" if animated else "static"
            cases.append(
                ExportCase(
                    name=f"{kind}-{suffix}",
                    test_symbol=(
                        f"export_document::tests::effects_native_coverage::{test_name}"
                    ),
                    coverage=True,
                    case_spec=raw,
                )
            )
    if set(declarations) != expected_declarations:
        raise ExportAdapterError(
            "Rust coverage declarations and cases.json variants differ"
        )

    panel_source = (workspace / PANEL_TESTS).read_text(encoding="utf-8")
    panel_declarations = _panel_declarations(panel_source)
    for case_name, test_name in panel_declarations.items():
        if re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", case_name) is None:
            raise ExportAdapterError(f"unsafe panel case name: {case_name!r}")
        fixture = workspace / PANEL_FIXTURES / f"{case_name}.fx.json"
        if not fixture.is_file():
            raise ExportAdapterError(f"{case_name}: panel FX fixture is missing")
        cases.append(
            ExportCase(
                name=case_name,
                test_symbol=f"export_document::tests::effects_native_panel::{test_name}",
                coverage=False,
                case_spec=None,
            )
        )
    if (
        len(raw_cases) != 30
        or sum(bool(case.get("tracks")) for case in raw_cases) != 27
    ):
        raise ExportAdapterError(
            "explicit coverage population must remain 30 static plus 27 animated"
        )
    if len(panel_declarations) != 8 or len(cases) != 65:
        raise ExportAdapterError(
            "explicit export population must remain 57 coverage plus 8 panel cases"
        )
    vector_declarations = _panel_declarations(
        (workspace / VECTOR_TESTS).read_text(encoding="utf-8")
    )
    if not vector_declarations:
        raise ExportAdapterError("native vector panel has no declared cases")
    for case_name, test_name in vector_declarations.items():
        if re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", case_name) is None:
            raise ExportAdapterError(f"unsafe vector panel case name: {case_name!r}")
        for suffix in ("fx.json", "expected.json"):
            if not (workspace / VECTOR_FIXTURES / f"{case_name}.{suffix}").is_file():
                raise ExportAdapterError(f"{case_name}: vector panel {suffix} is missing")
        cases.append(ExportCase(
            name=case_name,
            test_symbol=f"export_document::tests::vector_native_panel::{test_name}",
            coverage=False,
            case_spec=None,
            fixture_dir=VECTOR_FIXTURES,
            test_source=VECTOR_TESTS,
        ))
    for test_source, fixture_dir, module in ADDITIONAL_PANELS:
        declarations = _panel_declarations((workspace / test_source).read_text(encoding="utf-8"))
        if not declarations:
            raise ExportAdapterError(f"{module}: export panel has no declared cases")
        for case_name, test_name in declarations.items():
            if re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", case_name) is None:
                raise ExportAdapterError(f"unsafe non-audio case name: {case_name!r}")
            for suffix in ("fx.json", "expected.json"):
                if not (workspace / fixture_dir / f"{case_name}.{suffix}").is_file():
                    raise ExportAdapterError(f"{case_name}: non-audio panel {suffix} is missing")
            cases.append(ExportCase(
                name=case_name,
                test_symbol=f"export_document::tests::{module}::{test_name}",
                coverage=False,
                case_spec=None,
                fixture_dir=fixture_dir,
                test_source=test_source,
            ))
    if len({case.case_id for case in cases}) != len(cases):
        raise ExportAdapterError("duplicate explicit export case ID")
    return cases


def _test_binary_error(value: Any) -> str | None:
    if not isinstance(value, dict):
        return "test_binary must be an object"
    if not isinstance(value.get("path"), str) or not value["path"]:
        return "test_binary.path must be a nonempty string"
    sha256 = value.get("sha256")
    if (
        not isinstance(sha256, str)
        or re.fullmatch(r"[0-9a-fA-F]{64}", sha256) is None
    ):
        return "test_binary.sha256 must be a 64-character hexadecimal SHA-256"
    size = value.get("bytes")
    if not isinstance(size, int) or isinstance(size, bool) or size <= 0:
        return "test_binary.bytes must be a positive integer"
    return None


def _record_error(record: dict[str, Any], case: ExportCase) -> str | None:
    expected = {
        "schema_version": 1,
        "case_id": case.case_id,
        "case_name": case.name,
        "direction": "export",
        "test_symbol": case.test_symbol,
        "attempted": True,
        "assertions_executed": True,
    }
    for key, value in expected.items():
        if record.get(key) != value:
            return f"{key} mismatch: expected {value!r}, got {record.get(key)!r}"
    test_binary_problem = _test_binary_error(record.get("test_binary"))
    if test_binary_problem is not None:
        return test_binary_problem
    status = record.get("status")
    error = record.get("error")
    if status not in ("success", "failure"):
        return "status must be success or failure"
    if status == "success" and error is not None:
        return "successful record must have error=null"
    if status == "failure" and (not isinstance(error, str) or not error.strip()):
        return "failed record must have a nonempty error string"
    artifacts = record.get("artifacts")
    expected_artifacts = {
        "fx_json",
        "expected_json",
        "aep",
        *(["tsrct"] if case.coverage else []),
    }
    if not isinstance(artifacts, dict) or set(artifacts) != expected_artifacts:
        return f"artifacts must contain exactly {sorted(expected_artifacts)}"
    for name, artifact in artifacts.items():
        if not isinstance(artifact, dict):
            return f"artifacts.{name} must be an object"
        if not isinstance(artifact.get("path"), str) or not artifact["path"]:
            return f"artifacts.{name}.path must be a nonempty string"
        sha256 = artifact.get("sha256")
        if not isinstance(sha256, str) or re.fullmatch(r"[0-9a-f]{64}", sha256) is None:
            return f"artifacts.{name}.sha256 must be a lowercase SHA-256"
        size = artifact.get("bytes")
        if not isinstance(size, int) or isinstance(size, bool) or size <= 0:
            return f"artifacts.{name}.bytes must be a positive integer"
    return None


def parse_records(
    records_path: Path, cases: list[ExportCase]
) -> tuple[dict[str, ParsedRecord], list[str]]:
    """Validate the fixed-population JSONL without inventing unrecorded passes."""
    try:
        if records_path.is_symlink() or not records_path.is_file():
            raise ExportAdapterError(
                "CPU records path must be a regular non-symlink file"
            )
        if records_path.stat().st_size > MAX_RECORD_BYTES:
            raise ExportAdapterError("CPU records exceed the bounded 2 MiB limit")
        lines = records_path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        raise ExportAdapterError(f"cannot read CPU records: {exc}") from exc

    # The parent CPU run emits both families into the same JSONL. The fixed
    # public allowlist avoids importing the private Adobe runner from conv;
    # unknown IDs still fail this 65-case population rather than disappearing
    # by prefix.
    known = {case.case_id: case for case in cases}
    records: dict[str, list[dict[str, Any]]] = {case.case_id: [] for case in cases}
    batch_errors: list[str] = []
    for line_number, line in enumerate(lines, 1):
        if not line.strip():
            batch_errors.append(f"record line {line_number} is blank")
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            batch_errors.append(f"record line {line_number} is invalid JSON: {exc.msg}")
            continue
        if not isinstance(value, dict):
            batch_errors.append(f"record line {line_number} is not an object")
            continue
        case_id = value.get("case_id")
        if isinstance(case_id, str) and case_id in ADJUSTMENT_EXPORT_CASE_IDS:
            continue
        if not isinstance(case_id, str) or case_id not in known:
            batch_errors.append(
                f"record line {line_number} has unknown case_id {case_id!r}"
            )
            continue
        records[case_id].append(value)

    parsed: dict[str, ParsedRecord] = {}
    for case in cases:
        candidates = records[case.case_id]
        if not candidates:
            reason = "missing fresh CPU record"
            phase = {
                "attempted": False,
                "status": "failure",
                "reason": reason,
                "record": None,
            }
            parsed[case.case_id] = ParsedRecord(phase, False, False)
            continue
        if len(candidates) != 1:
            reason = f"duplicate CPU records: expected one, got {len(candidates)}"
            phase = {
                "attempted": True,
                "status": "failure",
                "reason": reason,
                "record": None,
            }
            parsed[case.case_id] = ParsedRecord(phase, False, False)
            continue
        record = candidates[0]
        problem = _record_error(record, case)
        if problem is not None:
            phase = {
                "attempted": bool(record.get("attempted")),
                "status": "failure",
                "reason": problem,
                "record": record,
            }
            parsed[case.case_id] = ParsedRecord(phase, False, False)
            continue
        succeeded = record["status"] == "success"
        reason = "fresh Rust export assertions passed" if succeeded else record["error"]
        phase = {
            "attempted": True,
            "status": "success" if succeeded else "failure",
            "reason": reason,
            "assertions_executed": True,
            "record": record,
        }
        parsed[case.case_id] = ParsedRecord(phase, True, succeeded)
    return parsed, batch_errors


def _expected_coverage_oracle(spec: dict[str, Any], animated: bool) -> dict[str, Any]:
    controls = []
    for control in spec["controls"]:
        if animated and "end" in control:
            start = control["value"]
            end = control["end"]
            middle = [
                a if control["segment"] == "hold" else (float(a) + float(b)) / 2
                for a, b in zip(start, end)
            ]
            controls.append(
                {
                    "name": control["name"],
                    "keys": [[0, *start], [1, *end]],
                    "segments": [control["segment"]],
                    "valueAtTime": control.get("samples", [[0.5, *middle]]),
                }
            )
        else:
            controls.append({"name": control["name"], "value": control["value"]})
    return {"effect": spec["native_effect"], "enabled": True, "controls": controls}


def _expected_coverage_artifact_oracle(case: ExportCase) -> dict[str, Any]:
    """Return the native controls asserted by the fresh CPU artifact."""
    if not case.coverage or case.case_spec is None:
        raise ExportAdapterError(f"{case.name}: coverage oracle requested for a panel case")
    requested = _expected_coverage_oracle(
        case.case_spec, case.name.endswith("-animated")
    )
    if case.name != HUE_ANIMATED_CASE:
        return requested

    requested_names = tuple(control.get("name") for control in requested["controls"])
    if requested_names != HUE_ANIMATED_REQUESTED_CONTROLS:
        raise ExportAdapterError(
            "hueSaturation-animated: requested control contract changed"
        )
    # AE reports Master H/S/L and the Colorize toggle as non-keyable in both the
    # independent source and fresh exports. The Rust case still asserts all four
    # authored bases plus omission diagnostics; its panel oracle can independently
    # assert only the three numeric Colorize leaves that the source actually keys.
    return {
        **requested,
        "controls": [
            control
            for control in requested["controls"]
            if control["name"] in HUE_ANIMATED_NATIVE_ORACLE_CONTROLS
        ],
    }


def _validate_fx_input(value: dict[str, Any], case: ExportCase) -> None:
    try:
        composition = value["composition"]
        if composition["name"] != case.name:
            raise ValueError("composition name differs from the allowlisted case")
        if value["dimensions"] != {"width": EXPECTED_WIDTH, "height": EXPECTED_HEIGHT}:
            raise ValueError("composition dimensions are not 320x180")
        if not math.isclose(
            float(value["duration"]), EXPECTED_DURATION, rel_tol=0, abs_tol=1e-9
        ):
            raise ValueError("composition duration is not 2 seconds")
        if case.coverage:
            effects = composition["layers"][0]["effects"]
            if effects != [
                {"id": 9001, "enabled": True, "effect": case.case_spec["effect"]}
            ]:
                raise ValueError("coverage effect input differs from cases.json")
            animated = case.name.endswith("-animated")
            expected_params = (
                {track["param"] for track in case.case_spec["tracks"]}
                if animated
                else set()
            )
            actual_params = {
                entry["target"]["paramName"]
                for entry in composition["dynamics"]["entries"]
                if entry.get("target", {}).get("kind") == "effectProperty"
            }
            if actual_params != expected_params:
                raise ValueError("coverage animation targets differ from cases.json")
    except (KeyError, IndexError, TypeError, ValueError) as exc:
        raise ExportAdapterError(f"{case.name}: invalid FX input: {exc}") from exc


def _artifact_entry(path: Path, run_dir: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise ExportAdapterError(
            f"artifact is not a regular non-symlink file: {path.name}"
        )
    size = path.stat().st_size
    if size <= 0:
        raise ExportAdapterError(f"artifact is empty: {path.name}")
    return {
        "path": str(path.relative_to(run_dir)),
        "bytes": size,
        "sha256": aep_test.sha256_file(path),
    }


def _artifact_identity(
    workspace: Path,
    run_dir: Path,
    records_path: Path,
    artifact_dir: Path,
    case: ExportCase,
    record: dict[str, Any],
) -> dict[str, Any]:
    suffixes = [
        "fx.json",
        "expected.json",
        "aep",
        *(["tsrct"] if case.coverage else []),
    ]
    paths = {
        suffix.replace(".", "_"): artifact_dir / f"{case.name}.{suffix}"
        for suffix in suffixes
    }
    generator_paths = [workspace / case.test_source, workspace / ADOBE_TEST_SUPPORT]
    if case.coverage:
        generator_paths.append(workspace / COVERAGE_TESTS)
    for generator in generator_paths:
        if not generator.is_file():
            raise ExportAdapterError(
                f"current generator source is missing: {generator}"
            )
    bindings = record["artifacts"]
    for name, path in paths.items():
        if path.is_symlink() or not path.is_file():
            raise ExportAdapterError(f"missing fresh artifact: {path.name}")
        binding = bindings[name]
        bound_path = Path(binding["path"])
        if not bound_path.is_absolute():
            bound_path = run_dir / bound_path
        try:
            if bound_path.resolve(strict=True) != path.resolve(strict=True):
                raise ExportAdapterError(
                    f"CPU record path does not bind the owned {name} artifact"
                )
        except OSError as exc:
            raise ExportAdapterError(
                f"cannot resolve CPU-bound {name} artifact: {exc}"
            ) from exc
        size = path.stat().st_size
        sha256 = aep_test.sha256_file(path)
        if binding["bytes"] != size or binding["sha256"] != sha256:
            raise ExportAdapterError(
                f"CPU record SHA/size does not match fresh {name} artifact"
            )

    fx_value = _load_object(paths["fx_json"])
    expected_value = _load_object(paths["expected_json"])
    _validate_fx_input(fx_value, case)
    requested_control_oracle = None
    if case.coverage:
        expected_artifact_oracle = _expected_coverage_artifact_oracle(case)
        if expected_value != expected_artifact_oracle:
            raise ExportAdapterError(
                f"{case.name}: expected oracle differs from its reviewed native-control scope"
            )
        requested_control_oracle = _expected_coverage_oracle(
            case.case_spec, case.name.endswith("-animated")
        )
    else:
        committed_fx = workspace / case.fixture_dir / f"{case.name}.fx.json"
        committed_expected = workspace / case.fixture_dir / f"{case.name}.expected.json"
        if paths["fx_json"].read_bytes() != committed_fx.read_bytes():
            raise ExportAdapterError(
                f"{case.name}: panel FX artifact differs from committed input"
            )
        if paths["expected_json"].read_bytes() != committed_expected.read_bytes():
            raise ExportAdapterError(
                f"{case.name}: panel oracle artifact differs from committed oracle"
            )

    artifacts = {key: _artifact_entry(path, run_dir) for key, path in paths.items()}
    identity = {
        "case_name": case.name,
        "test_symbol": case.test_symbol,
        "artifacts": artifacts,
        "input_sha256": artifacts["fx_json"]["sha256"],
        "native_aep_sha256": artifacts["aep"]["sha256"],
        "generator_sources": {
            str(path.relative_to(workspace)): aep_test.sha256_file(path)
            for path in generator_paths
        },
        "records": {
            "path": str(records_path),
            "sha256": aep_test.sha256_file(records_path),
        },
    }
    if (
        requested_control_oracle is not None
        and requested_control_oracle != expected_value
    ):
        identity["requested_control_oracle"] = requested_control_oracle
        identity["artifact_control_oracle_scope"] = (
            "three independently keyable numeric Colorize leaves; the fresh Rust "
            "assertion separately checks all requested Master/toggle bases and "
            "animation-omission diagnostics"
        )
    if case.control_kind == "vector":
        identity["vector_control_oracle"] = expected_value
        identity["vector_inspector_sha256"] = aep_test.sha256_file(workspace / VECTOR_INSPECTOR)
    return identity


def _control_evidence_inventory(
    workspace: Path, cases: list[ExportCase]
) -> tuple[dict[str, tuple[str, dict[str, Any]]], str | None]:
    """Load immutable independent-control outcomes without claiming fresh inspection."""
    try:
        coverage = _load_object(workspace / COVERAGE_CONTROL_EVIDENCE)
        panel = _load_object(workspace / PANEL_CONTROL_EVIDENCE)
        vector = (_load_object(workspace / VECTOR_CONTROL_EVIDENCE)
                  if (workspace / VECTOR_CONTROL_EVIDENCE).is_file() else {"cases": []})
        non_audio = (_load_object(workspace / NON_AUDIO_CONTROL_EVIDENCE)
                     if (workspace / NON_AUDIO_CONTROL_EVIDENCE).is_file() else {"cases": []})
        inventory: dict[str, tuple[str, dict[str, Any]]] = {}
        for kind, entries in (
            ("coverage", coverage.get("cases")),
            ("panel", panel.get("cases")),
            ("vector", vector.get("cases")),
            ("non_audio", non_audio.get("cases")),
        ):
            if not isinstance(entries, list):
                raise ExportAdapterError(
                    f"{kind} native-control evidence has no cases array"
                )
            for entry in entries:
                if not isinstance(entry, dict):
                    raise ExportAdapterError(
                        f"{kind} native-control evidence case is not an object"
                    )
                name = entry.get("name") if kind == "coverage" else entry.get("case")
                if not isinstance(name, str) or name in inventory:
                    raise ExportAdapterError(
                        f"duplicate or invalid native-control evidence case: {name!r}"
                    )
                inventory[name] = (kind, entry)
        expected = {case.name for case in cases}
        established = {case.name for case in cases if case.coverage or case.test_source == PANEL_TESTS}
        if (not PENDING_CONTROL_EVIDENCE <= established
                or not established - PENDING_CONTROL_EVIDENCE <= set(inventory)
                or set(inventory) & PENDING_CONTROL_EVIDENCE
                or set(inventory) - expected):
            raise ExportAdapterError(
                "native-control evidence must retain the 63 inspected Effects cases, "
                "exclude the two pending Vignette cases, and contain no undeclared cases"
            )
        # Other new panels fail individually when independent evidence is absent.
        return inventory, None
    except (ExportAdapterError, OSError, json.JSONDecodeError) as exc:
        return {}, str(exc)


def _native_control_evidence(
    case: ExportCase,
    identity: dict[str, Any],
    evidence: tuple[str, dict[str, Any]] | None,
    inventory_problem: str | None,
) -> dict[str, Any]:
    if evidence is None:
        detail = inventory_problem or "case is absent from independent control evidence"
        return {
            "attempted": False,
            "status": "failure",
            "reason": f"unverified independent native-control proof: {detail}",
            "provenance": "unverified",
            "fresh_adobe_inspection": False,
        }
    kind, entry = evidence
    try:
        if kind != case.control_kind:
            raise ValueError("native-control evidence family differs from the case")
        if kind == "vector":
            expected_fx = entry["input"]["sha256"]
            expected_aep = entry["generated_aep"]["sha256"]
            if (entry["oracle_sha256"] != identity["artifacts"]["expected_json"]["sha256"]
                    or entry["inspector_sha256"] != identity["vector_inspector_sha256"]
                    or entry["source_sha256"] != identity["independent_source_declared"]["sha256"]):
                raise ValueError("vector source/oracle/inspector identity differs")
            if entry["source_readback"]["mode"] != "author" or entry["export_readback"]["mode"] != "inspect":
                raise ValueError("vector readback does not distinguish independent authoring from export inspection")
            failures = adobe_vector_controls.compare(case.name, identity["vector_control_oracle"], entry["source_readback"])
            failures += adobe_vector_controls.compare(case.name, identity["vector_control_oracle"], entry["export_readback"], entry["source_readback"])
            passed = not failures
        elif kind == "coverage":
            expected_fx = entry["explicit_fx_sha256"]
            expected_aep = entry["export_aep_sha256"]
            outcome = entry["native_control_readback"]
            status = outcome["status"]
            failures = outcome["failures"]
            if status not in ("passed", "failed") or not isinstance(failures, list):
                raise ValueError("native_control_readback outcome is malformed")
            passed = status == "passed" and not failures
        elif kind == "non_audio":
            # Publication alone is not independent editable-control inspection.
            # The authored-but-uninspected panel cannot be promoted by a receipt.
            raise ValueError("independent native-control inspection is not implemented")
        else:
            expected_fx = entry["input"]["sha256"]
            expected_aep = entry["generated_aep"]["sha256"]
            comparison = entry["comparison"]
            passed = comparison["static_key_values_and_interpolation"] == "passed"
            failures = (
                [] if passed else ["independent panel control comparison did not pass"]
            )
    except (KeyError, TypeError, ValueError) as exc:
        return {
            "attempted": False,
            "status": "failure",
            "reason": f"unverified independent native-control proof: malformed evidence: {exc}",
            "provenance": "unverified",
            "fresh_adobe_inspection": False,
        }
    if (
        identity["input_sha256"] != expected_fx
        or identity["native_aep_sha256"] != expected_aep
    ):
        return {
            "attempted": False,
            "status": "failure",
            "reason": "unverified independent native-control proof: current FX/AEP hashes do not match the inspected bytes",
            "provenance": "unverified",
            "fresh_adobe_inspection": False,
        }
    return {
        "attempted": False,
        "status": "success" if passed else "failure",
        "reason": (
            "independent native-control outcome reused because current FX and AEP hashes exactly match inspected bytes"
            if passed
            else "matching independently inspected native-control outcome failed: "
            + (
                "; ".join(str(item) for item in failures)
                or "failure recorded without detail"
            )
        ),
        "provenance": "reused_by_exact_hash",
        "fresh_adobe_inspection": False,
        "evidence_path": str(
            VECTOR_CONTROL_EVIDENCE if kind == "vector" else (
                COVERAGE_CONTROL_EVIDENCE if kind == "coverage" else PANEL_CONTROL_EVIDENCE
            )
        ),
        "bound_hashes": {"fx_json": expected_fx, "aep": expected_aep},
    }


def _reference_targets(
    workspace: Path, cases: list[ExportCase]
) -> dict[str, tuple[dict[str, Any], dict[str, Any]]]:
    manifest = _load_object(workspace / REFERENCES)
    try:
        validated = proof.validate_manifest(manifest)
    except proof.ProofError as exc:
        raise ExportAdapterError(
            f"invalid independent reference inventory: {exc}"
        ) from exc
    by_name: dict[str, tuple[dict[str, Any], dict[str, Any]]] = {}
    for source, composition in validated.values():
        if "/effects_coverage/" not in source["source_path"]:
            continue
        name = composition.get("composition_name")
        if not isinstance(name, str) or name in by_name:
            raise ExportAdapterError(
                f"duplicate or invalid effects coverage reference name: {name!r}"
            )
        by_name[name] = (source, composition)
    # The independently authored hue-master import oracle shares the effects
    # directory but has no explicit FX-export CPU case. It must not turn into
    # an export scoring target; reject any other unexpected reference.
    import_only = by_name.pop("hueMasterStatic", None)
    if import_only is None or (
        import_only[0]["source_path"]
        != "crates/aftereffects_file/tests/fixtures/effects_coverage/hue_master_static_adobe.aep"
        or import_only[0]["source_sha256"]
        != "e720cfaa8bcebf2fd4ede1ad4116d4e267c3bc37e3a148d5f6c555c2b559be38"
    ):
        raise ExportAdapterError("hueMasterStatic import-only reference identity differs")
    expected = {case.name for case in cases if case.coverage}
    if set(by_name) != expected:
        missing = sorted(expected - set(by_name))
        extra = sorted(set(by_name) - expected)
        raise ExportAdapterError(
            f"effects coverage reference inventory differs; missing={missing}, extra={extra}"
        )
    # Vector oracles are separate Adobe-authored .aep files, never our exported
    # artifacts. Bind both their exact fixture path and composition name; source
    # SHA, composition ID and immutable video pins are validated by the manifest.
    vector_sources = {
        str(case.fixture_dir / f"{case.name}.aep"): case.name
        for case in cases if case.test_source == VECTOR_TESTS
    }
    for source, composition in validated.values():
        name = vector_sources.get(source["source_path"])
        if name is None:
            continue
        if composition.get("composition_name") != name:
            raise ExportAdapterError(f"{name}: vector reference composition name differs")
        if name in by_name:
            raise ExportAdapterError(f"{name}: ambiguous vector reference composition")
        if (
            composition["width"] != EXPECTED_WIDTH
            or composition["height"] != EXPECTED_HEIGHT
            or composition["expected_frame_count"] != EXPECTED_FRAMES
            or Fraction(composition["duration_numerator"], composition["duration_denominator"])
            != EXPECTED_DURATION
        ):
            raise ExportAdapterError(f"{name}: vector oracle differs from the export canvas/duration")
        by_name[name] = (source, composition)
    # Pending independent sources never become references merely because an Asset
    # was published. Route names and source pins are explicitly reviewed separately.
    route_entries = []
    for _, fixture_dir, module in ADDITIONAL_PANELS:
        routes = _load_object(workspace / fixture_dir / "reference-routes.json")
        if routes.get("schema_version") != 1 or not isinstance(routes.get("cases"), list):
            raise ExportAdapterError(f"{module}: native reference routes are malformed")
        route_entries.extend(routes["cases"])
    routed = {entry.get("case_id"): entry for entry in route_entries if isinstance(entry, dict)}
    non_audio_cases = {case.case_id: case for case in cases if case.control_kind == "non_audio"}
    if set(routed) != set(non_audio_cases) or len(routed) != len(route_entries):
        raise ExportAdapterError("non-audio source routes differ from registered cases")
    non_audio_sources = {}
    for case_id, case in non_audio_cases.items():
        entry = routed[case_id]
        if entry.get("composition_name") != case.name:
            raise ExportAdapterError(f"{case.name}: native reference route name differs")
        non_audio_sources[str(case.fixture_dir / "native" / f"{case_id}.aep")] = case.name
    for source, composition in validated.values():
        name = non_audio_sources.get(source["source_path"])
        if name is None:
            continue
        if composition.get("composition_name") != name or name in by_name:
            raise ExportAdapterError(f"{name}: ambiguous or mismatched independent target")
        route = routed[f"fx-export-{name}"]
        if route.get("source_sha256") is not None and route["source_sha256"] != source["source_sha256"]:
            raise ExportAdapterError(f"{name}: source hash differs from pinned native route")
        if route.get("composition_id") is not None and route["composition_id"] != composition["composition_id"]:
            raise ExportAdapterError(f"{name}: composition ID differs from pinned native route")
        if composition.get("status") != "verified" or not all(
            composition.get("verification", {}).get(key) is True
            for key in ("native_render", "full_decode")
        ) or not composition.get("verification", {}).get("visual_inspection"):
            continue  # published_uninspected is explicitly NOT a scoring oracle
        if (
            composition["width"] != EXPECTED_WIDTH
            or composition["height"] != EXPECTED_HEIGHT
            or composition["expected_frame_count"] != EXPECTED_FRAMES
            or Fraction(composition["duration_numerator"], composition["duration_denominator"])
            != EXPECTED_DURATION
        ):
            raise ExportAdapterError(f"{name}: independent target canvas/duration differs")
        by_name[name] = (source, composition)
    return by_name


def _verify_source(workspace: Path, source: dict[str, Any]) -> dict[str, Any]:
    try:
        source_path = proof.resolve_repo_path(
            aep_test.conversion_workspace(workspace), source["source_path"], must_exist=True
        )
    except proof.ProofError as exc:
        raise ExportAdapterError(f"cannot resolve independent source: {exc}") from exc
    if source_path.is_symlink() or not source_path.is_file():
        raise ExportAdapterError("independent source is not a regular non-symlink file")
    if (
        source_path.stat().st_size != source["source_bytes"]
        or aep_test.sha256_file(source_path) != source["source_sha256"]
    ):
        raise ExportAdapterError(
            "independent source bytes differ from immutable inventory"
        )
    return {
        "path": source["source_path"],
        "bytes": source["source_bytes"],
        "sha256": source["source_sha256"],
    }


def _has_adobe_diagnostic(output: str) -> bool:
    return any(
        DIAGNOSTIC_RE.search(line) and not ZERO_DIAGNOSTIC_RE.search(line)
        for line in output.splitlines()
    )


def _command_failure(
    result: aep_test.CompletedCommand, stage: str
) -> ExportAdapterError:
    diagnostic = (result.stderr.strip() or result.stdout.strip()).splitlines()
    detail = diagnostic[-1] if diagnostic else "no diagnostic output"
    return ExportAdapterError(f"{stage} exited {result.returncode}: {detail}")


def _run_native_and_score(
    *,
    workspace: Path,
    run_dir: Path,
    case_dir: Path,
    case: ExportCase,
    identity: dict[str, Any],
    reference_target: tuple[dict[str, Any], dict[str, Any]] | None,
    executor: aep_test.CommandExecutor,
    cache_dir: Path,
    local_reference: Path | None,
    tools: dict[str, Path],
    timeouts: dict[str, int],
) -> tuple[dict[str, Any], dict[str, Any], float | None, str | None, dict[str, Any]]:
    commands: list[dict[str, Any]] = []
    aep_path = run_dir / identity["artifacts"]["aep"]["path"]
    original_aep_hash = identity["native_aep_sha256"]
    output = case_dir / f"{case.name}.mp4"
    if output.exists() or output.is_symlink():
        raise ExportAdapterError("Adobe output already exists before render")
    render_error: BaseException | None = None
    native_artifact: dict[str, Any] | None = None
    native_work = case_dir / "native-worker"
    try:
        native_artifact = adobe_native.execute(
            "render_aep",
            {"source": adobe_native.source_ref(aep_path), "composition_id": case.name,
             "settings": {"format": "mp4", "fps": 30, "audio": "off"}},
            native_work, timeout=timeouts["render"],
        )
        adobe_native.copy_artifact(native_artifact, output)
    except (Exception, KeyboardInterrupt) as exc:  # noqa: BLE001
        # Even an unexpected injected-executor failure must not bypass the mutation check.
        render_error = exc
    try:
        if (
            not aep_path.is_file()
            or aep_path.is_symlink()
            or aep_test.sha256_file(aep_path) != original_aep_hash
        ):
            raise ExportAdapterError("fresh AEP changed during Adobe render")
    except OSError as exc:
        raise ExportAdapterError(
            f"cannot recheck fresh AEP after render: {exc}"
        ) from exc
    if render_error is not None:
        raise render_error
    assert native_artifact is not None
    diagnostics = adobe_native.read_render_log(native_artifact).decode('utf-8', errors='replace')
    if _has_adobe_diagnostic(diagnostics):
        raise ExportAdapterError(
            "Adobe render emitted warning/error/fatal diagnostic text"
        )
    if output.is_symlink() or not output.is_file() or output.stat().st_size <= 0:
        raise ExportAdapterError(
            "Adobe exited successfully without a regular nonempty MP4"
        )

    composition = {
        "width": EXPECTED_WIDTH,
        "height": EXPECTED_HEIGHT,
        "duration_numerator": EXPECTED_DURATION,
        "duration_denominator": 1,
        "expected_frame_count": EXPECTED_FRAMES,
    }
    try:
        actual_metadata = aep_test._metadata(
            output,
            tools["ffprobe"],
            timeouts["metadata"],
            case_dir,
            commands,
            executor,
            "adobe-output-metadata",
        )
        aep_test._verify_video_metadata(
            actual_metadata, composition, "Adobe output", aep_test.RenderFailed
        )
    except aep_test.AepTestError as exc:
        raise ExportAdapterError(str(exc)) from exc
    if actual_metadata["audio_streams"]:
        raise ExportAdapterError("Adobe output unexpectedly contains audio")
    native_phase = {
        "attempted": True,
        "status": "success",
        "reason": "fresh AEP rendered and fully decoded as 320x180 30fps 60-frame 2s video",
        "output": {
            "path": str(output.relative_to(run_dir)),
            "bytes": output.stat().st_size,
            "sha256": aep_test.sha256_file(output),
        },
        "metadata": {**actual_metadata, "fps": str(actual_metadata["fps"])},
        "aep_unchanged": True,
        "native_artifact": native_artifact,
    }

    if reference_target is None:
        reason = (
            "required independent native render reference is unavailable"
            if case.coverage
            else PANEL_SCORE_REASON
        )
        scoring = {
            "attempted": False,
            "status": "failure",
            "reason": reason,
        }
        return native_phase, scoring, None, reason, {"commands": commands}

    source, reference_composition = reference_target
    reference = reference_composition["reference"]
    extra_identity: dict[str, Any] = {
        "commands": commands,
        "independent_reference_path": reference["path"],
    }
    try:
        source_identity = _verify_source(workspace, source)
        extra_identity["independent_source"] = source_identity
        reference_path, origin = aep_test.resolve_reference(
            workspace, reference, local_reference
        )
        staged_reference = aep_test._copy_reference_input(
            reference_path, case_dir / "reference.mp4"
        )
        reference_metadata = aep_test._metadata(
            staged_reference,
            tools["ffprobe"],
            timeouts["metadata"],
            case_dir,
            commands,
            executor,
            "reference-metadata",
        )
        aep_test._verify_reference_metadata(reference_metadata, reference_composition)
        comparison_result = aep_test.run_recorded(
            [
                str(tools["validation"]),
                "video",
                "--left",
                str(staged_reference),
                "--right",
                str(output),
                "--sample-interval-secs",
                aep_test.SAMPLE_INTERVAL_CLI,
                "--max-dimension",
                str(max(EXPECTED_WIDTH, EXPECTED_HEIGHT)),
                "--max-samples",
                str(MAX_SAMPLES),
                "--json",
                "--canonical-rgb24",
            ],
            timeouts["compare"],
            "comparison",
            case_dir,
            commands,
            executor,
        )
        if comparison_result.returncode:
            raise _command_failure(comparison_result, "comparison")
        comparison = json.loads(comparison_result.stdout)
        score_detail = aep_test.evaluate_comparison(
            comparison,
            reference_composition,
            [Fraction(index, aep_test.SAMPLE_FPS) for index in range(EXPECTED_FRAMES)],
            MAX_SAMPLES,
        )
    except KeyboardInterrupt:
        raise
    except Exception as exc:  # noqa: BLE001
        # Scoring infrastructure errors are case results; later cases must still run.
        reason = f"independent RGB scoring failed: {type(exc).__name__}: {exc}"
        scoring = {"attempted": True, "status": "failure", "reason": reason}
        return native_phase, scoring, None, reason, extra_identity
    score = score_detail["half_open_native_frames"]["min_frame_similarity"]
    scoring = {
        "attempted": True,
        "status": "success",
        "reason": "descriptive canonical RGB24 comparison completed; no quality threshold is defined",
        "comparison": score_detail,
        "reference": {
            "path": reference["path"],
            "origin": origin,
            "metadata": {**reference_metadata, "fps": str(reference_metadata["fps"])},
        },
    }
    return native_phase, scoring, score, None, extra_identity


def _base_row(case: ExportCase, assertions: dict[str, Any]) -> dict[str, Any]:
    limitations = [
        "Render acceptance does not perform scripted Adobe control readback or prove editable control values.",
        "Canonical RGB24 similarity is descriptive only and does not verify alpha or audio.",
    ]
    if not case.coverage:
        limitations.append(
            "Panel cases require a separately pinned independent native-render reference; Rust property assertions and historical readback are not fresh Adobe acceptance."
        )
    if case.name == "hueSaturation-animated":
        limitations.append(
            "The independent animated Hue/Saturation reference has equivalent animation only for the three keyable colorize leaves, not the full master-control animation; fresh CPU/native-property assertions are separate evidence for all mapped controls."
        )
    return {
        "case_id": case.case_id,
        "direction": "export",
        "status": "failure",
        "score": None,
        "score_reason": "validation has not started",
        "identity": {"case_name": case.name, "test_symbol": case.test_symbol},
        "assertions": assertions,
        "native_control_evidence": {
            "attempted": False,
            "status": "failure",
            "reason": "independent native-control evidence has not been bound",
            "provenance": "unverified",
            "fresh_adobe_inspection": False,
        },
        "native_acceptance": {
            "attempted": False,
            "status": "failure",
            "reason": "native acceptance has not started",
        },
        "scoring": {
            "attempted": False,
            "status": "failure",
            "reason": "scoring has not started",
        },
        "attempted": {
            "assertions": assertions["attempted"],
            "native_control_evidence": False,
            "native_acceptance": False,
            "scoring": False,
        },
        "limitations": limitations,
    }


def _finalize(row: dict[str, Any]) -> None:
    row["attempted"] = {
        "assertions": bool(row["assertions"]["attempted"]),
        "native_control_evidence": bool(row["native_control_evidence"]["attempted"]),
        "native_acceptance": bool(row["native_acceptance"]["attempted"]),
        "scoring": bool(row["scoring"]["attempted"]),
    }
    phases = (
        row["assertions"],
        row["native_control_evidence"],
        row["native_acceptance"],
        row["scoring"],
    )
    successful = all(phase["status"] == "success" for phase in phases)
    row["status"] = "success" if successful else "failure"
    if successful:
        row.pop("failure_reason", None)
    else:
        row["failure_reason"] = "; ".join(
            phase["reason"] for phase in phases if phase["status"] != "success"
        )


def _failure_phase(reason: str, attempted: bool = False) -> dict[str, Any]:
    return {"attempted": attempted, "status": "failure", "reason": reason}


def run_exports(
    *,
    workspace: Path,
    run_dir: Path,
    records_path: Path,
    executor: aep_test.CommandExecutor = aep_test.default_executor,
    cache_dir: Path,
    local_references: dict[str, Path],
    tools: dict[str, Path | str],
    timeouts: dict[str, int],
    on_row: RowCallback | None = None,
    cancelled: CancelCheck | None = None,
    selected_case_ids: list[str] | None = None,
) -> list[dict[str, Any]]:
    """Validate declared exports, or an explicit bounded subset, with one row each."""
    workspace = workspace.resolve()
    if run_dir.is_symlink():
        raise ExportAdapterError("run_dir must not be a symlink")
    if records_path.is_symlink():
        raise ExportAdapterError("records_path must not be a symlink")
    if cache_dir.is_symlink():
        raise ExportAdapterError("cache_dir must not be a symlink")
    run_dir = run_dir.resolve()
    records_path = records_path.resolve()
    cache_dir = cache_dir.resolve()
    aep_test._reject_fixture_output_path(workspace, run_dir, "export run directory")
    aep_test._reject_fixture_output_path(workspace, cache_dir, "reference cache")
    if not run_dir.is_dir():
        raise ExportAdapterError(
            "run_dir must be an existing regular scratch directory"
        )
    artifact_dir = run_dir / "fx_exports"
    if not artifact_dir.is_dir() or artifact_dir.is_symlink():
        raise ExportAdapterError(
            "run_dir/fx_exports must be the fresh CPU artifact directory"
        )
    if artifact_dir.resolve().parent != run_dir:
        raise ExportAdapterError("FX artifacts escaped run_dir")
    required_tools = {"ffprobe", "validation"}
    if not required_tools <= set(tools) or set(tools) - required_tools - {"aerender"}:
        raise ExportAdapterError(f"tools must contain {sorted(required_tools)}; aerender is retired")
    if set(timeouts) != {"render", "metadata", "compare"} or any(
        not isinstance(value, int) or isinstance(value, bool) or value <= 0
        for value in timeouts.values()
    ):
        raise ExportAdapterError(
            "timeouts must contain positive integer render/metadata/compare values"
        )

    cases = load_cases(workspace)
    known_ids = {case.case_id for case in cases}
    if selected_case_ids is not None:
        if not selected_case_ids or len(selected_case_ids) != len(set(selected_case_ids)):
            raise ExportAdapterError("export selection requires distinct case IDs")
        unknown_selection = set(selected_case_ids) - known_ids
        if unknown_selection:
            raise ExportAdapterError(f"unknown selected export case IDs: {sorted(unknown_selection)}")
    selected_ids = set(selected_case_ids) if selected_case_ids is not None else known_ids
    unknown_references = set(local_references) - known_ids
    if unknown_references:
        raise ExportAdapterError(
            f"local reference map contains unknown case IDs: {sorted(unknown_references)}"
        )
    try:
        parsed, batch_errors = parse_records(records_path, cases)
    except ExportAdapterError as exc:
        reason = f"CPU record batch is unavailable: {exc}"
        parsed = {
            case.case_id: ParsedRecord(
                {
                    "attempted": False,
                    "status": "failure",
                    "reason": reason,
                    "record": None,
                },
                False,
                False,
            )
            for case in cases
        }
        batch_errors = [reason]
    control_inventory, control_inventory_problem = _control_evidence_inventory(
        workspace, cases
    )
    try:
        reference_targets = _reference_targets(workspace, cases)
        reference_inventory_problem = None
    except ExportAdapterError as exc:
        reference_targets = {}
        reference_inventory_problem = str(exc)
    resolved_tools: dict[str, Path] = {}
    tool_identities: dict[str, dict[str, Any]] = {}
    tool_problems: list[str] = []
    for name, value in tools.items():
        if name == "aerender":
            # Legacy dictionaries may still carry this value; never resolve or execute it.
            continue
        try:
            resolved = aep_test._tool_path(value, workspace)
            resolved_tools[name] = resolved
            tool_identities[name] = aep_test.tool_identity(resolved)
        except (aep_test.AepTestError, OSError) as exc:
            tool_problems.append(f"{name}: {exc}")
    tool_problem = "; ".join(tool_problems) if tool_problems else None

    adapter_path = Path(__file__).resolve()
    try:
        adapter_identity = aep_test.tool_identity(adapter_path)
    except OSError as exc:
        raise ExportAdapterError(f"cannot identify export adapter code: {exc}") from exc
    control_evidence_identities: dict[str, dict[str, Any]] = {}
    for kind, relative_path in (
        ("coverage", COVERAGE_CONTROL_EVIDENCE),
        ("panel", PANEL_CONTROL_EVIDENCE),
        ("vector", VECTOR_CONTROL_EVIDENCE),
        ("non_audio", NON_AUDIO_CONTROL_EVIDENCE),
    ):
        evidence_path = workspace / relative_path
        try:
            control_evidence_identities[kind] = aep_test.tool_identity(
                evidence_path.resolve(strict=True)
            )
        except OSError as exc:
            control_evidence_identities[kind] = {
                "path": str(evidence_path),
                "error": str(exc),
            }

    cases_root = run_dir / "cases"
    cases_root.mkdir(exist_ok=False)
    rows: list[dict[str, Any]] = []
    cancellation_latched = False
    for case in cases:
        if case.case_id not in selected_ids:
            continue
        assertion = parsed[case.case_id]
        row = _base_row(case, dict(assertion.phase))
        row["identity"].update(
            {
                "adapter_source": adapter_identity,
                "tool_identity": tool_identities,
                "native_control_evidence_file": control_evidence_identities[
                    case.control_kind
                ],
            }
        )
        record = assertion.phase.get("record")
        if (
            isinstance(record, dict)
            and _test_binary_error(record.get("test_binary")) is None
        ):
            row["identity"]["test_binary"] = dict(record["test_binary"])
        case_dir = cases_root / case.case_id
        case_dir.mkdir()
        if cancellation_latched or (cancelled is not None and cancelled()):
            cancellation_latched = True
            row["cancelled"] = True
            reason = "run cancelled before this export case started"
            row["native_acceptance"] = _failure_phase(reason)
            row["scoring"] = _failure_phase(reason)
            row["score_reason"] = reason
        elif batch_errors:
            reason = "CPU record batch identity failed: " + "; ".join(batch_errors)
            row["assertions"] = _failure_phase(reason, row["assertions"]["attempted"])
            row["native_acceptance"] = _failure_phase(
                "native acceptance blocked by CPU batch identity failure"
            )
            row["scoring"] = _failure_phase(
                "scoring blocked by CPU batch identity failure"
            )
            row["score_reason"] = row["scoring"]["reason"]
        elif not assertion.valid:
            reason = "native acceptance blocked by invalid or missing fresh CPU record binding"
            row["native_acceptance"] = _failure_phase(reason)
            row["scoring"] = _failure_phase(
                "scoring blocked by invalid CPU record binding"
            )
            row["score_reason"] = row["scoring"]["reason"]
        else:
            native_attempted = False
            try:
                record = row["assertions"]["record"]
                identity = _artifact_identity(
                    workspace, run_dir, records_path, artifact_dir, case, record
                )
                reference_target = reference_targets.get(case.name)
                if reference_target is None:
                    identity["independent_reference_path"] = None
                else:
                    source, composition = reference_target
                    identity["independent_source_declared"] = {
                        "path": source["source_path"],
                        "bytes": source["source_bytes"],
                        "sha256": source["source_sha256"],
                    }
                    identity["independent_reference_path"] = composition["reference"][
                        "path"
                    ]
                row["identity"].update(identity)
                row["native_control_evidence"] = _native_control_evidence(
                    case,
                    identity,
                    control_inventory.get(case.name),
                    control_inventory_problem,
                )
                if tool_problem is not None:
                    raise ExportAdapterError(tool_problem)
                native_attempted = True
                native, scoring, score, score_reason, extra = _run_native_and_score(
                    workspace=workspace,
                    run_dir=run_dir,
                    case_dir=case_dir,
                    case=case,
                    identity=identity,
                    reference_target=reference_target,
                    executor=executor,
                    cache_dir=cache_dir,
                    local_reference=local_references.get(case.case_id),
                    tools=resolved_tools,
                    timeouts=timeouts,
                )
                row["native_acceptance"] = native
                row["scoring"] = scoring
                row["score"] = score
                row["score_reason"] = score_reason
                row["identity"].update(extra)
                if case.coverage and reference_inventory_problem is not None:
                    row["scoring"]["reason"] += f": {reference_inventory_problem}"
                    row["score_reason"] = row["scoring"]["reason"]
            except KeyboardInterrupt:
                cancellation_latched = True
                row["cancelled"] = True
                reason = "cancelled during export validation"
                row["native_acceptance"] = _failure_phase(reason, native_attempted)
                row["scoring"] = _failure_phase(reason)
                row["score_reason"] = reason
            except (
                ExportAdapterError,
                aep_test.AepTestError,
                subprocess.TimeoutExpired,
                OSError,
            ) as exc:
                reason = f"{type(exc).__name__}: {exc}"
                row["native_acceptance"] = _failure_phase(reason, native_attempted)
                row["scoring"] = _failure_phase(
                    "scoring unavailable because native acceptance or scoring infrastructure failed"
                )
                row["score_reason"] = reason
            except Exception as exc:  # noqa: BLE001
                # One unexpected case failure must not hide the remaining fixed population.
                reason = (
                    f"unhandled export validation failure: {type(exc).__name__}: {exc}"
                )
                row["native_acceptance"] = _failure_phase(reason, native_attempted)
                row["scoring"] = _failure_phase(
                    "scoring unavailable after unhandled case failure"
                )
                row["score_reason"] = reason
        _finalize(row)
        aep_test.atomic_json(case_dir / "result.json", row)
        rows.append(row)
        if on_row is not None:
            on_row(row)
    return rows
