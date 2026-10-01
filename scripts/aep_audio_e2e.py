"""Offline AEP audio checks against committed Adobe source references.

Reference media is loaded from repository-relative paths. Export inspection uses
our own native-file reader, NOT Adobe acceptance or native-render evidence.
"""

from __future__ import annotations

import json
import math
import os
import re
from fractions import Fraction
import signal
import subprocess
import time
from pathlib import Path

from aep_audio_test import AudioTestError, _run as probe_run, compare, decode, require_hash, sha256, validate_policy

CONV_ROOT = Path(__file__).resolve().parents[1]
REFERENCE_PREFIX = Path("tests/references/aep/audio")


def _reference_pin(root: Path, reference: dict | None) -> Path:
    if not isinstance(reference, dict) or set(reference) != {"path"}:
        raise AudioTestError("reference requires one committed repository-relative path")
    path = reference["path"]
    relative = Path(path) if isinstance(path, str) else Path()
    if (not path or relative.is_absolute() or ".." in relative.parts
            or relative.parent != REFERENCE_PREFIX or relative.suffix != ".mp4"):
        raise AudioTestError("reference path must be a contained AEP audio MP4")
    repository = CONV_ROOT if root.resolve().is_relative_to(CONV_ROOT) else root.resolve()
    candidate = repository / relative
    local = candidate.resolve()
    if candidate.is_symlink() or not local.is_relative_to(repository) or not local.is_file():
        raise AudioTestError("missing committed independent Adobe reference")
    return local


def _acquire_reference(root: Path, reference: dict, _work: Path) -> Path:
    return _reference_pin(root, reference)

POLICY_FIELDS = {"sample_rate", "channels", "duration_seconds", "duration_tolerance_seconds",
                 "relative_rms_error_max", "window_rms_error_max", "silence_reference_rms_max",
                 "silence_leak_rms_max"}


def _path(root: Path, item: dict, label: str) -> Path:
    if not isinstance(item, dict) or not {"path", "sha256"} <= set(item) or not isinstance(item["path"], str):
        raise AudioTestError(f"{label}: expected path and sha256")
    relative = Path(item["path"])
    if not item["path"] or relative.is_absolute() or ".." in relative.parts:
        raise AudioTestError(f"{label}: path must stay inside the fixture directory")
    candidate = root / relative
    path = candidate.resolve()
    if candidate.is_symlink() or not path.is_relative_to(root.resolve()) or not path.is_file():
        raise AudioTestError(f"{label}: missing or unsafe fixture input")
    require_hash(path, item["sha256"], label)
    return path


def load_case(manifest_path: Path, case_id: str, direction: str) -> tuple[dict, dict, Path]:
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("version") != 1 or set(manifest) != {"version", "policy", "cases"}:
        raise AudioTestError("unknown E2E manifest version/fields")
    validate_policy(manifest["policy"])
    if (manifest["policy"]["sample_rate"], manifest["policy"]["channels"],
            manifest["policy"]["duration_seconds"]) != (48000, 2, 6):
        raise AudioTestError("E2E policy must be 48kHz stereo, 6s")
    if not isinstance(manifest["cases"], list) or len({c["id"] for c in manifest["cases"]}) != len(manifest["cases"]):
        raise AudioTestError("invalid or duplicate E2E cases")
    found = [c for c in manifest["cases"] if c["id"] == case_id]
    if len(found) != 1 or not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", case_id):
        raise AudioTestError("unknown/invalid case slug")
    case = found[0]
    if set(case) != {"id", "directions", "source", "primary", "fx_input", "reference", "expected_diagnostics", "native_expected", "reference_expectation"}:
        raise AudioTestError("case fields missing or unknown")
    if (direction != "both" and direction not in case["directions"]) or not set(case["directions"]) <= {"import", "export"}:
        raise AudioTestError("direction not applicable to this case")
    source = case["source"]
    if set(source) != {"path", "sha256", "composition_id", "composition_name"} or not (
            isinstance(source["composition_id"], int) and not isinstance(source["composition_id"], bool)
            and source["composition_id"] > 0 and isinstance(source["composition_name"], str)
            and source["composition_name"]):
        raise AudioTestError("native source must pin composition ID and name")
    if not isinstance(case["primary"], list) or not case["primary"]:
        raise AudioTestError("primary media must be pinned")
    diagnostics = case["expected_diagnostics"]
    if set(diagnostics) != {"import", "export"} or any(
            not isinstance(diagnostics[d], list) or any(not isinstance(x, str) or not x for x in diagnostics[d])
            for d in ("import", "export")):
        raise AudioTestError("expected diagnostics must be direction-specific string lists")
    expectations = case["native_expected"]
    if set(expectations) != {"import", "export"} or any(
            not isinstance(expectations[d], list) or not expectations[d]
            or any(set(item) != {"layer", "field", "value"} for item in expectations[d])
            for d in case["directions"]):
        raise AudioTestError("native expectations must be feature-specific in each direction")
    _validate_reference_expectation(case["reference_expectation"])
    if "export" in case["directions"]:
        fx = case["fx_input"]
        if not isinstance(fx, dict) or not set(fx) <= {"document_sha256", "path", "sha256"} or "document_sha256" not in fx:
            raise AudioTestError("explicit FX input requires pinned document JSON SHA-256")
        if ("path" in fx) != ("sha256" in fx):
            raise AudioTestError("explicit FX archive path and hash must be paired")
    return manifest["policy"], case, manifest_path.parent.resolve()


def _run(command: list[str], log: Path, timeout: int) -> str:
    if timeout < 1 or timeout > 3600:
        raise AudioTestError("command timeout outside 1..3600 seconds")
    if log.exists():
        raise AudioTestError(f"refusing to overwrite command log: {log}")
    with log.open("xb") as stream:
        stream.write((json.dumps({"argv": command, "started_epoch": time.time()}) + "\n").encode())
        stream.flush()
        process = subprocess.Popen(command, stdout=stream, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=timeout)
        except BaseException:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()
            raise
        stream.write(("\n" + json.dumps({"returncode": code, "ended_epoch": time.time()}) + "\n").encode())
    text = log.read_text(errors="replace")
    if code:
        raise AudioTestError(f"command failed (exit {code}); see {log}")
    return text


def _validate_reference_expectation(expectation: dict) -> None:
    if not isinstance(expectation, dict) or set(expectation) != {"audible_windows", "silent_windows", "minimum_rms", "maximum_silent_rms"}:
        raise AudioTestError("reference expectation must declare audible/silent windows and RMS limits")
    minimum, maximum = expectation["minimum_rms"], expectation["maximum_silent_rms"]
    if any(not isinstance(v, (int, float)) or isinstance(v, bool) or not math.isfinite(v) or v < 0 or v > 1
           for v in (minimum, maximum)) or minimum <= maximum:
        raise AudioTestError("invalid reference RMS limits")
    if not isinstance(expectation["audible_windows"], list) or not isinstance(expectation["silent_windows"], list) or not (expectation["audible_windows"] or expectation["silent_windows"]):
        raise AudioTestError("reference needs at least one declared window")
    for windows in (expectation["audible_windows"], expectation["silent_windows"]):
        for window in windows:
            if (not isinstance(window, list) or len(window) != 2 or any(
                    not isinstance(t, (int, float)) or isinstance(t, bool) or not math.isfinite(t) for t in window)
                    or not 0 <= window[0] < window[1] <= 6):
                raise AudioTestError("invalid reference window (must lie within six seconds)")


def _reference_windows(reference: Path, expectation: dict, policy: dict) -> dict:
    rate, channels, samples = decode(reference)
    frames = len(samples) // channels
    if (rate, channels) != (policy["sample_rate"], policy["channels"]) or abs(frames / rate - 6) > policy["duration_tolerance_seconds"]:
        raise AudioTestError("reference PCM format/duration differs from policy")
    results = []
    for kind in ("audible", "silent"):
        for start, end in expectation[kind + "_windows"]:
            first, last = round(start * rate), round(end * rate)
            if first >= last or last > frames:
                raise AudioTestError("reference window empty or exceeds decoded audio")
            rms = [math.sqrt(sum(samples[frame * channels + channel] ** 2
                                 for frame in range(first, last)) / (last - first))
                   for channel in range(channels)]
            limit = expectation["minimum_rms"] if kind == "audible" else expectation["maximum_silent_rms"]
            results.append({"kind": kind, "window": [start, end], "rms_by_channel": rms,
                            "limit": limit, "passed": all(v >= limit for v in rms) if kind == "audible"
                            else all(v <= limit for v in rms)})
    if not all(result["passed"] for result in results):
        raise AudioTestError(f"independent reference violates feature-specific audible/silent windows: {results}")
    return {"passed": True, "windows": results}


def _video_contract(path: Path) -> dict:
    """Reject an audio-correct but wrong-video artifact before scoring."""
    try:
        metadata = json.loads(probe_run(["ffprobe", "-v", "error", "-count_frames", "-show_streams",
                                         "-of", "json", str(path)]))
        videos = [s for s in metadata.get("streams", []) if s.get("codec_type") == "video"]
        if len(videos) != 1:
            raise AudioTestError("expected exactly one video stream")
        video = videos[0]
        checks = (video.get("codec_name") == "h264", video.get("width") == 320,
                  video.get("height") == 180, Fraction(video.get("avg_frame_rate", "0/1")) == 30,
                  Fraction(video.get("r_frame_rate", "0/1")) == 30,
                  int(video.get("nb_read_frames", -1)) == 180,
                  abs(Fraction(video["duration"]) - 6) <= Fraction(1, 1000))
        if not all(checks):
            raise AudioTestError("video contract requires H.264 320x180 30fps 180 frames/6s")
    except (KeyError, ValueError, ZeroDivisionError, TypeError) as exc:
        raise AudioTestError(f"invalid video stream metadata: {exc}") from exc
    return {"codec": "h264", "width": 320, "height": 180, "fps": 30, "frames": 180,
            "duration": 6, "sha256": sha256(path)}


def _assert_inspection(path: Path, case: dict, direction: str) -> dict:
    data = json.loads(path.read_text())
    if (data.get("case_id") != case["id"] or data.get("assertion") != "passed"
            or not isinstance(data.get("audio"), list) or not data["audio"]
            or not isinstance(data.get("audio_count"), int) or data["audio_count"] < 1):
        raise AudioTestError("Rust fresh-import editable assertions missing or failed")
    return data


def _assert_export_inspection(path: Path, case: dict) -> dict:
    data = json.loads(path.read_text())
    if data.get("case_id") != case["id"] or data.get("assertion") != "passed":
        raise AudioTestError("own-reader exported AEP editable assertions missing or failed")
    return data


def execute(args, direction: str, work: Path, case: dict, root: Path, policy: dict, report: dict) -> None:
    stage = work / direction
    stage.mkdir()  # never reuse stale conversions, files or logs
    stages = report["directions"][direction] = {}

    def step(name, fn):
        stages[name] = {"status": "running"}
        save_report(work, report)
        try:
            value = fn()
            stages[name] = {"status": "passed"}
            save_report(work, report)
            return value
        except BaseException as exc:
            stages[name] = {"status": "blocked", "error": str(exc)}
            save_report(work, report)
            raise

    source = _path(root, case["source"], "independent native source")
    primary = [_path(root, item, "primary media") for item in case["primary"]]
    fx_pin = case["fx_input"] if direction == "export" else None
    fx_input = _path(root, fx_pin, "explicit edited FX input") if fx_pin and "path" in fx_pin else None
    stages["provenance"] = {"status": "recorded", "source_sha256": sha256(source),
                            "primary_sha256": {str(p): sha256(p) for p in primary},
                            "reference_path": case["reference"]["path"],
                            "evidence": "own-reader/roundtrip; not Adobe acceptance of exported AEP",
                            "tool_sha256": {key: sha256(Path(getattr(args, key))) for key in
                                            ("preparer", "converter", "tsrct")}}
    if fx_pin:
        stages["provenance"]["fx_input_pin"] = fx_pin
    save_report(work, report)
    expected = case["expected_diagnostics"][direction]
    reference = step("pinned_reference", lambda: _acquire_reference(root, case["reference"], work))
    stages["pinned_reference"].update({"path": str(reference)})
    # Relink is a scratch-only native source copy, not a replacement oracle.
    scratch = stage / "independent-source"
    scratch.mkdir()
    (scratch / "audio_cases.aep").write_bytes(source.read_bytes())
    if sha256(scratch / "audio_cases.aep") != sha256(source):
        raise AudioTestError("scratch native source copy hash mismatch")
    for item in primary:
        target = scratch / item.name
        if target.exists():
            raise AudioTestError("primary media filenames collide in scratch")
        target.write_bytes(item.read_bytes())
    scratch_source = scratch / "source.aep"
    step("scratch_relink", lambda: _run(
        [args.preparer, "stage-source", str(scratch), str(scratch_source)],
        stage / "scratch-relink.log", args.timeout))
    if not scratch_source.is_file() or scratch_source.is_symlink():
        raise AudioTestError("relink did not produce a regular source.aep")
    for item in primary:
        copied = scratch / item.name
        if sha256(copied) != sha256(item):
            raise AudioTestError("scratch primary media differs from pinned source")
    stages["scratch_relink"]["scratch_source_sha256"] = sha256(scratch_source)
    fx_actual = None
    if direction == "import":
        conversion = stage / "conversion"
        output = step("fresh_import", lambda: _run(
            [args.converter, "convert", str(scratch_source), "--to", "tesseract", "--composition",
             str(case["source"]["composition_id"]), "--output", str(conversion)],
            stage / "import.log", args.timeout))
        document = conversion / "project.tsrct"
        if not document.is_file():
            raise AudioTestError("fresh import did not produce project.tsrct")
        inspected = stage / "import-inspection.json"
        step("editable_import", lambda: _run(
            [args.preparer, "inspect-import", case["id"], str(document), str(inspected)],
            stage / "inspect-import.log", args.timeout))
        step("editable_import_assertions", lambda: _assert_inspection(inspected, case, direction))
        stages["editable_import"]["inspection_sha256"] = sha256(inspected)
        actual = stage / "actual.mp4"
        step("tsrct_audio_render", lambda: _run(
            [args.tsrct, "export", "--project", str(document), "--output", str(actual), "--fps", "30"],
            stage / "tsrct-render.log", args.timeout))
        stages["fresh_import"]["document_sha256"] = sha256(document)
    else:
        prepared = stage / "prepared"
        step("prepare_explicit_fx", lambda: _run(
            [args.preparer, "prepare", str(root), case["id"], str(prepared)],
            stage / "prepare.log", args.timeout))
        document = prepared / "project.tsrct"
        if not document.is_file() or not (prepared / "document.json").is_file():
            raise AudioTestError("preparer did not produce project.tsrct and document.json")
        require_hash(prepared / "document.json", case["fx_input"]["document_sha256"], "prepared explicit FX document")
        if fx_input is not None and sha256(document) != sha256(fx_input):
            raise AudioTestError("prepared FX input differs from pinned edited FX input")
        # Native-reference agreement alone can hide gain/dB approximation
        # against playback of the exact edited FX input.
        fx_actual = stage / "fx-input.mp4"
        step("edited_fx_audio_render", lambda: _run(
            [args.tsrct, "export", "--project", str(document), "--output", str(fx_actual), "--fps", "30"],
            stage / "edited-fx-render.log", args.timeout))
        conversion = stage / "conversion"
        output = step("fresh_export", lambda: _run(
            [args.converter, "convert", str(document), "--to", "after-effects", "--fps", "24",
             "--output", str(conversion)], stage / "export.log", args.timeout))
        exported = conversion / "project.aep"
        if not exported.is_file():
            raise AudioTestError("fresh export did not produce project.aep")
        inspected = stage / "export-inspection.json"
        step("own_reader_export", lambda: _run(
            [args.preparer, "inspect-export", case["id"], str(exported), str(inspected)],
            stage / "inspect-export.log", args.timeout))
        step("own_reader_export_assertions", lambda: _assert_export_inspection(inspected, case))
        stages["own_reader_export"].update({"inspection_sha256": sha256(inspected),
                                            "evidence": "own-reader only; not Adobe acceptance"})
        stages["prepare_explicit_fx"].update({"document_json_sha256": sha256(prepared / "document.json"),
                                               "archive_sha256": sha256(document)})
        stages["fresh_export"]["aep_sha256"] = sha256(exported)
        roundtrip = stage / "reimport"
        step("fresh_reimport", lambda: _run(
            [args.converter, "convert", str(exported), "--to", "tesseract", "--composition", "1",
             "--output", str(roundtrip)], stage / "reimport.log", args.timeout))
        roundtrip_document = roundtrip / "project.tsrct"
        if not roundtrip_document.is_file():
            raise AudioTestError("fresh reimport did not produce project.tsrct")
        stages["fresh_reimport"].update({"document_sha256": sha256(roundtrip_document),
                                          "evidence": "own-reader/roundtrip only"})
        actual = stage / "actual.mp4"
        step("roundtrip_fx_audio_render", lambda: _run(
            [args.tsrct, "export", "--project", str(roundtrip_document), "--output", str(actual), "--fps", "30"],
            stage / "roundtrip-fx-render.log", args.timeout))
    if any(x.lower() not in output.lower() for x in expected):
        raise AudioTestError(f"missing expected {direction} contextual diagnostics: {expected}")
    stages["diagnostics"] = {"status": "passed", "expected": expected}
    if actual.resolve() == reference.resolve() or not actual.is_file():
        raise AudioTestError("missing actual or actual/reference alias")
    for label, artifact in (("actual", actual), ("reference", reference)):
        result = step(label + "_video_contract", lambda: _video_contract(artifact))
        stages[label + "_video_contract"].update(result)
    windows = step("reference_window_contract", lambda: _reference_windows(reference, case["reference_expectation"], policy))
    stages["reference_window_contract"].update(windows)
    stages["comparison"] = {"status": "running", "actual_sha256": sha256(actual),
                            "reference_path": case["reference"]["path"]}
    save_report(work, report)
    result = step("comparison", lambda: compare(actual, reference, policy))
    stages["comparison"].update({"result": result, "actual_sha256": sha256(actual),
                                 "reference_path": case["reference"]["path"],
                                 "status": "passed" if result["passed"] else "failed_score"})
    scores_passed = result["passed"]
    if fx_actual is not None:
        fx_video = step("edited_fx_video_contract", lambda: _video_contract(fx_actual))
        stages["edited_fx_video_contract"].update(fx_video)
        for label, left, right in (("edited_fx_vs_independent_reference", fx_actual, reference),
                                   ("roundtrip_fx_vs_edited_fx", actual, fx_actual)):
            measured = step(label, lambda: compare(left, right, policy))
            stages[label].update({"status": "passed" if measured["passed"] else "failed_score",
                                  "result": measured, "actual_sha256": sha256(left),
                                  "reference_sha256": sha256(right)})
            scores_passed = scores_passed and measured["passed"]
    if not scores_passed:
        raise AudioTestError("audio score below pinned policy; see persisted report")


def save_report(work: Path, report: dict) -> None:
    target = work / "result.json"
    temp = work / "result.json.tmp"
    temp.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    temp.replace(target)


def run(args) -> dict:
    manifest = json.loads(args.manifest.read_text())
    case_ids = [case["id"] for case in manifest["cases"]] if args.case == "all" else [args.case]
    if not case_ids:
        raise AudioTestError("no registered cases")
    selections = []
    for case_id in case_ids:
        policy, case, root = load_case(args.manifest, case_id, args.direction)
        directions = [d for d in ("import", "export") if d in case["directions"]] if args.direction == "both" else [args.direction]
        # Preflight every pin before creating output or launching commands.
        _path(root, case["source"], "independent native source")
        for item in case["primary"]:
            _path(root, item, "primary media")
        if "export" in directions:
            require_hash(root / "fx" / (case["id"] + ".json"),
                         case["fx_input"]["document_sha256"], "explicit edited FX document")
            if "path" in case["fx_input"]:
                _path(root, case["fx_input"], "explicit edited FX input")
        _reference_pin(root, case["reference"])
        selections.append((policy, case, root, directions))
    args.work.mkdir(parents=True, exist_ok=False)
    aggregate = {"status": "running", "cases": {},
                 "provenance": "offline FX render vs pinned independent source reference; export own-reader/roundtrip only, not Adobe acceptance or generated-AEP native rendering"}
    for policy, case, root, directions in selections:
        work = args.work / case["id"] if args.case == "all" else args.work
        if work != args.work:
            work.mkdir()
        report = {"case_id": case["id"], "status": "running", "directions": {},
                  "provenance": aggregate["provenance"]}
        save_report(work, report)
        for direction in directions:
            try:
                execute(args, direction, work, case, root, policy, report)
            except (AudioTestError, OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError) as exc:
                report["directions"].setdefault(direction, {})["failure"] = str(exc)
                report["status"] = "failed"
                save_report(work, report)
        if report["status"] != "failed":
            report["status"] = "scored_local_reference"
        save_report(work, report)
        aggregate["cases"][case["id"]] = report
    aggregate["status"] = "failed" if any(case["status"] == "failed" for case in aggregate["cases"].values()) else "scored_local_reference"
    if args.case == "all":
        save_report(args.work, aggregate)
    if aggregate["status"] == "failed":
        raise AudioTestError(f"one or more case/direction attempts failed; see {args.work / 'result.json'}")
    return aggregate if args.case == "all" else aggregate["cases"][args.case]
