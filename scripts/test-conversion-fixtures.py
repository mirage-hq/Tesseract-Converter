"""Focused, network-free checks for the conversion fixture index contract."""

import copy
import gzip
import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace

import pytest

SCRIPT = Path(__file__).with_name("conversion-fixtures.py")
spec = importlib.util.spec_from_file_location("conversion_fixtures", SCRIPT)
fixtures = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixtures)


@pytest.fixture
def ctx(tmp_path, monkeypatch):
    ctx = SimpleNamespace()
    ctx.root = tmp_path
    ctx.original = fixtures.load_manifest(fixtures.DEFAULT_MANIFEST)
    ctx.case = copy.deepcopy(
        next(case for case in ctx.original["cases"] if case["id"] == "premiere_two_video_tracks")
    )
    return ctx


def check(ctx, cases):
    manifest = ctx.root / "manifest.json"
    manifest.write_text(json.dumps({"schema_version": 1, "cases": cases}))
    return fixtures.load_manifest(manifest)


def test_checked_in_case_stages_the_declared_paths_and_bytes(ctx):
    output = ctx.root / "staged"
    fixtures.stage_case(ctx.case, output)
    for file in ctx.case["files"]:
        staged = output / file["path"]
        fixtures.verify_bytes(staged, file, file["path"])
    xml = gzip.decompress((output / ctx.case["project"]).read_bytes())
    assert b"media/clip-a.mp4" in xml
    assert b"media/clip-b.mp4" in xml
    with pytest.raises(fixtures.FixtureError, match="already exists"):
        fixtures.stage_case(ctx.case, output)
    dangling = ctx.root / "dangling"
    dangling.symlink_to(ctx.root / "absent")
    with pytest.raises(fixtures.FixtureError, match="already exists"):
        fixtures.stage_case(ctx.case, dangling)


def test_numeric_prefix_real_project_id_is_safe(ctx):
    ctx.case["id"] = "2128_aiedit_en_na_multicreators"
    assert check(ctx, [ctx.case])["cases"][0]["id"] == ctx.case["id"]
    ctx.case["id"] = "../escape"
    with pytest.raises(fixtures.FixtureError, match="invalid or duplicate case id"):
        check(ctx, [ctx.case])


def test_duplicate_case_ids_fail(ctx):
    with pytest.raises(fixtures.FixtureError, match="duplicate case id"):
        check(ctx, [ctx.case, copy.deepcopy(ctx.case)])


def test_missing_project_binding_fails(ctx):
    ctx.case["project"] = "absent.prproj"
    with pytest.raises(fixtures.FixtureError, match="project has no matching"):
        check(ctx, [ctx.case])


def test_traversal_and_casefold_collision_fail(ctx):
    ctx.case["files"][1]["path"] = "../escape.mp4"
    with pytest.raises(fixtures.FixtureError, match="unsafe relative path"):
        check(ctx, [ctx.case])
    ctx.case["files"][1]["path"] = "MEDIA/CLIP-B.MP4"
    with pytest.raises(fixtures.FixtureError, match="duplicate case-insensitive"):
        check(ctx, [ctx.case])


def test_package_file_directory_conflict_fails(ctx):
    ctx.case["files"][1]["path"] = "media"
    with pytest.raises(fixtures.FixtureError, match="file/directory conflict"):
        check(ctx, [ctx.case])


def test_wrong_hash_or_missing_file_fails_without_skipping(ctx):
    ctx.case["files"][0]["sha256"] = "0" * 64
    with pytest.raises(fixtures.FixtureError, match="mismatch"):
        check(ctx, [ctx.case])
    ctx.case["files"][0]["sha256"] = next(
        case for case in ctx.original["cases"] if case["id"] == "premiere_two_video_tracks"
    )["files"][0]["sha256"]
    ctx.case["files"][0]["repo_path"] = "crates/premiere_file/tests/fixtures/absent.prproj"
    with pytest.raises(fixtures.FixtureError, match="missing or escaping"):
        check(ctx, [ctx.case])


def test_json_duplicate_keys_fail(ctx):
    manifest = ctx.root / "manifest.json"
    manifest.write_text('{"schema_version": 1, "schema_version": 1, "cases": []}')
    with pytest.raises(fixtures.FixtureError, match="duplicate JSON key"):
        fixtures.load_manifest(manifest)


def test_video_reference_needs_independent_premiere_provenance(ctx):
    ctx.case.pop("reference_video")
    ctx.case.pop("score_policy")
    ctx.case["proof"] = "video_reference"
    with pytest.raises(fixtures.FixtureError, match="requires an independently baked"):
        check(ctx, [ctx.case])
    ctx.case["reference_video"] = copy.deepcopy(
        next(case for case in ctx.original["cases"] if case["id"] == ctx.case["id"])["reference_video"]
    )
    with pytest.raises(fixtures.FixtureError, match="score_policy"):
        check(ctx, [ctx.case])
    ctx.case["score_policy"] = {
        "min_mean_similarity": 0.98,
        "min_frame_similarity": 0.95,
        "sample_interval_secs": 0.25,
    }
    assert check(ctx, [ctx.case])["cases"][0]["proof"] == "video_reference"
    ctx.case["reference_video"]["sha256"] = "a" * 64
    with pytest.raises(fixtures.FixtureError, match="expected keys"):
        check(ctx, [ctx.case])
    ctx.case["reference_video"].pop("sha256")
    ctx.case["reference_video"]["format"] = "mov"
    with pytest.raises(fixtures.FixtureError, match="must be an MP4"):
        check(ctx, [ctx.case])
    ctx.case["reference_video"]["format"] = "mp4"
    del ctx.case["reference_video"]["sequence"]
    with pytest.raises(fixtures.FixtureError, match="Premiere provenance"):
        check(ctx, [ctx.case])


def test_structural_only_case_is_not_registered(ctx):
    ctx.case["proof"] = "structural_only"
    with pytest.raises(fixtures.FixtureError, match="only strict video_reference"):
        check(ctx, [ctx.case])


def test_reference_keeps_pinned_sequence_and_rejects_retired_status(ctx):
    check(ctx, [ctx.case])
    video = ctx.case["reference_video"]
    video["sequence_uid"] = "a-different-sequence"
    with pytest.raises(fixtures.FixtureError, match="video sequence differs"):
        check(ctx, [ctx.case])
    video["sequence_uid"] = ctx.case["sequence"]
    video["source_project_sha256"] = "b" * 64
    with pytest.raises(fixtures.FixtureError, match="video source project differs"):
        check(ctx, [ctx.case])
    video["source_project_sha256"] = ctx.case["files"][0]["sha256"]
    project_file = next(file for file in ctx.case["files"] if file["path"] == ctx.case["project"])
    identity = {field: project_file.pop(field) for field in ("size_bytes", "sha256")}
    with pytest.raises(fixtures.FixtureError, match="pinned project file"):
        check(ctx, [ctx.case])
    project_file.update(identity)
    video["review_status"] = "unreviewed"
    with pytest.raises(fixtures.FixtureError, match="expected keys"):
        check(ctx, [ctx.case])
    video.pop("review_status")
    ctx.case["candidate_video"] = video.copy()
    with pytest.raises(fixtures.FixtureError, match="expected keys"):
        check(ctx, [ctx.case])


def test_path_sanitization_keeps_original_reference_identity_and_checks_public_bytes(ctx):
    ctx.case = copy.deepcopy(next(
        case for case in ctx.original["cases"]
        if any("path_sanitization" in file for file in case["files"])
    ))
    source = next(file for file in ctx.case["files"] if file["path"] == ctx.case["project"])
    original = source["path_sanitization"]["original_sha256"]
    assert ctx.case["reference_video"]["source_project_sha256"] == original
    assert original != source["sha256"]
    check(ctx, [ctx.case])

    source["sha256"] = "0" * 64
    with pytest.raises(fixtures.FixtureError, match="mismatch"):
        check(ctx, [ctx.case])
    source["sha256"] = next(
        file["sha256"] for case in ctx.original["cases"] if case["id"] == ctx.case["id"]
        for file in case["files"] if file["path"] == ctx.case["project"]
    )
    source["path_sanitization"]["original_sha256"] = source["sha256"]
    with pytest.raises(fixtures.FixtureError, match="sanitization provenance"):
        check(ctx, [ctx.case])
    source["path_sanitization"]["original_sha256"] = "invalid"
    with pytest.raises(fixtures.FixtureError, match="sanitization provenance"):
        check(ctx, [ctx.case])
    source["path_sanitization"]["original_sha256"] = "b" * 64
    with pytest.raises(fixtures.FixtureError, match="video source project differs"):
        check(ctx, [ctx.case])


def test_remote_input_fields_are_rejected(ctx):
    remote = ctx.case["files"][0]
    remote.pop("repo_path")
    remote["asset_id"] = "obsolete_remote_id"
    remote["retention"] = "long_term"
    with pytest.raises(fixtures.FixtureError, match="expected keys"):
        check(ctx, [ctx.case])


def test_unsupported_schema_version_fails(ctx):
    manifest = ctx.root / "manifest.json"
    manifest.write_text(json.dumps({"schema_version": 2, "cases": [ctx.case]}))
    with pytest.raises(fixtures.FixtureError, match="unsupported"):
        fixtures.load_manifest(manifest)
