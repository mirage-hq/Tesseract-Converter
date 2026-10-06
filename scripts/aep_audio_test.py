"""Strict, channel-preserving audio comparison for manually supplied AEP proof artifacts.

No conversion, Adobe render, asset download or alignment is performed here.
"""

from __future__ import annotations

import array
import hashlib
import json
import math
import subprocess
import sys
import tempfile
from decimal import Decimal, InvalidOperation
from pathlib import Path


class AudioTestError(ValueError):
    pass


MAX_SECONDS = 30
MAX_CHANNELS = 2
MAX_RATE = 48000
WINDOW_SECONDS = 0.01


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def require_hash(path: Path, expected: str, label: str) -> None:
    if not path.is_file() or not isinstance(expected, str) or len(expected) != 64:
        raise AudioTestError(f"{label}: missing file or pinned SHA-256")
    try:
        int(expected, 16)
    except ValueError as exc:
        raise AudioTestError(f"{label}: invalid SHA-256") from exc
    if sha256(path) != expected.lower():
        raise AudioTestError(f"{label}: SHA-256 mismatch")


def _run(command: list[str], timeout: int = 90) -> bytes:
    try:
        result = subprocess.run(command, capture_output=True, timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise AudioTestError(f"decoder/probe unavailable or timed out: {exc}") from exc
    if result.returncode:
        raise AudioTestError(f"decoder/probe failed: {result.stderr.decode('utf-8', 'replace')[-1000:]}")
    return result.stdout


def decode(path: Path) -> tuple[int, int, array.array]:
    if not path.is_file():
        raise AudioTestError(f"missing audio artifact: {path}")
    info = json.loads(_run(["ffprobe", "-v", "error", "-show_entries",
                            "stream=codec_type,codec_name,sample_rate,channels,duration,start_time:format=duration,start_time,format_name",
                            "-of", "json", str(path)]))
    streams = [stream for stream in info.get("streams", []) if stream.get("codec_type") == "audio"]
    if len(streams) != 1:
        raise AudioTestError(f"expected exactly one audio stream, got {len(streams)}: {path}")
    stream = streams[0]
    container = info.get("format", {})
    # Decoded PCM does not retain presentation timestamps. Require a zero-origin
    # artifact rather than silently aligning audio delayed in its container.
    # Raw WAV may omit start_time; timestamped containers must declare it.
    wav = container.get("format_name") == "wav"
    for label, metadata in (("audio stream", stream), ("container", container)):
        origin = metadata.get("start_time")
        if origin in (None, "N/A") and wav:
            continue
        try:
            time = Decimal(str(origin))
        except InvalidOperation as exc:
            raise AudioTestError(f"missing or invalid {label} start_time: {path}") from exc
        if not time.is_finite() or time != 0:
            raise AudioTestError(f"nonzero or invalid {label} start_time: {path}")
    try:
        rate = int(stream["sample_rate"])
        channels = int(stream["channels"])
        duration = float(stream.get("duration") or info["format"]["duration"])
    except (KeyError, TypeError, ValueError) as exc:
        raise AudioTestError(f"missing or invalid sample rate/channels/duration: {path}") from exc
    if not (1 <= rate <= MAX_RATE and 1 <= channels <= MAX_CHANNELS
            and math.isfinite(duration) and 0 < duration <= MAX_SECONDS):
        raise AudioTestError(f"unsupported or excessive stream metadata: {path}")
    # Do not request -ar/-ac, filters, loudness normalization or sample alignment.
    # Decode 1 second beyond the limit so truncated long streams cannot pass.
    with tempfile.TemporaryFile() as output:
        try:
            result = subprocess.run(["ffmpeg", "-v", "error", "-nostdin", "-i", str(path),
                                     "-map", "0:a:0", "-vn", "-sn", "-dn", "-c:a", "pcm_f32le",
                                     "-t", str(MAX_SECONDS + 1), "-f", "f32le", "pipe:1"],
                                    stdout=output, stderr=subprocess.PIPE, timeout=90, check=False)
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise AudioTestError(f"decoder unavailable or timed out: {exc}") from exc
        if result.returncode:
            raise AudioTestError(f"decode failed: {result.stderr.decode('utf-8', 'replace')[-1000:]}")
        size = output.tell()
        if not size or size > (MAX_SECONDS * rate + 1024) * channels * 4:
            raise AudioTestError(f"empty or excessive decoded audio: {path}")
        output.seek(0)
        raw = output.read()
    if len(raw) % (4 * channels):
        raise AudioTestError(f"partial decoded sample frame: {path}")
    samples = array.array("f")
    samples.frombytes(raw)
    if sys.byteorder != "little":
        samples.byteswap()
    if any(not math.isfinite(sample) for sample in samples):
        raise AudioTestError(f"nonfinite decoded audio: {path}")
    # AAC decoders can expose the final padded packet beyond the MP4 stream's
    # declared presentation end. Remove only that bounded, non-presented tail;
    # never shift samples, normalize gain, or trim to the comparison policy.
    declared_frames = round(duration * rate)
    excess = len(samples) // channels - declared_frames
    if stream.get("codec_name") == "aac" and 0 < excess <= 1024:
        del samples[declared_frames * channels:]
    decoded_duration = len(samples) / channels / rate
    if decoded_duration > MAX_SECONDS or abs(decoded_duration - duration) > 0.1:
        raise AudioTestError(f"metadata/decoded duration mismatch: {path}")
    return rate, channels, samples


def validate_policy(policy: dict) -> None:
    if not isinstance(policy, dict):
        raise AudioTestError("policy must be an object")
    expected_keys = {"sample_rate", "channels", "duration_seconds", "duration_tolerance_seconds",
                     "relative_rms_error_max", "window_rms_error_max", "silence_reference_rms_max",
                     "silence_leak_rms_max"}
    if set(policy) != expected_keys:
        raise AudioTestError("policy fields missing or unknown")
    if any(not isinstance(v, (int, float)) or isinstance(v, bool) or not math.isfinite(v)
           for v in policy.values()):
        raise AudioTestError("policy must contain finite numeric values")
    if not (isinstance(policy["sample_rate"], int) and isinstance(policy["channels"], int)
            and 0 < policy["sample_rate"] <= MAX_RATE and 1 <= policy["channels"] <= MAX_CHANNELS
            and 0 < policy["duration_seconds"] <= MAX_SECONDS
            and 0 <= policy["duration_tolerance_seconds"] <= 0.1
            and 0 <= policy["relative_rms_error_max"] < 1
            and 0 <= policy["window_rms_error_max"] < 1
            and 0 <= policy["silence_reference_rms_max"] < 1
            and 0 <= policy["silence_leak_rms_max"] < 1):
        raise AudioTestError("invalid policy limits")


def compare(actual: Path, reference: Path, policy: dict, *, report_duration_failure: bool = False) -> dict:
    """Optionally score duration mismatches too, but never let them pass."""
    validate_policy(policy)
    ref_rate, ref_channels, ref = decode(reference)
    rate, channels, data = decode(actual)
    if (rate, channels) != (ref_rate, ref_channels) or (rate, channels) != (
            policy["sample_rate"], policy["channels"]):
        raise AudioTestError("sample rate or channel count differs from pinned policy/reference")
    ref_frames, frames = len(ref) // channels, len(data) // channels
    duration = policy["duration_seconds"]
    tolerance = policy["duration_tolerance_seconds"]
    duration_passed = (all(abs(n / rate - duration) <= tolerance for n in (ref_frames, frames))
                       and abs(ref_frames - frames) / rate <= tolerance)
    if not duration_passed and not report_duration_failure:
        raise AudioTestError("reference/actual duration differs from pinned policy")
    # Overlapping samples are measured, but a tolerated duration tail is never
    # silently ignored: zero-pad the shorter side and score it as a mismatch.
    window = max(1, round(rate * WINDOW_SECONDS))
    results = []
    for channel in range(channels):
        ref_power = error_power = peak_error = 0.0
        worst_window = 0.0
        max_silence_leak = 0.0
        for start in range(0, max(frames, ref_frames), window):
            window_ref = window_error = window_actual = 0.0
            count = min(window, max(frames, ref_frames) - start)
            for frame in range(start, start + count):
                r = ref[frame * channels + channel] if frame < ref_frames else 0.0
                a = data[frame * channels + channel] if frame < frames else 0.0
                error = a - r
                window_ref += r * r
                window_actual += a * a
                window_error += error * error
                peak_error = max(peak_error, abs(error))
            ref_power += window_ref
            error_power += window_error
            worst_window = max(worst_window, math.sqrt(window_error / count))
            if math.sqrt(window_ref / count) <= policy["silence_reference_rms_max"]:
                max_silence_leak = max(max_silence_leak, math.sqrt(window_actual / count))
        total = max(frames, ref_frames)
        ref_rms = math.sqrt(ref_power / total)
        error_rms = math.sqrt(error_power / total)
        relative_error = error_rms / ref_rms if ref_rms else (0.0 if error_rms == 0 else None)
        silent_reference = ref_rms <= policy["silence_reference_rms_max"]
        passed = ((silent_reference or (relative_error is not None
                                        and relative_error <= policy["relative_rms_error_max"]))
                  and worst_window <= policy["window_rms_error_max"]
                  and max_silence_leak <= policy["silence_leak_rms_max"])
        results.append({"channel": channel, "reference_rms": ref_rms,
                        "error_rms": error_rms, "relative_rms_error": relative_error,
                        "peak_sample_error": peak_error, "worst_window_rms_error": worst_window,
                        "max_silence_window_actual_rms": max_silence_leak, "passed": passed})
    result = {"passed": duration_passed and all(item["passed"] for item in results), "rate": rate,
              "channels": channels, "actual_frames": frames, "reference_frames": ref_frames,
              "window_seconds": WINDOW_SECONDS, "channel_results": results,
              "note": "Artifact comparison only; no conversion or native-render proof",
              "decode_contract": "Zero-origin samples; at most one AAC packet beyond the declared stream end is discarded"}
    if report_duration_failure:
        result["duration"] = {"passed": duration_passed, "expected_seconds": duration,
                              "tolerance_seconds": tolerance, "actual_seconds": frames / rate,
                              "reference_seconds": ref_frames / rate}
    return result
