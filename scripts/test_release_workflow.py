"""Offline checks for the credential boundary and library release wiring."""

from pathlib import Path
import hashlib
import json
import os
import sys
import tempfile
import textwrap
import unittest
from unittest.mock import patch
from zipfile import ZipFile, ZipInfo

from test_native_bundle import fixture_bundle, pinned_source_fixture


WORKFLOW = Path(__file__).resolve().parents[1] / ".github/workflows/release.yml"


class ReleaseWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW.read_text()

    def test_library_build_and_extracted_archive_runtime_smoke(self):
        source = self.workflow
        build = source.index("cargo build --locked --release -p tsrct-conv --features ffmpeg-library")
        bundle = source.index("python scripts/native_bundle.py --binary")
        archive = source.index("python scripts/release_archive.py build")
        extract = source.index("archive.extractall(sys.argv[2])")
        smoke = source.index('python scripts/test_release_runtime.py --bundle "$extracted" --platform "$RELEASE_PLATFORM" --ffmpeg "$helper"')
        self.assertLess(build, bundle)
        self.assertLess(bundle, archive)
        self.assertLess(archive, extract)
        self.assertLess(extract, smoke)
        self.assertIn("--native-bundle target/native-bundle", source)
        build_job = source.split("  build:", 1)[1].split("  sign-macos:", 1)[0]
        self.assertIn("RELEASE_PLATFORM: ${{ matrix.platform }}", build_job)
        self.assertNotIn("      PLATFORM: ${{ matrix.platform }}", build_job)
        self.assertIn('case "$RELEASE_PLATFORM" in', build_job)
        self.assertIn('export PATH="$(dirname "$(command -v cl)"):$PATH"', build_job)
        self.assertIn('python scripts/ffmpeg.py "$prefix"', source)
        self.assertIn('python3 scripts/ffmpeg.py "$prefix"', source)
        for marker in ("PKG_CONFIG_PATH", "FFMPEG_DIR", "LIBCLANG_PATH", "MACOSX_DEPLOYMENT_TARGET"):
            self.assertIn(marker, source)

    def test_signer_has_no_checkout_or_repo_code_with_secrets(self):
        signer = self.workflow.split("  sign-macos:", 1)[1].split("  github-release:", 1)[0]
        self.assertNotIn("actions/checkout@", signer)
        self.assertNotIn("python scripts/", signer)
        self.assertLess(signer.index("native manifest hash mismatch"), signer.index("CERTIFICATE: ${{ secrets."))
        self.assertLess(signer.index('for library in "$RUNNER_TEMP/macho"/lib/*.dylib'),
                        signer.index('codesign --force --options runtime --timestamp --identifier com.captions.tsrct-conv'))
        self.assertIn("codesign --verify --strict \"$packaged\"", signer)
        self.assertIn("native/manifest.json", signer)
        self.assertIn('xcrun notarytool submit "${signed[0]}"', signer)
        self.assertNotIn('"$binary" --help', signer)
        self.assertNotIn('"$binary" --version', signer)

    def test_presecret_verifier_accepts_only_the_exact_hashed_macho_set(self):
        signer = self.workflow.split("  sign-macos:", 1)[1]
        program = textwrap.dedent(signer.split("<<'PY'\n", 1)[1].split("\n          PY", 1)[0])
        for mutation in (None, "missing", "extra", "tampered"):
            with self.subTest(mutation=mutation), pinned_source_fixture(), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                bundle = fixture_bundle(root / "bundle", "darwin-arm64")
                payload = {str(path.relative_to(bundle)): path.read_bytes() for path in bundle.rglob("*") if path.is_file()}
                manifest = json.loads(payload["native/manifest.json"])
                library = "lib/libavcodec.61.dylib"
                if mutation == "missing":
                    del payload[library]
                    del manifest["files"][library]
                elif mutation == "extra":
                    payload["lib/libevil.1.dylib"] = b"unapproved"
                    manifest["files"]["lib/libevil.1.dylib"] = hashlib.sha256(b"unapproved").hexdigest()
                elif mutation == "tampered":
                    payload[library] = b"changed after hashing"
                payload["native/manifest.json"] = json.dumps(manifest).encode()
                payload["build.json"] = json.dumps({"version": "0.1.0", "source_sha": "a" * 40, "platform": "darwin-arm64"}).encode()
                archive_path = root / "input.zip"
                with ZipFile(archive_path, "w") as archive:
                    for name, data in payload.items():
                        info = ZipInfo("Tesseract-Converter-0.1.0-darwin-arm64/" + name)
                        info.external_attr = 0o100644 << 16
                        archive.writestr(info, data)
                destination = root / "extracted"
                with patch.dict(os.environ, {"VERSION": "0.1.0", "SOURCE_SHA": "a" * 40, "PLATFORM": "darwin-arm64"}), patch.object(sys, "argv", ["verify", str(archive_path), str(destination)]):
                    if mutation:
                        with self.assertRaises(SystemExit):
                            exec(compile(program, "release-presecret-verifier", "exec"), {})
                    else:
                        exec(compile(program, "release-presecret-verifier", "exec"), {})
                        self.assertTrue((destination / "tsrct-conv").is_file())
                        self.assertEqual(len(list((destination / "lib").glob("*.dylib"))), 7)

    def test_release_hold_unchanged(self):
        self.assertIn("run-name: Tesseract-Converter Release · ${{ github.event_name }}", self.workflow)
        self.assertNotIn("run-name: Converter release ${{ github.sha }}", self.workflow)
        self.assertIn("  group: converter-release-${{ github.sha }}", self.workflow)
        events = self.workflow.split("\non:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertEqual(events.strip(), "workflow_dispatch:")
        self.assertIn("python3 scripts/release_plan.py --phase validate", self.workflow)
        self.assertIn("python3 scripts/release_plan.py --phase publish", self.workflow)
        self.assertIn("--title \"Tesseract-Converter v$VERSION\"", self.workflow)
        self.assertIn("test -s LICENSE", self.workflow)
        self.assertIn("cargo install cargo-about --locked --version 0.9.2", self.workflow)
        self.assertIn("make generate-third-party-notices", self.workflow)
        self.assertIn("test -s target/THIRD_PARTY_NOTICES.md", self.workflow)
        self.assertIn("--notices target/THIRD_PARTY_NOTICES.md", self.workflow)
        self.assertIn("Source provenance is recorded in each archive’s build.json", self.workflow)
        self.assertNotIn("Built from %s", self.workflow)
        self.assertNotIn("make check-third-party-notices", self.workflow)
        self.assertIn('visibility="$(gh api "repos/$GITHUB_REPOSITORY" --jq .visibility)"', self.workflow)
        self.assertIn('if [[ "$visibility" = public ]]; then', self.workflow)


if __name__ == "__main__":
    unittest.main()
