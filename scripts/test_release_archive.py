#!/usr/bin/env python3
"""Offline tests for binary release packaging and publication gates."""

import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from zipfile import ZipFile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_archive import PLATFORMS, archive_name, build, legal_text, publish_cdn, verify_archive, verify_set
from test_native_bundle import fixture_bundle, pinned_source_fixture

VERSION = "0.1.0"
SHA = "a" * 40


class ReleaseArchiveTests(unittest.TestCase):
    def setUp(self):
        self.enterContext(pinned_source_fixture())
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        manifest = self.root / "apps/tesseract-conv/Cargo.toml"
        manifest.parent.mkdir(parents=True)
        manifest.write_text('[package]\nversion = "0.1.0"\n')
        (self.root / "README.md").write_text("Public CLI\n")
        notices = self.root / "target/THIRD_PARTY_NOTICES.md"
        notices.parent.mkdir()
        notices.write_text("Generated third-party notices\nMIT example license text\n")
        self.dist = self.root / "dist"

    def build(self, platform):
        name = "tsrct-conv.exe" if platform.startswith("windows-") else "tsrct-conv"
        binary = self.root / platform / name
        fixture_bundle(binary.parent, platform)
        return build(self.root, VERSION, SHA, platform, binary, self.dist)

    def build_all(self):
        for platform in PLATFORMS:
            self.build(platform)



    def test_internal_set_without_project_license(self):
        self.build_all()
        verify_set(self.root, VERSION, SHA, self.dist, require_license=False)
        with self.assertRaisesRegex(ValueError, "LICENSE"):
            verify_set(self.root, VERSION, SHA, self.dist, require_license=True)


    def test_licensed_set_has_exact_contents_and_provenance(self):
        (self.root / "LICENSE").write_text("test-only license")
        self.build_all()
        verify_set(self.root, VERSION, SHA, self.dist, require_license=True)
        path = next(self.dist.glob("*darwin-arm64.zip"))
        with ZipFile(path) as archive:
            folder = f"Tesseract-Converter-{VERSION}-darwin-arm64"
            self.assertEqual(json.loads(archive.read(f"{folder}/build.json"))["source_sha"], SHA)
            self.assertEqual(archive.read(f"{folder}/LICENSE"), b"test-only license")
            self.assertNotIn(f"{folder}/MP4-LICENSE", archive.namelist())
            self.assertTrue(archive.getinfo(f"{folder}/tsrct-conv").external_attr >> 16 & 0o111)

    def test_windows_crlf_text_matches_linux_checkout(self):
        texts = {
            "README.md": "Public CLI\nAvailable now\n",
            "LICENSE": "test license\nLicense terms\n",
            "target/THIRD_PARTY_NOTICES.md": "Generated license text\n",
        }
        for path, text in texts.items():
            (self.root / path).write_bytes(text.replace("\n", "\r\n").encode())
        windows = self.build("windows-x86_64")
        with ZipFile(windows) as archive:
            folder = f"Tesseract-Converter-{VERSION}-windows-x86_64"
            for path, text in texts.items():
                entry = "THIRD_PARTY_NOTICES.md" if path == "target/THIRD_PARTY_NOTICES.md" else path
                self.assertEqual(archive.read(f"{folder}/{entry}"), text.encode())
        for path, text in texts.items():
            (self.root / path).write_bytes(text.encode())
        for platform in PLATFORMS:
            if platform != "windows-x86_64":
                self.build(platform)
        verify_set(self.root, VERSION, SHA, self.dist, require_license=True)

    def test_rejects_symlinked_required_text(self):
        outside = self.root / "external.txt"
        outside.write_bytes(b"outside text\n")
        for name in ("README.md", "target/THIRD_PARTY_NOTICES.md"):
            with self.subTest(name=name):
                path = self.root / name
                original = path.read_bytes()
                path.unlink()
                path.symlink_to(outside)
                with self.assertRaisesRegex(ValueError, "regular text file"):
                    self.build("linux-x86_64")
                path.unlink()
                path.write_bytes(original)

    def test_rejects_empty_required_text(self):
        for name in ("README.md", "target/THIRD_PARTY_NOTICES.md"):
            with self.subTest(name=name):
                path = self.root / name
                original = path.read_bytes()
                path.write_bytes(b" \n")
                with self.assertRaisesRegex(ValueError, "regular text file"):
                    self.build("linux-x86_64")
                path.write_bytes(original)

    def test_rejects_content_different_from_checked_out_source(self):
        (self.root / "LICENSE").write_text("approved test license")
        self.build_all()
        (self.root / "README.md").write_text("changed after packaging")
        with self.assertRaisesRegex(ValueError, "README differs"):
            verify_set(self.root, VERSION, SHA, self.dist, require_license=True)
        (self.root / "README.md").write_text("Public CLI\n")
        (self.root / "LICENSE").write_text("different license")
        with self.assertRaisesRegex(ValueError, "LICENSE differs"):
            verify_set(self.root, VERSION, SHA, self.dist, require_license=True)




    def test_refuses_wrong_version_or_sha(self):
        binary = self.root / "tsrct-conv"
        binary.write_bytes(b"binary")
        with self.assertRaisesRegex(ValueError, "differs"):
            build(self.root, "0.2.0", SHA, "linux-x86_64", binary, self.dist)
        with self.assertRaisesRegex(ValueError, "full lowercase"):
            build(self.root, VERSION, "not-a-sha", "linux-x86_64", binary, self.dist)

    def test_stable_name_refuses_same_version_overwrite(self):
        first = self.build("linux-x86_64")
        digest = hashlib.sha256(first.read_bytes()).hexdigest()
        self.assertEqual(first.name, archive_name(VERSION, "linux-x86_64"))
        self.assertEqual(first.parent.name, "dist")
        self.assertEqual((self.dist / f"{first.name}.sha256").read_text(), f"{digest}  {first.name}\n")
        with ZipFile(first) as archive:
            self.assertTrue(all(name.startswith(f"Tesseract-Converter-{VERSION}-linux-x86_64/") for name in archive.namelist()))
        with self.assertRaisesRegex(ValueError, "overwrite"):
            self.build("linux-x86_64")

    def test_rejects_wrong_archive_name(self):
        path = self.build("linux-x86_64")
        renamed = path.with_name("wrong.zip")
        path.rename(renamed)
        with self.assertRaisesRegex(ValueError, "archive name"):
            verify_archive(renamed, self.root, VERSION, SHA, "linux-x86_64", False)

    def test_cdn_publish_reserves_skill_before_upload(self):
        (self.root / "LICENSE").write_text("test license")
        self.build_all()
        skill = self.root / "conv-skill/tsrct-conv"
        with patch("release_archive.subprocess.run") as upload:
            url = publish_cdn(self.root, VERSION, SHA, self.dist, skill)
            with self.assertRaises(FileExistsError):
                publish_cdn(self.root, VERSION, SHA, self.dist, skill)
            self.assertEqual(upload.call_count, 8)
            self.assertIn(url, (skill / "SKILL.md").read_text())

        other = self.root / "conv-skill/failed"
        with patch("release_archive.subprocess.run", side_effect=OSError("upload failed")) as upload:
            with self.assertRaisesRegex(OSError, "upload failed"):
                publish_cdn(self.root, VERSION, SHA, self.dist, other)
            self.assertEqual(upload.call_count, 1)
        self.assertFalse(other.exists())

    def test_cdn_publish_rejects_tampered_archive_before_remote_write(self):
        (self.root / "LICENSE").write_text("test license")
        self.build_all()
        next(self.dist.glob("*linux-x86_64.zip")).write_bytes(b"not a ZIP")
        skill = self.root / "conv-skill/tsrct-conv"
        with patch("release_archive.subprocess.run") as upload:
            with self.assertRaisesRegex(ValueError, "checksum"):
                publish_cdn(self.root, VERSION, SHA, self.dist, skill)
            upload.assert_not_called()
        self.assertFalse(skill.exists())

    def test_refuses_overwrite_and_tampered_checksum(self):
        self.build_all()
        archive = next(self.dist.glob("*linux-x86_64.zip"))
        binary = self.root / "linux-x86_64/tsrct-conv"
        with self.assertRaisesRegex(ValueError, "overwrite"):
            build(self.root, VERSION, SHA, "linux-x86_64", binary, self.dist)
        (self.dist / f"{archive.name}.sha256").write_text("0" * 64 + f"  {archive.name}\n")
        with self.assertRaisesRegex(ValueError, "checksum"):
            verify_set(self.root, VERSION, SHA, self.dist, require_license=False)

    def test_rejects_missing_or_unexpected_archives(self):
        self.build_all()
        windows_name = next(self.dist.glob("*windows-x86_64.zip")).name
        (self.dist / windows_name).unlink()
        with self.assertRaisesRegex(ValueError, "missing or unexpected"):
            verify_set(self.root, VERSION, SHA, self.dist, require_license=False)
        (self.dist / f"{windows_name}.sha256").unlink()
        self.build("windows-x86_64")
        (self.dist / "stray-file").touch()
        with self.assertRaisesRegex(ValueError, "missing or unexpected"):
            verify_set(self.root, VERSION, SHA, self.dist, require_license=False)

    def test_rejects_extra_zip_entries_even_with_matching_checksum(self):
        self.build_all()
        path = next(self.dist.glob("*linux-x86_64.zip"))
        name = path.name
        with ZipFile(path, "a") as archive:
            archive.writestr("../unexpected-file", b"not part of the release")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        new_name = archive_name(VERSION, "linux-x86_64")
        path.rename(self.dist / new_name)
        (self.dist / f"{name}.sha256").rename(self.dist / f"{new_name}.sha256")
        (self.dist / f"{new_name}.sha256").write_text(f"{digest}  {new_name}\n")
        with self.assertRaisesRegex(ValueError, "unexpected ZIP entries"):
            verify_set(self.root, VERSION, SHA, self.dist, require_license=False)

    def test_library_free_release_is_rejected(self):
        binary = self.root / "without-runtime" / "tsrct-conv"
        binary.parent.mkdir()
        binary.write_bytes(b"old feature-disabled executable")
        with self.assertRaises((ValueError, FileNotFoundError)):
            build(self.root, VERSION, SHA, "linux-x86_64", binary, self.dist)
        self.assertFalse(self.dist.exists())

    def test_runtime_library_and_manifest_survive_packaging_and_are_checked(self):
        path = self.build("linux-x86_64")
        prefix = f"Tesseract-Converter-{VERSION}-linux-x86_64/"
        with ZipFile(path) as archive:
            self.assertIn(prefix + "lib/libavcodec.so.61", archive.namelist())
            manifest = json.loads(archive.read(prefix + "native/manifest.json"))
            self.assertEqual(manifest["platform"], "linux-x86_64")
            for filename, digest in manifest["files"].items():
                self.assertEqual(hashlib.sha256(archive.read(prefix + filename)).hexdigest(), digest)
        runtime = self.root / "linux-x86_64"
        (runtime / "lib/libavcodec.so.61").write_bytes(b"corrupt library")
        with self.assertRaisesRegex(ValueError, "checksum"):
            build(self.root, VERSION, SHA, "linux-x86_64", runtime / "tsrct-conv", self.dist)

    def test_rejects_wrong_binary_name_or_symlink(self):
        source = self.root / "other"
        source.write_bytes(b"binary")
        with self.assertRaisesRegex(ValueError, "expected a regular"):
            build(self.root, VERSION, SHA, "darwin-arm64", source, self.dist)
        link = self.root / "tsrct-conv"
        link.symlink_to(source)
        with self.assertRaisesRegex(ValueError, "expected a regular"):
            build(self.root, VERSION, SHA, "darwin-arm64", link, self.dist)


if __name__ == "__main__":
    unittest.main()
