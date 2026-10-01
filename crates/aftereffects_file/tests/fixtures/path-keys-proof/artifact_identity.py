"""Fail closed on stale/swapped bounded-proof artifacts.

Hashes establish artifact identity, not a historical causal link. The manifest
explicitly labels import/readback/render provenance as procedurally observed.
"""
import hashlib
import json
from pathlib import Path


def verify_hash(path, expected):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    if digest.hexdigest() != expected:
        raise ValueError(f"artifact SHA-256 mismatch: {path}")


def validate(root, group):
    directory = Path(__file__).parent
    manifest = json.loads((directory / "artifacts.json").read_text())
    evidence = json.loads((directory / "evidence.json").read_text())
    for source in evidence["sources"]:
        verify_hash(directory / source["source"], source["sha256"])
    for relative, artifact in manifest[group].items():
        path = Path(relative)
        if path.is_absolute() or ".." in path.parts:
            raise ValueError(f"invalid artifact path: {relative}")
        verify_hash(root / path, artifact["sha256"])
    if group == "videos":
        conv_root = directory.resolve().parents[4]
        for filename, reference in evidence["references"].items():
            path = Path(reference["path"])
            if path.is_absolute() or ".." in path.parts or path.name != filename:
                raise ValueError(f"invalid committed reference path: {filename}")
            local = conv_root / path
            if not local.is_file() or local.is_symlink():
                raise ValueError(f"missing or unsafe committed reference: {local}")
    return manifest
