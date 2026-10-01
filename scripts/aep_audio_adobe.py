"""Run a reviewed JSX in a fresh, exclusively owned After Effects process.

Only test tooling calls this module. Never attach to a running Adobe session.
The JSX body defines ``runAudioCase(project)`` and returns JSON-safe readback.
"""

from __future__ import annotations

from contextlib import closing
import json
from pathlib import Path
import subprocess

DEFAULT_ADOBE = Path('/Applications/Adobe After Effects 2026/Adobe After Effects 2026.app/Contents/MacOS/After Effects')


def assert_no_adobe() -> None:
    result = subprocess.run(['ps', '-axo', 'pid=,comm='], capture_output=True,
                            text=True, timeout=10, check=False)
    if result.returncode != 0:
        raise RuntimeError('Cannot establish exclusive Adobe session ownership')
    # Match executable identities, not shell argv or persistent crash/reporting
    # helpers whose install path also contains "After Effects".
    hosts = {'After Effects', 'After Effects Render Engine', 'aerender', 'aerendercore'}
    for line in result.stdout.splitlines():
        fields = line.strip().split(None, 1)
        if len(fields) == 2 and Path(fields[1]).name in hosts:
            raise RuntimeError('An Adobe session is already running; refusing to attach or close it')


def wait_for_gui_exit(executable: Path, timeout: int) -> None:
    """Wait for asynchronous app.quit with kernel events, never poll or signal AE."""
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
    """Execute trusted test JSX; preserve its readback and complete native log.

    ``work`` must not exist. Opening a supplied project is read-only: the owned
    project is closed without saving. Authoring bodies must check new destinations.
    ``use_script_file`` opts into a short evalFile bridge instead of transporting
    the complete JSX as AppleScript text. It does not bypass ownership guards or
    suppress native UI prompts; timeouts preserve the GUI and must not be retried
    until its still-running script has finished or been safely cancelled.
    """
    executable = executable.resolve(strict=True)
    if project is not None:
        project = project.resolve(strict=True)
    if owned_pid is None:
        assert_no_adobe()
    else:
        probe = subprocess.run(['ps', '-p', str(owned_pid), '-o', 'comm='],
                               capture_output=True, text=True, check=True, timeout=10)
        if probe.stdout.strip() != str(executable):
            raise RuntimeError('Explicit owned Adobe PID no longer identifies the expected host')
    work.mkdir(parents=True, exist_ok=False)
    receipt = work / 'readback.json'
    source = work / 'run.jsx'
    opening = ('app.open(new File(' + json.dumps(str(project)) + '))'
               if project is not None else 'app.newProject()')
    # ExtendScript installations do not universally provide JSON.stringify.
    # The encoder handles only the JSON-safe plain values returned by our JSX.
    wrapper = r'''(function () {
    function quote(s) {
        return '"' + String(s).replace(/[\\"\x00-\x1f]/g, function(c) {
            if (c === '"') return '\\"';
            if (c === '\\') return '\\\\';
            var h = c.charCodeAt(0).toString(16);
            return '\\u' + ('0000' + h).slice(-4);
        }) + '"';
    }
    function encode(v) {
        if (v === null || typeof v === 'undefined') return 'null';
        if (typeof v === 'string') return quote(v);
        if (typeof v === 'number') {
            if (!isFinite(v)) throw new Error('nonfinite readback');
            return String(v);
        }
        if (typeof v === 'boolean') return v ? 'true' : 'false';
        var parts = [], i, k;
        if (v instanceof Array) {
            for (i = 0; i < v.length; i++) parts.push(encode(v[i]));
            return '[' + parts.join(',') + ']';
        }
        for (k in v) if (v.hasOwnProperty(k)) parts.push(quote(k) + ':' + encode(v[k]));
        return '{' + parts.join(',') + '}';
    }
    function publish(value) {
        var f = new File(RECEIPT_PATH);
        if (f.exists || !f.open('w')) throw new Error('readback destination unavailable');
        f.encoding = 'UTF-8'; f.write(encode(value)); f.close();
    }
    var owned = null, safe = false, answer = null, failure = null;
    try {
        var prior = app.project;
        if (prior && (prior.file !== null || prior.dirty || prior.numItems !== 0))
            throw new Error('existing file-backed/dirty/nonempty project; not owned');
        owned = OPEN_PROJECT;
        if (!owned || app.project !== owned) throw new Error('cannot establish project ownership');
        FEATURE_BODY
        answer = runAudioCase(owned);
        if (app.project !== owned) throw new Error('project ownership changed during test');
    } catch (error) { failure = String(error); }
    if (owned && app.project === owned) {
        owned.close(CloseOptions.DO_NOT_SAVE_CHANGES);
        safe = !app.project || (app.project.file === null && !app.project.dirty && app.project.numItems === 0);
    }
    publish({status: failure === null ? 'ok' : 'failed', error: failure,
             adobe_version: app.version, adobe_build: app.buildNumber,
             owned_project_closed: safe, result: answer});
    // This executable was started only after rejecting all existing AE processes.
    // Never quit when ownership was lost or a different project appeared.
    if (safe) app.quit();
})();
'''
    wrapper = wrapper.replace('RECEIPT_PATH', json.dumps(str(receipt.resolve())))
    wrapper = wrapper.replace('OPEN_PROJECT', opening).replace('FEATURE_BODY', body)
    if not quit_after:
        wrapper = wrapper.replace('if (safe) app.quit();', '// Leave explicitly owned authoring host open.')
    source.write_text(wrapper, encoding='utf-8')
    # The normal .app launch goes through LaunchServices; DoScript receives the
    # actual JSX text by default, with an opt-in native evalFile bridge below.
    app_path = executable.parent.parent.parent
    if app_path.suffix != '.app':
        raise ValueError(f'Adobe executable is not inside an .app: {executable}')

    def applescript_string(value: str) -> str:
        return '"' + value.replace('\\', '\\\\').replace('"', '\\"') + '"'

    script = (f'set scriptText to read (POSIX file {applescript_string(str(source.resolve()))}) '
              'as «class utf8»\n'
              f'with timeout of {timeout} seconds\n'
              f'    tell application {applescript_string(str(app_path))}\n'
              '        DoScript scriptText\n'
              '    end tell\n'
              'end timeout')
    if use_script_file:
        invocation = '$.evalFile(new File(' + json.dumps(str(source.resolve())) + '));'
        script = (f'with timeout of {timeout} seconds\n'
                  f'    tell application {applescript_string(str(app_path))} to '
                  f'DoScript {applescript_string(invocation)}\n'
                  'end timeout')
    with (work / 'adobe.log').open('wb') as log:
        try:
            result = subprocess.run(['osascript', '-e', script], stdin=subprocess.DEVNULL,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=timeout + 5,
                                    check=False)
        except subprocess.TimeoutExpired:
            # subprocess.run may stop osascript, but never signals the GUI app.
            raise RuntimeError('AppleScript timed out; Adobe GUI session preserved; '
                               'inspect readback.json and adobe.log') from None
    code = result.returncode
    if not receipt.is_file():
        raise RuntimeError(f'AppleScript exited {code} without native readback; '
                           'Adobe GUI session preserved; inspect adobe.log')
    state = json.loads(receipt.read_text(encoding='utf-8-sig'))
    if code or state.get('status') != 'ok' or not state.get('owned_project_closed'):
        raise RuntimeError(f'Adobe test failed (exit {code}): {state.get("error")}')
    if quit_after:
        wait_for_gui_exit(executable, timeout)
    return state
