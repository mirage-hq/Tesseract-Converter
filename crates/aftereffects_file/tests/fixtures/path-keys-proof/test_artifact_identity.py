"""Offline identity-guard regressions; these are not Adobe fidelity tests."""
import hashlib
import json
from pathlib import Path
import tempfile
from unittest.mock import patch

import pytest

import artifact_identity


def test_exact_bytes_pass_and_changed_or_missing_bytes_fail():
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "artifact"
        path.write_bytes(b"expected")
        expected = hashlib.sha256(b"expected").hexdigest()
        artifact_identity.verify_hash(path, expected)
        path.write_bytes(b"swapped")
        with pytest.raises(ValueError):
            artifact_identity.verify_hash(path, expected)
        path.unlink()
        with pytest.raises(FileNotFoundError):
            artifact_identity.verify_hash(path, expected)

def test_manifest_checks_source_and_committed_reference_path():
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        fixture = root / "conv/crates/aftereffects_file/tests/fixtures/path-keys-proof"
        fixture.mkdir(parents=True)
        reference = root / "conv/tests/references/aep/path-keys/ref.mov"
        reference.parent.mkdir(parents=True)
        reference.write_bytes(b"Adobe render")
        digest = hashlib.sha256(b"fixture").hexdigest()
        (fixture / "source.aep").write_bytes(b"fixture")
        evidence = {"sources": [{"source": "source.aep", "sha256": digest}],
                    "references": {"ref.mov": {
                        "path": "tests/references/aep/path-keys/ref.mov"}}}
        (fixture / "evidence.json").write_text(json.dumps(evidence))
        (fixture / "artifacts.json").write_text(json.dumps({"videos": {}}))
        with patch.object(artifact_identity, "__file__", str(fixture / "artifact_identity.py")):
            artifact_identity.validate(root, "videos")
            reference.unlink()
            with pytest.raises(ValueError, match="missing or unsafe committed reference"):
                artifact_identity.validate(root, "videos")
            reference.write_bytes(b"Adobe render")
            (fixture / "source.aep").write_bytes(b"changed source")
            with pytest.raises(ValueError, match="SHA-256 mismatch"):
                artifact_identity.validate(root, "videos")
