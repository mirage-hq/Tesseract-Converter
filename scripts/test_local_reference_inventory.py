"""The selected Adobe rendering references must be present in the source checkout.

No network, Asset ID, or separate checksum manifest is needed to run these tests.
"""

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REFERENCES = ROOT / "tests/references"
FIXTURES = ROOT / "crates/aftereffects_file/tests/fixtures"


def load(path):
    return json.loads((ROOT / path).read_text())


def assert_committed_reference(record):
    assert set(record).isdisjoint({"asset_id", "assetId", "sha256", "size_bytes", "bytes", "retention"})
    path = Path(record.get("repo_path", record.get("path", "")))
    assert not path.is_absolute() and ".." not in path.parts
    local = ROOT / path
    assert local.resolve().is_relative_to(REFERENCES.resolve())
    assert local.is_file() and not local.is_symlink(), local


def test_adobe_import_export_references_are_local():
    manifest = load("crates/aftereffects_file/tests/fixtures/aep_video_references.json")
    references = [composition["reference"] for source in manifest["sources"]
                  for composition in source["compositions"]
                  if composition.get("reference", {}).get("path")]
    assert len(references) == 678
    for record in references:
        assert_committed_reference(record)


def test_audio_adjustment_and_alpha_references_are_local():
    audio = load("crates/aftereffects_file/tests/fixtures/audio_e2e/cases.json")
    adjustment = load("crates/aftereffects_file/tests/fixtures/adjustment/cases.json")
    path_keys = load("crates/aftereffects_file/tests/fixtures/path-keys-proof/evidence.json")
    for cases, key, count in ((audio["cases"], "reference", 28),
                              (adjustment["cases"], "native_reference", 10)):
        references = [case[key] for case in cases if isinstance(case.get(key), dict)]
        assert len(references) == count
        for record in references:
            assert_committed_reference(record)
    assert len(path_keys["references"]) == 8
    for record in path_keys["references"].values():
        assert_committed_reference(record)


def test_strict_premiere_references_and_font_are_local():
    manifest = load("tests/manifest.json")
    cases = manifest["cases"]
    assert len(cases) == 41
    assert all(case["proof"] == "video_reference" for case in cases)
    for case in cases:
        assert_committed_reference(case["reference_video"])
        for file in case["files"]:
            assert "repo_path" in file and "asset_id" not in file
            if file["path"].endswith("Arial-BoldMT.ttf"):
                assert_committed_reference(file)


def test_active_conversion_scripts_do_not_use_the_asset_service():
    for script in (ROOT / "scripts").glob("*.py"):
        if script.name.startswith("test"):
            continue
        source = script.read_text()
        for forbidden in ("CAPTIONS_INTERNAL_AUTH_SECRET", "asset.captions-dev.xyz",
                          "conversion_asset_cache", "download_blob("):
            assert forbidden not in source, script
