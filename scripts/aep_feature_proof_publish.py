#!/usr/bin/env python3
"""Bounded native-reference operations for :mod:`aep_feature_proof`.

Nothing in this module performs an FX conversion or render comparison.  Local reference recording refuses replacement of a committed oracle.
"""

from __future__ import annotations

from datetime import datetime, timezone
from fractions import Fraction
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import uuid
from typing import Any, BinaryIO

from aep_feature_proof import (
    Paths,
    ProofError,
    _scan_secrets,
    atomic_write,
    load_json,
    relative_path,
    resolve_repo_path,
    sha256_file,
    validate_manifest,
    write_json,
)

RENDER_SETTINGS = "Use this frame rate: 30; Quality: Best; Resolution: Full"
OUTPUT_MODULE = "H.264 - Match Render Settings - 40 Mbps"
ERROR_RE = re.compile(
    r"aerender\s+Error|After Effects error|missing footage|could not be found|"
    r"missing font|font.*not found|offline media",
    re.I,
)
TOKEN_RE = re.compile(r"AEP_PROOF_TOKEN:([0-9a-f]{32})")
PROHIBITED_JSX_RE = re.compile(
    r"app\s*\.\s*(?:quit|beginSuppressDialogs|endSuppressDialogs)|"
    r"(?:app\s*\.\s*project|project)\s*\.\s*close\s*\(",
    re.I,
)


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def _scratch(paths: Paths, case: dict[str, Any]) -> Path:
    root = paths.workspace / "crates/aftereffects_file/tests/fixtures/render/.aep-feature-proof"
    folder = root / case["case_id"]
    root.mkdir(parents=True, exist_ok=True)
    if root.is_symlink() or (folder.exists() and folder.is_symlink()):
        raise ProofError("unsafe symlink in AE proof scratch directory")
    folder.mkdir(mode=0o700, exist_ok=True)
    return folder


def _journal_path(paths: Paths, case: dict[str, Any]) -> Path:
    return _scratch(paths, case) / "journal.json"


def _read_journal(paths: Paths, case: dict[str, Any]) -> dict[str, Any]:
    path = _journal_path(paths, case)
    if not path.exists():
        return {}
    journal = load_json(path)
    if journal.get("case_id") != case["case_id"]:
        raise ProofError("restart journal belongs to a different case")
    return journal


def _write_journal(paths: Paths, case: dict[str, Any], journal: dict[str, Any]) -> None:
    # Never persist signed URLs, credentials, or Authorization values here.
    encoded = json.dumps(journal, sort_keys=True)
    if re.search(r"X-Goog-(?:Credential|Signature)|Bearer\s|upload_url|Authorization", encoded, re.I):
        raise ProofError("refusing to persist credential or signed-URL material")
    write_json(_journal_path(paths, case), journal, mode=0o600)


def _new_output(path: Path, description: str) -> None:
    if path.exists() or path.is_symlink():
        raise ProofError(f"{description} already exists; refusing replacement: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.parent.is_symlink():
        raise ProofError(f"unsafe symlink parent for {description}: {path.parent}")


def _jsx_string(value: str) -> str:
    # JSON strings are valid JavaScript strings and avoid hand-built quoting.
    return json.dumps(value, ensure_ascii=False)


def author_source(paths: Paths, case: dict[str, Any], args: Any) -> None:
    """Emit an ownership-guarded wrapper; deliberately never launches Adobe."""
    feature_jsx = resolve_repo_path(paths.workspace, args.feature_jsx, must_exist=True)
    wrapper_jsx = resolve_repo_path(paths.workspace, args.wrapper_jsx, must_exist=False)
    readback = resolve_repo_path(paths.workspace, args.readback, must_exist=False)
    receipt = resolve_repo_path(paths.workspace, args.receipt, must_exist=False)
    source = resolve_repo_path(paths.workspace, case["source_path"], must_exist=False)
    outputs = (wrapper_jsx, readback, receipt, source)
    if len(set(outputs)) != len(outputs) or feature_jsx in outputs:
        raise ProofError("feature JSX, wrapper, readback, receipt, and AEP output paths must be distinct")
    for path, description in (
        (wrapper_jsx, "wrapper JSX"),
        (readback, "native readback"),
        (receipt, "ownership receipt"),
        (source, "native AEP source"),
    ):
        _new_output(path, description)

    body = feature_jsx.read_text(encoding="utf-8")
    if "function buildAep(project)" not in body or "function readbackAep(project)" not in body:
        raise ProofError(
            "feature JSX must define buildAep(project) and readbackAep(project); "
            "the helper will not invent generic feature geometry"
        )
    if PROHIBITED_JSX_RE.search(body):
        raise ProofError("feature JSX may not quit Adobe, suppress dialogs, or close a project")

    token = uuid.uuid4().hex
    source_value = relative_path(paths.workspace, source)
    readback_value = relative_path(paths.workspace, readback)
    receipt_value = relative_path(paths.workspace, receipt)
    wrapper = f"""// Generated safety wrapper. AEP_PROOF_TOKEN:{token}
// NOT EXECUTED BY THE HELPER. Before manual execution, verify no After Effects
// process is running. The wrapper refuses a file-backed, dirty, or nonempty
// project, creates and closes only its own project, never suppresses dialogs,
// and never quits the application.
(function () {{
    function fail(message) {{ throw new Error("AEP feature proof: " + message); }}
    function writeJson(path, value) {{
        var file = new File(path);
        if (file.exists) fail("output already exists: " + path);
        if (!file.open("w")) fail("cannot create output: " + path);
        file.encoding = "UTF-8";
        file.write(JSON.stringify(value, null, 2));
        file.close();
    }}

    var prior = app.project;
    if (prior && (prior.file !== null || prior.dirty || prior.numItems !== 0)) {{
        fail("existing project/file/dirty/nonempty state; session is not owned");
    }}
    var sourceFile = new File({_jsx_string(str(source))});
    var readbackFile = new File({_jsx_string(str(readback))});
    var receiptFile = new File({_jsx_string(str(receipt))});
    if (sourceFile.exists || readbackFile.exists || receiptFile.exists) {{
        fail("one or more outputs already exist");
    }}
    var ownedProject = app.newProject();
    if (!ownedProject || app.project !== ownedProject) fail("failed to establish owned project");

{body}

    buildAep(ownedProject);
    if (app.project !== ownedProject) fail("feature JSX changed project ownership");
    ownedProject.save(sourceFile);
    var featureReadback = readbackAep(ownedProject);
    writeJson({_jsx_string(str(readback))}, {{
        case_id: {_jsx_string(case['case_id'])},
        source_path: {_jsx_string(source_value)},
        composition_id: {case['composition_id']},
        feature: featureReadback
    }});
    if (app.project !== ownedProject) fail("project ownership changed before close");
    ownedProject.close(CloseOptions.DO_NOT_SAVE_CHANGES);
    writeJson({_jsx_string(str(receipt))}, {{
        case_id: {_jsx_string(case['case_id'])},
        source_path: {_jsx_string(source_value)},
        composition_id: {case['composition_id']},
        readback_path: {_jsx_string(readback_value)},
        receipt_path: {_jsx_string(receipt_value)},
        token: {_jsx_string(token)},
        adobe_version: app.version,
        completed: true,
        owned_project_closed: true
    }});
}})();
"""
    atomic_write(wrapper_jsx, wrapper)
    print(relative_path(paths.workspace, wrapper_jsx))
    print(
        "NOT EXECUTED: review both JSX files, confirm no After Effects process is running, "
        "then execute the wrapper manually without suppressing dialogs."
    )


def _record_authoring(paths: Paths, case: dict[str, Any], args: Any) -> dict[str, Any]:
    feature_jsx = resolve_repo_path(paths.workspace, args.feature_jsx, must_exist=True)
    wrapper_jsx = resolve_repo_path(paths.workspace, args.wrapper_jsx, must_exist=True)
    readback_path = resolve_repo_path(paths.workspace, args.readback, must_exist=True)
    receipt_path = resolve_repo_path(paths.workspace, args.receipt, must_exist=True)
    source_path = resolve_repo_path(paths.workspace, case["source_path"], must_exist=True)

    wrapper = wrapper_jsx.read_text(encoding="utf-8")
    token_match = TOKEN_RE.search(wrapper)
    if token_match is None:
        raise ProofError("wrapper lacks its ownership token")
    feature_body = feature_jsx.read_text(encoding="utf-8")
    if feature_body not in wrapper:
        raise ProofError("wrapper does not retain the supplied feature-specific JSX verbatim")
    for guard in (
        "existing project/file/dirty/nonempty state",
        "app.project !== ownedProject",
        "ownedProject.close(CloseOptions.DO_NOT_SAVE_CHANGES)",
    ):
        if guard not in wrapper:
            raise ProofError("wrapper is missing an ownership-safety guard")
    readback = load_json(readback_path)
    receipt = load_json(receipt_path)
    _scan_secrets(readback, "native readback")
    _scan_secrets(receipt, "ownership receipt")
    expected_identity = {
        "case_id": case["case_id"],
        "source_path": relative_path(paths.workspace, source_path),
        "composition_id": case["composition_id"],
    }
    for key, value in expected_identity.items():
        if readback.get(key) != value or receipt.get(key) != value:
            raise ProofError(f"authoring readback/receipt identity mismatch: {key}")
    if receipt.get("token") != token_match.group(1):
        raise ProofError("ownership receipt token does not match wrapper")
    if receipt.get("completed") is not True or receipt.get("owned_project_closed") is not True:
        raise ProofError("ownership receipt does not record completed owned-project close")
    if not isinstance(readback.get("feature"), dict) or not readback["feature"]:
        raise ProofError("native readback lacks feature-specific fields")
    adobe_version = receipt.get("adobe_version")
    if not isinstance(adobe_version, str) or not adobe_version.strip():
        raise ProofError("ownership receipt lacks Adobe version")
    # The AEP remains the authoritative source. Its hash is stored in the native
    # reference manifest when the new target is inventoried, not duplicated here.
    return {
        "feature_jsx_path": relative_path(paths.workspace, feature_jsx),
        "feature_jsx_sha256": sha256_file(feature_jsx),
        "wrapper_jsx_path": relative_path(paths.workspace, wrapper_jsx),
        "wrapper_jsx_sha256": sha256_file(wrapper_jsx),
        "readback_path": relative_path(paths.workspace, readback_path),
        "readback_sha256": sha256_file(readback_path),
        "receipt_path": relative_path(paths.workspace, receipt_path),
        "receipt_sha256": sha256_file(receipt_path),
        "adobe_version": adobe_version.strip(),
        "recorded_at": utc_now(),
    }


def _run_owned(command: list[str], timeout: int, output: BinaryIO | None = None) -> tuple[bytes, bytes]:
    if timeout <= 0 or timeout > 3600:
        raise ProofError("timeout must be between 1 and 3600 seconds")
    process = subprocess.Popen(
        command,
        stdout=output if output is not None else subprocess.PIPE,
        stderr=subprocess.STDOUT if output is not None else subprocess.PIPE,
        start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except BaseException:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.communicate()
        raise
    if process.returncode != 0:
        raise ProofError(f"owned command failed: {Path(command[0]).name} exit {process.returncode}")
    return stdout or b"", stderr or b""


def _tool(explicit: str | None, default: str) -> str:
    value = explicit or shutil.which(default)
    if not value:
        raise ProofError(f"required executable not found: {default}")
    path = Path(value).expanduser().resolve()
    if not path.is_file():
        raise ProofError(f"executable is not a file: {path}")
    return str(path)


def _verify_dependencies(source: dict[str, Any]) -> None:
    for dependency in source.get("media_dependencies", []):
        path_value = dependency.get("authored_path")
        expected = dependency.get("sha256")
        if not isinstance(path_value, str) or not isinstance(expected, str):
            raise ProofError("media dependency lacks authored_path/SHA-256")
        path = Path(path_value).expanduser()
        if not path.is_file() or path.is_symlink():
            raise ProofError("native media dependency is unavailable; path suppressed")
        if sha256_file(path) != expected:
            raise ProofError("native media dependency SHA-256 mismatch; path suppressed")


def _validate_video(video: Path, composition: dict[str, Any], ffprobe: str, ffmpeg: str) -> dict[str, Any]:
    stdout, stderr = _run_owned(
        [ffprobe, "-v", "error", "-count_frames", "-show_streams", "-show_format", "-of", "json", str(video)],
        180,
    )
    if stderr.strip():
        raise ProofError("ffprobe emitted errors while validating native output")
    try:
        metadata = json.loads(stdout)
    except json.JSONDecodeError as exc:
        raise ProofError("ffprobe returned invalid JSON") from exc
    videos = [stream for stream in metadata.get("streams", []) if stream.get("codec_type") == "video"]
    if len(videos) != 1:
        raise ProofError("native output must contain exactly one video stream")
    stream = videos[0]
    expected_frames = Fraction(
        composition["duration_numerator"], composition["duration_denominator"]
    ) * 30
    if expected_frames.denominator != 1:
        raise ProofError("composition duration does not map to a whole 30fps frame count")
    checks = (
        (stream.get("codec_name") == "h264", "codec is not H.264"),
        (stream.get("width") == composition["width"], "width mismatch"),
        (stream.get("height") == composition["height"], "height mismatch"),
        (Fraction(stream.get("avg_frame_rate", "0/1")) == 30, "average FPS is not 30"),
        (Fraction(stream.get("r_frame_rate", "0/1")) == 30, "reported FPS is not 30"),
        (int(stream.get("nb_read_frames", -1)) == expected_frames, "decoded frame-count mismatch"),
    )
    for valid, message in checks:
        if not valid:
            raise ProofError(message)
    expected_duration = Fraction(
        composition["duration_numerator"], composition["duration_denominator"]
    )
    try:
        actual_duration = Fraction(stream["duration"])
    except (KeyError, ValueError, ZeroDivisionError) as exc:
        raise ProofError("native output lacks an exact stream duration") from exc
    if abs(actual_duration - expected_duration) > Fraction(1, 1000):
        raise ProofError("native output duration mismatch")
    _, decode_stderr = _run_owned(
        [ffmpeg, "-v", "error", "-i", str(video), "-f", "null", "-"], 300
    )
    if decode_stderr.strip():
        raise ProofError("ffmpeg reported errors during full native-output decode")
    return {
        "sha256": sha256_file(video),
        "bytes": video.stat().st_size,
        "fps": 30,
        "frame_count": int(expected_frames),
        "duration_seconds": float(expected_duration),
        "width": stream["width"],
        "height": stream["height"],
        "pixel_format": stream.get("pix_fmt"),
        "color_space": stream.get("color_space"),
        "color_transfer": stream.get("color_transfer"),
        "color_primaries": stream.get("color_primaries"),
        "audio_streams": [
            {
                key: audio.get(key)
                for key in ("codec_name", "sample_rate", "channels", "duration")
            }
            for audio in metadata.get("streams", [])
            if audio.get("codec_type") == "audio"
        ],
    }


def render_reference(
    paths: Paths,
    case: dict[str, Any],
    source: dict[str, Any],
    composition: dict[str, Any],
    args: Any,
) -> None:
    if composition.get("status") != "pending" or composition.get("reference"):
        raise ProofError(
            "target is not an unpublished pending entry; refusing regeneration or blocked-state overwrite"
        )
    journal = _read_journal(paths, case)
    if journal and journal.get("phase") not in {"render_failed", "local_validated"}:
        raise ProofError(f"existing journal phase {journal.get('phase')!r} requires inspection/recovery")
    source_path = resolve_repo_path(paths.workspace, source["source_path"], must_exist=True)
    if sha256_file(source_path) != source["source_sha256"] or source_path.stat().st_size != source["source_bytes"]:
        raise ProofError("source AEP does not match manifest provenance")
    matching_names = [
        item
        for item in source.get("compositions", [])
        if item.get("composition_name") == composition["composition_name"]
    ]
    if len(matching_names) != 1 or matching_names[0].get("composition_id") != composition["composition_id"]:
        raise ProofError("composition name is not unique to the pinned manifest target")
    _verify_dependencies(source)
    folder = _scratch(paths, case)
    scratch = folder / "source.aep"
    video = folder / "expected.mp4"
    log = folder / "adobe-render.log"
    if video.exists():
        raise ProofError("local native output already exists; inspect/recover instead of rerendering")
    if not scratch.exists():
        shutil.copyfile(source_path, scratch)
    if scratch.is_symlink() or sha256_file(scratch) != source["source_sha256"]:
        raise ProofError("scratch AEP does not match immutable source")
    aerender = _tool(args.aerender, "aerender")
    ffprobe = _tool(None, "ffprobe")
    ffmpeg = _tool(None, "ffmpeg")
    command = [
        aerender,
        "-project",
        str(scratch),
        "-comp",
        composition["composition_name"],
        "-s",
        "0",
        "-e",
        str(composition["frame_count"] - 1),
        "-renderSettings",
        RENDER_SETTINGS,
        "-OMtemplate",
        OUTPUT_MODULE,
        "-output",
        str(video),
        "-mem_usage",
        "20",
        "40",
        "-mfr",
        "OFF",
        "50",
        "-v",
        "ERRORS_AND_PROGRESS",
    ]
    journal = {
        "schema_version": 1,
        "case_id": case["case_id"],
        "source_sha256": source["source_sha256"],
        "composition_id": composition["composition_id"],
        "phase": "rendering",
        "started_at": utc_now(),
    }
    _write_journal(paths, case, journal)
    try:
        with log.open("xb") as stream:
            _run_owned(command, args.timeout, output=stream)
        log_text = log.read_text(encoding="utf-8", errors="replace")
        if ERROR_RE.search(log_text):
            raise ProofError("Adobe render log reports missing/error content; inspect local log")
        if not video.is_file() or video.is_symlink() or video.stat().st_size <= 0:
            raise ProofError("Adobe did not produce a regular nonempty output")
        reference = _validate_video(video, composition, ffprobe, ffmpeg)
        reference["render_log_sha256"] = sha256_file(log)
        sheet = folder / "samples.png"
        middle = reference["frame_count"] // 2
        _run_owned(
            [
                ffmpeg,
                "-v",
                "error",
                "-i",
                str(video),
                "-vf",
                f"select='eq(n,0)+eq(n,{middle})+eq(n,{reference['frame_count'] - 1})',scale=320:-1,tile=3x1",
                "-frames:v",
                "1",
                str(sheet),
            ],
            180,
        )
        journal.update(
            phase="local_validated",
            validated_at=utc_now(),
            reference=reference,
            output_name=video.name,
            contact_sheet_name=sheet.name,
            inspection=None,
        )
        _write_journal(paths, case, journal)
    except BaseException as exc:
        journal.update(phase="render_failed", failed_at=utc_now(), error_type=type(exc).__name__)
        _write_journal(paths, case, journal)
        raise
    print(f"{case['case_id']}: local native render validated; reference not recorded")


def inspect_render(paths: Paths, case: dict[str, Any], args: Any, *, authoring: bool = False) -> Any:
    if authoring:
        return _record_authoring(paths, case, args)
    note = args.note.strip()
    if len(note) < 20:
        raise ProofError("inspection note must concretely describe visible feature content")
    if re.search(r"X-Goog-|https?://|Bearer\s|token", note, re.I):
        raise ProofError("inspection note may not contain URLs or credential material")
    journal = _read_journal(paths, case)
    if journal.get("phase") != "local_validated":
        raise ProofError("a machine-validated local native render is required before inspection")
    journal["inspection"] = {
        "recorded_at": utc_now(),
        "note": note,
        "scope": "native source-content inspection only; not an AEP/FX render comparison",
    }
    _write_journal(paths, case, journal)
    print(f"{case['case_id']}: inspection provenance recorded; visual comparison remains unmeasured")


def publish_reference(
    paths: Paths,
    case: dict[str, Any],
    source: dict[str, Any],
    composition: dict[str, Any],
) -> None:
    if composition.get("status") != "pending" or composition.get("reference"):
        raise ProofError("target is not an unpublished pending entry; refusing overwrite")
    journal = _read_journal(paths, case)
    if (journal.get("source_sha256") != source["source_sha256"]
            or journal.get("composition_id") != composition["composition_id"]):
        raise ProofError("local render belongs to a different source or composition")
    if not isinstance(journal.get("inspection"), dict):
        raise ProofError("record concrete native contact-sheet inspection before recording")
    reference = journal.get("reference")
    if not isinstance(reference, dict):
        raise ProofError("journal lacks machine-validated local reference metadata")
    video = _scratch(paths, case) / journal.get("output_name", "")
    if not video.is_file() or video.is_symlink():
        raise ProofError("validated local native output is missing")
    if video.stat().st_size != reference["bytes"] or sha256_file(video) != reference["sha256"]:
        raise ProofError("local native output changed after validation")
    source_path = resolve_repo_path(paths.workspace, source["source_path"], must_exist=True)
    if sha256_file(source_path) != source["source_sha256"]:
        raise ProofError("source AEP changed after render")

    current_manifest_sha = sha256_file(paths.manifest)
    current_manifest = load_json(paths.manifest)
    current_targets = validate_manifest(current_manifest)
    current_target = current_targets.get((source["source_path"], composition["composition_id"]))
    if current_target is None:
        raise ProofError("manifest target disappeared after local render")
    current_source, current_match = current_target
    if (current_source["source_sha256"] != source["source_sha256"]
            or current_source.get("source_bytes") != source.get("source_bytes")):
        raise ProofError("manifest source identity changed after render; refusing update")
    if current_match.get("status") != "pending" or current_match.get("reference"):
        raise ProofError("manifest target changed concurrently; refusing overwrite")

    destination = paths.workspace / "tests/references/aep" / f"{case['case_id']}.mp4"
    if (not destination.parent.resolve().is_relative_to(paths.workspace)
            or any(parent.is_symlink() for parent in
                   (destination.parent, *destination.parent.parents)
                   if parent != paths.workspace and parent.is_relative_to(paths.workspace))):
        raise ProofError("unsafe symlink or escaping directory for committed reference")
    _new_output(destination, "committed native reference")
    published = {key: value for key, value in reference.items()
                 if key not in {"sha256", "bytes", "render_log_sha256"}}
    published["path"] = relative_path(paths.workspace, destination)
    current_match["status"] = "verified"
    current_match["reference"] = published
    current_match["verification"] = {
        "native_render": True,
        "full_decode": True,
        "visual_inspection": journal["inspection"]["note"],
        "fx_render_comparison": "not_run",
        "alpha_fidelity": "unverified",
        "audio_fidelity": "unverified",
    }
    validate_manifest(current_manifest)
    if sha256_file(paths.manifest) != current_manifest_sha:
        raise ProofError("manifest changed concurrently; refusing update")
    try:
        with video.open("rb") as src, destination.open("xb") as dst:
            shutil.copyfileobj(src, dst)
            dst.flush()
            os.fsync(dst.fileno())
        if sha256_file(paths.manifest) != current_manifest_sha:
            raise ProofError("manifest changed during local reference copy")
        write_json(paths.manifest, current_manifest)
    except BaseException:
        destination.unlink(missing_ok=True)
        raise
    journal.update(phase="local_recorded", recorded_at=utc_now())
    _write_journal(paths, case, journal)
    print(f"{case['case_id']}: local native reference recorded; conversion comparison unmeasured")
