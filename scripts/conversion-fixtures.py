#!/usr/bin/env python3
"""Validate conversion fixtures and stage checked-in inputs only."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import sys
import tempfile

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = REPO_ROOT / "tests/manifest.json"
HASH = re.compile(r"[0-9a-f]{64}\Z")
CASE_ID = re.compile(r"[a-z0-9][a-z0-9_]*\Z")


class FixtureError(ValueError):
    """A manifest or staged fixture is unsafe or incomplete."""


def object_without_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise FixtureError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def require_keys(value, required, optional, context):
    if not isinstance(value, dict) or not required <= value.keys() or value.keys() - (required | optional):
        raise FixtureError(f"{context}: expected keys {sorted(required)}; optional {sorted(optional)}")


def relative_path(raw, context):
    if (not isinstance(raw, str) or not raw or "\\" in raw or ":" in raw
            or "\x00" in raw or any(part in ("", ".", "..") for part in raw.split("/"))):
        raise FixtureError(f"{context}: unsafe relative path: {raw!r}")
    return Path(raw)


def check_blob(blob, context, repo_root, *, video=False):
    provenance = {"format", "sequence_uid", "source_project_sha256", "preset_sha256"}
    required = provenance if video else {"path"}
    allowed = ({"repo_path", "premiere_version", "sequence", "export_settings"}
               if video else {"repo_path", "size_bytes", "sha256"})
    require_keys(blob, required | {"repo_path"}, allowed, context)
    has_identity = {"size_bytes", "sha256"} <= blob.keys()
    if ({"size_bytes", "sha256"} & blob.keys()) and not has_identity:
        raise FixtureError(f"{context}: size_bytes and sha256 must be provided together")
    if has_identity:
        if not isinstance(blob["size_bytes"], int) or isinstance(blob["size_bytes"], bool) or blob["size_bytes"] <= 0:
            raise FixtureError(f"{context}: size_bytes must be positive")
        if not isinstance(blob["sha256"], str) or not HASH.fullmatch(blob["sha256"]):
            raise FixtureError(f"{context}: sha256 must be a lowercase 64-digit hex digest")
    if video:
        if blob["format"] != "mp4":
            raise FixtureError(f"{context}: video reference must be an MP4")
        for field in ("premiere_version", "sequence", "export_settings"):
            if not isinstance(blob.get(field), str) or not blob[field].strip():
                raise FixtureError(f"{context}: missing independent Premiere provenance: {field}")
        if (not isinstance(blob["sequence_uid"], str) or not blob["sequence_uid"]
                or any(not isinstance(blob[field], str) or not HASH.fullmatch(blob[field])
                       for field in ("source_project_sha256", "preset_sha256"))):
            raise FixtureError(f"{context}: incomplete video sequence or source provenance")
    else:
        relative_path(blob["path"], f"{context}.path")
    path = relative_path(blob["repo_path"], f"{context}.repo_path")
    base = repo_root.resolve()
    resolved = (base / path).resolve()
    if not resolved.is_relative_to(base) or not resolved.is_file():
        raise FixtureError(f"{context}: missing or escaping repo file: {path}")
    if has_identity:
        verify_bytes(resolved, blob, context)


def verify_bytes(path, blob, context):
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
            size += len(chunk)
    if size != blob["size_bytes"] or digest.hexdigest() != blob["sha256"]:
        raise FixtureError(f"{context}: size/SHA-256 mismatch: {path}")


def load_manifest(manifest, repo_root=REPO_ROOT):
    try:
        data = json.loads(Path(manifest).read_text(), object_pairs_hook=object_without_duplicate_keys)
    except (OSError, json.JSONDecodeError) as exc:
        raise FixtureError(f"cannot read conversion fixture manifest: {exc}") from exc
    require_keys(data, {"schema_version", "cases"}, set(), "manifest")
    if type(data["schema_version"]) is not int or data["schema_version"] != 1:
        raise FixtureError("unsupported conversion fixture schema_version")
    cases = data["cases"]
    if not isinstance(cases, list) or not cases:
        raise FixtureError("manifest must contain at least one case")
    ids = set()
    for case in cases:
        require_keys(case, {"id", "direction", "proof", "project", "files"},
                     {"sequence", "reference_video", "score_policy", "conversion_status"}, "case")
        name = case["id"]
        if not isinstance(name, str) or not CASE_ID.fullmatch(name) or name in ids:
            raise FixtureError(f"invalid or duplicate case id: {name!r}")
        ids.add(name)
        if case["direction"] not in ("premiere_to_tesseract", "tesseract_to_premiere"):
            raise FixtureError(f"{name}: invalid direction")
        project = relative_path(case["project"], f"{name}.project").as_posix()
        extension = ".prproj" if case["direction"] == "premiere_to_tesseract" else ".tsrct"
        if not project.lower().endswith(extension):
            raise FixtureError(f"{name}: project must be a {extension} file")
        if "sequence" in case and (not isinstance(case["sequence"], str) or not case["sequence"].strip()):
            raise FixtureError(f"{name}: sequence must be nonempty")
        if case["proof"] != "video_reference":
            raise FixtureError(f"{name}: only strict video_reference cases are supported")
        if case["proof"] == "video_reference" and "reference_video" not in case:
            raise FixtureError(f"{name}: video_reference requires an independently baked reference_video")
        if ("score_policy" in case) != (case["proof"] == "video_reference"):
            raise FixtureError(f"{name}: video_reference requires an explicit score_policy")
        if "score_policy" in case:
            require_keys(case["score_policy"], {"min_mean_similarity", "min_frame_similarity", "sample_interval_secs"},
                         set(), f"{name}.score_policy")
            for field, value in case["score_policy"].items():
                if not isinstance(value, (int, float)) or isinstance(value, bool) or not 0 < value <= (1 if field != "sample_interval_secs" else 30):
                    raise FixtureError(f"{name}: invalid score_policy.{field}")
        if "conversion_status" in case:
            raise FixtureError(f"{name}: diagnostic conversion_status is not supported")
        if "reference_video" in case:
            check_blob(case["reference_video"], f"{name}.reference_video", repo_root, video=True)
        video = case.get("reference_video")
        if video is not None and case.get("sequence") != video["sequence_uid"]:
            raise FixtureError(f"{name}: video sequence differs from pinned case selection")
        files = case["files"]
        if not isinstance(files, list) or not files:
            raise FixtureError(f"{name}: files must be nonempty")
        paths = set()
        parent_dirs = set()
        for file in files:
            check_blob(file, f"{name}.files", repo_root)
            key = file["path"].casefold()
            parents = {"/".join(key.split("/")[:n]) for n in range(1, len(key.split("/")))}
            if key in paths:
                raise FixtureError(f"{name}: duplicate case-insensitive package path: {file['path']}")
            if key in parent_dirs or parents & paths:
                raise FixtureError(f"{name}: file/directory conflict: {file['path']}")
            paths.add(key)
            parent_dirs.update(parents)
        if project.casefold() not in paths:
            raise FixtureError(f"{name}: project has no matching package file")
        if video is not None:
            source = next(file for file in files if file["path"].casefold() == project.casefold())
            if video["source_project_sha256"] != source["sha256"]:
                raise FixtureError(f"{name}: video source project differs from pinned bytes")
    return data


def stage_case(case, output, *, reuse=False):
    if output.exists() or output.is_symlink():
        if not reuse or not output.is_dir() or output.is_symlink():
            raise FixtureError(f"destination already exists: {output}")
        for file in case["files"]:
            path = output / file["path"]
            if not path.is_file() or path.is_symlink():
                raise FixtureError(f"missing or unsafe staged file: {path}")
            if "sha256" in file:
                verify_bytes(path, file, f"{case['id']}.staged.{file['path']}")
        print(f"reused verified {case['id']} -> {output}")
        return
    if not output.parent.is_dir():
        raise FixtureError(f"destination parent does not exist: {output.parent}")
    stage = Path(tempfile.mkdtemp(prefix=".conversion-fixture-", dir=output.parent))
    try:
        for file in case["files"]:
            destination = stage / file["path"]
            destination.parent.mkdir(parents=True, exist_ok=True)
            source = REPO_ROOT / file["repo_path"]
            shutil.copyfile(source, destination)
            if "sha256" in file:
                verify_bytes(destination, file, f"{case['id']}.staged.{file['path']}")
        if output.exists() or output.is_symlink():
            raise FixtureError(f"destination appeared while staging: {output}")
        stage.rename(output)
    except BaseException:
        shutil.rmtree(stage)
        raise
    print(f"staged {case['id']} -> {output}")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("check", help="validate manifest and checked-in bytes")
    stage = subparsers.add_parser("stage", help="stage one case from checked-in inputs")
    stage.add_argument("--case", required=True)
    stage.add_argument("--output-dir", required=True, type=Path)
    stage.add_argument("--reuse", action="store_true", help="verify and reuse a complete staging directory")
    args = parser.parse_args(argv)
    try:
        manifest = load_manifest(args.manifest)
        if args.command == "stage":
            case = next((c for c in manifest["cases"] if c["id"] == args.case), None)
            if case is None:
                raise FixtureError(f"unknown case: {args.case}")
            stage_case(case, args.output_dir, reuse=args.reuse)
        else:
            print(f"validated {len(manifest['cases'])} local conversion case(s)")
    except FixtureError as exc:
        print(f"conversion fixture error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
