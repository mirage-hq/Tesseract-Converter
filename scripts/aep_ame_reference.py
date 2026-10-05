"""Independent AEP references through the optional typed headless-adobe worker.

No shared queue, BridgeTalk, borrowed Adobe host, or independent native launch.
Legacy log inspection is read-only; a historical log is never a new API success.
"""
from __future__ import annotations

import json
import math
import os
from pathlib import Path
import re
import uuid
import xml.etree.ElementTree as ET

import adobe_native


def sha256(path: Path) -> str:
    return adobe_native.sha256(path)


def save(path: Path, value: dict) -> None:
    temporary = path.with_suffix(path.suffix + '.tmp')
    with temporary.open('x') as output:
        json.dump(value, output, indent=2)
        output.write('\n')
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(path)


def inspect_preset(path: Path, expected_sha: str, composition: dict) -> dict:
    if not path.is_absolute() or path.suffix.lower() != '.epr' or not path.is_file():
        raise ValueError('Specify an absolute, existing AME H.264 .epr preset')
    actual = sha256(path)
    if not re.fullmatch(r'[0-9a-f]{64}', expected_sha) or actual != expected_sha:
        raise ValueError('AME preset SHA-256 differs from the selected preset')
    root = ET.parse(path).getroot()
    if (root.findtext('DoVideo') != 'true' or root.findtext('ExporterClassID') != '1313424203'
            or root.findtext('ExporterFileType') != '1211250228'):
        raise ValueError('AME preset must enable the H.264 MP4 exporter')
    expected = {'ADBEVideoFPS': '8467200000', 'ADBEVideoWidth': str(composition['width']),
                'ADBEVideoHeight': str(composition['height'])}
    for key, value in expected.items():
        params = [p for p in root.iter('ExporterParam') if p.findtext('ParamIdentifier') == key]
        if (len(params) != 1 or params[0].findtext('ParamIsDisabled') == 'true'
                or params[0].findtext('ParamValue') != value):
            raise ValueError(f'Preset must explicitly fix {key} to {value}, not Match Source')
    if root.findtext('StandardFilters/CropType') != '0':
        raise ValueError('Preset must preserve the full uncropped canvas')
    return {'path': str(path), 'sha256': actual, 'format': 'H.264 MP4', 'fps': 30,
            'fps_control': 'fixed preset and EncoderWrapper.setFrameRate("30")',
            'canvas': {'width': composition['width'], 'height': composition['height']},
            'crop': 'none', 'preset_name': root.findtext('PresetName')}


def _exact_log_entry(source: Path, output: Path, log: Path, start_byte: int) -> dict | None:
    try:
        data = log.read_bytes()
    except FileNotFoundError:
        return None
    if start_byte:
        if len(data) < start_byte:
            return None
        encoding = 'utf-16-le' if data.startswith(b'\xff\xfe') else 'utf-16-be' if data.startswith(b'\xfe\xff') else 'utf-8'
        text = data[start_byte:].decode(encoding, errors='replace')
    else:
        text = data.decode('utf-16' if data.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig', errors='replace')
    for block in reversed(re.split(r'(?=^\s*- Source File: )', text, flags=re.MULTILINE)):
        lines = block.splitlines()
        if (not lines or lines[0].strip() != '- Source File: ' + str(source)
                or not any(line.strip() == '- Output File: ' + str(output) for line in lines)):
            continue
        block = block.split('Queue Stopped')[0]
        failed = bool(re.search(r'(?:Encoding Failed|File Failed|Error (?:Code|Compiling|Rendering)|\bError\s*:|\bFailed\s*:)', block, re.I))
        return {'state': 'error' if failed else 'complete' if 'File Successfully Encoded' in block else 'pending',
                'detail': block, 'log_path': str(log)}
    return None


def job_log_entry(source: Path, output: Path, log: Path | None = None, *, start_byte: int = 0,
                  error_log: Path | None = None, error_start_byte: int = 0) -> dict | None:
    """Read-only legacy source/output lookup; never queues or controls Adobe."""
    if log is None:
        log = Path.home() / 'Documents/Adobe/Adobe Media Encoder/26.0/AMEEncodingLog.txt'
        error_log = error_log or log.with_name('AMEEncodingErrorLog.txt')
    if error_log is not None:
        error = _exact_log_entry(source, output, error_log, error_start_byte)
        if error and error['state'] == 'error':
            return error
    return _exact_log_entry(source, output, log, start_byte)


def completed_log_entry(source: Path, output: Path) -> str | None:
    log = Path.home() / 'Documents/Adobe/Adobe Media Encoder/26.0/AMEEncodingLog.txt'
    entry = _exact_log_entry(source, output, log, 0)
    return entry['detail'] if entry and entry['state'] == 'complete' else None


def produce(source: Path, output: Path, composition: dict, folder: Path,
            app: Path, preset: Path, expected_sha: str, timeout: int,
            state_dir: Path | None = None, *, queue_only: bool = False,
            root_only: bool = True, dependencies: dict | None = None) -> dict:
    if queue_only:
        raise ValueError('Shared queue-only mode is retired; use a synchronous owned headless-adobe operation')
    guid = composition.get('dynamic_link_guid')
    if not isinstance(guid, str) or not guid.strip():
        raise ValueError('Authored Dynamic Link GUID is required; numeric AE ID is NOT a GUID')
    if not 30 <= timeout <= 3600:
        raise ValueError('AME job timeout must be in [30, 3600] seconds')
    source, output, folder = source.resolve(), output.resolve(), folder.resolve()
    preset_info = inspect_preset(preset.resolve(), expected_sha, composition)
    folder.mkdir(parents=True, exist_ok=True)
    receipt = folder / 'ame-producer.json'
    identity = {'source_path': str(source), 'source_sha256': sha256(source),
                'composition_id': composition['composition_id'], 'composition_name': composition['composition_name'],
                'dynamic_link_guid': guid, 'root_only': root_only, 'preset': preset_info,
                'requested_ame_app': str(app), 'output_path': str(output), 'dependencies': dependencies or {}}
    if receipt.exists():
        record = json.loads(receipt.read_text())
        if record.get('transport') != 'headless-adobe':
            raise ValueError('Legacy shared-queue receipt requires operator reconciliation; never resubmit it')
        if any(record.get(key) != value for key, value in identity.items()):
            raise ValueError('Existing AME request has different input identity; refusing reuse')
        if record.get('state') != 'complete':
            raise ValueError('Prior AME attempt is incomplete; inspect the central worker fence, never blindly resubmit')
        if not output.is_file() or sha256(output) != record.get('output_sha256'):
            raise ValueError('Completed AME output no longer matches its receipt')
        return record
    if output.exists() or any((folder / name).exists() for name in ('ame-status.txt', 'ame-export.jsx', 'ame-dispatch.jsx')):
        raise ValueError('Unidentified prior AME artifacts; never duplicate an uncertain queued job')
    job_id = uuid.uuid4().hex
    record = {**identity, 'job_id': job_id, 'state': 'prepared', 'transport': 'headless-adobe',
              'tests_executed': False, 'visual_inspection': 'not_run', 'fidelity': 'unmeasured'}
    save(receipt, record)
    try:
        artifact = adobe_native.execute('render_aep_ame', {
            'source': adobe_native.source_ref(source, dependencies),
            'composition_id': str(composition['composition_id']), 'dynamic_link_guid': guid,
            'root_only': root_only, 'preset': {'path': str(preset.resolve()), 'sha256': expected_sha},
            'request_id': 'ae-ame-reference-' + job_id,
        }, folder / 'worker', timeout=timeout, ame_app=app)
        metadata = artifact.get('metadata', {})
        if not metadata.get('build') or not metadata.get('completion'):
            raise adobe_native.NativeAdobeError('AME native build/completion provenance is missing')
        if sha256(source) != identity['source_sha256']:
            raise ValueError('Source changed during native rendering; refusing publication')
        adobe_native.copy_artifact(artifact, output)
        record.update(state='complete', output_sha256=artifact['sha256'], output_bytes=output.stat().st_size,
                      ame_build=metadata['build'], completion_signal=metadata['completion'], headless_adobe=artifact)
        save(receipt, record)
        return record
    except BaseException as error:
        record.update(state='incomplete_or_pending', error_type=type(error).__name__, error_detail=str(error))
        save(receipt, record)
        raise


def verify_headers(reference: dict, composition: dict) -> None:
    duration = composition['duration_numerator'] / composition['duration_denominator']
    if not math.isclose(reference['fps'], 30, rel_tol=0, abs_tol=0.001):
        raise ValueError('AME output is not 30fps; do not relabel or resample')
    if reference['width'] != composition['width'] or reference['height'] != composition['height']:
        raise ValueError('AME output does not preserve the native canvas')
    if not math.isclose(reference['duration_seconds'], duration, rel_tol=0, abs_tol=1 / 30 + 0.001):
        raise ValueError('AME output duration does not cover the full composition')
    if reference['frame_count'] != composition['expected_frame_count']:
        raise ValueError('AME MP4 frame count does not cover the full composition at 30fps')
