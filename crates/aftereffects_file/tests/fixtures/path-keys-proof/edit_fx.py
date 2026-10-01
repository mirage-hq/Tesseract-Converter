"""Explicit edited-FX input, independent of exported native records.

Moves all authored Path anchors/controls by (+17, -9) and the first key to
125ms. author.jsx authors the corresponding independent Adobe oracle.
"""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile


def edit(source: Path, output: Path) -> dict:
    if output.exists():
        raise ValueError("refusing edited-input overwrite")
    with zipfile.ZipFile(source) as archive:
        entries = {name: archive.read(name) for name in archive.namelist()}
    project = json.loads(entries["project.json"])
    changed = []
    for entry in project["composition"]["dynamics"]["entries"]:
        if entry["target"].get("propertyType") != "shapePath":
            continue
        animator = entry["animator"]
        keys = animator["keyframes"]
        assert len(keys) == 5
        assert [key["layerTime"] for key in keys] == [0, 500, 1500, 2500, 4500]
        for key in keys:
            assert key["value"]["type"] == "path"
            for command in key["value"]["value"]["commands"]:
                for field in ("x", "c1x", "c2x"):
                    if field in command:
                        command[field] += 17
                for field in ("y", "c1y", "c2y"):
                    if field in command:
                        command[field] -= 9
        keys[0]["layerTime"] = 125
        changed.append(entry["target"])
    assert changed, "no editable Path keys to edit"
    payload = json.dumps(project, separators=(",", ":"), ensure_ascii=False).encode()
    metadata = json.loads(entries["metadata.json"])
    metadata["project"]["byteLength"] = len(payload)
    metadata["project"]["sha256"] = hashlib.sha256(payload).hexdigest()
    entries["project.json"] = payload
    entries["metadata.json"] = json.dumps(metadata, separators=(",", ":")).encode()
    with zipfile.ZipFile(output, "x", compression=zipfile.ZIP_STORED) as archive:
        for name, data in entries.items():
            archive.writestr(name, data)
    return {"input_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "output_sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
            "changed_path_targets": changed, "translation": [17, -9], "first_key_millis": 125}


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Proof assertions require Python without -O")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    print(json.dumps(edit(args.source, args.output), indent=2))
