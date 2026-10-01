#!/usr/bin/env python3
"""Explicit bounded local backend smoke; NOT an independent Adobe fidelity test.

Creates at most four <=8s synthetic sources and runs each through two backends.
No network/Adobe/Assets. Outputs are deliberately retained in a fresh work dir.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import re
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def execute(argv, *, cwd=None, binary=False):
    result = subprocess.run([str(arg) for arg in argv], cwd=cwd, capture_output=True)
    if result.returncode:
        raise RuntimeError(f"command failed ({result.returncode}): {argv}\n"
                           + result.stderr.decode(errors="replace")[-16000:])
    return result.stdout if binary else result.stdout.decode()


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def relink_aep(fixture, output, filename):
    # Only a disposable copy changes. Padding outside the JSON string keeps all
    # native RIFX chunk lengths/offsets unchanged; native fixture bytes stay pinned.
    pattern = rb'"fullpath"\s*:\s*"(?:[^"\\]|\\.)*"'
    count = 0

    def replacement(match):
        nonlocal count
        count += 1
        value = ('"fullpath":' + json.dumps(filename)).encode()
        if len(value) > len(match.group()):
            raise ValueError("replacement alias does not fit the native fixture")
        return value + b" " * (len(match.group()) - len(value))

    source = fixture.read_bytes()
    changed = re.sub(pattern, replacement, source)
    if count != 1 or len(source) != len(changed):
        raise ValueError("expected exactly one fixed-length native alias")
    output.write_bytes(changed)


def create_case(args, name):
    directory = args.work_dir / name
    directory.mkdir()
    native = ROOT / "crates/aftereffects_file/tests/fixtures/pr4442_native/sources"
    if name == "audio":
        media = directory / "input.aiff"
        execute([args.ffmpeg, "-v", "error", "-nostdin", "-n", "-f", "lavfi", "-i",
                 "aevalsrc=0.25*sin(2*PI*440*t)|0.125*sin(2*PI*880*t):s=48000:d=8",
                 "-c:a", "pcm_s24be", media])
        project = directory / "source.aep"
        relink_aep(native / "media_audio.aep", project, media.name)
    else:
        width, height, rate, duration = (1920, 1080, 30, 1) if name == "premiere" else (320, 180, 24, 8)
        media = directory / "input.mov"
        if name == "alpha":
            source = (f"nullsrc=s={width}x{height}:r={rate}:d={duration},format=rgba,"
                      "geq=r='X/W*255':g='Y/H*255':b='128':"
                      "a='mod(X,256)'")
            options = ["-pix_fmt", "argb"]
        else:
            source = f"testsrc2=s={width}x{height}:r={rate}:d={duration}"
            options = ["-pix_fmt", "rgb24"]
        command = [args.ffmpeg, "-v", "error", "-nostdin", "-n", "-f", "lavfi", "-i", source]
        if name == "ae":
            command += ["-f", "lavfi", "-i", f"sine=frequency=440:sample_rate=48000:duration={duration}",
                        "-c:a", "aac", "-b:a", "192k"]
        timecode = ["-timecode", "00:00:03:05"] if name == "alpha" else []
        execute(command + ["-c:v", "qtrle", *options, *timecode, media])
        if name == "premiere":
            project = directory / "source.prproj"
            xml = (ROOT / "crates/premiere_file/tests/fixtures/one-clip.xml").read_text()
            xml = (xml.replace("1270080000000", "254016000000")
                   .replace("2540160000000", "254016000000")
                   .replace("media/source.mp4", "input.mov"))
            project.write_bytes(gzip.compress(xml.encode(), mtime=0))
        else:
            project = directory / "source.aep"
            relink_aep(native / "media_video.aep", project, media.name)
    return project, media


def decoded(args, path, kind):
    if kind == "alpha":
        options = ["-map", "0:v:0", "-vf", "alphaextract", "-pix_fmt", "gray", "-f", "rawvideo"]
    elif kind == "audio":
        options = ["-map", "0:a:0", "-c:a", "pcm_s32le", "-f", "s32le"]
    else:
        options = ["-map", "0:v:0", "-pix_fmt", "rgb24", "-f", "rawvideo"]
    return execute([args.ffmpeg, "-v", "error", "-nostdin", "-i", path, *options, "pipe:1"], binary=True)


def media_layer_ids(value, kind):
    if isinstance(value, dict):
        if value.get("type") == kind:
            yield value["source"]["assetId"]
        for child in value.values():
            yield from media_layer_ids(child, kind)
    elif isinstance(value, list):
        for child in value:
            yield from media_layer_ids(child, kind)


def run_case(args, name, project, original, backend):
    before = (digest(project), digest(original))
    inventory = json.loads(execute([args.converter, "inspect", project, "--json"]))
    reports = inventory["media_preflight"]
    if len(reports) != 1 or not reports[0]["media"]:
        raise AssertionError("fixture must expose exactly one used native target")
    target = reports[0]["target"]
    selection = ["--sequence" if name == "premiere" else "--composition", target]
    assert inventory["media_admission"] == "blocked", inventory
    bundle = project.parent / f"prepared-{backend}"
    backend_options = ([] if backend == "library" else
                       ["--ffmpeg-path", args.ffmpeg, "--ffprobe-path", args.ffprobe])
    bundle.mkdir()
    extension = "wav" if name == "audio" else "mov" if name == "alpha" else "mp4"
    replacement = bundle / f"prepared.{extension}"
    result = json.loads(execute([args.converter, "transcode", original, "--output", replacement,
                                "--backend", backend, *backend_options, "--json"]))
    assert Path(result["output"]) == replacement
    assert result["input_sha256"] == before[1]
    assert result["output_sha256"] == digest(replacement)
    assert list(bundle.iterdir()) == [replacement], "transcode must publish only one media file"
    # Map assembly is the caller's separate project-aware responsibility, never
    # an implicit part of the single-file transcode command.
    media_map = bundle / "media-map.json"
    media_map.write_text(json.dumps({
        "version": 1,
        "source": {"format": reports[0]["format"], "sha256": before[0], "target": target},
        "replacements": [{"original": str(original.resolve()),
                          "original_sha256": before[1], "replacement": replacement.name,
                          "replacement_sha256": digest(replacement)}],
    }, indent=2) + "\n")
    mapped = json.loads(execute([args.converter, "inspect", project, *selection,
                                "--media-map", media_map, "--json"]))
    assert mapped["media_admission"] == "ready", mapped
    output = project.parent / f"converted-{backend}"
    execute([args.converter, "convert", project, "--to", "tesseract", *selection, "--output", output,
             "--media-map", media_map, "--check"])
    assert not output.exists()
    execute([args.converter, "convert", project, "--to", "tesseract", *selection, "--output", output,
             "--media-map", media_map])
    sidecar = json.loads(media_map.read_text())
    assert len(sidecar["replacements"]) == 1, sidecar
    replacement = bundle / sidecar["replacements"][0]["replacement"]
    with zipfile.ZipFile(output / "project.tsrct") as archive:
        document = json.loads(archive.read("project.json"))
        ids = list(media_layer_ids(document, "Audio" if name == "audio" else "Video"))
        assert len(ids) == 1, f"expected one editable media layer, found {ids}"
        metadata = json.loads(archive.read("metadata.json"))
        packaged = metadata["assets"][ids[0]]["path"]
        assert hashlib.sha256(archive.read(packaged)).hexdigest() == digest(replacement)
    # Generated-codec/sample evidence only, never an Adobe visual oracle.
    source_bytes = decoded(args, original, name)
    output_bytes = decoded(args, replacement, name)
    assert len(source_bytes) == len(output_bytes), "decoded frame/sample count changed"
    if name in ("alpha", "audio"):
        assert source_bytes == output_bytes, f"{name} samples changed"
        measurement = {"exact_sample_bytes": len(source_bytes)}
    else:
        error = sum(abs(a - b) for a, b in zip(source_bytes, output_bytes)) / len(source_bytes)
        # Explicit smoke floor, not the unrelated native-render/Adobe 0.99 gate.
        assert error <= 6.0, f"generated RGB mean absolute error too high: {error}"
        measurement = {"generated_rgb_mean_absolute_error": error, "sample_bytes": len(source_bytes)}
    if name == "alpha":
        def timecode(path):
            probe = json.loads(execute([args.ffprobe, "-v", "error", "-show_streams",
                                        "-of", "json", path]))
            tracks = [s for s in probe["streams"] if s.get("codec_tag_string") == "tmcd"]
            assert len(tracks) == 1
            payload = execute([args.ffmpeg, "-v", "error", "-i", path, "-map", "0:d:0",
                               "-c", "copy", "-f", "data", "pipe:1"], binary=True)
            return tracks[0]["tags"]["timecode"], payload
        assert timecode(original) == timecode(replacement), "timecode changed"
        measurement["timecode_preserved"] = True
    if name == "ae":
        source_audio = decoded(args, original, "audio")
        assert source_audio == decoded(args, replacement, "audio"), "embedded audio samples changed"
        measurement["exact_embedded_audio_bytes"] = len(source_audio)
    assert before == (digest(project), digest(original)), "original input changed"
    return {"case": name, "backend": backend, "result": result, "measurement": measurement}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--converter", type=Path, required=True)
    parser.add_argument("--ffmpeg", type=Path, required=True)
    parser.add_argument("--ffprobe", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--case", choices=["ae", "premiere", "alpha", "audio"], action="append")
    parser.add_argument("--backend", choices=["library", "external-ffmpeg-command"], action="append")
    args = parser.parse_args()
    for field in ("converter", "ffmpeg", "ffprobe", "work_dir"):
        setattr(args, field, getattr(args, field).absolute())
    args.work_dir.mkdir(parents=False, exist_ok=False)
    results = []
    for name in args.case or ["ae", "premiere", "alpha", "audio"]:
        project, media = create_case(args, name)
        for backend in args.backend or ["external-ffmpeg-command", "library"]:
            results.append(run_case(args, name, project, media, backend))
            (args.work_dir / "results.json").write_text(json.dumps(results, indent=2) + "\n")
            print(f"PASS {name} {backend}", flush=True)
    print(f"Results: {args.work_dir / 'results.json'}")


if __name__ == "__main__":
    main()
