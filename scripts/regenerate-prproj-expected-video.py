#!/usr/bin/env python3
"""Generate an independent Premiere MP4 via AME's native script host.

Requires a logged-in macOS desktop session, Premiere Pro, and Media Encoder.
No plugin, UI automation, or claim of true headless operation is involved.
"""

import argparse
import contextlib
import fcntl
import gzip
import hashlib
import html
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
import xml.etree.ElementTree as ET
from xml.sax.saxutils import escape
import zlib

import adobe_native

REPO = Path(__file__).resolve().parents[1]
MANIFEST = REPO / "tests/manifest.json"
CACHE = Path.home() / ".cache/jerboa/conversion"
PRESET_RELATIVE = Path("Contents/MediaIO/systempresets/4E49434B_48323634/01 - Match Source - High bitrate.epr")
PATH_ELEMENT = re.compile(rb"<(FilePath|ActualMediaFilePath|RelativePath)>([^<]*)</\1>")
MAX_XML_BYTES = 64 * 1024 * 1024


class ExportError(ValueError):
    pass


def size_and_sha(path):
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
            size += len(chunk)
    return size, digest.hexdigest()


def atomic_json(path, data):
    fd, temporary = tempfile.mkstemp(prefix=".job-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as stream:
            json.dump(data, stream, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        Path(temporary).replace(path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def decode_project(source):
    raw = source.read_bytes()
    if not raw.startswith(b"\x1f\x8b"):
        raise ExportError("expected a gzip-compressed Premiere .prproj")
    try:
        inflater = zlib.decompressobj(31)
        xml = inflater.decompress(raw, MAX_XML_BYTES + 1)
        if len(xml) > MAX_XML_BYTES or inflater.unconsumed_tail or not inflater.eof or inflater.unused_data:
            raise ExportError("Premiere XML exceeds size limit or has trailing data")
        ET.fromstring(xml)
    except (zlib.error, ET.ParseError) as exc:
        raise ExportError(f"invalid Premiere XML: {exc}") from exc
    return xml


def choose_sequence(xml, requested):
    root = ET.fromstring(xml)
    sequences = [(item.findtext("Name"), item.attrib["ObjectUID"])
                 for item in root.iter("Sequence") if "ObjectUID" in item.attrib]
    if not sequences or any(not name for name, _ in sequences):
        raise ExportError("Premiere project has no named sequence")
    matches = [(name, uid) for name, uid in sequences if requested in (name, uid)] if requested else sequences
    if len(matches) != 1:
        names = ", ".join(f"{name!r} ({uid})" for name, uid in sequences)
        raise ExportError(f"sequence must identify exactly one sequence; available: {names}")
    return {"name": matches[0][0], "uid": matches[0][1]}


def preflight(case_id, preset, cache, sequence=None):
    if not re.fullmatch(r"[a-zA-Z0-9_-]+", case_id):
        raise ExportError("invalid conversion case ID")
    manifest = json.loads(MANIFEST.read_text())
    cases = [case for case in manifest["cases"] if case["id"] == case_id]
    if len(cases) != 1:
        raise ExportError(f"unknown or ambiguous conversion case: {case_id}")
    case = cases[0]
    if case["direction"] != "premiere_to_tesseract":
        raise ExportError("this reference generator accepts .prproj projects only")
    if preset is None or not preset.is_absolute() or preset.suffix.lower() != ".epr" or not preset.is_file():
        raise ExportError("supply an absolute, existing H.264 .epr preset: preset=/absolute/path/to/file.epr")
    staged = cache / "cases" / case_id
    project = staged / case["project"]
    for file in case["files"]:
        relative = PurePosixPath(file["path"])
        if relative.is_absolute() or ".." in relative.parts:
            raise ExportError(f"unsafe package path: {file['path']}")
        path = staged / file["path"]
        if not path.is_file() or path.is_symlink() or size_and_sha(path) != (file["size_bytes"], file["sha256"]):
            raise ExportError(f"missing or corrupt staged input: {file['path']}; run make prproj-download")
    if project.suffix.lower() != ".prproj" or not any(f["path"] == case["project"] for f in case["files"]):
        raise ExportError("case has no pinned .prproj project")
    xml = decode_project(project)
    selected = choose_sequence(xml, sequence or case.get("sequence"))
    return case, project, xml, selected


def rebase_xml(xml, case, package):
    """Rebase media paths; add Adobe's absolute locators for relative-only fixtures."""
    media = [f["path"] for f in case["files"] if f["path"] != case["project"]]
    project_parent = PurePosixPath(case["project"]).parent
    root = ET.fromstring(xml)
    expected = sum(1 for node in root.iter() if node.tag in ("FilePath", "ActualMediaFilePath", "RelativePath"))
    relative_only = set()
    for node in root.iter("Media"):
        relative = node.findtext("RelativePath")
        if relative is not None:
            has_file = node.find("FilePath") is not None
            has_actual = node.find("ActualMediaFilePath") is not None
            if has_file != has_actual:
                raise ExportError("incomplete Premiere absolute media locators")
            if not has_file:
                relative_only.add(relative)
    count = 0
    paths = set()

    def replace(match):
        nonlocal count
        count += 1
        tag = match.group(1).decode("ascii")
        original = html.unescape(match.group(2).decode("utf-8"))
        if not original or original.isdecimal():
            return match.group(0)  # Generated media may have no external path.
        if tag == "RelativePath":
            relative = PurePosixPath(original.replace("\\", "/"))
            if relative.is_absolute() or ".." in relative.parts:
                raise ExportError(f"unsafe Premiere relative media path: {original}")
            name = (project_parent / relative).as_posix()
            matches = [name] if name in media else []
        else:
            if not original.startswith("/"):
                raise ExportError(f"unsupported Premiere media path: {original}")
            matches = [name for name in media if original.endswith("/" + name)]
        if len(matches) != 1:
            raise ExportError(f"Premiere media path has no unique pinned file: {original}")
        name = matches[0]
        paths.add(name)
        value = escape(str(package / name)).encode("utf-8")
        if tag == "RelativePath":
            # AME does not resolve a generated project's bare RelativePath:
            # its export completes with Media offline. Keep the original relative
            # hint but add the two native absolute locators in the throwaway copy.
            if original in relative_only:
                return (match.group(0) + b"<FilePath>" + value + b"</FilePath>" +
                        b"<ActualMediaFilePath>" + value + b"</ActualMediaFilePath>")
            return match.group(0)
        return b"<" + match.group(1) + b">" + value + b"</" + match.group(1) + b">"

    rewritten = PATH_ELEMENT.sub(replace, xml)
    if count != expected:
        raise ExportError("unsupported Premiere media path element; refusing an incomplete rebase")
    ET.fromstring(rewritten)  # Reject an accidental malformed disposable project before AME.
    return rewritten, sorted(paths)


def prepare_package(case, source_project, xml, cache, job_dir):
    package = job_dir / "package"
    package.mkdir()
    rewritten, referenced = rebase_xml(xml, case, package)
    for file in case["files"]:
        destination = package / file["path"]
        destination.parent.mkdir(parents=True, exist_ok=True)
        if file["path"] == case["project"]:
            destination.write_bytes(gzip.compress(rewritten, mtime=0))
        else:
            shutil.copyfile(cache / "cases" / case["id"] / file["path"], destination)
            if size_and_sha(destination) != (file["size_bytes"], file["sha256"]):
                raise ExportError(f"copied media changed during package preparation: {file['path']}")
    return package / case["project"], referenced


def generate_jsx(job_id, project, output, preset, sequence, status):
    """All interpolated strings are JSON-escaped literals (valid ExtendScript)."""
    js = lambda value: json.dumps(str(value), ensure_ascii=True)
    return f'''// AME's documented --console es.processFile entry point; no plugin.
var jobID = {js(job_id)};
var statusPath = {js(status)};
var outputPath = {js(output)};
function report(state, detail) {{
    var next = new File(statusPath + '.tmp');
    next.encoding = 'UTF-8';
    if (!next.open('w')) return;
    next.writeln(jobID);
    next.writeln(state);
    next.writeln(String(detail || '').replace(/[\\r\\n]/g, ' ').slice(0, 1000));
    next.writeln(String(app.buildNumber));
    next.close();
    var old = new File(statusPath);
    if (old.exists && !old.remove()) return;
    next.rename('status.txt');
}}
function finish(state, detail) {{
    report(state, detail);
    app.scheduleTask('app.quit()', 1500, false); // Only a cold, command-owned AME instance.
}}
try {{
    var exporter = app.getExporter();
    var host = app.getEncoderHost();
    if (!exporter || !host) throw new Error('AME exporter/encoder host missing');
    if (host.isBatchRunning()) throw new Error('AME batch was already running');
    host.addEventListener('onItemEncodeComplete', function(event) {{
        if (String(event.outputFilePath) !== outputPath) return;
        var result = String(event.result).toLowerCase();
        finish(result === 'true' ? 'complete' : 'error', 'onItemEncodeComplete=' + result);
    }}, false);
    exporter.addEventListener('onError', function(event) {{
        finish('error', 'AME onError');
    }}, false);
    report('starting', 'submitting selected Premiere sequence');
    var accepted = exporter.exportSequence({js(project)}, outputPath, {js(preset)},
                                           false, false, 0, 0, {js(sequence)});
    if (!accepted) throw new Error('AME exportSequence returned false');
    report('queued', 'AME accepted sequence');
    if (!host.runBatch()) throw new Error('AME runBatch returned false');
}} catch (error) {{
    finish('error', String(error));
}}
'''


def ame_running(app):
    try:
        result = subprocess.run(["ps", "-A", "-o", "comm="], capture_output=True,
                                text=True, timeout=10, check=True)
    except (OSError, subprocess.SubprocessError) as exc:
        raise ExportError("cannot determine whether AME is already running") from exc
    return str(app / "Contents/MacOS" / app.stem) in result.stdout.splitlines()


def await_ame_exit(app, timeout=25):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not ame_running(app):
            return
        time.sleep(0.25)
    raise ExportError("AME finished encoding but did not exit; stop before submitting another project")


def launch_ame(app, jsx):
    raise ExportError('Independent AME script launch is retired; use the typed headless-adobe operation')


def wait_for_status(status_path, job_id, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if status_path.is_file():
            lines = status_path.read_text(errors="replace").splitlines()
            if len(lines) >= 4:
                if lines[0] != job_id:
                    raise ExportError("AME status has the wrong job ID")
                if lines[1] == "complete":
                    return {"ame_build": lines[3], "message": lines[2]}
                if lines[1] == "error":
                    raise ExportError(f"AME export failed: {lines[2]}")
                if lines[1] not in ("starting", "queued"):
                    raise ExportError("AME reported an unknown status")
        time.sleep(0.25)
    raise ExportError("timed out waiting for AME; in-flight output is incomplete; check AME before retrying")


def probe_mp4(path):
    if not path.is_file() or path.stat().st_size == 0:
        raise ExportError("AME reported completion but no nonempty MP4 exists")
    try:
        result = subprocess.run(
            ["ffprobe", "-v", "error", "-show_entries", "stream=codec_type,codec_name,width,height:format=duration,size",
             "-of", "json", str(path)], capture_output=True, text=True, timeout=60, check=True)
        info = json.loads(result.stdout)
    except (OSError, subprocess.SubprocessError, ValueError) as exc:
        raise ExportError("exported MP4 failed ffprobe verification") from exc
    videos = [stream for stream in info.get("streams", []) if stream.get("codec_type") == "video"]
    if not videos or any(not isinstance(stream.get("width"), int) or stream["width"] <= 0 or
                         not isinstance(stream.get("height"), int) or stream["height"] <= 0 for stream in videos):
        raise ExportError("exported MP4 has no valid video stream")
    try:
        duration = float(info["format"]["duration"])
    except (KeyError, ValueError, TypeError) as exc:
        raise ExportError("exported MP4 has no valid duration") from exc
    if not math.isfinite(duration) or duration <= 0:
        raise ExportError("exported MP4 has zero or invalid duration")
    return duration, videos


@contextlib.contextmanager
def exclusive_export(cache):
    cache.mkdir(parents=True, exist_ok=True)
    with (cache / "ame-export.lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise ExportError("another conversion reference export already owns AME") from exc
        yield


def regenerate(case_id, preset, cache, sequence, timeout, app):
    with exclusive_export(cache):
        case, project, xml, selected = preflight(case_id, preset, cache, sequence)
        job_id = uuid.uuid4().hex
        job_dir = cache / "export-jobs" / job_id
        job_dir.mkdir(parents=True, exist_ok=False)
        output = cache / "expected-candidates" / case_id / f"{job_id}.mp4"
        output.parent.mkdir(parents=True, exist_ok=True)
        if output.exists():
            raise ExportError(f"refusing to overwrite existing export: {output}")
        work_project, referenced = prepare_package(case, project, xml, cache, job_dir)
        dependencies = {}
        for index, relative in enumerate(referenced):
            path = (job_dir / 'package' / relative).resolve()
            dependencies[f'media-{index}'] = {'path': str(path), 'sha256': size_and_sha(path)[1]}
        print(f"Central AME export {case_id} / {selected['name']} ({job_id}); timeout {timeout}s", flush=True)
        artifact = adobe_native.execute('render_premiere_ame', {
            'source': adobe_native.source_ref(work_project, dependencies), 'sequence_id': selected['uid'],
            'preset': {'path': str(preset.resolve()), 'sha256': size_and_sha(preset)[1]},
            'request_id': 'premiere-reference-' + job_id,
        }, job_dir / 'worker', timeout=timeout, ame_app=app)
        metadata = artifact.get('metadata', {})
        if not metadata.get('build') or not metadata.get('completion'):
            raise ExportError('Central AME artifact lacks native build/completion provenance')
        adobe_native.copy_artifact(artifact, output)
        duration, videos = probe_mp4(output)
        record = {"case_id": case_id, "job_id": job_id, "sequence_name": selected["name"],
                  "sequence_uid": selected["uid"], "source_project_sha256": size_and_sha(project)[1],
                  "rebased_project_sha256": size_and_sha(work_project)[1],
                  "media_paths_checked": referenced,
                  "media_link_verified": "absolute_media_paths_checked",
                  "preset_path": str(preset), "preset_sha256": size_and_sha(preset)[1],
                  "ame_app": str(app), "ame_build": metadata['build'],
                  "ame_result": metadata['completion'], 'headless_adobe': artifact,
                  "duration_seconds": duration,
                  "video_streams": videos, "output_sha256": size_and_sha(output)[1],
                  "output_size_bytes": output.stat().st_size, "output_path": str(output),
                  "proof": "AME native Premiere sequence export"}
        atomic_json(output.with_suffix(".provenance.json"), record)
        print(f"Premiere MP4: {output}\nProvenance: {output.with_suffix('.provenance.json')}")
        return output


def select_ame_app(explicit):
    if explicit is not None:
        app = explicit.expanduser().resolve()
        if not (app.is_dir() and (app / "Contents/MacOS" / app.stem).is_file()):
            raise ExportError(f"no Adobe Media Encoder application executable at {app}")
        return app
    candidates = sorted(Path("/Applications").glob("Adobe Media Encoder 20*/Adobe Media Encoder 20*.app"))
    if len(candidates) != 1:
        raise ExportError("select one installed AME with --ame-app=/Applications/.../Adobe Media Encoder.app")
    return candidates[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", required=True)
    parser.add_argument("--preset", type=Path, default=os.environ.get("PREMIERE_H264_PRESET"))
    parser.add_argument("--sequence", help="exact sequence name; required if the project has multiple sequences")
    parser.add_argument("--ame-app", type=Path, default=os.environ.get("PREMIERE_AME_APP"))
    parser.add_argument("--cache-dir", type=Path, default=CACHE)
    parser.add_argument("--timeout-seconds", type=int, default=1800)
    args = parser.parse_args()
    if not 30 <= args.timeout_seconds <= 3600:
        parser.error("timeout-seconds must be in [30, 3600]")
    if sys.platform != "darwin":
        raise ExportError("AME reference export currently requires macOS with a logged-in GUI session")
    app = select_ame_app(args.ame_app)
    preset = Path(args.preset).expanduser().resolve() if args.preset else app / PRESET_RELATIVE
    regenerate(args.project, preset, args.cache_dir.expanduser().resolve(), args.sequence,
               args.timeout_seconds, app)


if __name__ == "__main__":
    try:
        main()
    except (ExportError, adobe_native.NativeAdobeError, OSError) as exc:
        print(f"Premiere expected-video export: {exc}", file=sys.stderr)
        sys.exit(1)
