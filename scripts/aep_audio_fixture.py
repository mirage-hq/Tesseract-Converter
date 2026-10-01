#!/usr/bin/env python3
"""Create reproducible PRIMARY audio inputs for the AE audio authoring recipe.

No Adobe invocation, conversion, render, reference creation, or scoring. The
movie is input footage, not an expected/reference movie. Refuse to overwrite.
"""

import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import wave

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "crates/aftereffects_file/tests/fixtures/audio_e2e"
RATE = 48_000
FRAMES = 4 * RATE


def pcm_frame(index, variant):
    second, within = divmod(index, RATE)
    # Independent left/right carriers, seconds, and source variant; audible
    # 40-ms tick at the beginning of each second gives an unambiguous clock.
    envelope = min(1.0, within / 240.0, (RATE - 1 - within) / 240.0)
    tick = 0.14 * math.sin(2 * math.pi * (1100 + variant * 200) * index / RATE) if within < 1920 else 0
    left_hz = (220, 330, 440, 550)[second] + variant * 137
    right_hz = (670, 770, 870, 970)[second] + variant * 113
    left = envelope * (0.28 * math.sin(2 * math.pi * left_hz * index / RATE) + tick)
    right = envelope * (0.18 * math.sin(2 * math.pi * right_hz * index / RATE) - tick * 0.5)
    return struct.pack("<hh", round(left * 32767), round(right * 32767))


def make_wave(path, variant):
    with wave.open(str(path), "wb") as output:
        output.setnchannels(2)
        output.setsampwidth(2)
        output.setframerate(RATE)
        for index in range(FRAMES):
            output.writeframesraw(pcm_frame(index, variant))


def fingerprint(path):
    return {"bytes": path.stat().st_size, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dir", type=Path, default=FIXTURE, help="new empty output directory")
    parser.add_argument("--ffmpeg", default="ffmpeg")
    args = parser.parse_args()
    target = args.dir.resolve()
    if any((target / name).exists() for name in ("sound.wav", "other.wav", "movie.mov")):
        parser.error(f"refusing to overwrite existing primary media in {target}")
    target.mkdir(parents=True, exist_ok=True)
    make_wave(target / "sound.wav", 0)
    make_wave(target / "other.wav", 1)
    command = [args.ffmpeg, "-hide_banner", "-nostdin", "-loglevel", "error", "-y",
               "-f", "lavfi", "-i", "color=c=blue:s=320x180:r=24:d=8",
               "-stream_loop", "1", "-i", str(target / "sound.wav"), "-map", "0:v:0", "-map", "1:a:0",
               "-c:v", "libx264", "-threads", "1", "-bf", "0", "-pix_fmt", "yuv420p",
               "-c:a", "aac", "-b:a", "192k", "-af", "atrim=end_sample=382976", "-t", "8",
               # 8s is exactly 375 AAC packets and 192 video frames. Keep AAC
               # priming in this PRIMARY signal, not an edit list: the existing
               # native MOV profile requires constant timing/no edit lists.
               "-use_editlist", "0", "-avoid_negative_ts", "disabled",
               "-metadata", "creation_time=1970-01-01T00:00:00Z",
               "-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact",
               "-f", "mp4", "-brand", "qt  ", str(target / "movie.mov")]
    subprocess.run(command, check=True)
    print(json.dumps({name: fingerprint(target / name) for name in ("sound.wav", "other.wav", "movie.mov")}, indent=2))


if __name__ == "__main__":
    main()
