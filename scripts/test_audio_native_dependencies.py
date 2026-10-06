"""Hermetic dependency checks; no Adobe execution or fidelity claim."""
from pathlib import Path
from unittest import mock

import pytest
import adobe_audio_cases as audio


def setup_case(tmp_path):
    primary = tmp_path / 'primary.wav'
    primary.write_bytes(b'pinned audio')
    root = tmp_path / 'work/export/conversion'
    root.mkdir(parents=True)
    copy = root / 'renamed.wav'
    copy.write_bytes(primary.read_bytes())
    case = {'primary': [{'path': 'primary.wav', 'sha256': audio.sha256(primary)}]}
    return primary, copy, case, tmp_path / 'work'


def test_actual_copy_paths_and_original_are_pinned(tmp_path):
    primary, copy, case, work = setup_case(tmp_path)
    with mock.patch.object(audio, 'FIXTURES', tmp_path):
        pins = audio._native_dependencies(case, work)
    assert {pin['path'] for pin in pins.values()} == {str(primary), str(copy)}
    assert {pin['sha256'] for pin in pins.values()} == {audio.sha256(primary)}


def test_changed_copy_rejected(tmp_path):
    _, copy, case, work = setup_case(tmp_path)
    copy.write_bytes(b'changed')
    with mock.patch.object(audio, 'FIXTURES', tmp_path), pytest.raises(audio.AudioAdapterError, match='changed or unexpected'):
        audio._native_dependencies(case, work)


def test_symlink_copy_rejected(tmp_path):
    primary, copy, case, work = setup_case(tmp_path)
    copy.unlink()
    copy.symlink_to(primary)
    with mock.patch.object(audio, 'FIXTURES', tmp_path), pytest.raises(audio.AudioAdapterError, match='symlink'):
        audio._native_dependencies(case, work)
