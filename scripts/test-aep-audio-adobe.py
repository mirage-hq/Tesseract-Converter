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
@pytest.mark.parametrize('owned_pid,quit_after', ((None, True), (123, False)))
def test_retired_raw_execution_never_launches_or_touches_work(
        tmp_path, monkeypatch, use_script_file, owned_pid, quit_after):
    launch = Mock(side_effect=AssertionError("native execution forbidden"))
    monkeypatch.setattr('aep_audio_adobe.subprocess.run', launch)
    work = tmp_path / 'work'
    with pytest.raises(RuntimeError, match='raw JSX execution is retired'):
        run_adobe('function runAudioCase(p) { return {}; }', work,
                  use_script_file=use_script_file, owned_pid=owned_pid,
                  quit_after=quit_after)
    launch.assert_not_called()
    assert not work.exists()


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
