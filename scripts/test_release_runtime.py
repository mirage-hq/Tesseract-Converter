#!/usr/bin/env python3
"""Exercise the relocated release CLI's default library backend without host FFmpeg.

The explicitly supplied controlled FFmpeg is used only to generate source video.
Every converter invocation has an empty PATH and no library-loader overrides.
No Adobe, GPU renderer, network service, signing credentials or publication is used.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import wave


def clean_environment():
    return {key: ("" if key.upper() == "PATH" else value)
            for key, value in os.environ.items()
            if not key.upper().startswith(("LD_", "DYLD_"))
            and key.upper() not in {"FFMPEG_DIR", "PKG_CONFIG_PATH", "RUST_LOG"}}


def aiff(path):
    """A deterministic, big-endian mono PCM source requiring AIFF -> WAVE conversion."""
    values = [round(12000 * math.sin(2 * math.pi * 440 * n / 48000)) for n in range(4800)]
    pcm = struct.pack(f">{len(values)}h", *values)
    comm = struct.pack(">hIh", 1, len(values), 16) + bytes.fromhex("400ebb80000000000000")
    sound = struct.pack(">II", 0, 0) + pcm
    content = b"AIFF" + b"COMM" + struct.pack(">I", len(comm)) + comm
    content += b"SSND" + struct.pack(">I", len(sound)) + sound
    path.write_bytes(b"FORM" + struct.pack(">I", len(content)) + content)
    return struct.pack(f"<{len(values)}h", *values)


def raw_video(path, *, alpha):
    """Twelve raw frames for FFmpeg's demuxer; no lavfi input device is needed."""
    frame = bytearray()
    for y in range(144):
        for x in range(256):
            frame.extend((x, x, y, 128) if alpha else (x, y, 128))
    path.write_bytes(frame * 12)
    return "argb" if alpha else "rgb24"


def convert(binary, source, output, *, should_fail=False):
    source_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    result = subprocess.run([str(binary), "transcode", str(source), "--output", str(output), "--json"],
                            cwd=source.parent, env=clean_environment(), capture_output=True,
                            text=True, timeout=90)
    if hashlib.sha256(source.read_bytes()).hexdigest() != source_hash:
        raise AssertionError("transcode changed the source")
    if should_fail:
        if (result.returncode == 0 or result.stdout.strip() or output.exists()
                or "no approved H.264 encoder" not in result.stderr):
            raise AssertionError(f"expected explicit Linux H.264 policy rejection: {result}")
        return None
    if result.returncode:
        raise AssertionError(f"bundled library transcode failed: {result.stderr}")
    report = json.loads(result.stdout)
    if report["backend"] != "library" or report["output_sha256"] != hashlib.sha256(output.read_bytes()).hexdigest():
        raise AssertionError(f"wrong backend or output identity: {report}")
    return report


def smoke(bundle, platform, ffmpeg):
    with tempfile.TemporaryDirectory(prefix="converter-release-relocated-") as temporary:
        work = Path(temporary)
        relocated = work / "with spaces" / "bundle"
        shutil.copytree(bundle, relocated)
        binary = relocated / ("tsrct-conv.exe" if platform.startswith("windows-") else "tsrct-conv")
        source = work / "source.aiff"
        expected_pcm = aiff(source)
        audio = work / "converted.wav"
        report = convert(binary, source, audio)
        if report["operation"] != "transcode" or report["encoder"] != "pcm_s16le":
            raise AssertionError(f"audio smoke did not encode PCM: {report}")
        with wave.open(str(audio), "rb") as decoded:
            if (decoded.getnchannels(), decoded.getsampwidth(), decoded.getframerate()) != (1, 2, 48000):
                raise AssertionError("PCM layout changed")
            if decoded.readframes(decoded.getnframes()) != expected_pcm:
                raise AssertionError("PCM samples changed")
        copied = convert(binary, audio, work / "copied.wav")
        if copied["operation"] != "copy":
            raise AssertionError("compatible media should use copy")

        for alpha in (False, True):
            source = work / ("alpha.mov" if alpha else "opaque.mov")
            raw = work / ("alpha.raw" if alpha else "opaque.raw")
            pixel_format = raw_video(raw, alpha=alpha)
            subprocess.run([str(ffmpeg), "-v", "error", "-nostdin", "-n", "-f", "rawvideo",
                            "-pixel_format", pixel_format, "-video_size", "256x144", "-framerate", "24",
                            "-i", str(raw), "-c:v", "qtrle", "-pix_fmt", pixel_format, str(source)],
                           check=True, timeout=60)
            output = work / ("alpha-prepared.mov" if alpha else "opaque-prepared.mp4")
            report = convert(binary, source, output, should_fail=not alpha and platform == "linux-x86_64")
            if report is not None:
                expected_encoder = "prores_ks" if alpha else "h264_mf" if platform.startswith("windows-") else "h264_videotoolbox"
                if report["operation"] != "transcode" or report["encoder"] != expected_encoder:
                    raise AssertionError(f"unexpected native encoder: {report}")
        print(f"PASS {platform}: relocated default-library PCM, copy, ProRes and H.264 policy/encoding")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--platform", choices=("darwin-arm64", "darwin-x86_64", "windows-x86_64", "linux-x86_64"), required=True)
    parser.add_argument("--ffmpeg", type=Path, required=True, help="Controlled FFmpeg used only to generate fixtures")
    args = parser.parse_args()
    smoke(args.bundle.resolve(), args.platform, args.ffmpeg.resolve())
