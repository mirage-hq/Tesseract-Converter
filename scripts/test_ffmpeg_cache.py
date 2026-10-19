"""Offline compiler/native cache regression tests."""

import json
from unittest.mock import patch

import pytest

import ffmpeg


def test_matching_package_is_validated_without_downloading_or_building(tmp_path):
    identity = {"compiler": "pinned", "prefix": str(tmp_path)}
    (tmp_path / "build.json").write_text(json.dumps(identity))
    with patch.object(ffmpeg, "build_identity", return_value=identity), \
            patch.object(ffmpeg, "validate") as validate, \
            patch.object(ffmpeg, "fetch_source") as fetch, \
            patch.object(ffmpeg.subprocess, "run") as run:
        ffmpeg.build(tmp_path)
        validate.assert_called_once_with(tmp_path)
        fetch.assert_not_called()
        run.assert_not_called()


def test_identity_mismatch_takes_the_build_path_without_trusting_old_package(tmp_path):
    old = json.dumps({"compiler": "old"})
    (tmp_path / "build.json").write_text(old)
    with patch.object(ffmpeg, "build_identity", return_value={"compiler": "new"}), \
            patch.object(ffmpeg, "validate") as validate, \
            patch.object(ffmpeg, "fetch_source", side_effect=RuntimeError("offline build sentinel")) as fetch:
        with pytest.raises(RuntimeError, match="offline build sentinel"):
            ffmpeg.build(tmp_path)
        fetch.assert_called_once()
        validate.assert_not_called()
        assert (tmp_path / "build.json").read_text() == old


@pytest.mark.parametrize("manifest", ('{broken', json.dumps({"compiler": "pinned"})))
def test_invalid_cached_package_fails_closed(tmp_path, manifest):
    (tmp_path / "build.json").write_text(manifest)
    with patch.object(ffmpeg, "build_identity", return_value={"compiler": "pinned"}), \
            patch.object(ffmpeg, "validate", side_effect=ValueError("invalid cached runtime")), \
            patch.object(ffmpeg, "fetch_source") as fetch:
        with pytest.raises(ValueError):
            ffmpeg.build(tmp_path)
        fetch.assert_not_called()
        assert (tmp_path / "build.json").read_text() == manifest
