"""Optional typed headless-adobe CLI adapter; never launches Adobe directly.

Standalone/offline converter workflows do not import the private worker package.
Native maintainer workflows require an installed headless-adobe command, or an
explicit JSON argv prefix in HEADLESS_ADOBE_COMMAND. There is no native fallback.
"""
from __future__ import annotations

import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import signal
import subprocess
from typing import Any

OPERATIONS = frozenset({"create_aep", "create_premiere", "render_aep", "render_premiere",
                       "inspect_aep", "capture_aep_expression_samples",
                       "render_aep_ame", "render_premiere_ame"})


class NativeAdobeError(RuntimeError):
    """The central worker rejected, failed, or could not verify an operation."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def source_ref(source: Path, dependencies: dict[str, Any] | None = None) -> dict[str, Any]:
    source = source.resolve(strict=True)
    return {"path": str(source), "sha256": sha256(source), "dependencies": dependencies or {}}


def command_prefix() -> list[str]:
    raw = os.environ.get("HEADLESS_ADOBE_COMMAND")
    try:
        command = json.loads(raw) if raw else ["headless-adobe"]
    except json.JSONDecodeError as error:
        raise NativeAdobeError("HEADLESS_ADOBE_COMMAND must be a JSON argv array") from error
    if (not isinstance(command, list) or not command
            or any(not isinstance(part, str) or not part or "\x00" in part for part in command)):
        raise NativeAdobeError("HEADLESS_ADOBE_COMMAND must be a nonempty JSON argv array")
    if not shutil.which(command[0]):
        raise NativeAdobeError("headless-adobe is unavailable; install the central worker or set "
                               "HEADLESS_ADOBE_COMMAND. Direct Adobe execution is not a fallback.")
    return command


def _settle_cli(child: subprocess.Popen) -> None:
    """Give the owned CLI time to run its own Adobe cleanup, never signal Adobe."""
    if child.poll() is not None:
        return
    child.send_signal(signal.SIGINT)
    try:
        child.communicate(timeout=120)
    except subprocess.TimeoutExpired:
        # An unresponsive CLI leaves its durable RUNNING fence. Never clear it
        # or submit again: recovery belongs to the central worker/operator.
        child.kill()
        child.communicate()


def execute(operation: str, request: dict[str, Any], work: Path, *, timeout: float = 300,
            ame_app: Path | None = None) -> dict[str, Any]:
    if operation not in OPERATIONS:
        raise NativeAdobeError("Unsupported typed Adobe operation")
    if not math.isfinite(timeout) or not 0 < timeout <= 3600:
        raise NativeAdobeError("Adobe timeout must be positive and at most 3600 seconds")
    work = work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    payload = dict(request)
    identity = hashlib.sha256(json.dumps({"operation": operation, "input": payload,
                                         "work": str(work), "ame_app": str(ame_app) if ame_app else None},
                                        sort_keys=True).encode()).hexdigest()
    payload.setdefault("request_id", "conv-" + identity[:48])
    request_path = work / (operation + "-request.json")
    encoded = json.dumps(payload, indent=2, sort_keys=True) + "\n"
    if request_path.exists() and request_path.read_text() != encoded:
        raise NativeAdobeError("Existing worker request differs; inspect it and use a fresh work directory")
    request_path.write_text(encoded)
    command = command_prefix() + [operation, str(request_path), "--dedicated-worker", "--timeout", str(timeout)]
    if ame_app is not None:
        command += ['--ame-app', str(ame_app.resolve())]
    child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True)
    try:
        stdout, stderr = child.communicate(timeout=timeout + 120)
    except BaseException:
        _settle_cli(child)
        raise
    (work / (operation + "-stdout.log")).write_text(stdout)
    (work / (operation + "-stderr.log")).write_text(stderr)
    try:
        result = json.loads(stdout)
    except (json.JSONDecodeError, TypeError) as error:
        raise NativeAdobeError("Central worker returned no valid JSON result; inspect the CLI logs") from error
    if (child.returncode != 0 or not isinstance(result, dict)
            or result.get("status") != "succeeded" or result.get("ready") is not True):
        failure = result.get("error", {}) if isinstance(result, dict) else {}
        raise NativeAdobeError(f"Central Adobe operation failed without publication: {failure}")
    artifact = result.get("artifact")
    if not isinstance(artifact, dict) or artifact.get("request_id") != payload["request_id"]:
        raise NativeAdobeError("Central worker artifact is absent or belongs to another request")
    try:
        path = Path(artifact["path"]).resolve(strict=True)
        if not path.is_file() or sha256(path) != artifact["sha256"]:
            raise NativeAdobeError("Central worker artifact hash changed")
    except (KeyError, TypeError, OSError) as error:
        raise NativeAdobeError("Central worker artifact receipt is invalid") from error
    (work / (operation + "-result.json")).write_text(json.dumps(result, indent=2) + "\n")
    return artifact


def read_render_log(artifact: dict[str, Any]) -> bytes:
    """Read the actual native log bound to this result; CLI stdout is not a render log."""
    ref = artifact.get('metadata', {}).get('render_log')
    if not isinstance(ref, dict) or not isinstance(ref.get('path'), str):
        raise NativeAdobeError('Native render-log provenance is missing')
    path = Path(ref['path'])
    if not path.is_absolute() or path.is_symlink() or not path.is_file():
        raise NativeAdobeError('Native render log is unavailable')
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != ref.get('sha256'):
        raise NativeAdobeError('Native render log changed after publication')
    return data


def read_json(artifact: dict[str, Any]) -> Any:
    if artifact.get("kind") != "json":
        raise NativeAdobeError("Expected a native JSON inspection artifact")
    path = Path(artifact["path"])
    if sha256(path) != artifact["sha256"]:
        raise NativeAdobeError("Native JSON artifact changed before readback")
    return json.loads(path.read_text())


def copy_artifact(artifact: dict[str, Any], output: Path) -> None:
    source = Path(artifact["path"])
    if sha256(source) != artifact["sha256"]:
        raise NativeAdobeError("Native artifact changed before copying")
    output.parent.mkdir(parents=True, exist_ok=True)
    # No overwrite of pinned references or an uncertain previous partial output.
    with source.open("rb") as incoming, output.open("xb") as destination:
        shutil.copyfileobj(incoming, destination)
    if sha256(output) != artifact["sha256"]:
        raise NativeAdobeError("Copied native artifact differs from worker receipt; inspect partial output")
