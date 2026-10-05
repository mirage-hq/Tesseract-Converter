"""Offline regressions for the conversion callers' central worker boundary."""
from pathlib import Path
from unittest import mock
import hashlib

import pytest
import adobe_audio_cases as audio
import adobe_native
from aep_audio_adobe import run_adobe


def test_raw_jsx_is_rejected_without_process_or_files(tmp_path):
    with mock.patch('aep_audio_adobe.subprocess.run') as process:
        with pytest.raises(RuntimeError, match='raw JSX execution is retired'):
            run_adobe('app.quit()', tmp_path / 'native', owned_pid=123, quit_after=False)
    process.assert_not_called()
    assert not (tmp_path / 'native').exists()


def test_audio_acceptance_dispatches_native_frames_and_audio(tmp_path):
    exported = tmp_path / 'export/conversion/project.aep'
    exported.parent.mkdir(parents=True)
    exported.write_bytes(b'pinned-export')
    native = tmp_path / 'native.mp4'
    native.write_bytes(b'mock-native')
    log = tmp_path / 'aerender.log'
    log.write_text('PROGRESS: Done')
    artifact = {'path': str(native), 'sha256': hashlib.sha256(native.read_bytes()).hexdigest(),
                'kind': 'video', 'metadata': {'renderer': 'mock-only', 'render_log': {
                    'path': str(log), 'sha256': adobe_native.sha256(log)}}}
    case = {'slug': 'audio-case', 'primary': [], 'reference': {'path': 'pinned.mp4'},
            'reference_expectation': {}, 'policy': {}}
    with mock.patch.object(adobe_native, 'execute', return_value=artifact) as execute, \
         mock.patch.object(audio.e2e, '_video_contract', return_value={}), \
         mock.patch.object(audio.e2e, '_acquire_reference', return_value=native), \
         mock.patch.object(audio.e2e, '_reference_windows'), \
         mock.patch.object(audio, 'compare', return_value={'passed': True}):
        phase = audio._native_acceptance(case=case, work=tmp_path,
                                         aerender='/missing/retired/aerender', timeout=10)
    operation, request, work = execute.call_args.args
    assert operation == 'render_aep'
    assert request['composition_id'] == 'audio-case'
    assert request['settings'] == {'format': 'mp4', 'fps': 30, 'start_frame': 0,
                                   'end_frame': 143, 'audio': 'on'}
    assert request['source']['sha256'] == hashlib.sha256(b'pinned-export').hexdigest()
    assert phase['native_artifact'] == artifact
    assert (tmp_path / 'export/adobe-native.mp4').read_bytes() == b'mock-native'


def test_audio_source_mutation_is_checked_on_native_failure(tmp_path):
    exported = tmp_path / 'export/conversion/project.aep'
    exported.parent.mkdir(parents=True)
    exported.write_bytes(b'original')
    def fail(*args, **kwargs):
        exported.write_bytes(b'mutated')
        raise adobe_native.NativeAdobeError('native failed')
    with mock.patch.object(adobe_native, 'execute', side_effect=fail):
        with pytest.raises(audio.AudioAdapterError, match='changed during native acceptance'):
            audio._native_acceptance(case={'slug': 'case', 'primary': []}, work=tmp_path,
                                     aerender='retired', timeout=10)
