#!/usr/bin/env python3
"""Bounded local AEP import scoring against pinned independent Adobe renders.

This module intentionally has no CI integration and never performs an implicit
corpus sweep.  The feature registry's ``UNRUN`` fields are historical fixture
metadata; live results are written only below the selected scratch directory.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
from fractions import Fraction
import hashlib
import json
import math
import os
from pathlib import Path
import signal
import shutil
import subprocess
import sys
import tempfile
from typing import Any, Callable

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
DEFAULT_REGISTRY = REPO / "crates/aftereffects_file/tests/fixtures/aep_feature_cases.json"
DEFAULT_REFERENCES = REPO / "crates/aftereffects_file/tests/fixtures/aep_video_references.json"
DEFAULT_CACHE = Path.home() / ".cache/jerboa/conversion"
DEFAULT_SCRATCH = REPO / "tmp/aep-test-runs"
SAMPLE_FPS = 30
SAMPLE_INTERVAL = Fraction(1, SAMPLE_FPS)
SAMPLE_INTERVAL_CLI = "0.03333333333333333"

sys.path.insert(0, str(HERE))
import aep_feature_proof as proof  # noqa: E402


class AepTestError(RuntimeError):
    """A classified case failure that must never be represented as a score."""

    status = "failed"


class IdentityRejected(AepTestError):
    status = "identity_rejected"


class MalformedResult(AepTestError):
    status = "malformed"


class BlockedCase(AepTestError):
    status = "blocked"


class MissingDependency(AepTestError):
    status = "missing_dependency"


class ImportFailed(AepTestError):
    status = "import_failed"


class UnsupportedConversion(AepTestError):
    status = "unsupported"


class FontImportFailed(AepTestError):
    status = "font_import_failed"


class RenderFailed(AepTestError):
    status = "render_failed"


class ComparisonFailed(AepTestError):
    status = "comparison_failed"


class IoFailed(AepTestError):
    status = "io_failed"


class CommandTimedOut(AepTestError):
    status = "timeout"


@dataclass(frozen=True)
class CompletedCommand:
    returncode: int
    stdout: str
    stderr: str


CommandExecutor = Callable[[list[str], int], CompletedCommand]


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.parent.is_symlink() or (path.exists() and path.is_symlink()):
        raise IoFailed(f"refusing symlink JSON output: {path}")
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent, prefix=f".{path.name}.", suffix=".tmp"
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def _terminate_owned_process_group(process: subprocess.Popen[str]) -> None:
    """Kill the new process group, including decoder/renderer descendants."""
    if os.name != "posix":
        process.kill()
        process.wait()
        return
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def default_executor(command: list[str], timeout: int) -> CompletedCommand:
    process = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=os.name == "posix",
    )
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired as exc:
        _terminate_owned_process_group(process)
        stdout, stderr = process.communicate()
        raise subprocess.TimeoutExpired(command, timeout, output=stdout, stderr=stderr) from exc
    except KeyboardInterrupt:
        _terminate_owned_process_group(process)
        process.communicate()
        raise
    return CompletedCommand(process.returncode, stdout, stderr)


def _load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise MalformedResult(f"cannot load {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise MalformedResult(f"{path} must contain a JSON object")
    return value


def load_catalog(
    workspace: Path,
    registry_path: Path = DEFAULT_REGISTRY,
    references_path: Path = DEFAULT_REFERENCES,
) -> tuple[dict[str, dict[str, Any]], dict[tuple[str, int], tuple[dict[str, Any], dict[str, Any]]], dict[str, Any]]:
    """Load and validate the existing import-only registry and reference inventory."""
    registry = _load_json(registry_path)
    references = _load_json(references_path)
    try:
        proof.validate_registry(registry)
        targets = proof.validate_manifest(references)
    except proof.ProofError as exc:
        raise MalformedResult(str(exc)) from exc
    cases = {case["case_id"]: case for case in registry["cases"]}
    return cases, targets, references


def select_cases(cases: dict[str, dict[str, Any]], case_ids: list[str]) -> list[dict[str, Any]]:
    if not case_ids:
        raise BlockedCase("at least one explicit --case-id is required; full sweeps are never implicit")
    if len(case_ids) != len(set(case_ids)):
        raise BlockedCase("duplicate --case-id values are not allowed")
    missing = [case_id for case_id in case_ids if case_id not in cases]
    if missing:
        raise BlockedCase(f"unknown case ID(s): {', '.join(missing)}")
    return [cases[case_id] for case_id in case_ids]


def _positive_int(value: Any, where: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise IdentityRejected(f"{where} must be a positive integer")
    return value


def _positive_fraction(numerator: Any, denominator: Any, where: str) -> Fraction:
    numerator = _positive_int(numerator, f"{where} numerator")
    denominator = _positive_int(denominator, f"{where} denominator")
    return Fraction(numerator, denominator)


def conversion_workspace(workspace: Path) -> Path:
    """Locate the standalone converter when invoked from the enclosing repository."""
    nested = workspace / "opensource/conv"
    return nested if (nested / "crates/aftereffects_file").is_dir() else workspace


def validate_target_identity(
    workspace: Path,
    case: dict[str, Any],
    target: tuple[dict[str, Any], dict[str, Any]] | None,
    reference_settings: dict[str, Any],
) -> tuple[Path, dict[str, Any], dict[str, Any], list[Fraction]]:
    """Bind a selected case to exact source bytes and a verified native reference."""
    workspace = workspace.resolve()
    if target is None:
        raise IdentityRejected("selected source/composition is absent from the reference inventory")
    source, composition = target
    problems = proof._reference_problems(composition)
    if problems:
        raise IdentityRejected("; ".join(problems))
    if source.get("source_path") != case["source_path"] or composition.get("composition_id") != case["composition_id"]:
        raise IdentityRejected("registry and reference target identities differ")
    if reference_settings.get("output_fps") != SAMPLE_FPS:
        raise IdentityRejected("reference producer does not declare 30fps output")
    if reference_settings.get("source_fps_unchanged") is not True:
        raise IdentityRejected("reference producer does not declare unchanged native source FPS")
    for key in (
        "renderer",
        "adobe_version",
        "render_settings",
        "output_module_template",
        "range_policy",
        "color_policy",
    ):
        if not isinstance(reference_settings.get(key), str) or not reference_settings[key].strip():
            raise IdentityRejected(f"reference producer identity lacks {key}")

    try:
        source_path = proof.resolve_repo_path(conversion_workspace(workspace), source["source_path"], must_exist=True)
        source_size = source_path.stat().st_size
        source_sha = sha256_file(source_path)
    except (OSError, proof.ProofError) as exc:
        raise IdentityRejected(f"cannot verify pinned source bytes: {exc}") from exc
    if source_size != source.get("source_bytes") or source_sha != source.get("source_sha256"):
        raise IdentityRejected("pinned source byte count or SHA-256 does not match the reference inventory")

    width = _positive_int(composition.get("width"), "composition width")
    height = _positive_int(composition.get("height"), "composition height")
    duration = _positive_fraction(
        composition.get("duration_numerator"),
        composition.get("duration_denominator"),
        "composition duration",
    )
    expected_frames = _positive_int(composition.get("expected_frame_count"), "expected frame count")
    if duration * SAMPLE_FPS != expected_frames:
        raise IdentityRejected("composition duration and expected 30fps frame count disagree")
    native_fps = _positive_fraction(
        composition.get("fps_numerator"),
        composition.get("fps_denominator"),
        "composition native FPS",
    )
    native_frames = _positive_int(composition.get("frame_count"), "composition native frame count")
    if duration * native_fps != native_frames:
        raise IdentityRejected("composition duration, native FPS and native frame count disagree")
    _positive_fraction(
        composition.get("pixel_aspect_numerator"),
        composition.get("pixel_aspect_denominator"),
        "composition pixel aspect",
    )
    display_start_numerator = composition.get("display_start_numerator")
    display_start_denominator = composition.get("display_start_denominator")
    if (
        not isinstance(display_start_numerator, int)
        or isinstance(display_start_numerator, bool)
        or not isinstance(display_start_denominator, int)
        or isinstance(display_start_denominator, bool)
        or display_start_denominator <= 0
    ):
        raise IdentityRejected("composition display start must be an exact rational")
    reference = composition["reference"]
    if (reference.get("width"), reference.get("height")) != (width, height):
        raise IdentityRejected("reference canvas differs from the selected composition")
    try:
        reference_duration = Fraction(str(reference.get("duration_seconds")))
    except (ValueError, ZeroDivisionError) as exc:
        raise IdentityRejected("reference duration is missing or invalid") from exc
    if reference_duration != duration:
        raise IdentityRejected("reference duration differs from the selected composition")

    # Critical timestamps annotate feature evidence; they do not select samples.
    # The comparator still covers every native frame when no annotations exist.
    critical: list[Fraction] = []
    for value in case["critical_frames"]:
        try:
            timestamp = Fraction(value)
        except (TypeError, ValueError, ZeroDivisionError) as exc:
            raise MalformedResult(f"invalid critical frame {value!r}") from exc
        if timestamp in critical:
            raise MalformedResult(f"duplicate critical-grid timestamp: {timestamp}")
        if timestamp < 0 or timestamp >= duration:
            raise BlockedCase(f"critical frame {value} is outside the half-open native render range")
        sample_index = timestamp * SAMPLE_FPS
        if sample_index.denominator != 1:
            raise BlockedCase(
                f"critical frame {value} is not on the exact 30fps comparator grid; "
                "no arbitrary-time comparison is implemented"
            )
        critical.append(timestamp)
    return source_path, source, composition, critical


def _tool_path(value: Path | str, workspace: Path) -> Path:
    path = Path(value)
    if not path.is_absolute() and path.parent != Path("."):
        path = workspace / path
    if path.parent == Path("."):
        found = shutil.which(str(path))
        if found is None:
            raise MissingDependency(f"required executable is unavailable: {path}")
        path = Path(found)
    try:
        resolved = path.resolve(strict=True)
    except OSError as exc:
        raise MissingDependency(f"required executable is unavailable: {path}") from exc
    if not resolved.is_file() or not os.access(resolved, os.X_OK):
        raise MissingDependency(f"required executable is not executable: {resolved}")
    return resolved


def tool_identity(path: Path) -> dict[str, Any]:
    return {"path": str(path), "bytes": path.stat().st_size, "sha256": sha256_file(path)}


def _write_command_logs(case_dir: Path, index: int, stage: str, result: CompletedCommand) -> dict[str, Any]:
    stem = f"{index:02d}-{stage}"
    stdout_path = case_dir / f"{stem}.stdout.txt"
    stderr_path = case_dir / f"{stem}.stderr.txt"
    stdout_path.write_text(result.stdout, encoding="utf-8")
    stderr_path.write_text(result.stderr, encoding="utf-8")
    return {
        "stdout_path": stdout_path.name,
        "stdout_sha256": sha256_file(stdout_path),
        "stderr_path": stderr_path.name,
        "stderr_sha256": sha256_file(stderr_path),
    }


def run_recorded(
    command: list[str],
    timeout: int,
    stage: str,
    case_dir: Path,
    commands: list[dict[str, Any]],
    executor: CommandExecutor,
) -> CompletedCommand:
    rendered = [str(part) for part in command]
    record: dict[str, Any] = {"stage": stage, "command": rendered, "timeout_seconds": timeout}
    commands.append(record)
    try:
        result = executor(rendered, timeout)
    except subprocess.TimeoutExpired as exc:
        record.update({"status": "timeout", "elapsed_output_available": bool(exc.stdout or exc.stderr)})
        raise CommandTimedOut(f"{stage} exceeded {timeout} seconds") from exc
    except KeyboardInterrupt:
        record["status"] = "cancelled"
        raise
    except AepTestError as exc:
        record["status"] = exc.status
        raise
    except FileNotFoundError as exc:
        record["status"] = "missing_dependency"
        raise MissingDependency(f"{stage} executable is unavailable: {rendered[0]}") from exc
    except OSError as exc:
        record["status"] = "io_failed"
        raise IoFailed(f"{stage} could not start: {type(exc).__name__}") from exc
    record.update({"status": "completed", "returncode": result.returncode})
    record.update(_write_command_logs(case_dir, len(commands), stage, result))
    return result


def _last_diagnostic(result: CompletedCommand) -> str:
    lines = [line.strip() for line in result.stderr.splitlines() if line.strip()]
    return lines[-1][:500] if lines else "no diagnostic"


def _metadata(
    video: Path,
    ffprobe: Path,
    timeout: int,
    case_dir: Path,
    commands: list[dict[str, Any]],
    executor: CommandExecutor,
    stage: str,
) -> dict[str, Any]:
    result = run_recorded(
        [
            str(ffprobe),
            "-v", "error",
            "-count_frames",
            "-show_entries",
            (
                "format=duration:stream=codec_type,codec_name,width,height,avg_frame_rate,"
                "nb_frames,nb_read_frames,pix_fmt,color_space,color_transfer,color_primaries,"
                "sample_rate,channels,duration"
            ),
            "-of", "json",
            str(video),
        ],
        timeout,
        stage,
        case_dir,
        commands,
        executor,
    )
    if result.returncode:
        raise MalformedResult(f"{stage} rejected the video: {_last_diagnostic(result)}")
    try:
        data = json.loads(result.stdout)
        video_streams = [stream for stream in data["streams"] if stream.get("codec_type") == "video"]
        if len(video_streams) != 1:
            raise ValueError("expected exactly one video stream")
        stream = video_streams[0]
        fps = Fraction(stream["avg_frame_rate"])
        duration = float(data["format"]["duration"])
        frames_raw = stream.get("nb_read_frames", stream.get("nb_frames"))
        frames = int(frames_raw)
        width = int(stream["width"])
        height = int(stream["height"])
        if width <= 0 or height <= 0 or fps <= 0 or frames <= 0 or not math.isfinite(duration) or duration <= 0:
            raise ValueError("non-positive or non-finite video metadata")
        audio_streams = [
            {
                key: audio[key]
                for key in ("codec_name", "sample_rate", "channels", "duration")
                if key in audio
            }
            for audio in data["streams"]
            if audio.get("codec_type") == "audio"
        ]
    except (KeyError, TypeError, ValueError, ZeroDivisionError, json.JSONDecodeError) as exc:
        raise MalformedResult(f"{stage} returned invalid video metadata: {exc}") from exc
    return {
        "width": width,
        "height": height,
        "fps": fps,
        "duration_seconds": duration,
        "frame_count": frames,
        "pixel_format": stream.get("pix_fmt"),
        "color_space": stream.get("color_space"),
        "color_transfer": stream.get("color_transfer"),
        "color_primaries": stream.get("color_primaries"),
        "audio_streams": audio_streams,
    }


def _verify_video_metadata(
    metadata: dict[str, Any],
    composition: dict[str, Any],
    label: str,
    error_type: type[AepTestError],
) -> None:
    duration = Fraction(composition["duration_numerator"], composition["duration_denominator"])
    expected_frames = composition["expected_frame_count"]
    if (metadata["width"], metadata["height"]) != (composition["width"], composition["height"]):
        raise error_type(f"{label} canvas does not match the pinned composition")
    if metadata["fps"] != SAMPLE_FPS:
        raise error_type(f"{label} is not exactly 30fps")
    if metadata["frame_count"] != expected_frames:
        raise error_type(f"{label} decoded frame count does not match the pinned reference")
    tolerance = float(Fraction(1, SAMPLE_FPS)) / 2
    if not math.isclose(metadata["duration_seconds"], float(duration), rel_tol=0, abs_tol=tolerance):
        raise error_type(f"{label} duration does not match the pinned composition")


def _verify_reference_metadata(metadata: dict[str, Any], composition: dict[str, Any]) -> None:
    _verify_video_metadata(metadata, composition, "reference", IdentityRejected)
    reference = composition["reference"]
    for key in ("pixel_format", "color_space", "color_transfer", "color_primaries"):
        expected = reference.get(key)
        if expected is not None and metadata[key] != expected:
            raise IdentityRejected(f"reference {key} does not match the immutable inventory")
    if metadata["audio_streams"] != reference.get("audio_streams", []):
        raise IdentityRejected("reference audio-stream metadata does not match the immutable inventory")


def _copy_verified_input(source: Path, destination: Path, expected_bytes: int, expected_sha256: str) -> Path:
    """Copy immutable input bytes to owned scratch before invoking external tools."""
    try:
        with source.open("rb") as input_stream, destination.open("xb") as output_stream:
            remaining = expected_bytes + 1
            while remaining > 0:
                chunk = input_stream.read(min(1024 * 1024, remaining))
                if not chunk:
                    break
                output_stream.write(chunk)
                remaining -= len(chunk)
        destination.chmod(0o444)
        if destination.stat().st_size != expected_bytes or sha256_file(destination) != expected_sha256:
            raise IdentityRejected(f"staged input identity differs from {source}")
        return destination
    except OSError as exc:
        raise IoFailed(f"cannot stage immutable input {source}: {exc}") from exc


def _verify_staged_identity(path: Path, expected_bytes: int, expected_sha256: str, label: str) -> None:
    try:
        if path.stat().st_size != expected_bytes or sha256_file(path) != expected_sha256:
            raise IdentityRejected(f"{label} bytes changed while external tools were running")
    except OSError as exc:
        raise IdentityRejected(f"cannot re-verify {label}: {exc}") from exc


def resolve_reference(
    workspace: Path,
    reference: dict[str, Any],
    local_reference: Path | None,
) -> tuple[Path, str]:
    try:
        if local_reference is not None:
            candidate = local_reference
            path = candidate.resolve(strict=True)
            origin = "explicit_local_file"
        else:
            candidate = conversion_workspace(workspace) / reference["path"]
            path = proof.resolve_repo_path(
                conversion_workspace(workspace), reference["path"], must_exist=True
            )
            origin = "committed_local_reference"
        if candidate.is_symlink() or not path.is_file():
            raise IdentityRejected("reference path is not a regular non-symlink file")
        return path, origin
    except (OSError, KeyError, proof.ProofError) as exc:
        raise IoFailed(f"cannot read reference MP4: {exc}") from exc


def _copy_reference_input(source: Path, destination: Path) -> Path:
    try:
        shutil.copyfile(source, destination)
    except OSError as exc:
        raise IoFailed(f"cannot stage reference MP4: {exc}") from exc
    return destination


def _expected_sample_times(composition: dict[str, Any], max_samples: int) -> list[float]:
    frame_count = composition["expected_frame_count"]
    inclusive_count = frame_count + 1
    if inclusive_count > max_samples:
        raise BlockedCase(
            f"inclusive 30fps comparator schedule needs {inclusive_count} entries, exceeding max-samples={max_samples}"
        )
    return [float(Fraction(index, SAMPLE_FPS)) for index in range(inclusive_count)]


def evaluate_comparison(
    comparison: dict[str, Any],
    composition: dict[str, Any],
    critical: list[Fraction],
    max_samples: int,
) -> dict[str, Any]:
    """Validate all samples and report inclusive and half-open summaries separately."""
    expected_times = _expected_sample_times(composition, max_samples)
    try:
        samples = comparison["frame_results"]
        count = comparison["compared_frames"]
        if not isinstance(count, int) or isinstance(count, bool) or count != len(expected_times) or len(samples) != count:
            raise ValueError("comparison does not cover the complete inclusive 30fps schedule")
        raw_scores = [sample["score"]["similarity"] for sample in samples]
        raw_times = [sample["time_secs"] for sample in samples]
        indices = [sample["index"] for sample in samples]
        if any(isinstance(value, bool) for value in (*raw_scores, *raw_times, *indices)):
            raise ValueError("boolean sample values are invalid")
        scores = [float(value) for value in raw_scores]
        times = [float(value) for value in raw_times]
        if indices != list(range(count)):
            raise ValueError("sample indices are not the complete ordered comparator schedule")
        if any(not math.isfinite(score) or not 0 <= score <= 1 for score in scores):
            raise ValueError("sample similarity is non-finite or outside [0,1]")
        if any(
            not math.isfinite(actual) or not math.isclose(actual, expected, rel_tol=0, abs_tol=1e-6)
            for actual, expected in zip(times, expected_times)
        ):
            raise ValueError("sample timestamps differ from the exact 30fps schedule")
        if not math.isclose(scores[-1], scores[-2], rel_tol=0, abs_tol=1e-12):
            raise ValueError("terminal duration probe does not repeat the final native frame")
        inclusive_mean = sum(scores) / count
        inclusive_minimum = min(scores)
        reported_mean = comparison["score"]["similarity"]
        reported_minimum = comparison["min_frame_similarity"]
        if isinstance(reported_mean, bool) or isinstance(reported_minimum, bool):
            raise ValueError("boolean summary values are invalid")
        if not math.isclose(float(reported_mean), inclusive_mean, rel_tol=0, abs_tol=1e-6):
            raise ValueError("reported mean disagrees with frame results")
        if not math.isclose(float(reported_minimum), inclusive_minimum, rel_tol=0, abs_tol=1e-6):
            raise ValueError("reported minimum disagrees with frame results")
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
        raise MalformedResult(f"invalid canonical RGB24 comparison: {exc}") from exc

    unique_count = composition["expected_frame_count"]
    unique_scores = scores[:unique_count]
    inclusive_worst = min(range(count), key=scores.__getitem__)
    unique_worst = min(range(unique_count), key=unique_scores.__getitem__)
    critical_results = []
    for timestamp in critical:
        index = int(timestamp * SAMPLE_FPS)
        critical_results.append(
            {
                "time": str(timestamp),
                "time_seconds": times[index],
                "sample_index": index,
                "similarity": scores[index],
            }
        )
    return {
        "metric": "canonical_rgb24_similarity",
        "resolution": {
            "width": composition["width"],
            "height": composition["height"],
            "max_dimension": max(composition["width"], composition["height"]),
            "downsampling": False,
        },
        "quality_threshold": None,
        "quality_verdict": "not_defined_descriptive_only",
        "comparator_inclusive": {
            "sample_count": count,
            "mean_similarity": inclusive_mean,
            "min_frame_similarity": inclusive_minimum,
            "worst_time_seconds": times[inclusive_worst],
        },
        "half_open_native_frames": {
            "unique_frame_count": unique_count,
            "mean_similarity": sum(unique_scores) / unique_count,
            "min_frame_similarity": min(unique_scores),
            "worst_time_seconds": times[unique_worst],
            "range": f"[0,{composition['duration_numerator']}/{composition['duration_denominator']})",
        },
        "terminal_probe": {
            "included_by_comparator": True,
            "time_seconds": expected_times[-1],
            "sample_index": count - 1,
            "similarity": scores[-1],
            "unique_native_frame": False,
            "note": "The inclusive duration probe repeats the terminal decoded frame and is not a 61st unique frame.",
        },
        "critical_frame_schedule_status": "declared" if critical else "not_declared",
        "critical_frames": critical_results,
        "frame_results": samples,
        "rgb_only": True,
        "alpha_fidelity": "unverified",
        "audio_fidelity": "unverified",
    }


def _command_failure(result: CompletedCommand, stage: str) -> AepTestError:
    """Classify by owned stage, never by unstable human-readable diagnostics."""
    diagnostic = _last_diagnostic(result)
    if stage == "import":
        return ImportFailed(f"{stage} exited {result.returncode}: {diagnostic}")
    if stage == "render":
        return RenderFailed(f"{stage} exited {result.returncode}: {diagnostic}")
    return ComparisonFailed(f"{stage} exited {result.returncode}: {diagnostic}")


def _relative_or_absolute(path: Path, root: Path) -> str:
    try:
        return str(path.relative_to(root))
    except ValueError:
        return str(path)


def _reject_fixture_output_path(workspace: Path, path: Path, label: str) -> None:
    resolved = path.resolve(strict=False)
    for fixture_root in (
        workspace / "crates/aftereffects_file/tests/fixtures",
        workspace / "opensource/conv/crates/aftereffects_file/tests/fixtures",
    ):
        if resolved.is_relative_to(fixture_root.resolve(strict=False)):
            raise BlockedCase(f"{label} must not be inside immutable fixture storage: {resolved}")


def _package_fonts(
    font_files: list[Path] | tuple[Path, ...],
    workspace: Path,
    document: Path,
    tsrct: Path,
    case_dir: Path,
    commands: list[dict[str, Any]],
    identities: list[dict[str, Any]],
    timeout: int,
    executor: CommandExecutor,
) -> None:
    """Provision only explicitly supplied local fonts; never search or substitute."""
    for index, font in enumerate(font_files):
        try:
            source = (workspace / font.expanduser()).resolve()
            size, digest = source.stat().st_size, sha256_file(source)
        except (OSError, ValueError, RuntimeError) as exc:
            raise FontImportFailed(f"cannot read explicit font {font}: {exc}") from exc
        directory = case_dir / "fonts" / str(index)
        directory.mkdir(parents=True)
        staged = _copy_verified_input(source, directory / source.name, size, digest)
        identities.append({
            "path": str(source),
            "staged_path": str(staged.relative_to(case_dir)),
            "bytes": size,
            "sha256": digest,
            "reference_font_identity": "unverified",
        })
        result = run_recorded(
            [str(tsrct), "project", "import-font", "--project", str(document), "--file", str(staged)],
            timeout, "font-import", case_dir, commands, executor,
        )
        _verify_staged_identity(staged, size, digest, "staged font")
        if result.returncode:
            raise FontImportFailed(f"font-import exited {result.returncode}: {_last_diagnostic(result)}")


def score_case(
    workspace: Path,
    case: dict[str, Any],
    target: tuple[dict[str, Any], dict[str, Any]] | None,
    reference_settings: dict[str, Any],
    case_dir: Path,
    tools: dict[str, Path | str],
    cache: Path,
    local_reference: Path | None = None,
    *,
    executor: CommandExecutor = default_executor,
    font_files: list[Path] | tuple[Path, ...] = (),
    import_timeout: int = 300,
    render_timeout: int = 900,
    compare_timeout: int = 900,
    metadata_timeout: int = 60,
    max_samples: int = 601,
) -> dict[str, Any]:
    """Run one fresh AEP import/render/compare job in an owned scratch directory."""
    started_at = utc_now()
    _reject_fixture_output_path(workspace, case_dir, "case output")
    _reject_fixture_output_path(workspace, cache, "reference cache")
    case_dir.mkdir(parents=True, exist_ok=False)
    commands: list[dict[str, Any]] = []
    base: dict[str, Any] = {
        "case_id": case["case_id"],
        "direction": "import",
        "feature": case["feature"],
        "started_at": started_at,
        "commands": commands,
        "font_inputs": [],
        "declared_cpu_tests": case["tests"],
        "cpu_test_execution": {
            "run_by_this_command": False,
            "registry_historical_status": case["execution"]["status"],
            "note": "Visual scoring does not execute or rewrite the fixture registry's CPU-test status.",
        },
    }
    try:
        source_path, source, composition, critical = validate_target_identity(
            workspace, case, target, reference_settings
        )
        try:
            verified_expression_samples = proof.verify_expression_samples(
                conversion_workspace(workspace),
                case,
                source["source_sha256"],
                composition["composition_id"],
            )
        except proof.ProofError as exc:
            raise IdentityRejected(str(exc)) from exc
        expected_times = _expected_sample_times(composition, max_samples)
        if len(expected_times) < 3:
            raise BlockedCase("comparison schedule does not contain enough samples")
        staged_source = _copy_verified_input(
            source_path,
            case_dir / "source.aep",
            source["source_bytes"],
            source["source_sha256"],
        )
        staged_expression_samples: Path | None = None
        expression_samples_identity: dict[str, Any] | None = None
        expression_samples_metadata: dict[str, Any] | None = None
        if verified_expression_samples is not None:
            expression_samples_path, expression_samples_identity = verified_expression_samples
            staged_expression_samples = _copy_verified_input(
                expression_samples_path,
                case_dir / "expression_samples.json",
                expression_samples_identity["bytes"],
                expression_samples_identity["sha256"],
            )
            expression_samples_metadata = {
                "source": {
                    "path": expression_samples_identity["path"],
                    "bytes": expression_samples_identity["bytes"],
                    "sha256": expression_samples_identity["sha256"],
                },
                "staged": {
                    "path": staged_expression_samples.name,
                    "bytes": staged_expression_samples.stat().st_size,
                    "sha256": sha256_file(staged_expression_samples),
                },
                "bound_aep_source_sha256": expression_samples_identity[
                    "embedded_source_sha256"
                ],
                "capture_scope": expression_samples_identity["capture_scope"],
            }
        resolved_tools = {name: _tool_path(value, workspace) for name, value in tools.items()}
        reference_path, reference_origin = resolve_reference(
            workspace, composition["reference"], local_reference
        )
        staged_reference = _copy_reference_input(reference_path, case_dir / "reference.mp4")
        base.update(
            {
                "source": {
                    "path": source["source_path"],
                    "bytes": source["source_bytes"],
                    "sha256": source["source_sha256"],
                    "staged_path": staged_source.name,
                    "composition": {
                        "id": composition["composition_id"],
                        "name": composition.get("composition_name"),
                        "width": composition["width"],
                        "height": composition["height"],
                        "native_fps": (
                            f"{composition['fps_numerator']}/{composition['fps_denominator']}"
                        ),
                        "duration": (
                            f"{composition['duration_numerator']}/{composition['duration_denominator']}"
                        ),
                        "native_frame_count": composition["frame_count"],
                        "display_start": (
                            f"{composition['display_start_numerator']}/"
                            f"{composition['display_start_denominator']}"
                        ),
                        "pixel_aspect": (
                            f"{composition['pixel_aspect_numerator']}/"
                            f"{composition['pixel_aspect_denominator']}"
                        ),
                        "output_fps": SAMPLE_FPS,
                        "expected_output_frame_count": composition["expected_frame_count"],
                    },
                },
                "reference": {
                    "path": composition["reference"]["path"],
                    "origin": reference_origin,
                    "local_path": str(reference_path),
                    "staged_path": staged_reference.name,
                    "inventory_metadata": {
                        key: composition["reference"].get(key)
                        for key in (
                            "fps",
                            "frame_count",
                            "duration_seconds",
                            "width",
                            "height",
                            "pixel_format",
                            "color_space",
                            "color_transfer",
                            "color_primaries",
                            "audio_streams",
                        )
                    },
                    "producer": {
                        **reference_settings,
                        "render_log_sha256": composition["reference"].get("render_log_sha256"),
                    },
                },
                "tool_identity": {name: tool_identity(path) for name, path in resolved_tools.items()},
            }
        )
        if expression_samples_metadata is not None:
            base["expression_samples"] = expression_samples_metadata

        try:
            reference_metadata = _metadata(
                staged_reference,
                resolved_tools["ffprobe"],
                metadata_timeout,
                case_dir,
                commands,
                executor,
                "reference-metadata",
            )
        except MalformedResult as exc:
            raise IdentityRejected(str(exc)) from exc
        _verify_reference_metadata(reference_metadata, composition)

        conversion_dir = case_dir / "conversion"
        import_command = [
            str(resolved_tools["tesseract_conv"]),
            "convert",
            str(staged_source),
            "--to", "tesseract",
            "--composition", str(composition["composition_id"]),
            "--output", str(conversion_dir),
        ]
        if staged_expression_samples is not None:
            import_command.extend(["--expression-samples", str(staged_expression_samples)])
        import_result = run_recorded(
            import_command,
            import_timeout,
            "import",
            case_dir,
            commands,
            executor,
        )
        _verify_staged_identity(
            staged_source,
            source["source_bytes"],
            source["source_sha256"],
            "staged source",
        )
        if staged_expression_samples is not None and expression_samples_identity is not None:
            _verify_staged_identity(
                staged_expression_samples,
                expression_samples_identity["bytes"],
                expression_samples_identity["sha256"],
                "staged expression samples",
            )
        if import_result.returncode:
            raise _command_failure(import_result, "import")
        documents = list(conversion_dir.glob("*.tsrct")) if conversion_dir.is_dir() else []
        if len(documents) != 1 or documents[0].name != "project.tsrct":
            raise ImportFailed("fresh import did not produce exactly conversion/project.tsrct")
        document = documents[0]
        warnings = [
            {"stage": "import", "message": line.strip()}
            for line in import_result.stderr.splitlines()
            if line.strip().startswith("warning:")
        ]

        _package_fonts(
            font_files, workspace, document, resolved_tools["tsrct"], case_dir,
            commands, base["font_inputs"], import_timeout, executor,
        )

        actual = case_dir / "tesseract.mp4"
        render_result = run_recorded(
            [
                str(resolved_tools["tsrct"]),
                "export",
                "--project", str(document),
                "--output", str(actual),
                "--fps", str(SAMPLE_FPS),
            ],
            render_timeout,
            "render",
            case_dir,
            commands,
            executor,
        )
        if render_result.returncode:
            raise _command_failure(render_result, "render")
        warnings.extend(
            {"stage": "render", "message": line.strip()}
            for line in render_result.stderr.splitlines()
            if line.strip().startswith("warning:")
        )
        if not actual.is_file():
            raise RenderFailed("renderer exited successfully without producing tesseract.mp4")

        try:
            actual_metadata = _metadata(
                actual,
                resolved_tools["ffprobe"],
                metadata_timeout,
                case_dir,
                commands,
                executor,
                "actual-metadata",
            )
        except MalformedResult as exc:
            raise RenderFailed(str(exc)) from exc
        _verify_video_metadata(actual_metadata, composition, "rendered output", RenderFailed)

        comparison_path = case_dir / "comparison.json"
        comparison_result = run_recorded(
            [
                str(resolved_tools["validation"]),
                "video",
                "--left", str(staged_reference),
                "--right", str(actual),
                "--sample-interval-secs", SAMPLE_INTERVAL_CLI,
                "--max-dimension", str(max(composition["width"], composition["height"])),
                "--max-samples", str(max_samples),
                "--json",
                "--canonical-rgb24",
            ],
            compare_timeout,
            "comparison",
            case_dir,
            commands,
            executor,
        )
        if comparison_result.returncode:
            raise _command_failure(comparison_result, "comparison")
        try:
            comparison = json.loads(comparison_result.stdout)
        except json.JSONDecodeError as exc:
            raise MalformedResult(f"comparison returned invalid JSON: {exc}") from exc
        if not isinstance(comparison, dict):
            raise MalformedResult("comparison returned a non-object JSON value")
        atomic_json(comparison_path, comparison)
        score = evaluate_comparison(comparison, composition, critical, max_samples)

        base.update(
            {
                "status": "scored",
                "status_meaning": "comparison_completed_descriptive_not_a_quality_pass",
                "warnings": warnings,
                "metadata": {
                    "reference": {**reference_metadata, "fps": str(reference_metadata["fps"])},
                    "actual": {**actual_metadata, "fps": str(actual_metadata["fps"])},
                },
                "outputs": {
                    "project_tsrct": {
                        "path": _relative_or_absolute(document, case_dir),
                        "bytes": document.stat().st_size,
                        "sha256": sha256_file(document),
                    },
                    "tesseract_mp4": {
                        "path": _relative_or_absolute(actual, case_dir),
                        "bytes": actual.stat().st_size,
                        "sha256": sha256_file(actual),
                    },
                    "comparison_json": {
                        "path": comparison_path.name,
                        "bytes": comparison_path.stat().st_size,
                        "sha256": sha256_file(comparison_path),
                    },
                },
                "comparison": score,
                "limitations": [
                    "Descriptive RGB similarity has no quality threshold and is not a fidelity pass.",
                    "RGB24 comparison does not verify alpha or audio.",
                    "Import scoring does not establish FX-to-AEP export behavior or proof.",
                    "Explicit font hashes identify local inputs, not the Adobe reference's font versions.",
                ],
            }
        )
    except KeyboardInterrupt:
        base.update({"status": "cancelled", "reason": "cancelled by operator"})
    except AepTestError as exc:
        base.update({"status": exc.status, "reason": str(exc)})
    except (OSError, ValueError) as exc:
        base.update({"status": "io_failed", "reason": f"{type(exc).__name__}: {exc}"})
    base["finished_at"] = utc_now()
    atomic_json(case_dir / "result.json", base)
    return base


def _git_identity(workspace: Path, executor: CommandExecutor) -> dict[str, Any]:
    try:
        head = executor(["git", "-C", str(workspace), "rev-parse", "HEAD"], 10)
        if head.returncode == 0 and len(head.stdout.strip()) == 40:
            return {"commit": head.stdout.strip()}
    except (OSError, subprocess.TimeoutExpired):
        pass
    return {"commit": "unavailable"}


def run_selected(
    workspace: Path,
    selected: list[dict[str, Any]],
    targets: dict[tuple[str, int], tuple[dict[str, Any], dict[str, Any]]],
    reference_settings: dict[str, Any],
    run_dir: Path,
    tools: dict[str, Path | str],
    cache: Path,
    local_references: dict[str, Path],
    *,
    executor: CommandExecutor = default_executor,
    font_files: list[Path] | tuple[Path, ...] = (),
    timeouts: dict[str, int] | None = None,
    max_samples: int = 601,
) -> tuple[dict[str, Any], int]:
    _reject_fixture_output_path(workspace, run_dir, "run output")
    _reject_fixture_output_path(workspace, cache, "reference cache")
    run_dir.mkdir(parents=True, exist_ok=False)
    report_path = run_dir / "report.json"
    report: dict[str, Any] = {
        "schema_version": 1,
        "runner": "local_non_ci_aep_import_rgb24",
        "started_at": utc_now(),
        "workspace": str(workspace),
        "producer_code": {
            **_git_identity(workspace, executor),
            "runner_path": str(Path(__file__).resolve()),
            "runner_sha256": sha256_file(Path(__file__).resolve()),
        },
        "selected_case_ids": [case["case_id"] for case in selected],
        "sequential": True,
        "quality_threshold": None,
        "cases": [],
    }
    atomic_json(report_path, report)
    configured = {"import": 300, "render": 900, "compare": 900, "metadata": 60}
    if timeouts:
        configured.update(timeouts)
    cancelled = False
    for case in selected:
        if cancelled:
            result = {
                "case_id": case["case_id"],
                "direction": "import",
                "status": "cancelled",
                "reason": "run cancelled before this sequential case started",
                "started_at": None,
                "finished_at": utc_now(),
                "commands": [],
            }
        else:
            result = score_case(
                workspace,
                case,
                targets.get((case["source_path"], case["composition_id"])),
                reference_settings,
                run_dir / case["case_id"],
                tools,
                cache,
                local_references.get(case["case_id"]),
                executor=executor,
                font_files=font_files,
                import_timeout=configured["import"],
                render_timeout=configured["render"],
                compare_timeout=configured["compare"],
                metadata_timeout=configured["metadata"],
                max_samples=max_samples,
            )
            cancelled = result["status"] == "cancelled"
        report["cases"].append(result)
        atomic_json(report_path, report)
    report["finished_at"] = utc_now()
    report["counts"] = {
        status: sum(case["status"] == status for case in report["cases"])
        for status in sorted({case["status"] for case in report["cases"]})
    }
    atomic_json(report_path, report)
    if cancelled:
        return report, 130
    return report, int(any(case["status"] != "scored" for case in report["cases"]))


def _parse_local_references(values: list[str], selected_ids: set[str]) -> dict[str, Path]:
    references: dict[str, Path] = {}
    for value in values:
        case_id, separator, path = value.partition("=")
        if not separator or not case_id or not path:
            raise BlockedCase("--reference must use CASE_ID=/path/to/reference.mp4")
        if case_id not in selected_ids:
            raise BlockedCase(f"local reference names an unselected case: {case_id}")
        if case_id in references:
            raise BlockedCase(f"duplicate local reference for case: {case_id}")
        references[case_id] = Path(path)
    return references


def _new_run_dir(scratch: Path) -> Path:
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    candidate = scratch / f"run-{stamp}-{os.getpid()}"
    suffix = 1
    while candidate.exists():
        candidate = scratch / f"run-{stamp}-{os.getpid()}-{suffix}"
        suffix += 1
    return candidate


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, default=REPO)
    parser.add_argument("--registry", type=Path, default=DEFAULT_REGISTRY)
    parser.add_argument("--references", type=Path, default=DEFAULT_REFERENCES)
    subparsers = parser.add_subparsers(dest="command", required=True)

    listing = subparsers.add_parser("list", help="list validated import cases without executing them")
    listing.add_argument("--case-id", action="append", default=[])
    listing.add_argument("--json", action="store_true")

    run = subparsers.add_parser("run", help="score only explicitly selected case IDs, sequentially")
    run.add_argument("--case-id", action="append", required=True)
    run.add_argument("--tsrct-conv", "--tesseract-conv", dest="tesseract_conv", type=Path, default=REPO / "target/debug/tsrct-conv")
    run.add_argument("--tsrct", type=Path, required=True, help="matching source-built tsrct executable")
    run.add_argument("--validation", type=Path, default=REPO.parents[1] / "target/debug/validation_cli")
    run.add_argument("--ffprobe", default="ffprobe")
    run.add_argument("--cache-dir", type=Path, default=DEFAULT_CACHE)
    run.add_argument("--scratch-dir", type=Path, default=DEFAULT_SCRATCH)
    run.add_argument("--reference", action="append", default=[], metavar="CASE_ID=MP4")
    run.add_argument(
        "--font", type=Path, action="append", default=[], metavar="FILE",
        help="embed an explicitly supplied local font in each scratch project (repeatable; license permitting)",
    )
    run.add_argument("--import-timeout", type=int, default=300)
    run.add_argument("--render-timeout", type=int, default=900)
    run.add_argument("--compare-timeout", type=int, default=900)
    run.add_argument("--metadata-timeout", type=int, default=60)
    run.add_argument("--max-samples", type=int, default=601)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    workspace = args.workspace.resolve()
    try:
        cases, targets, references = load_catalog(workspace, args.registry, args.references)
        if args.command == "list":
            selected = select_cases(cases, args.case_id) if args.case_id else list(cases.values())
            rows = [
                {
                    "case_id": case["case_id"],
                    "feature": case["feature"],
                    "source_path": case["source_path"],
                    "composition_id": case["composition_id"],
                    "critical_frame_count": len(case["critical_frames"]),
                    "registry_execution_status": case["execution"]["status"],
                    "live_result": "not_run",
                }
                for case in selected
            ]
            if args.json:
                print(json.dumps({"cases": rows}, indent=2))
            else:
                for row in rows:
                    print(
                        f"{row['case_id']}\tcomp={row['composition_id']}\t"
                        f"critical={row['critical_frame_count']}\tregistry={row['registry_execution_status']}\t"
                        f"{row['feature']}"
                    )
            return 0

        selected = select_cases(cases, args.case_id)
        local_references = _parse_local_references(args.reference, {case["case_id"] for case in selected})
        for name in ("import_timeout", "render_timeout", "compare_timeout", "metadata_timeout", "max_samples"):
            if getattr(args, name) <= 0:
                raise BlockedCase(f"--{name.replace('_', '-')} must be positive")
        run_dir = _new_run_dir(args.scratch_dir.resolve())
        report, code = run_selected(
            workspace,
            selected,
            targets,
            references["reference_settings"],
            run_dir,
            {
                "tesseract_conv": args.tesseract_conv,
                "tsrct": args.tsrct,
                "validation": args.validation,
                "ffprobe": args.ffprobe,
            },
            args.cache_dir,
            local_references,
            font_files=args.font,
            timeouts={
                "import": args.import_timeout,
                "render": args.render_timeout,
                "compare": args.compare_timeout,
                "metadata": args.metadata_timeout,
            },
            max_samples=args.max_samples,
        )
        print(json.dumps({"report": str(run_dir / "report.json"), "counts": report["counts"]}, indent=2))
        return code
    except (AepTestError, proof.ProofError) as exc:
        parser.error(str(exc))
    return 2


if __name__ == "__main__":
    sys.exit(main())
