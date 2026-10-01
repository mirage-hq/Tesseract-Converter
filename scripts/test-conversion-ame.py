"""Offline tests for native AME Premiere-reference export (no Adobe dependency)."""

import gzip
import hashlib
import importlib.util
import json
import xml.etree.ElementTree as ET
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import pytest

SCRIPT = Path(__file__).with_name("regenerate-prproj-expected-video.py")
spec = importlib.util.spec_from_file_location("conversion_ame", SCRIPT)
export = importlib.util.module_from_spec(spec)
spec.loader.exec_module(export)


@pytest.fixture
def ctx(tmp_path, monkeypatch):
    ctx = SimpleNamespace()
    ctx.root = tmp_path
    ctx.cache = ctx.root / "cache"
    ctx.staged = ctx.cache / "cases/one_case"
    (ctx.staged / "media").mkdir(parents=True)
    ctx.xml = b'<?xml version="1.0" encoding="UTF-8" ?>\n<PremiereData><Sequence ObjectUID="sequence-one"><Name>Main</Name></Sequence><FilePath>/source/one_case/media/scene.mov</FilePath><ActualMediaFilePath>/source/one_case/media/scene.mov</ActualMediaFilePath><RelativePath>./media/scene.mov</RelativePath></PremiereData>'
    ctx.project_bytes = gzip.compress(ctx.xml, mtime=0)
    ctx.project = ctx.staged / "one_case.prproj"
    ctx.project.write_bytes(ctx.project_bytes)
    ctx.media_bytes = b"pinned movie content"
    (ctx.staged / "media/scene.mov").write_bytes(ctx.media_bytes)
    ctx.case = {
        "id": "one_case",
        "direction": "premiere_to_tesseract",
        "project": "one_case.prproj",
        "files": [
            blob("one_case.prproj", ctx.project_bytes),
            blob("media/scene.mov", ctx.media_bytes),
        ],
    }
    ctx.manifest = ctx.root / "manifest.json"
    ctx.manifest.write_text(json.dumps({"cases": [ctx.case]}))
    ctx.preset = ctx.root / "H264.epr"
    ctx.preset.write_bytes(b"H264 preset")
    ctx.app = ctx.root / "Adobe Media Encoder.app"
    monkeypatch.setattr(export, "MANIFEST", ctx.manifest)
    return ctx


def blob(name, data):
    return {
        "path": name,
        "sha256": hashlib.sha256(data).hexdigest(),
        "size_bytes": len(data),
    }


def test_preflight_requires_pinned_bytes_valid_preset_and_exact_sequence(ctx):
    case, project, xml, sequence = export.preflight("one_case", ctx.preset, ctx.cache)
    assert case == ctx.case
    assert project == ctx.project
    assert xml == ctx.xml
    assert sequence == {"name": "Main", "uid": "sequence-one"}
    with pytest.raises(export.ExportError, match="unknown"):
        export.preflight("missing", ctx.preset, ctx.cache)
    with pytest.raises(export.ExportError, match="preset"):
        export.preflight("one_case", None, ctx.cache)
    with pytest.raises(export.ExportError, match="sequence"):
        export.preflight("one_case", ctx.preset, ctx.cache, "Wrong")
    (ctx.staged / "media/scene.mov").write_bytes(b"damaged")
    with pytest.raises(export.ExportError, match="corrupt"):
        export.preflight("one_case", ctx.preset, ctx.cache)


def test_ambiguous_sequence_fails_closed(ctx):
    xml = ctx.xml.replace(
        b"</PremiereData>",
        b'<Sequence ObjectUID="sequence-two"><Name>Other</Name></Sequence></PremiereData>',
    )
    with pytest.raises(export.ExportError, match="identify exactly one"):
        export.choose_sequence(xml, None)
    assert export.choose_sequence(xml, "Other")["uid"] == "sequence-two"
    assert export.choose_sequence(xml, "sequence-two")["name"] == "Other"
    duplicate = xml.replace(b"<Name>Other</Name>", b"<Name>Main</Name>")
    with pytest.raises(export.ExportError, match="identify exactly one"):
        export.choose_sequence(duplicate, "Main")


def test_linear_wipe_fixture_links_both_sources_to_selected_sequence():
    fixture = (SCRIPT.parent.parent / "crates/premiere_file/tests/fixtures"
               / "feature_linear_wipe_strict.prproj")
    root = ET.fromstring(gzip.decompress(fixture.read_bytes()))
    selected_uid = "49b892d1-dfc3-4be2-a83a-93789626be7c"
    sequence_uids = {item.attrib["ObjectUID"] for item in root.iter("Sequence")
                     if "ObjectUID" in item.attrib}
    source_uids = [source.find("Sequence").attrib["ObjectURef"]
                   for source in root.iter("SequenceSource")]
    assert sequence_uids == {selected_uid}
    assert source_uids == [selected_uid, selected_uid]


def test_project_rebase_is_byte_targeted_and_self_contained(ctx):
    case, project, xml, _ = export.preflight("one_case", ctx.preset, ctx.cache)
    package_dir = ctx.root / "job"
    package_dir.mkdir()
    work_project, references = export.prepare_package(case, project, xml, ctx.cache, package_dir)
    updated = gzip.decompress(work_project.read_bytes())
    assert references == ["media/scene.mov"]
    assert (package_dir / "package/media/scene.mov").read_bytes() == ctx.media_bytes
    assert [n.text for n in ET.fromstring(updated).iter("FilePath")] == [
        str(package_dir / "package/media/scene.mov")
    ]
    assert b"<RelativePath>./media/scene.mov</RelativePath>" in updated
    assert (
        updated.replace(
            str(package_dir / "package/media/scene.mov").encode(),
            b"/source/one_case/media/scene.mov",
        )
        == xml
    )
    assert project.read_bytes() == ctx.project_bytes


def test_generated_relative_only_media_is_not_offline_in_ame(ctx):
    xml = b'<PremiereData><Media ObjectUID="media-one"><RelativePath>media/scene.mov</RelativePath><Title>scene.mov</Title></Media></PremiereData>'
    rebased, references = export.rebase_xml(xml, ctx.case, ctx.root / "job")
    assert references == ["media/scene.mov"]
    node = ET.fromstring(rebased).find("Media")
    assert node.findtext("RelativePath") == "media/scene.mov"
    assert node.findtext("FilePath") == str(ctx.root / "job/media/scene.mov")
    assert node.findtext("ActualMediaFilePath") == str(ctx.root / "job/media/scene.mov")
    assert b"<Title>scene.mov</Title>" in rebased
    incomplete = xml.replace(b"<Title>", b"<FilePath>/old</FilePath><Title>")
    with pytest.raises(export.ExportError, match="incomplete Premiere absolute"):
        export.rebase_xml(incomplete, ctx.case, ctx.root / "job")


def test_unknown_ambiguous_or_unsafe_media_never_gets_relinked(ctx):
    for replacement in (b"/source/other/missing.mov", b"../../wrong.mov", b"file:///tmp/scene.mov"):
        xml = ctx.xml.replace(b"/source/one_case/media/scene.mov", replacement)
        with pytest.raises(export.ExportError):
            export.rebase_xml(xml, ctx.case, ctx.root / "job")
    xml = ctx.xml.replace(b"./media/scene.mov", b"../other.mov")
    with pytest.raises(export.ExportError, match="unsafe Premiere relative"):
        export.rebase_xml(xml, ctx.case, ctx.root / "job")
    xml = ctx.xml.replace(b"<FilePath>", b'<FilePath Version="1">')
    with pytest.raises(export.ExportError, match="incomplete rebase"):
        export.rebase_xml(xml, ctx.case, ctx.root / "job")


def test_jsx_escapes_input_and_correlates_output_event(ctx):
    jsx = export.generate_jsx(
        "job",
        ctx.project,
        ctx.root / "output.mp4",
        ctx.preset,
        'Main"; app.quit(); //',
        ctx.root / "status.txt",
    )
    assert "onItemEncodeComplete" in jsx
    assert "String(event.outputFilePath) !== outputPath" in jsx
    assert '"Main\\"; app.quit(); //"' in jsx
    assert "app.scheduleTask('app.quit()'" in jsx
    assert "UXP" not in jsx


def test_status_requires_matching_job_and_success_event(ctx):
    path = ctx.root / "status.txt"
    path.write_text("wrong\ncomplete\nresult=true\n85\n")
    with pytest.raises(export.ExportError, match="wrong job ID"):
        export.wait_for_status(path, "job", 1)
    path.write_text("job\nerror\nmissing source\n85\n")
    with pytest.raises(export.ExportError, match="missing source"):
        export.wait_for_status(path, "job", 1)
    path.write_text("job\ncomplete\nonItemEncodeComplete=true\n85\n")
    assert export.wait_for_status(path, "job", 1)["ame_build"] == "85"
    with (
        patch.object(export.time, "monotonic", side_effect=[0, 2]),
        pytest.raises(export.ExportError, match="timed out"),
    ):
        export.wait_for_status(ctx.root / "absent", "job", 1)


def test_ffprobe_rejects_missing_empty_corrupt_or_nonvideo_outputs(ctx):
    with pytest.raises(export.ExportError, match="no nonempty MP4"):
        export.probe_mp4(ctx.root / "missing.mp4")
    video = ctx.root / "candidate.mp4"
    video.write_bytes(b"")
    with pytest.raises(export.ExportError, match="no nonempty MP4"):
        export.probe_mp4(video)
    video.write_bytes(b"invalid")
    with pytest.raises(export.ExportError, match="ffprobe"):
        export.probe_mp4(video)
    fake = type(
        "Result", (), {"stdout": json.dumps({"streams": [], "format": {"duration": "3"}})}
    )()
    with (
        patch.object(export.subprocess, "run", return_value=fake),
        pytest.raises(export.ExportError, match="no valid video"),
    ):
        export.probe_mp4(video)


def test_success_requires_event_and_mp4_before_provenance(ctx):

    def complete(status_path, job_id, timeout):
        package = status_path.parent / "package"
        assert (
            str(package / "media/scene.mov")
            in gzip.decompress((package / "one_case.prproj").read_bytes()).decode()
        )
        output = ctx.cache / "expected-candidates/one_case" / f"{job_id}.mp4"
        output.write_bytes(b"mock independent MP4")
        return {"ame_build": "85", "message": "onItemEncodeComplete=true"}

    with (
        patch.object(export, "ame_running", return_value=False),
        patch.object(export, "launch_ame"),
        patch.object(export, "wait_for_status", side_effect=complete),
        patch.object(
            export,
            "probe_mp4",
            return_value=(2.1, [{"codec_type": "video", "width": 1280, "height": 720}]),
        ),
    ):
        output = export.regenerate("one_case", ctx.preset, ctx.cache, None, 60, ctx.app)
    record = json.loads(output.with_suffix(".provenance.json").read_text())
    assert record["source_project_sha256"] == ctx.case["files"][0]["sha256"]
    assert record["sequence_uid"] == "sequence-one"
    assert record["ame_build"] == "85"
    assert record["media_paths_checked"] == ["media/scene.mov"]
    assert record["output_sha256"] == hashlib.sha256(b"mock independent MP4").hexdigest()
    assert json.loads(ctx.manifest.read_text())["cases"] == [ctx.case]


def test_running_ame_prevents_launch_or_output_creation(ctx):
    with (
        patch.object(export, "ame_running", return_value=True),
        patch.object(export, "launch_ame", side_effect=AssertionError("should not launch")),
        pytest.raises(export.ExportError, match="already running"),
    ):
        export.regenerate("one_case", ctx.preset, ctx.cache, None, 60, ctx.app)
    assert not (ctx.cache / "expected-candidates").exists()


def test_timeout_error_and_missing_mp4_never_publish_provenance(ctx):
    for scenario in (
        export.ExportError("timed out waiting"),
        export.ExportError("offline media"),
        {"ame_build": "85", "message": "onItemEncodeComplete=true"},
    ):
        with (
            patch.object(export, "ame_running", return_value=False),
            patch.object(export, "launch_ame"),
            patch.object(
                export,
                "wait_for_status",
                side_effect=scenario if isinstance(scenario, Exception) else None,
                return_value=scenario if isinstance(scenario, dict) else None,
            ),
            pytest.raises(export.ExportError),
        ):
            export.regenerate("one_case", ctx.preset, ctx.cache, None, 60, ctx.app)
    assert list((ctx.cache / "expected-candidates/one_case").glob("*.provenance.json")) == []


def test_existing_candidate_never_overwritten(ctx):
    fixed = type("Job", (), {"hex": "fixed"})()
    output = ctx.cache / "expected-candidates/one_case/fixed.mp4"
    output.parent.mkdir(parents=True)
    output.write_bytes(b"approved content")
    with (
        patch.object(export, "ame_running", return_value=False),
        patch.object(export.uuid, "uuid4", return_value=fixed),
        patch.object(export, "launch_ame", side_effect=AssertionError("should not launch")),
        pytest.raises(export.ExportError, match="overwrite"),
    ):
        export.regenerate("one_case", ctx.preset, ctx.cache, None, 60, ctx.app)
    assert output.read_bytes() == b"approved content"


def test_concurrent_export_lock_fails_closed(ctx):
    with (
        export.exclusive_export(ctx.cache),
        pytest.raises(export.ExportError, match="already owns AME"),
        export.exclusive_export(ctx.cache),
    ):
        pass
