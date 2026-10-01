"""Author explicit edited FX media fixtures; never import our AEP or render as oracle."""
from copy import deepcopy
from pathlib import Path
import json

ROOT = Path(__file__).resolve().parent
BASE = {
    "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
    "formatVersion": 1, "duration": 2,
    "dimensions": {"width": 320, "height": 180},
    "backgroundColor": [0, 0, 0, 1],
    "composition": {
        "id": "main", "name": "", "segmentLayerIds": [],
        "motionBlur": {"enabled": False, "shutterAngle": 180, "shutterPhase": 0,
                       "samplesPerFrame": 16, "adaptiveSampleLimit": 128},
        "dynamics": {"entries": []}, "layers": []
    }
}


def transform(position=(160, 90), scale=(100, 100), opacity=100):
    return {"anchorPoint": [0, 0], "position": list(position), "scale": list(scale),
            "rotation": 0, "opacity": opacity}


def image(id, name, asset="red", fit="contain"):
    return {"type": "Image", "id": id, "name": name, "parent": None,
            "activeRange": {"start": 0, "duration": 2000}, "transform": transform(),
            "source": {"assetId": asset, "fit": fit}}


def video(id, name, asset="movie"):
    return {"type": "Video", "id": id, "name": name, "parent": None,
            "activeRange": {"start": 0, "duration": 2000},
            "sourceRange": {"start": 0, "duration": 2000},
            "sourceIntrinsicDuration": 8000, "transform": transform(),
            "source": {"assetId": asset, "fit": "contain"}}


def property_keys(layer, prop, values, easing="linear"):
    return {"target": {"kind": "layer", "layerId": layer, "propertyType": prop},
            "animator": {"type": "keyframes", "enabled": True,
                         "keyframes": [{"id": f"media-{layer}-{prop}-{i}", "layerTime": ms,
                                        "value": {"type": kind, "value": value},
                                        "easing": {"type": easing}}
                                       for i, (ms, kind, value) in enumerate(values)]},
            "dependencies": [], "layerRefs": {}}


def save(name, layers, entries=()):
    value = deepcopy(BASE)
    value["composition"]["name"] = name
    value["composition"]["layers"] = layers
    value["composition"]["dynamics"]["entries"] = list(entries)
    (ROOT / (name + ".fx.json")).write_text(json.dumps(value, indent=2) + "\n")


cover = image(101, "Red Cover Crop", fit="cover")
cover["source"]["frameRect"] = {"x": 40, "y": 20, "width": 120, "height": 80}
cover["transform"] = transform((0, 0))
save("media-image-fit", [cover], [property_keys(101, "opacity", [(0, "float", 100), (1000, "float", 35)])])

trim = video(102, "Trimmed Blue Movie")
trim["sourceRange"] = {"start": 500, "duration": 2000}
trim["startTime"] = 0.5
save("media-video-clock", [trim])

stretch = video(103, "Double Speed Movie")
stretch["sourceRange"] = {"start": 250, "duration": 4000}
stretch["playback"] = {"before": "inactive", "after": "inactive", "keyframes": [
    {"id": "stretch-in", "time": 0, "value": 250, "easing": {"type": "linear"}},
    {"id": "stretch-out", "time": 2000, "value": 4250, "easing": {"type": "linear"}}
]}
save("media-video-stretch", [stretch])

mix = video(104, "Frame Mix Movie")
mix["frameBlending"] = True
save("media-frame-blending", [mix])

left = video(105, "Blue Movie Left")
left["transform"] = transform((80, 90), (45, 45))
right = video(106, "Blue Movie Right")
right["transform"] = transform((240, 90), (45, 45))
save("media-shared-source", [left, right])

switched = image(107, "Red to Green")
save("media-source-switch", [switched], [property_keys(107, "mediaSourceAssetId", [
    (0, "string", "red"), (1000, "string", "green")], "hold")])

sibling = image(108, "Retained Red Sibling")
bad = image(109, "Unsupported PNG", "unsupported-png")
save("media-unsupported-sibling", [sibling, bad])

static = video(110, "Held Blue Frame")
static["source"]["timeRemap"] = 0.75
save("media-static-remap", [static])

spatial = image(111, "Spatial Red")
spatial["transform"] = {**transform((160, 90), (110, 85), 70), "position": [160, 90, 35],
                        "rotationX": 12, "rotationY": -8, "orientation": [3, 4, 5]}
save("media-image-3d", [spatial])

variant = image(112, "Constant Green", "red")
save("media-constant-source", [variant], [{"target": {"kind": "layer", "layerId": 112,
    "propertyType": "mediaSourceAssetId"}, "animator": {"type": "constant", "enabled": True,
    "value": {"type": "string", "value": "green"}}, "dependencies": [], "layerRefs": {}}])

legacy = {"type": "Media", "id": 113, "name": "Legacy Red Stretch", "parent": None,
          "activeRange": {"start": 0, "duration": 2000},
          "transform": transform((160, 90), (125, 80), 55),
          "source": {"assetId": "red", "kind": "image", "fit": "stretch",
                     "sourceRect": {"x": 20, "y": 20, "width": 160, "height": 90}}}
save("media-legacy-image", [legacy])

# Hand-specified native structural contracts, not serialized observations from
# our reader/writer. Native Adobe sources and renders remain independent/pending.
ORACLES = {
    "media-image-fit": {"assets": ["red"], "layers": ["Red Cover Crop"],
                        "maskCount": 1, "opacityKeys": [[0, 1], [1, .35]]},
    "media-video-clock": {"assets": ["movie"], "layers": ["Trimmed Blue Movie"],
                          "clock": {"in": 0, "out": 2, "start": -.5, "stretch": 1}},
    "media-video-stretch": {"assets": ["movie"], "layers": ["Double Speed Movie"],
                            "clock": {"in": 0, "out": 2, "start": -.125, "stretch": .5}},
    "media-frame-blending": {"assets": ["movie"], "layers": ["Frame Mix Movie"],
                             "frameBlending": "frameMix"},
    "media-shared-source": {"assets": ["movie"],
                            "layers": ["Blue Movie Left", "Blue Movie Right"],
                            "sharedFootage": True},
    "media-source-switch": {"assets": ["red", "green"],
                            "layers": ["Red to Green", "Red to Green"],
                            "sourceOrder": ["red", "green"],
                            "ranges": [[0, 1], [1, 2]]},
    "media-unsupported-sibling": {"assets": ["red", "unsupported-png"],
                                   "published": ["red"],
                                   "layers": ["Retained Red Sibling"],
                                   "diagnostic": "image asset does not use the source-backed OpenEXR native profile"},
    "media-static-remap": {"assets": ["movie"], "layers": ["Held Blue Frame"],
                           "timeRemap": True},
    "media-image-3d": {"assets": ["red"], "layers": ["Spatial Red"],
                       "threeD": True, "position": [160, 90, 35],
                       "orientation": [3, 4, 5], "opacity": [.7]},
    "media-constant-source": {"assets": ["red", "green"],
                              "published": ["green"], "layers": ["Constant Green"],
                              "sourceOrder": ["green"]},
    "media-legacy-image": {"assets": ["red"], "layers": ["Legacy Red Stretch"],
                           "position": [160, 90, 0], "opacity": [.55]},
}
for name, oracle in ORACLES.items():
    (ROOT / (name + ".expected.json")).write_text(json.dumps({
        "status": "AUTHORING_REQUEST_UNRUN_UNMEASURED",
        "caseId": "fx-export-" + name, **oracle
    }, indent=2) + "\n")
