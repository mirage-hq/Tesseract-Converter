"""Offline tests for the optional central-worker adapter; no Adobe execution."""
import json
from pathlib import Path
import sys

import pytest

import adobe_native as native


@pytest.fixture
def fake_worker(tmp_path, monkeypatch):
    script = tmp_path / "worker.py"
    script.write_text('''import hashlib,json,pathlib,sys
request=json.loads(pathlib.Path(sys.argv[2]).read_text())
output=pathlib.Path(sys.argv[2]).parent/'inspection.json'
output.write_text('{"compositions": []}')
print(json.dumps({'status':'succeeded','ready':True,'artifact':{
'path':str(output),'sha256':hashlib.sha256(output.read_bytes()).hexdigest(),
'kind':'json','request_id':request['request_id'],'metadata':{}}}))
''')
    monkeypatch.setenv("HEADLESS_ADOBE_COMMAND", json.dumps([sys.executable, str(script)]))
    return script


def test_typed_dispatch_and_verified_json_artifact(tmp_path, fake_worker):
    source = tmp_path / "source.aep"
    source.write_bytes(b"native source bytes")
    artifact = native.execute("inspect_aep", {"source": native.source_ref(source)}, tmp_path / "work")
    assert native.read_json(artifact) == {"compositions": []}
    assert (tmp_path / "work/inspect_aep-result.json").exists()
    payload = json.loads((tmp_path / "work/inspect_aep-request.json").read_text())
    assert payload["source"]["sha256"] == native.sha256(source)
    assert payload["request_id"].startswith("conv-")


def test_changed_request_never_dispatches(tmp_path, fake_worker):
    work = tmp_path / "work"
    native.execute("inspect_aep", {"source": {"path": "first"}}, work)
    with pytest.raises(native.NativeAdobeError, match="Existing worker request differs"):
        native.execute("inspect_aep", {"source": {"path": "changed"}}, work)


def test_unknown_operation_and_nonfinite_timeout_reject(tmp_path):
    with pytest.raises(native.NativeAdobeError, match="Unsupported typed"):
        native.execute("jsx", {}, tmp_path)
    with pytest.raises(native.NativeAdobeError, match="timeout"):
        native.execute("inspect_aep", {}, tmp_path, timeout=float("nan"))


@pytest.mark.parametrize("command", ["not json", "[]", '["headless-adobe", 1]', '"headless-adobe"'])
def test_invalid_command_is_not_a_shell_fallback(command, monkeypatch):
    monkeypatch.setenv("HEADLESS_ADOBE_COMMAND", command)
    with pytest.raises(native.NativeAdobeError, match="JSON argv"):
        native.command_prefix()


def test_missing_central_worker_never_falls_back(monkeypatch):
    monkeypatch.delenv("HEADLESS_ADOBE_COMMAND", raising=False)
    monkeypatch.setattr(native.shutil, "which", lambda _: None)
    with pytest.raises(native.NativeAdobeError, match="not a fallback"):
        native.command_prefix()


def test_failed_cli_is_not_published(tmp_path, fake_worker):
    fake_worker.write_text("import json; print(json.dumps({'status':'unavailable','ready':False,"
                           "'error':{'code':'BUSY'}})); raise SystemExit(3)")
    with pytest.raises(native.NativeAdobeError, match="BUSY"):
        native.execute("inspect_aep", {}, tmp_path / "work")
    assert not (tmp_path / "work/inspect_aep-result.json").exists()


def test_artifact_copy_rejects_changed_bytes_and_overwrite(tmp_path, fake_worker):
    artifact = native.execute("inspect_aep", {}, tmp_path / "work")
    output = tmp_path / "copy.json"
    native.copy_artifact(artifact, output)
    with pytest.raises(FileExistsError):
        native.copy_artifact(artifact, output)
    Path(artifact["path"]).write_text("changed")
    with pytest.raises(native.NativeAdobeError, match="changed"):
        native.copy_artifact(artifact, tmp_path / "new.json")
    with pytest.raises(native.NativeAdobeError, match="changed"):
        native.read_json(artifact)
