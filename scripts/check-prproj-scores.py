#!/usr/bin/env python3
"""Independently score pinned Premiere MP4s against converted Tesseract exports.

``--references`` is the strict regression gate for checked-in MP4s. Native
AME reference generation is separate and is never performed by this runner.
"""

import argparse
import contextlib
from fractions import Fraction
import importlib.util
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
CACHE = Path.home() / ".cache/jerboa/conversion"


def local_module(name, filename):
    spec = importlib.util.spec_from_file_location(name, HERE / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


sys.path.insert(0, str(HERE))
fixtures = local_module("conversion_score_fixtures", "conversion-fixtures.py")


class ScoreError(RuntimeError):
    pass


class UnsupportedConversion(ScoreError):
    """The selected Premiere sequence exceeds the converter's supported subset."""


def run(args, timeout, *, env=None):
    try:
        result = subprocess.run([str(arg) for arg in args], capture_output=True, text=True,
                                timeout=timeout, check=False, env=env)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise ScoreError(f"{Path(args[0]).name} failed to complete ({type(exc).__name__})") from exc
    if result.returncode:
        # Convert/encode tools can print local paths; never print signed URLs.
        message = result.stderr.strip().splitlines()[-1][:400] if result.stderr.strip() else "no diagnostic"
        raise ScoreError(f"{Path(args[0]).name} exited {result.returncode}: {message}")
    return result.stdout


def metadata(path):
    raw = run(["ffprobe", "-v", "error", "-show_entries",
               "format=duration:stream=codec_type,width,height,avg_frame_rate",
               "-of", "json", path], timeout=60)
    try:
        info = json.loads(raw)
        streams = [item for item in info["streams"] if item["codec_type"] == "video"]
        if len(streams) != 1:
            raise ValueError("expected exactly one video stream")
        video = streams[0]
        fps = Fraction(video["avg_frame_rate"])
        duration = float(info["format"]["duration"])
        if (video["width"] <= 0 or video["height"] <= 0 or fps <= 0 or
                not math.isfinite(duration) or duration <= 0):
            raise ValueError("invalid dimensions, frame rate, or duration")
        return {"width": video["width"], "height": video["height"],
                "fps": fps, "duration_secs": duration}
    except (KeyError, TypeError, ValueError, ZeroDivisionError) as exc:
        raise ScoreError(f"{path.name}: invalid ffprobe video metadata: {exc}") from exc


def check_alignment(reference, actual):
    if (reference["width"], reference["height"]) != (actual["width"], actual["height"]):
        raise ScoreError("Premiere and Tesseract have different video dimensions")
    if reference["fps"] != actual["fps"]:
        raise ScoreError("Premiere and Tesseract have different frame rates")
    # validation::compare_videos truncates to the shorter movie. Guard this
    # separately so a missing end segment cannot earn a high similarity score.
    tolerance = float(1 / reference["fps"]) + 0.001
    if abs(reference["duration_secs"] - actual["duration_secs"]) > tolerance:
        raise ScoreError("Premiere and Tesseract durations differ by more than one frame")


def export_fps(reference):
    """Render at the reference rate. tsrct export supports only whole 24, 30, and 60 fps."""
    fps = reference["fps"]
    if fps.denominator != 1 or fps.numerator not in (24, 30, 60):
        raise ScoreError(f"tsrct export cannot render the {fps} fps Premiere reference rate")
    return fps.numerator


def expected_sample_times(duration_secs, interval_secs, max_samples):
    # Mirror validation::video::sample_times: multiplication is compared with
    # duration, rather than rounding duration / interval at a floating boundary.
    times = []
    for index in range(max_samples + 1):
        time = index * interval_secs
        if time > duration_secs:
            return times
        times.append(time)
    raise ScoreError(f"more than max-samples={max_samples} frames; do not silently subsample")


def evaluate(comparison, policy=None, *, expected_times=None):
    try:
        samples = comparison["frame_results"]
        count = comparison["compared_frames"]
        if not isinstance(count, int) or isinstance(count, bool) or count < 2 or len(samples) != count:
            raise ValueError("invalid frame count")
        scores = [float(frame["score"]["similarity"]) for frame in samples]
        times = [float(frame["time_secs"]) for frame in samples]
        if (any(not math.isfinite(value) or not 0 <= value <= 1 for value in scores) or
                any(not math.isfinite(value) or value < 0 for value in times)):
            raise ValueError("invalid frame score or timestamp")
        if expected_times is not None:
            if count != len(expected_times):
                raise ValueError("comparison does not cover the complete sampling schedule")
            if any(not math.isclose(actual, expected, rel_tol=0, abs_tol=1e-6)
                   for actual, expected in zip(times, expected_times)):
                raise ValueError("comparison timestamps differ from the sampling schedule")
        mean = sum(scores) / count
        minimum = min(scores)
        reported_mean = float(comparison["score"]["similarity"])
        reported_min = float(comparison["min_frame_similarity"])
        if (not math.isclose(mean, reported_mean, rel_tol=0, abs_tol=1e-6) or
                not math.isclose(minimum, reported_min, rel_tol=0, abs_tol=1e-6)):
            raise ValueError("summary disagrees with decoded frame results")
        if policy and (mean < policy["min_mean_similarity"] or
                       minimum < policy["min_frame_similarity"]):
            worst = scores.index(minimum)
            raise ScoreError(
                "score below configured mean or worst-frame similarity threshold: "
                f"mean={mean:.6f}, min={minimum:.6f} at {times[worst]:.3f}s"
            )
        worst = scores.index(minimum)
        return {"mean_similarity": mean, "min_frame_similarity": minimum,
                "worst_time_secs": times[worst], "sampled_frames": count,
                "frame_results": samples}
    except (KeyError, TypeError, ValueError) as exc:
        raise ScoreError(f"invalid video comparison result: {exc}") from exc


def score_case(case, cache, binary, validation, tsrct, *, max_samples=600,
               reference_file=None, artifacts_dir=None, ffmpeg=None):
    video = case.get("reference_video")
    if video is None:
        return {"case_id": case["id"], "status": "not_evaluated",
                "reason": "no pinned reference_video"}
    if not case.get("sequence"):
        return {"case_id": case["id"], "status": "failed", "reason": "no pinned sequence UID"}
    policy = case.get("score_policy")
    interval = policy["sample_interval_secs"] if policy else 1.0
    try:
        output = cache / "score-jobs"
        output.mkdir(parents=True, exist_ok=True)
        reference = REPO / video["repo_path"] if reference_file is None else Path(reference_file)
        if not reference.is_file() or reference.is_symlink():
            raise ScoreError(f"missing or unsafe local reference: {reference}")
        if artifacts_dir is not None:
            artifacts_dir = Path(artifacts_dir)
            artifacts_dir.mkdir(parents=True, exist_ok=True)
            for name in ("tesseract.mp4", "comparison.json"):
                (artifacts_dir / name).unlink(missing_ok=True)
        stage = cache / "cases"
        stage.mkdir(parents=True, exist_ok=True)
        with contextlib.redirect_stdout(sys.stderr):
            fixtures.stage_case(case, stage / case["id"], reuse=True)
        with tempfile.TemporaryDirectory(prefix=f"{case['id']}-", dir=output) as temporary:
            work = Path(temporary)
            conversion = work / "conversion"
            try:
                run([binary, "convert", stage / case["id"] / case["project"],
                     "--to", "tesseract", "--sequence", case["sequence"],
                     "--output", conversion], 300)
            except ScoreError as exc:
                if ("invalid Premiere XML shape:" in str(exc) or
                        "unsupported conversion:" in str(exc)):
                    raise UnsupportedConversion(str(exc)) from exc
                raise
            documents = list(conversion.glob("*.tsrct"))
            if len(documents) != 1:
                raise ScoreError("expected exactly one converted .tsrct from the pinned sequence")
            # Fonts are pinned input bytes, not ambient OS or renderer defaults.
            for file in case["files"]:
                font = stage / case["id"] / file["path"]
                if font.suffix.lower() in {".ttf", ".otf", ".ttc"}:
                    run([tsrct, "project", "import-font", "--project", documents[0],
                         "--file", font], 300)
            actual = work / "tesseract.mp4"
            expected_meta = metadata(reference)
            encoder = ([] if ffmpeg is None else
                       ["--encoder-backend", "external-ffmpeg-command", "--ffmpeg-path", ffmpeg])
            # Pin decode as well as encode: M5 VM VideoToolbox changes chroma
            # before rendering, so a software encoder alone is not enough.
            run([tsrct, "export", "--project", documents[0], "--output", actual,
                 "--fps", export_fps(expected_meta), *encoder], 900,
                env={**os.environ, "JERBOA_FORCE_SOFTWARE_DECODE": "1"})
            if artifacts_dir is not None:
                shutil.copyfile(actual, artifacts_dir / "tesseract.mp4")
            check_alignment(expected_meta, metadata(actual))
            expected_times = expected_sample_times(expected_meta["duration_secs"], interval,
                                                   max_samples)
            if len(expected_times) < 2:
                raise ScoreError("video duration does not cover two scheduled samples")
            validation_args = [validation, "video", "--left", reference,
                               "--right", actual, "--sample-interval-secs", interval,
                               "--max-dimension", 1280, "--json", "--canonical-rgb24"]
            comparison = json.loads(run(validation_args, 900))
            if artifacts_dir is not None:
                (artifacts_dir / "comparison.json").write_text(json.dumps(comparison, indent=2) + "\n")
            score = evaluate(comparison, policy, expected_times=expected_times)
            return {"case_id": case["id"],
                    "status": "scored" if policy is not None else "comparison_only",
                    "sequence_uid": case["sequence"],
                    "reference_path": video["repo_path"],
                    "duration_secs": expected_meta["duration_secs"],
                    **score}
    except UnsupportedConversion as exc:
        return {"case_id": case["id"], "status": "unsupported", "reason": str(exc)}
    except (ScoreError, fixtures.FixtureError, OSError,
            ValueError, subprocess.SubprocessError) as exc:
        # A renderer or scoring failure must not be misclassified as
        # a converter feature limitation, regardless of its diagnostic text.
        return {"case_id": case["id"], "status": "failed", "reason": str(exc)}


def reference_gate_cases(data):
    return [case for case in data["cases"]
            if case["proof"] == "video_reference" and "score_policy" in case]


def summarize(results, *, strict, allow_unscored=False, allow_unsupported=False):
    if not results or any(item["status"] == "failed" for item in results):
        return 1
    if strict:
        return int(any(item["status"] != "scored" for item in results))
    if any(item["status"] == "unsupported" for item in results) and not allow_unsupported:
        return 1
    if any(item["status"] != "scored" for item in results) and not allow_unscored:
        return 1
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--case")
    group.add_argument("--references", action="store_true",
                       help="strictly score every video_reference case with a score policy")
    parser.add_argument("--allow-unscored", action="store_true",
                        help="diagnostic mode: permit cases without a score")
    parser.add_argument("--allow-unsupported", action="store_true",
                        help="diagnostic mode: report unsupported as a non-score")
    parser.add_argument("--manifest", type=Path, default=fixtures.DEFAULT_MANIFEST)
    parser.add_argument("--cache-dir", type=Path, default=CACHE)
    parser.add_argument("--binary", type=Path, default=REPO / "target/debug/tsrct-conv")
    parser.add_argument("--validation", type=Path, default=REPO.parents[1] / "target/debug/validation_cli")
    parser.add_argument("--tsrct", type=Path, default=REPO.parents[1] / "target/debug/tsrct")
    parser.add_argument("--ffmpeg", type=Path, default=shutil.which("ffmpeg"))
    parser.add_argument("--max-samples", type=int, default=600)
    local = parser.add_mutually_exclusive_group()
    local.add_argument("--reference-file", type=Path,
                       help="local override for one reference video")
    local.add_argument("--reference-dir", type=Path,
                       help="local overrides for every strict reference")
    parser.add_argument("--artifacts-dir", type=Path,
                        help="keep each rendered MP4 and comparison JSON for CI diagnostics")
    args = parser.parse_args()
    if args.reference_file is not None and not args.case:
        parser.error("--reference-file requires one explicit --case")
    if args.reference_dir is not None and not args.references:
        parser.error("--reference-dir requires --references")
    if args.ffmpeg is None:
        parser.error("no ffmpeg on PATH; pass --ffmpeg")
    if args.max_samples < 2:
        parser.error("--max-samples must be at least 2")
    if args.allow_unsupported and not args.allow_unscored:
        parser.error("--allow-unsupported requires explicit --allow-unscored diagnostic mode")
    if args.references and (args.allow_unscored or args.allow_unsupported):
        parser.error("--references is strict and does not allow unscored or unsupported cases")
    data = fixtures.load_manifest(args.manifest)
    if args.references:
        selected = reference_gate_cases(data)
    else:
        selected = [case for case in data["cases"] if case["id"] == args.case]
        if not selected:
            parser.error(f"unknown case: {args.case}")
    if args.reference_file is not None and "reference_video" not in selected[0]:
        parser.error("local reference requires a case with reference_video")
    if args.reference_dir is not None:
        on_disk = {path.stem for path in args.reference_dir.glob("*.mp4")}
        reference_ids = {case["id"] for case in selected}
        if on_disk != reference_ids:
            parser.error(f"strict references and local MP4s differ: "
                         f"missing={sorted(reference_ids - on_disk)}, "
                         f"unexpected={sorted(on_disk - reference_ids)}")
    results = [score_case(
        case, args.cache_dir, args.binary, args.validation, args.tsrct,
        max_samples=args.max_samples, ffmpeg=args.ffmpeg,
        reference_file=(args.reference_dir / f"{case['id']}.mp4"
                        if args.reference_dir is not None else args.reference_file),
        artifacts_dir=(args.artifacts_dir / case["id"]
                       if args.artifacts_dir is not None else None),
    ) for case in selected]
    print(json.dumps({"mode": "strict" if args.references else "diagnostic",
                      "scored": sum(item["status"] == "scored" for item in results),
                      "cases": results}, indent=2))
    return summarize(results, strict=args.references,
                     allow_unscored=args.allow_unscored,
                     allow_unsupported=args.allow_unsupported)


if __name__ == "__main__":
    sys.exit(main())
