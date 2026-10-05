"""Offline native release payload tests; never trust fixture pins in production."""
import contextlib
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import ffmpeg
import native_bundle


def _tar(members, mode="w:xz"):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w" if mode == "w:gz" else mode) as archive:
        for name, contents in members.items():
            info = tarfile.TarInfo(name)
            info.size = len(contents)
            archive.addfile(info, io.BytesIO(contents))
    if mode != "w:gz":
        return stream.getvalue()
    compressed = io.BytesIO()
    with gzip.GzipFile(fileobj=compressed, mode="wb", filename="", mtime=0) as archive:
        archive.write(stream.getvalue())
    return compressed.getvalue()


@contextlib.contextmanager
def pinned_source_fixture():
    """Patch only the source SHA in tests so small source archives can exercise the real validator."""
    source = _tar({"ffmpeg-7.1.5/COPYING.LGPLv2.1": b"LGPL fixture\n",
                   "ffmpeg-7.1.5/LICENSE.md": b"License fixture\n"})
    zlib = _tar({"zlib-1.3.1/README": b"zlib fixture license\n"}, mode="w:gz")
    with patch.object(ffmpeg, "SOURCE_SHA256", hashlib.sha256(source).hexdigest()), \
         patch.object(ffmpeg, "ZLIB_SHA256", hashlib.sha256(zlib).hexdigest()):
        yield source, zlib


def fixture_bundle(directory: Path, platform: str, binary_data: bytes = b"fake executable") -> Path:
    """Generate an offline synthetic payload; call inside pinned_source_fixture()."""
    directory.mkdir(parents=True, exist_ok=True)
    cli = "tsrct-conv.exe" if platform.startswith("windows-") else "tsrct-conv"
    files = {cli: binary_data, **{path: b"synthetic native runtime" for path in native_bundle.expected_runtime_paths(platform)}}
    files.update({"native/sources/ffmpeg-7.1.5.tar.xz": _tar({
        "ffmpeg-7.1.5/COPYING.LGPLv2.1": b"LGPL fixture\n",
        "ffmpeg-7.1.5/LICENSE.md": b"License fixture\n"}),
        "native/licenses/ffmpeg/COPYING.LGPLv2.1": b"LGPL fixture\n",
        "native/licenses/ffmpeg/LICENSE.md": b"License fixture\n"})
    host = {"darwin": "Darwin", "linux": "Linux", "windows": "Windows"}[platform.split("-")[0]]
    options = [*ffmpeg.COMMON_OPTIONS, *ffmpeg.PLATFORM_OPTIONS[host]]
    identity = {"version": ffmpeg.VERSION, "sourceSha256": ffmpeg.SOURCE_SHA256,
                "options": options, "architecture": "arm64" if platform == "darwin-arm64" else "x86_64",
                "deploymentTarget": ffmpeg.DEPLOYMENT_TARGET,
                "zlib": ffmpeg.ZLIB_VERSION if host == "Windows" else "system"}
    files["native/ffmpeg-build.json"] = json.dumps(identity).encode()
    instructions = f"{ffmpeg.SOURCE_URL} {ffmpeg.SOURCE_SHA256} ./configure {' '.join(options)} make install\n"
    if host == "Windows":
        zlib = _tar({"zlib-1.3.1/README": b"zlib fixture license\n"}, mode="w:gz")
        files["native/sources/zlib-1.3.1.tar.gz"] = zlib
        files["native/licenses/ffmpeg/LICENSE.zlib"] = b"zlib fixture license\n"
        files["native/licenses/Microsoft-Visual-C++-Runtime.txt"] = b"Microsoft Visual C++ Runtime https://visualstudio.microsoft.com/license-terms/\n"
        instructions += f"{ffmpeg.ZLIB_URL} {ffmpeg.ZLIB_SHA256}\n"
    files["native/sources/BUILD.txt"] = instructions.encode()
    manifest = {"schemaVersion": 1, "platform": platform, "ffmpegVersion": ffmpeg.VERSION,
                "files": {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}}
    files["native/manifest.json"] = json.dumps(manifest).encode()
    for name, data in files.items():
        path = directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        if name == cli or name in native_bundle.expected_runtime_paths(platform):
            path.chmod(0o755)
    return directory


class NativeBundleTest(unittest.TestCase):
    def test_zlib_nmake_does_not_inherit_gnu_makeflags(self):
        env = {"MAKEFLAGS": " -- prefix=D:\\a\\jerboa\\target\\release-ffmpeg",
               "PATH": "MSVC tools", "INCLUDE": "MSVC headers", "LIB": "MSVC libraries"}
        original_env = env.copy()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            source.mkdir()
            for name, contents in {"zlib.h": "header", "zconf.h": "#ifdef HAVE_UNISTD_H\n",
                                   "zlib.lib": "library", "README": "license"}.items():
                (source / name).write_text(contents)
            with patch.object(ffmpeg, "fetch_source", return_value=source), \
                 patch.object(ffmpeg.subprocess, "run") as run, \
                 patch.object(ffmpeg, "output", side_effect=["D:/prefix/zlib/include", "D:/prefix/zlib/lib"]):
                flags = ffmpeg.build_zlib(root / "sources", root / "work", root / "prefix", env)
            run.assert_called_once_with(["nmake", "-f", "win32/Makefile.msc", "zlib.lib"],
                                        cwd=source, env={key: value for key, value in env.items()
                                                         if key != "MAKEFLAGS"}, check=True)
            self.assertEqual(env, original_env)
            self.assertEqual(flags, ["--extra-cflags=-ID:/prefix/zlib/include",
                                     "--extra-ldflags=-LIBPATH:D:/prefix/zlib/lib"])
            self.assertEqual((root / "prefix/zlib/include/zconf.h").read_text(),
                             "#if defined(HAVE_UNISTD_H) && HAVE_UNISTD_H\n")

    def test_all_platforms(self):
        with pinned_source_fixture(), tempfile.TemporaryDirectory() as temporary:
            for platform in sorted(native_bundle.PLATFORMS):
                bundle = fixture_bundle(Path(temporary) / platform, platform)
                self.assertEqual(native_bundle.entries(bundle, platform)["native/manifest.json"][0],
                                 (bundle / "native/manifest.json").read_bytes())

    def test_tampering_and_unknown_files(self):
        with pinned_source_fixture(), tempfile.TemporaryDirectory() as temporary:
            bundle = fixture_bundle(Path(temporary), "linux-x86_64")
            (bundle / "lib/libavcodec.so.61").write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "checksum"):
                native_bundle.entries(bundle, "linux-x86_64")
            (bundle / "lib/libavcodec.so.61").write_bytes(b"synthetic native runtime")
            (bundle / "evil.so").write_bytes(b"extra")
            with self.assertRaisesRegex(ValueError, "unapproved"):
                native_bundle.entries(bundle, "linux-x86_64")

    def test_empty_binary_rejected_even_with_valid_hash(self):
        with pinned_source_fixture(), tempfile.TemporaryDirectory() as temporary:
            bundle = fixture_bundle(Path(temporary), "linux-x86_64", binary_data=b"")
            with self.assertRaisesRegex(ValueError, "empty native executable"):
                native_bundle.entries(bundle, "linux-x86_64")

    def test_unused_macos_library_is_relocated(self):
        with tempfile.TemporaryDirectory() as temporary:
            prefix = Path(temporary) / "prefix"
            directory = Path(temporary) / "lib"
            directory.mkdir()
            cli = Path(temporary) / "tsrct-conv"
            unused = directory / "libavdevice.61.dylib"
            unused.write_bytes(b"unused")
            listing = (f"{unused}:\n"
                       f"\t{prefix}/lib/libavdevice.61.dylib (compatibility version 61.0.0)\n"
                       f"\t{prefix}/lib/libavutil.59.dylib (compatibility version 59.0.0)\n"
                       "\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n")
            (directory / "libavutil.59.dylib").write_bytes(b"dependency")
            with patch.object(native_bundle, "output", side_effect=[f"{cli}:\n", listing]), \
                 patch.object(native_bundle.subprocess, "run") as run:
                native_bundle.relocate_macos([cli, unused], prefix, directory)
            run.assert_called_once_with([
                "install_name_tool", "-change", f"{prefix}/lib/libavutil.59.dylib",
                "@executable_path/lib/libavutil.59.dylib", "-id",
                "@executable_path/lib/libavdevice.61.dylib", str(unused)], check=True)

    def test_symlink_and_bad_manifest(self):
        with pinned_source_fixture(), tempfile.TemporaryDirectory() as temporary:
            bundle = fixture_bundle(Path(temporary), "darwin-arm64")
            (bundle / "redirect").symlink_to(bundle / "tsrct-conv")
            with self.assertRaisesRegex(ValueError, "symlink"):
                native_bundle.entries(bundle, "darwin-arm64")
            (bundle / "redirect").unlink()
            manifest = bundle / "native/manifest.json"
            manifest.write_text(manifest.read_text().replace('"schemaVersion": 1', '"schemaVersion": 2'))
            with self.assertRaisesRegex(ValueError, "manifest identity"):
                native_bundle.entries(bundle, "darwin-arm64")


if __name__ == "__main__":
    unittest.main()
