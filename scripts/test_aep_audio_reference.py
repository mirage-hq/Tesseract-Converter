"""Offline typed audio settings/render regression tests; never execute Adobe."""
import json
from pathlib import Path
from unittest import mock

import pytest
import aep_audio_reference as audio


@pytest.fixture
def native(tmp_path, monkeypatch):
    fixture = tmp_path / 'fixtures'
    fixture.mkdir()
    for name in ('source.aep', 'one.wav', 'two.wav'):
        (fixture / name).write_bytes(name.encode())
    source = {'path': 'source.aep', 'sha256': audio.sha256(fixture / 'source.aep'),
              'composition_id': 199, 'composition_name': 'audio'}
    case = {'id': 'audio', 'source': source, 'reference': None,
            'primary': [{'path': 'one.wav', 'sha256': audio.sha256(fixture / 'one.wav')}]}
    sibling = {**case, 'primary': [{'path': 'two.wav', 'sha256': audio.sha256(fixture / 'two.wav')}]}
    manifest = fixture / 'cases.json'
    manifest.write_text(json.dumps({'cases': [case, sibling]}))
    monkeypatch.setattr(audio, 'FIXTURE', fixture)
    monkeypatch.setattr(audio, 'MANIFEST', manifest)
    settings = {'compositionId': 199, 'compositionName': 'audio', 'width': 320,
                'height': 180, 'sourceFps': 24, 'duration': 6,
                'renderSettings': {'Use this frame rate': '30'},
                'outputSettings': {'Format': 'H.264'}, 'settableSettings': {'Output Audio': 'On'},
                'rendered': False, 'outputExists': False}
    return case, settings


def test_settings_probe_pins_all_shared_source_media(native, tmp_path):
    case, settings = native
    with mock.patch.object(audio.adobe_native, 'execute', return_value={'kind': 'json'}) as execute, \
         mock.patch.object(audio.adobe_native, 'read_json', return_value=settings):
        audio.render(case, tmp_path / 'work', settings_only=True)
    operation, request, _ = execute.call_args.args
    assert operation == 'inspect_aep'
    assert request['profile'] == 'audio_render_settings'
    assert request['composition_id'] == '199'
    assert {Path(item['path']).name for item in request['source']['dependencies'].values()} == {'one.wav', 'two.wav'}
    assert not (tmp_path / 'work/reference.mp4').exists()


@pytest.mark.parametrize('field,value', [('sourceFps', 30), ('duration', 5), ('width', 640),
    ('compositionId', 200), ('compositionName', 'wrong'), ('rendered', True), ('outputExists', True)])
def test_settings_identity_drift_never_renders(native, tmp_path, field, value):
    case, settings = native
    settings[field] = value
    with mock.patch.object(audio.adobe_native, 'execute', return_value={}) as execute, \
         mock.patch.object(audio.adobe_native, 'read_json', return_value=settings):
        with pytest.raises(RuntimeError, match='identity/settings drift'):
            audio.render(case, tmp_path / 'work')
    assert execute.call_count == 1


def test_render_follows_native_settings_and_preserves_full_range(native, tmp_path):
    case, settings = native
    artifact = {'path': 'receipt', 'provenance': {'renderer': 'After Effects'}}
    def copy(_artifact, output):
        output.write_bytes(b'native mp4')
    with mock.patch.object(audio.adobe_native, 'execute', return_value=artifact) as execute, \
         mock.patch.object(audio.adobe_native, 'read_json', return_value=settings), \
         mock.patch.object(audio.adobe_native, 'copy_artifact', side_effect=copy), \
         mock.patch.object(audio, '_validate_video', return_value={'bytes': 10, 'sha256': 'native'}), \
         mock.patch.object(audio, 'validate_audio', return_value={'rate': 48000}), \
         mock.patch.object(audio, 'validate_black_canvas', return_value={'maximum_rgb_channel': 0}):
        audio.render(case, tmp_path / 'work')
    assert [call.args[0] for call in execute.call_args_list] == ['inspect_aep', 'render_aep']
    assert execute.call_args.args[1]['settings'] == {'format': 'mp4', 'fps': 30, 'audio': 'on'}
    evidence = json.loads((tmp_path / 'work/validated.json').read_text())
    assert evidence['native_settings'] == settings
    assert evidence['native_receipt'] == artifact
    assert evidence['source_copy_sha256'] == case['source']['sha256']


def test_borrowed_pid_rejected_before_worker(native, tmp_path):
    with mock.patch.object(audio.adobe_native, 'execute') as execute:
        with pytest.raises(RuntimeError, match='Borrowed'):
            audio.render(native[0], tmp_path / 'work', owned_pid=123)
    execute.assert_not_called()
