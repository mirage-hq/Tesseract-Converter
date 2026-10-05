"""Audio case inventory and fail-closed adapter for the unified Adobe runner.

Audio is scored as audio, never as RGB. The E2E runner's own-reader export
roundtrip is supplementary; a separate fresh Adobe render of its exported AEP
is required before an export row can succeed.
"""

from __future__ import annotations

import json
from pathlib import Path
import re
from types import SimpleNamespace
from typing import Any, Callable

import aep_audio_e2e as e2e
import adobe_native
from aep_audio_test import compare, sha256

FIXTURES = Path(__file__).resolve().parents[1] / "crates/aftereffects_file/tests/fixtures/audio_e2e"
MANIFEST = FIXTURES / "cases.json"
PROVENANCE = FIXTURES / "provenance.json"
SOURCE_SHA256 = "799aa17ad253c73edc6c3e9ac8e2f9b1c7cd8bfa9794cdfcd917d4e42c28a72c"


class AudioAdapterError(ValueError):
    """Invalid inventory, binding, or native acceptance evidence."""


def case_id(slug: str, direction: str) -> str:
    return f"aep-audio-{direction}-{slug}"


def load_inventory(manifest: Path = MANIFEST,
                   provenance: Path = PROVENANCE) -> dict[str, dict[str, Any]]:
    """Register each source composition/direction with its committed oracle.

    This reads metadata only; it does not inspect media or run cases.
    """
    data = json.loads(manifest.read_text())
    origin = json.loads(provenance.read_text())
    if data.get("version") != 1 or not isinstance(data.get("cases"), list):
        raise AudioAdapterError("unknown audio inventory version")
    if origin.get("source", {}).get("sha256") != SOURCE_SHA256:
        raise AudioAdapterError("audio native source provenance changed")
    links = {entry["composition_id"]: entry for entry in origin["supporting_targets"]}
    edited_inputs = {entry["case"]: entry["sha256"] for entry in origin["edited_fx_inputs"]}
    if len(links) != 8 or any(not isinstance(link.get("consuming_cases"), list)
                              or not link["consuming_cases"] for link in links.values()):
        raise AudioAdapterError("eight linked supporting audio compositions required")
    inventory: dict[str, dict[str, Any]] = {}
    slugs: set[str] = set()
    for item in data["cases"]:
        slug = item["id"]
        if not isinstance(slug, str) or not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", slug) or slug in slugs:
            raise AudioAdapterError("duplicate or invalid audio case slug")
        slugs.add(slug)
        source, reference = item["source"], item["reference"]
        expected_reference = f"tests/references/aep/audio/{slug}.mp4"
        if (source["sha256"] != SOURCE_SHA256 or source["path"] != "audio_cases.aep"
                or item["primary"] != origin["primary_media"]
                or reference != {"path": expected_reference}):
            raise AudioAdapterError(f"{slug}: native source/reference provenance does not match local inventory")
        directions = item["directions"]
        if (not isinstance(directions, list) or not directions
                or len(directions) != len(set(directions))
                or not set(directions) <= {"import", "export"}):
            raise AudioAdapterError(f"{slug}: invalid directions")
        if "export" in directions and item["fx_input"]["document_sha256"] != edited_inputs.get(slug):
            raise AudioAdapterError(f"{slug}: edited FX input provenance differs")
        for direction in directions:
            identity = case_id(slug, direction)
            inventory[identity] = {
                "case_id": identity, "slug": slug, "direction": direction,
                "source": source, "primary": item["primary"],
                "reference": reference, "fx_input": item["fx_input"] if direction == "export" else None,
                "native_expected": item["native_expected"][direction],
                "expected_diagnostics": item["expected_diagnostics"][direction],
                "reference_expectation": item["reference_expectation"],
                "policy": data["policy"], "execution": "UNRUN", "measurement": "unmeasured",
            }
    if len(slugs) != 28 or sum(c["direction"] == "import" for c in inventory.values()) != 19 or sum(c["direction"] == "export" for c in inventory.values()) != 25:
        raise AudioAdapterError("audio inventory must contain 28 sources / 19 imports / 25 exports")
    if any(not set(link["consuming_cases"]) <= slugs for link in links.values()):
        raise AudioAdapterError("unlinked supporting composition")
    return inventory


def _phase(attempted: bool, status: str, reason: str | None = None) -> dict[str, Any]:
    return {"attempted": attempted, "status": status, "reason": reason}


def _native_dependencies(case: dict[str, Any], work: Path) -> dict[str, Any]:
    """Pin original media and exact paths of verified task-owned media copies."""
    dependencies: dict[str, Any] = {}
    hashes: set[str] = set()
    for index, item in enumerate(case["primary"]):
        path = e2e._path(FIXTURES, item, "primary media")
        digest = sha256(path)
        hashes.add(digest)
        dependencies[f"primary-{index}"] = {"path": str(path.resolve()), "sha256": digest}
    media_suffixes = {".wav", ".mov", ".mp4", ".mp3", ".aif", ".aiff", ".m4a"}
    for directory in ("independent-source", "prepared", "conversion"):
        root = work / "export" / directory
        if root.is_symlink():
            raise AudioAdapterError("symlinked native media directory")
        if not root.exists():
            continue
        pending = [root]
        count = 0
        while pending:
            folder = pending.pop()
            for path in sorted(folder.iterdir()):
                count += 1
                if count > 1000:
                    raise AudioAdapterError("native media scan exceeds owned-directory bound")
                if path.is_symlink():
                    raise AudioAdapterError("symlink in native media directory")
                if path.is_dir():
                    pending.append(path)
                elif path.suffix.lower() in media_suffixes:
                    digest = sha256(path)
                    if digest not in hashes:
                        raise AudioAdapterError("changed or unexpected native media copy")
                    dependencies[f"copy-{len(dependencies)}"] = {
                        "path": str(path.resolve()), "sha256": digest}
    return dependencies


def _native_acceptance(*, case: dict[str, Any], work: Path,
                       timeout: int, aerender: Path | str | None = None) -> dict[str, Any]:
    """Render the *fresh exported AEP* in Adobe and compare its audio to native source.

    A successful own-reader inspection, local roundtrip or FX render cannot
    substitute for this phase. A fresh native render never replaces the oracle.
    """
    exported = work / "export/conversion/project.aep"
    if exported.is_symlink() or not exported.is_file():
        raise AudioAdapterError("fresh exported AEP missing for native acceptance")
    original = sha256(exported)
    output = work / "export/adobe-native.mp4"
    if output.exists() or output.is_symlink():
        raise AudioAdapterError("native output already exists")
    name = case["slug"]  # pinned explicit FX root name, not source composition ID
    if not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", name):
        raise AudioAdapterError("invalid exported composition name")
    error: BaseException | None = None
    diagnostics = ""
    native_artifact = None
    native_work = work / "export/native-worker"
    try:
        native_artifact = adobe_native.execute(
            "render_aep", {"source": adobe_native.source_ref(exported, _native_dependencies(case, work)),
                           "composition_id": name,
                           "settings": {"format": "mp4", "fps": 30, "start_frame": 0,
                                        "end_frame": 143, "audio": "on"}},
            native_work, timeout=timeout)
        adobe_native.copy_artifact(native_artifact, output)
        diagnostics = adobe_native.read_render_log(native_artifact).decode('utf-8', errors='replace')
    except BaseException as exc:
        error = exc
    if not exported.is_file() or exported.is_symlink() or sha256(exported) != original:
        raise AudioAdapterError("fresh exported AEP changed during native acceptance")
    if error is not None:
        raise error
    if re.search(r"\b(error|fatal|warning)\b", diagnostics, re.I):
        raise AudioAdapterError("Adobe native render emitted error/warning diagnostics")
    if not output.is_file() or output.is_symlink():
        raise AudioAdapterError("Adobe did not produce a regular native render")
    native_contract = e2e._video_contract(output)
    reference = e2e._acquire_reference(FIXTURES, case["reference"], work)
    e2e._video_contract(reference)
    policy = case["policy"]
    e2e._reference_windows(reference, case["reference_expectation"], policy)
    result = compare(output, reference, policy)
    return {"attempted": True, "status": "success" if result["passed"] else "failure",
            "reason": None if result["passed"] else "Adobe native audio differs from independent source reference",
            "aep_sha256": original, "output": native_contract,
            "native_artifact": native_artifact,
            "reference_path": case["reference"]["path"], "audio_comparison": result,
            "evidence": "fresh generated-AEP Adobe render versus independent native source audio"}


def run_cases(*, selected: list[dict[str, Any]], run_dir: Path,
              tools: dict[str, Path | str], timeout: int,
              on_row: Callable[[dict[str, Any]], None], cancelled: Callable[[], bool],
              e2e_runner: Callable[..., dict[str, Any]] = e2e.run,
              native_acceptance: Callable[..., dict[str, Any]] = _native_acceptance) -> list[dict[str, Any]]:
    """Run separate case/direction workdirs, preserving partial stage reports."""
    rows: list[dict[str, Any]] = []
    for case in selected:
        work = run_dir / "audio" / case["case_id"]
        identity = {key: case[key] for key in ("case_id", "slug", "direction", "source", "primary", "reference", "fx_input", "native_expected", "expected_diagnostics", "reference_expectation")}
        row = {"case_id": case["case_id"], "direction": case["direction"],
               "identity": identity, "status": "failure", "score": None,
               "score_reason": "audio execution has not started", "audio_metric": "pinned_stereo_pcm_audio_policy",
               "assertions": _phase(False, "failure", "not started"),
               "scoring": _phase(False, "failure", "not started"),
               "native_acceptance": _phase(False, "failure", "not started") if case["direction"] == "export" else _phase(False, "not_applicable"),
               "execution": "UNRUN", "measurement": "unmeasured"}
        if cancelled():
            row["cancelled"] = True
            row["score_reason"] = "cancelled before audio execution"
            row["assertions"]["reason"] = row["score_reason"]
            row["scoring"]["reason"] = row["score_reason"]
            rows.append(row)
            on_row(row)
            continue
        try:
            args = SimpleNamespace(manifest=MANIFEST, case=case["slug"], direction=case["direction"],
                                   work=work, converter=str(tools["tesseract_conv"]),
                                   tsrct=str(tools["tsrct"]), preparer=str(tools["audio_preparer"]),
                                   timeout=timeout)
            runner_error = None
            try:
                e2e_runner(args)
            except Exception as exc:
                runner_error = f"{type(exc).__name__}: {exc}"
            result_path = work / "result.json"
            result = json.loads(result_path.read_text()) if result_path.is_file() else {}
            if result.get("case_id") != case["slug"]:
                raise AudioAdapterError("E2E result is missing or belongs to a different case")
            row["execution"] = "attempted"
            row["e2e_result_path"] = str(result_path.relative_to(run_dir))
            row["e2e_result_sha256"] = sha256(result_path)
            stages = result.get("directions", {}).get(case["direction"], {})
            if not isinstance(stages, dict):
                raise AudioAdapterError("missing direction-specific E2E evidence")
            assertion_stage = "editable_import_assertions" if case["direction"] == "import" else "own_reader_export_assertions"
            row["assertions"] = _phase(assertion_stage in stages, "success" if stages.get(assertion_stage, {}).get("status") == "passed" else "failure", stages.get("failure") or runner_error)
            measured = stages.get("comparison", {})
            if measured.get("status") in ("passed", "failed_score") and measured.get("reference_path") != case["reference"]["path"]:
                raise AudioAdapterError("E2E comparison is not bound to the independent native reference")
            row["scoring"] = _phase("comparison" in stages, "success" if measured.get("status") == "passed" else "failure", stages.get("failure") or runner_error)
            if measured.get("status") in ("passed", "failed_score"):
                row["measurement"] = "measured"
                row["audio_comparison"] = measured.get("result")
            if case["direction"] == "export" and stages.get("fresh_export", {}).get("status") == "passed":
                try:
                    row["native_acceptance"] = native_acceptance(case=case, work=work,
                        timeout=timeout)
                    if row["native_acceptance"].get("status") != "success":
                        row["native_acceptance"]["status"] = "failure"
                except Exception as exc:
                    row["native_acceptance"] = _phase(True, "failure", f"Adobe native acceptance failed: {type(exc).__name__}: {exc}")
            if (runner_error is None and result.get("status") == "scored_local_reference"
                    and row["assertions"]["status"] == "success"
                    and row["scoring"]["status"] == "success"
                    and (case["direction"] == "import" or row["native_acceptance"]["status"] == "success")):
                row["status"] = "success"
            row["score_reason"] = (None if row["status"] == "success" else runner_error or stages.get("failure")
                                   or row["native_acceptance"].get("reason") or "audio evidence incomplete")
        except KeyboardInterrupt:
            row["cancelled"] = True
            row["score_reason"] = "cancelled during audio execution"
        except Exception as exc:
            row["score_reason"] = f"audio adapter failure: {type(exc).__name__}: {exc}"
        rows.append(row)
        on_row(row)
    return rows
