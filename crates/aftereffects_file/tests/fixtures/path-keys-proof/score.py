"""Bounded proof scoring; no Adobe execution, uploads or baseline mutation.

Uses the existing full-resolution validation_cli metrics. A successful CLI video
exit can only gate an aggregate: enforce the preregistered PER-FRAME minimum here.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess

from artifact_identity import validate


def command(args, log):
    with log.open("w") as stream:
        result = subprocess.run(args, stdout=stream, stderr=subprocess.STDOUT, timeout=300)
    return result.returncode


def frames(video, directory, indices, alpha_video=None):
    directory.mkdir()
    select = "+".join(f"eq(n\\,{i})" for i in indices)
    for label, suffix in [("rgba", ",format=rgba"), ("alpha", ",alphaextract")]:
        source = alpha_video if label == "alpha" and alpha_video is not None else video
        args = ["ffmpeg", "-v", "error", "-i", str(source), "-vf", f"select={select}{suffix}",
                "-fps_mode", "vfr", "-frames:v", str(len(indices)), str(directory / f"{label}-%02d.png")]
        subprocess.run(args, check=True, timeout=120)
    for label in ["rgba", "alpha"]:
        assert len(list(directory.glob(f"{label}-*.png"))) == len(indices)


def score(root, validation):
    validate(root, "videos")
    policy = json.loads(Path(__file__).with_name("policy.json").read_text())
    references = json.loads(Path(__file__).with_name("evidence.json").read_text())["references"]
    conv_root = Path(__file__).resolve().parents[5]
    pairs = [
        ("shape-import", "native/shape-v2", "shape-import"),
        ("mask-import", "native/mask-v2", "mask-import"),
        ("shape-edited-fx", "native/shape-edited-oracle-v2", "shape-edited"),
        ("mask-edited-fx", "native/mask-edited-oracle-v2", "mask-edited"),
        ("shape-export", "native/shape-edited-oracle-v2", "native/shape-export"),
        ("mask-export", "native/mask-edited-oracle-v2", "native/mask-export"),
    ]
    out = root / "scores"
    out.mkdir()
    indices = policy["critical_frame_indices"]
    cache = {}
    alpha_sources = {}
    report = {"policy": policy, "comparison_alpha_convention": "premultiplied black",
              "cases": [], "all_passed": False}
    for name, left, right in pairs:
        videos = [conv_root / references[Path(left).name + ".mov"]["path"],
                  root / f"{right}.mov"]
        # The preregistered convention is premultiplied black. Adobe Animation
        # already uses it; the FX renderer's get_frame/ProRes contract is straight RGBA.
        # Normalize only a derived FX copy, keeping source/reference bytes intact.
        if not right.startswith("native/"):
            normalized = out / f"{name}-premultiplied.mov"
            subprocess.run(["ffmpeg", "-v", "error", "-i", str(videos[1]),
                            "-vf", "format=gbrap,premultiply=inplace=1,format=argb",
                            "-an", "-c:v", "qtrle", str(normalized)], check=True, timeout=120)
            # Alpha's independent gate reads the ORIGINAL plane, avoiding even
            # one-code-value quantization drift in the derived 8-bit RGB copy.
            alpha_sources[normalized] = videos[1]
            videos[1] = normalized
        for video in videos:
            if video not in cache:
                directory = out / (video.stem + ("-native" if video.parent.name == "native" else "-fx"))
                frames(video, directory, indices, alpha_sources.get(video))
                cache[video] = directory
        rgb = out / f"{name}-rgb.json"
        with rgb.open("w") as stdout, (out / f"{name}-rgb.log").open("w") as stderr:
            result = subprocess.run([str(validation), "video", "--left", str(videos[0]), "--right", str(videos[1]),
                "--sample-interval-secs", str(1/30), "--max-samples", "150", "--algorithm", "rgb-hybrid",
                "--min-similarity", "0.99", "--max-dimension", "0", "--canonical-rgb24", "--json"],
                stdout=stdout, stderr=stderr, timeout=300)
        data = json.loads(rgb.read_text())
        assert data["compared_frames"] == 150
        for side in ["left", "right"]:
            assert [data[side]["width"], data[side]["height"]] == policy["canvas"]
            assert abs(data[side]["duration_secs"] - 5) < 0.001 and data[side]["fps"] == 30
        worst = min(data["frame_results"], key=lambda frame: frame["score"]["similarity"])
        row = {"name":name, "rgb_mean":data["score"]["similarity"], "rgb_min":data["min_frame_similarity"],
               "rgb_worst_time":worst["time_secs"], "rgb_frame_count":150, "critical":[]}
        for number, index in enumerate(indices, 1):
            sample = {"frame":index, "time":index/30, "passed":True}
            for label, algorithm in [("rgba", "rgba-hybrid"), ("alpha", "rgb-rms")]:
                log = out / f"{name}-{label}-{index}.log"
                status = command([str(validation), "image", "--left", str(cache[videos[0]] / f"{label}-{number:02d}.png"),
                    "--right", str(cache[videos[1]] / f"{label}-{number:02d}.png"), "--algorithm", algorithm,
                    "--min-similarity", "0.99"], log)
                match = re.search(r"similarity=([0-9.]+)", log.read_text())
                if not match:
                    raise RuntimeError(f"missing score: {log}")
                sample[label] = float(match.group(1))
                sample["passed"] = sample["passed"] and status == 0
            row["critical"].append(sample)
        row["rgba_min"] = min(s["rgba"] for s in row["critical"])
        row["alpha_min"] = min(s["alpha"] for s in row["critical"])
        row["passed"] = (result.returncode == 0
                         and all(sample["passed"] for sample in row["critical"])
                         and min(row["rgb_min"], row["rgba_min"], row["alpha_min"]) >= 0.99)
        report["cases"].append(row)
        (out / "report.json").write_text(json.dumps(report, indent=2))
        print(name, "RGB", row["rgb_min"], "RGBA", row["rgba_min"], "alpha", row["alpha_min"], "pass", row["passed"], flush=True)
    report["all_passed"] = all(row["passed"] for row in report["cases"])
    (out / "report.json").write_text(json.dumps(report, indent=2))
    return report["all_passed"]


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Proof assertions require Python without -O")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--validation", required=True, type=Path)
    args = parser.parse_args()
    raise SystemExit(0 if score(args.root.resolve(), args.validation.resolve()) else 1)
