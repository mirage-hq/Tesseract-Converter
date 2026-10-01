"""Verify the pinned native-import and independent Adobe-export control receipts."""
import argparse
import json
from pathlib import Path
import zipfile

from artifact_identity import validate


def path_properties(project):
    return [p for c in project["compositions"] for p in c["paths"] if p["keys"]]


def native_exports(expected, actual):
    assert expected["completed"] and actual["completed"]
    reference = path_properties(expected["projects"][0])
    generated = path_properties(actual)
    assert len(reference) == len(generated) == 1
    assert len(reference[0]["keys"]) == len(generated[0]["keys"]) == 5
    maximum = 0.0
    for left, right in zip(reference[0]["keys"], generated[0]["keys"]):
        assert abs(left["time"] - right["time"]) < 0.00005
        assert (left["incoming"], left["outgoing"]) == (right["incoming"], right["outgoing"])
        assert left["value"]["closed"] == right["value"]["closed"]
        for field in ["vertices", "inTangents", "outTangents"]:
            assert len(left["value"][field]) == len(right["value"][field])
            for a, b in zip(left["value"][field], right["value"][field]):
                maximum = max(maximum, *(abs(x-y) for x, y in zip(a, b)))
        for side in ["inEase", "outEase"]:
            for field in ["speed", "influence"]:
                assert abs(left[side][field] - right[side][field]) < 0.00001
    assert maximum < 0.001
    return maximum


def layers(rows):
    for row in rows:
        yield row
        yield from layers(row.get("layers", []))


def native_import(source, authored, kind):
    with zipfile.ZipFile(source) as archive:
        document = json.loads(archive.read("project.json"))
    composition = document["composition"]
    entries = composition["dynamics"]["entries"]
    assert all(e["animator"]["type"] != "jsScript" for e in entries)
    paths = [e for e in entries if e["target"].get("propertyType") == "shapePath"]
    assert len(paths) == 1
    entry = paths[0]
    if kind == "shape":
        visible = {l["id"] for l in layers(composition["layers"]) if l["type"] == "Shape" and not l.get("isHidden", False)}
        assert entry["target"]["layerId"] in visible
    else:
        guides = {mask["layer"] for layer in layers(composition["layers"]) for mask in layer.get("masks", [])}
        assert entry["target"]["layerId"] in guides
    keys = entry["animator"]["keyframes"]
    assert len(keys) == len(authored["keys"]) == 5
    assert len({key["id"] for key in keys}) == 5
    assert [key["easing"]["type"] for key in keys] == ["linear", "linear", "hold", "cubicBezier", "cubicBezier"]
    for key, controls in zip(keys[3:], [(0.7, 0.0, 0.65, 1.0), (1/6, 1/6, 0.2, 1.0)]):
        assert all(abs(key["easing"][field] - value) < 1e-8 for field, value in zip(["x1", "y1", "x2", "y2"], controls))
    maximum = 0.0
    for key, native in zip(keys, authored["keys"]):
        assert key["layerTime"] == round(native["time"] * 1000)
        commands = key["value"]["value"]["commands"]
        shape = native["value"]
        vertices = shape["vertices"]
        segments = len(vertices) if shape["closed"] else len(vertices)-1
        assert len(commands) == 1 + segments + int(shape["closed"])
        assert (commands[-1]["type"] == "close") == shape["closed"]
        values = [(commands[0]["x"], vertices[0][0]), (commands[0]["y"], vertices[0][1])]
        for i in range(segments):
            next_index = (i+1) % len(vertices)
            command = commands[i+1]
            assert command["type"] == "cubicTo"
            for axis, suffix in enumerate(["x", "y"]):
                values.extend([(command["c1"+suffix], vertices[i][axis]+shape["outTangents"][i][axis]),
                    (command["c2"+suffix], vertices[next_index][axis]+shape["inTangents"][next_index][axis]),
                    (command[suffix], vertices[next_index][axis])])
        maximum = max(maximum, *(abs(a-b) for a, b in values))
    assert maximum < 0.001
    return maximum


def verify(root):
    manifest = validate(root, "controls")
    authored = json.loads((root / "authoring.json").read_text())
    exports = json.loads((root / "export-readback.json").read_text())
    assert authored["completed"] and exports["completed"]
    results = {}
    for kind in ["shape", "mask"]:
        native = next(c for c in authored["cases"] if c["name"] == kind+"-v2")
        results[kind+"-import"] = native_import(root / f"import-{kind}/project.tsrct", native, kind)
        oracle = json.loads((root / f"{kind}-oracle-reopen.json").read_text())
        actual = next(p for p in exports["projects"] if p["name"] == kind+"-export")
        for project, suffix in [(actual, "-v2"), (oracle["projects"][0], "-edited-oracle-v2")]:
            composition = next(c for c in project["compositions"] if c["id"] == 1)
            assert composition["name"] == "PATH_PROOF_" + kind + suffix
            assert [composition[k] for k in ["width", "height", "fps", "duration"]] == [640, 360, 24, 5]
            expected = "ADBE Vector Shape" if kind == "shape" else "ADBE Mask Shape"
            assert all(p["property"] == expected and p["layer"] == kind.upper()+"_CONTENT"
                       for p in path_properties(project))
        # Mark the independently observed project complete only after the receipt guard.
        actual = dict(actual, completed=exports["completed"])
        results[kind+"-export"] = native_exports(oracle, actual)
    return {"passed": True, "maximum_geometry_error_pixels": results,
            "evidence": "fresh editable FX import plus Adobe-script control readback; not UI inspection or pixel equality",
            "artifact_identity": "SHA-256 checked against artifacts.json",
            "historical_lineage": manifest["historical_lineage"]}


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Proof assertions require Python without -O")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    args = parser.parse_args()
    print(json.dumps(verify(args.root), indent=2))
