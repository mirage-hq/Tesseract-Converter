#!/usr/bin/env python3
"""Ownership/launcher unit tests; never starts Adobe."""
import json
import subprocess
from unittest.mock import Mock

import pytest

from aep_audio_adobe import assert_no_adobe, run_adobe


@pytest.mark.parametrize('name', ('After Effects', 'After Effects Render Engine',
                                   'aerender', 'aerendercore'))
def test_active_host_is_rejected_without_attaching(monkeypatch, name):
    monkeypatch.setattr('aep_audio_adobe.subprocess.run',
                        Mock(return_value=Mock(stdout='123 /Applications/Adobe/' + name, returncode=0)))
    with pytest.raises(RuntimeError, match='already running'):
        assert_no_adobe()


def test_helper_and_shell_argv_are_not_native_sessions(monkeypatch):
    monkeypatch.setattr('aep_audio_adobe.subprocess.run', Mock(return_value=Mock(
        stdout='10 /Applications/Adobe After Effects 2026/crashpad_handler\n11 /bin/zsh',
        returncode=0)))
    assert_no_adobe()


def test_failed_ownership_probe_fails_closed(monkeypatch):
    monkeypatch.setattr('aep_audio_adobe.subprocess.run',
                        Mock(return_value=Mock(stdout='', returncode=1)))
    with pytest.raises(RuntimeError, match='Cannot establish'):
        assert_no_adobe()


@pytest.mark.parametrize('use_script_file', (False, True))
@pytest.mark.parametrize('status,closed,code,success', (
    ('ok', True, 0, True), ('failed', True, 0, False),
    ('ok', False, 0, False), ('ok', True, 1, False),
))
def test_native_readback_requires_owned_close_and_success(
        tmp_path, monkeypatch, status, closed, code, success, use_script_file):
    exe = tmp_path / 'Adobe.app' / 'Contents' / 'MacOS' / 'After Effects'
    exe.parent.mkdir(parents=True)
    exe.touch()
    work = tmp_path / 'work'
    wait_exit = Mock()
    monkeypatch.setattr('aep_audio_adobe.assert_no_adobe', Mock())
    monkeypatch.setattr('aep_audio_adobe.wait_for_gui_exit', wait_exit)

    def launch(argv, **kwargs):
        assert argv[0:2] == ['osascript', '-e']
        assert 'tell application "' + str((tmp_path / 'Adobe.app').resolve()) + '"' in argv[2]
        if use_script_file:
            assert '$.evalFile(new File(' in argv[2]
            assert 'read (POSIX file' not in argv[2]
            assert str((work / 'run.jsx').resolve()) in argv[2]
        else:
            assert 'DoScript scriptText' in argv[2]
            assert 'read (POSIX file' in argv[2]
        assert 'with timeout of 180 seconds' in argv[2]
        assert kwargs['timeout'] == 185
        script = (work / 'run.jsx').read_text()
        assert 'prior.dirty' in script
        assert 'app.project !== owned' in script
        assert 'if (safe) app.quit()' in script
        (work / 'readback.json').write_text(json.dumps({
            'status': status, 'owned_project_closed': closed,
            'result': {'synthetic': True}}))
        return Mock(returncode=code)

    monkeypatch.setattr('aep_audio_adobe.subprocess.run', launch)
    if success:
        result = run_adobe('function runAudioCase(p) { return {}; }', work,
                           executable=exe, use_script_file=use_script_file)
        assert result['owned_project_closed']
        wait_exit.assert_called_once_with(exe.resolve(), 180)
    else:
        with pytest.raises(RuntimeError, match='Adobe test failed'):
            run_adobe('function runAudioCase(p) { return {}; }', work,
                      executable=exe, use_script_file=use_script_file)


def test_lost_ownership_receipt_preserves_native_process(tmp_path, monkeypatch):
    exe = tmp_path / 'Adobe.app' / 'Contents' / 'MacOS' / 'After Effects'
    exe.parent.mkdir(parents=True)
    exe.touch()
    work = tmp_path / 'work'

    def launch(*args, **kwargs):
        (work / 'readback.json').write_text(json.dumps({'owned_project_closed': False}))
        raise subprocess.TimeoutExpired('osascript', 185)

    run = Mock(side_effect=launch)
    monkeypatch.setattr('aep_audio_adobe.assert_no_adobe', Mock())
    monkeypatch.setattr('aep_audio_adobe.subprocess.run', run)
    with pytest.raises(RuntimeError, match='session preserved'):
        run_adobe('function runAudioCase(p) { return {}; }', work, executable=exe)
    assert run.call_count == 1


@pytest.mark.parametrize('events', ([object()], []))
def test_exit_wait_closes_non_context_manager_kqueue(tmp_path, monkeypatch, events):
    from types import SimpleNamespace
    import select
    from aep_audio_adobe import wait_for_gui_exit

    executable = tmp_path / 'After Effects'
    monkeypatch.setattr('aep_audio_adobe.subprocess.run', Mock(return_value=Mock(
        stdout=f'123 {executable}\n', returncode=0)))
    queue = SimpleNamespace(control=Mock(side_effect=[[], events]), close=Mock())
    monkeypatch.setattr(select, 'kqueue', lambda: queue, raising=False)
    monkeypatch.setattr(select, 'kevent', Mock(), raising=False)
    for name in ('KQ_FILTER_PROC', 'KQ_EV_ADD', 'KQ_EV_ONESHOT', 'KQ_NOTE_EXIT'):
        monkeypatch.setattr(select, name, 1, raising=False)
    if events:
        wait_for_gui_exit(executable, 5)
    else:
        with pytest.raises(RuntimeError, match='GUI session preserved'):
            wait_for_gui_exit(executable, 5)
    queue.close.assert_called_once()
