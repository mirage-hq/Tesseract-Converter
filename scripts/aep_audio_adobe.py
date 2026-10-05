"""Legacy Adobe helper: read-only diagnostics and a retired raw-JSX API.

Native execution belongs to the optional installed headless-adobe typed CLI.
This module never launches, scripts, quits, or signals Adobe.
"""
from __future__ import annotations

from contextlib import closing
from pathlib import Path
import subprocess

DEFAULT_ADOBE = Path('/Applications/Adobe After Effects 2026/Adobe After Effects 2026.app/Contents/MacOS/After Effects')


def assert_no_adobe() -> None:
    result = subprocess.run(['ps', '-axo', 'pid=,comm='], capture_output=True,
                            text=True, timeout=10, check=False)
    if result.returncode != 0:
        raise RuntimeError('Cannot establish exclusive Adobe session ownership')
    hosts = {'After Effects', 'After Effects Render Engine', 'aerender', 'aerendercore'}
    for line in result.stdout.splitlines():
        fields = line.strip().split(None, 1)
        if len(fields) == 2 and Path(fields[1]).name in hosts:
            raise RuntimeError('An Adobe session is already running; refusing to attach or close it')


def wait_for_gui_exit(executable: Path, timeout: int) -> None:
    """Read-only wait retained for diagnostics; never poll or signal AE."""
    import select
    result = subprocess.run(['ps', '-axo', 'pid=,comm='], capture_output=True,
                            text=True, check=True, timeout=10)
    pids = [int(parts[0]) for line in result.stdout.splitlines()
            if len(parts := line.strip().split(None, 1)) == 2
            and parts[1] == str(executable)]
    if len(pids) > 1:
        raise RuntimeError('Ambiguous Adobe hosts after quit; sessions preserved')
    if not pids:
        return
    with closing(select.kqueue()) as queue:
        try:
            queue.control([select.kevent(pids[0], filter=select.KQ_FILTER_PROC,
                           flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                           fflags=select.KQ_NOTE_EXIT)], 0, 0)
        except ProcessLookupError:
            return
        if not queue.control(None, 1, timeout):
            raise RuntimeError('Adobe quit did not complete; GUI session preserved')


def run_adobe(body: str, work: Path, *, project: Path | None = None,
              executable: Path = DEFAULT_ADOBE, timeout: int = 180,
              owned_pid: int | None = None, quit_after: bool = True,
              use_script_file: bool = False) -> dict:
    """Reject the retired arbitrary JSX/session-borrowing interface."""
    raise RuntimeError(
        'run_adobe raw JSX execution is retired. Use adobe_native.execute with '
        'a supported typed headless-adobe operation; configure the installed CLI '
        'with HEADLESS_ADOBE_COMMAND. Borrowing a PID or leaving Adobe open is '
        'unsupported. Missing authoring/readback profiles require a scoped '
        'central-worker extension; direct Adobe execution is not a fallback.'
    )
