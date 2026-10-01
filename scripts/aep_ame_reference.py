"""Append independent AEP reference jobs to AME's shared queue via BridgeTalk.

Never clears the queue, stops another job, quits AME, or waits for app exit.
Only this job's completion event/output is observed. No conversion tests run.
A pinned preset fixes 30fps and the source canvas; no post-export resampling.
"""
from __future__ import annotations

import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import time
import uuid
import xml.etree.ElementTree as ET


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def save(path: Path, value: dict) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("x") as output:
        json.dump(value, output, indent=2)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(path)


def inspect_preset(path: Path, expected_sha: str, composition: dict) -> dict:
    if not path.is_absolute() or path.suffix.lower() != ".epr" or not path.is_file():
        raise ValueError("Specify an absolute, existing AME H.264 .epr preset")
    actual = sha256(path)
    if not re.fullmatch(r"[0-9a-f]{64}", expected_sha) or actual != expected_sha:
        raise ValueError("AME preset SHA-256 differs from the selected preset")
    root = ET.parse(path).getroot()
    if root.findtext("DoVideo") != "true":
        raise ValueError("AME preset does not enable video")
    if root.findtext("ExporterClassID") != "1313424203" or root.findtext("ExporterFileType") != "1211250228":
        raise ValueError("AME preset must identify the H.264 MP4 exporter")
    expected = {"ADBEVideoFPS": "8467200000", "ADBEVideoWidth": str(composition["width"]),
                "ADBEVideoHeight": str(composition["height"])}
    params = {}
    for param in root.iter("ExporterParam"):
        key = param.findtext("ParamIdentifier")
        if key in expected:
            if key in params:
                raise ValueError(f"Ambiguous preset setting: {key}")
            params[key] = param
    for key, value in expected.items():
        param = params.get(key)
        if param is None or param.findtext("ParamIsDisabled") == "true" or param.findtext("ParamValue") != value:
            raise ValueError(f"Preset must explicitly fix {key} to {value}, not Match Source")
    standard = root.find("StandardFilters")
    if standard is None or standard.findtext("CropType") != "0":
        raise ValueError("Preset must preserve the full uncropped canvas")
    return {"path": str(path), "sha256": actual, "format": "H.264 MP4", "fps": 30,
            "fps_control": "fixed preset and EncoderWrapper.setFrameRate(\"30\")",
            "canvas": {"width": composition["width"], "height": composition["height"]},
            "crop": "none", "preset_name": root.findtext("PresetName")}


def script(job_id: str, source: Path, output: Path, preset: Path, guid: str, status: Path,
           *, queue_only: bool = False, root_only: bool = True) -> str:
    def js(value):
        return json.dumps(str(value), ensure_ascii=True)

    return f'''// Append one owned job; preserve all existing queue entries and application state.
(function () {{
    var jobID = {js(job_id)}, sourcePath = {js(source)}, outputPath = {js(output)};
    var statusPath = {js(status)}, presetPath = {js(preset)}, requestedGUID = {js(guid)};
    var finished = false;
    if (!$._aepReferenceJobs) $._aepReferenceJobs = {{}};
    function report(state, detail) {{
        var next = new File(statusPath + '.tmp'); next.encoding = 'UTF-8';
        if (!next.open('w')) return;
        next.writeln(jobID); next.writeln(state);
        next.writeln(String(detail || '').split(String.fromCharCode(13)).join(' ').split(String.fromCharCode(10)).join(' ').slice(0, 1000));
        next.writeln(String(app.buildNumber)); next.close();
        var previous = new File(statusPath);
        if (previous.exists && !previous.remove()) return;
        next.rename(previous.name);
    }}
    function finish(state, detail) {{
        if (finished) return;
        finished = true; report(state, detail);
        delete $._aepReferenceJobs[jobID];
    }}
    try {{
        var frontend = app.getFrontend(), host = app.getEncoderHost();
        if (!frontend || !host) throw new Error('AME queue API unavailable');
        // Root enumeration excludes foldered comps. Only bypass after AE has
        // independently verified this GUID against the matching open project.
        if ({str(root_only).lower()}) {{
            var roots = frontend.getDLItemsAtRoot(sourcePath), matches = 0;
            if (!roots) throw new Error('Dynamic Link returned no root compositions');
            for (var i = 0; i < roots.length; i++) if (String(roots[i]) === requestedGUID) matches++;
            if (matches !== 1) throw new Error('Authored Dynamic Link GUID not uniquely found at root');
        }}
        report('starting', 'selected exact authored composition GUID');
        var encoder = frontend.addDLToBatch(sourcePath, 'H.264', presetPath, requestedGUID, outputPath);
        if (!encoder) throw new Error('AME addDLToBatch returned no encoder');
        $._aepReferenceJobs[jobID] = encoder;
        // From this point, even a failure can leave an owned job in the
        // shared queue. Preserve uncertainty instead of claiming no job exists.
        report('configuring', 'job appended; encoder configuration not yet confirmed');
        if (!encoder.setFrameRate('30')) throw new Error('AME rejected fixed 30fps');
        if (!encoder.setWorkArea(0, 0, 0)) throw new Error('AME rejected Entire composition range');
        encoder.addEventListener('onEncodeFinished', function(event) {{
            var result = String(event.result);
            finish(result === 'Done!' ? 'complete' : 'error', 'onEncodeFinished=' + result);
        }}, false);
        report('queued', 'appended owned job to shared queue');
        if (!{str(queue_only).lower()} && !host.isBatchRunning() && !host.runBatch()) throw new Error('AME runBatch returned false');
    }} catch (error) {{
        if (typeof encoder !== 'undefined' && encoder) {{
            report('uncertain', 'possibly queued job: ' + String(error));
        }} else {{ finish('error', String(error)); }}
    }}
}})();
'''


def dispatch_script(job_id: str, body: str, status: Path) -> str:
    # AE is only the BridgeTalk sender; this never opens or changes its project.
    return '''(function () {
    var jobID = %s, statusPath = %s;
    if (!$._aepReferenceDispatches) $._aepReferenceDispatches = {};
    function fail(detail) {
        var file = new File(statusPath); file.encoding = 'UTF-8';
        if (file.open('w')) {
            file.writeln(jobID); file.writeln('error');
            file.writeln(String(detail).split(String.fromCharCode(13)).join(' ').split(String.fromCharCode(10)).join(' ').slice(0, 1000));
            file.writeln('dispatch'); file.close();
        }
        delete $._aepReferenceDispatches[jobID];
    }
    try {
        var target = BridgeTalk.getSpecifier('ame');
        if (!target) throw new Error('No installed AME BridgeTalk target');
        var request = new BridgeTalk(); request.target = target;
        request.body = %s;
        request.onError = function(message) { fail(message.body); };
        request.onResult = function() { delete $._aepReferenceDispatches[jobID]; };
        $._aepReferenceDispatches[jobID] = request;
        // false means queued while AME starts, not failure. Keep callbacks alive.
        request.send();
    } catch (error) { fail(String(error)); }
})();
''' % (json.dumps(job_id), json.dumps(str(status)), json.dumps(body))


def _exact_log_entry(source: Path, output: Path, log: Path, start_byte: int) -> dict | None:
    try:
        data = log.read_bytes()
    except FileNotFoundError:
        return None  # AME may rotate a shared log; never attribute an older entry.
    if start_byte:
        if len(data) < start_byte:
            return None
        encoding = "utf-16-le" if data.startswith(b"\xff\xfe") else "utf-16-be" if data.startswith(b"\xfe\xff") else "utf-8"
        text = data[start_byte:].decode(encoding, errors="replace")
    else:
        text = data.decode("utf-16" if data.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8-sig", errors="replace")
    for block in reversed(re.split(r"(?=^\s*- Source File: )", text, flags=re.MULTILINE)):
        lines = block.splitlines()
        if (not lines or lines[0].strip() != "- Source File: " + str(source)
                or not any(line.strip() == "- Output File: " + str(output) for line in lines)):
            continue
        block = block.split("Queue Stopped")[0]
        failed = bool(re.search(r"(?:Encoding Failed|File Failed|Error (?:Code|Compiling|Rendering)|\bError\s*:|\bFailed\s*:)", block, re.I))
        return {"state": "error" if failed else "complete" if "File Successfully Encoded" in block else "pending",
                "detail": block, "log_path": str(log)}
    return None


def job_log_entry(source: Path, output: Path, log: Path | None = None,
                  *, start_byte: int = 0, error_log: Path | None = None,
                  error_start_byte: int = 0) -> dict | None:
    """Exact source/output AME job entry; inspect the separate error log first."""
    if log is None:
        log = Path.home() / "Documents/Adobe/Adobe Media Encoder/26.0/AMEEncodingLog.txt"
        error_log = error_log or log.with_name("AMEEncodingErrorLog.txt")
    if error_log is not None:
        error = _exact_log_entry(source, output, error_log, error_start_byte)
        if error and error["state"] == "error":
            return error
    return _exact_log_entry(source, output, log, start_byte)


def completed_log_entry(source: Path, output: Path) -> str | None:
    """AME's durable exact source/output success signal (legacy contract)."""
    # Preserve the old success-log-only lookup: an earlier failed encode to the
    # same path in a separate error log must not hide a later legacy success.
    log = Path.home() / "Documents/Adobe/Adobe Media Encoder/26.0/AMEEncodingLog.txt"
    entry = _exact_log_entry(source, output, log, 0)
    return entry["detail"] if entry and entry["state"] == "complete" else None


def produce(source: Path, output: Path, composition: dict, folder: Path,
            app: Path, preset: Path, expected_sha: str, timeout: int,
            state_dir: Path | None = None, *, queue_only: bool = False,
            root_only: bool = True) -> dict:
    # state_dir is retained for command-line compatibility; shared queues are not inspected or cleared.
    guid = composition.get("dynamic_link_guid")
    if not isinstance(guid, str) or not guid.strip():
        raise ValueError("Authored composition.dynamic_link_guid is required; numeric AE ID is NOT a GUID")
    if not 30 <= timeout <= 3600:
        raise ValueError("AME job timeout must be in [30, 3600] seconds")
    preset_info = inspect_preset(preset.resolve(), expected_sha, composition)
    status, jsx = folder / "ame-status.txt", folder / "ame-export.jsx"
    dispatcher, receipt_path = folder / "ame-dispatch.jsx", folder / "ame-producer.json"
    resume = receipt_path.exists()
    if resume:
        record = json.loads(receipt_path.read_text())
        if (record.get("source_sha256") != sha256(source)
                or record.get("composition_id") != composition["composition_id"]
                or record.get("composition_name") != composition["composition_name"]
                or record.get("preset", {}).get("path") != str(preset.resolve())
                or record.get("requested_ame_app") != str(app)
                or record.get("dynamic_link_guid") != guid
                or record.get("preset", {}).get("sha256") != expected_sha
                or record.get("output_path") != str(output)
                or record.get("jsx_sha256") != sha256(jsx)
                or record.get("queue_only", False) != queue_only
                or record.get("root_only", True) != root_only):
            raise ValueError("Existing queued job has different input identity; refusing reuse")
        job_id = record["job_id"]
    else:
        if any(path.exists() for path in (output, status, jsx, dispatcher)):
            raise ValueError("Unidentified prior AME artifacts; never duplicate an uncertain queued job")
        job_id = uuid.uuid4().hex
        body = script(job_id, source, output, preset, guid, status,
                      queue_only=queue_only, root_only=root_only)
        jsx.write_text(body)
        dispatcher.write_text(dispatch_script(job_id, body, status))
        command = ["/usr/bin/osascript", "-e", 'tell application "Adobe After Effects 2026" to DoScriptFile POSIX file ' + json.dumps(str(dispatcher))]
        log_path = Path.home() / "Documents/Adobe/Adobe Media Encoder/26.0/AMEEncodingLog.txt"
        error_log = log_path.with_name("AMEEncodingErrorLog.txt")
        record = {"job_id": job_id, "state": "prepared", "transport": "BridgeTalk_shared_AME_queue",
                  "log_start_byte": log_path.stat().st_size if log_path.exists() else 0,
                  "error_log_start_byte": error_log.stat().st_size if error_log.exists() else 0,
                  "source_path": str(source), "source_sha256": sha256(source),
                  "composition_id": composition["composition_id"],
                  "composition_name": composition["composition_name"], "dynamic_link_guid": guid,
                  "preset": preset_info, "requested_ame_app": str(app), "command": command,
                  "output_path": str(output), "script_path": str(jsx), "jsx_sha256": sha256(jsx),
                  "status_path": str(status), "queue_only": queue_only, "root_only": root_only,
                  "tests_executed": False,
                  "visual_inspection": "not_run", "fidelity": "unmeasured"}
        save(receipt_path, record)
    try:
        if not resume:
            subprocess.run(command, capture_output=True, text=True, timeout=60, check=True)
            record["state"] = "dispatched"
            save(receipt_path, record)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            entry = job_log_entry(source, output, start_byte=record.get("log_start_byte", 0),
                                  error_start_byte=record.get("error_log_start_byte", 0))
            if entry is not None:
                if entry["state"] == "error":
                    raise RuntimeError(f"AME failed source={source} output={output}: {entry['detail']}")
                completion = entry["detail"] if entry["state"] == "complete" else None
                if completion is not None:
                    evidence = folder / "ame-completion-log.txt"
                    if not evidence.exists():
                        evidence.write_text(completion)
                    lines = status.read_text(errors="replace").splitlines() if status.exists() else []
                    record.update(completion_signal="AMEEncodingLog exact source/output success",
                                  completion_log_sha256=sha256(evidence),
                                  ame_build=lines[3] if len(lines) >= 4 else None)
                    break
            if status.exists():
                lines = status.read_text(errors="replace").splitlines()
                if len(lines) >= 4:
                    if lines[0] != job_id:
                        raise RuntimeError("AME status job ID mismatch")
                    if lines[1] == "error":
                        if lines[3] == "dispatch":
                            # A late BridgeTalk transport callback can overwrite
                            # a prior queued status. The queue state is unknown.
                            record.update(state="incomplete_or_pending", error_detail=lines[2])
                            save(receipt_path, record)
                            return record
                        raise RuntimeError(f"AME failed source={source} output={output}: {lines[2]}; see {status}")
                    if lines[1] == "uncertain":
                        record.update(state="incomplete_or_pending", error_detail=lines[2])
                        save(receipt_path, record)
                        return record
                    if lines[1] == "complete":
                        record.update(ame_build=lines[3], ame_result=lines[2])
                        break
                    if lines[1] == "queued" and queue_only:
                        record["state"] = "queued"
                        save(receipt_path, record)
                        return record
                    if lines[1] not in ("starting", "configuring", "queued"):
                        raise RuntimeError("Unknown AME job status")
            time.sleep(0.25)
        else:
            raise TimeoutError("Own AME job is still pending; shared queue preserved, do not resubmit")
        if not output.is_file() or not output.stat().st_size:
            raise RuntimeError("AME completion did not produce a nonempty MP4")
        record.update(state="complete", output_sha256=sha256(output), output_bytes=output.stat().st_size)
        save(receipt_path, record)
        return record
    except BaseException as error:
        record.update(state="incomplete_or_pending", error_type=type(error).__name__,
                      error_detail=str(error))
        save(receipt_path, record)
        # Never terminate AME or remove a queued item, even on caller cancellation.
        raise


def verify_headers(reference: dict, composition: dict) -> None:
    """Container metadata only; never decode frames or compare against an FX render."""
    duration = composition["duration_numerator"] / composition["duration_denominator"]
    if not math.isclose(reference["fps"], 30, rel_tol=0, abs_tol=0.001):
        raise ValueError("AME output is not 30fps; do not relabel or resample")
    if reference["width"] != composition["width"] or reference["height"] != composition["height"]:
        raise ValueError("AME output does not preserve the native canvas")
    if not math.isclose(reference["duration_seconds"], duration, rel_tol=0, abs_tol=1 / 30 + 0.001):
        raise ValueError("AME output duration does not cover the full composition")
    if reference["frame_count"] != composition["expected_frame_count"]:
        raise ValueError("AME MP4 frame count does not cover the full composition at 30fps")
