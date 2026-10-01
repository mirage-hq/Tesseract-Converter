#!/usr/bin/env python3
"""Build and offline-verify the converter's relocatable native FFmpeg payload."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform as host_platform
import re
import shutil
import subprocess
import sys
import tarfile

import ffmpeg

PLATFORMS = {"darwin-arm64", "darwin-x86_64", "linux-x86_64", "windows-x86_64"}
VC_RUNTIME_DLLS = {"vcruntime140.dll", "vcruntime140_1.dll"}
SYSTEM_DLL_PREFIXES = ("api-ms-win-", "ext-ms-")
LINUX_SYSTEM_LIBRARIES = {
    "libc.so.6", "libm.so.6", "libdl.so.2", "librt.so.1", "libpthread.so.0",
    "libgcc_s.so.1", "libstdc++.so.6", "libz.so.1", "libasound.so.2",
    "ld-linux-x86-64.so.2",
}
VC_RUNTIME_NOTICE = """Microsoft Visual C++ Runtime

The Windows bundle includes Visual C++ runtime files ({files}) copied
unmodified from the Visual Studio redistributable directory (toolset {version}).
They are Distributable Code under the Microsoft Visual Studio license terms
(https://visualstudio.microsoft.com/license-terms/) and are provided solely for
use with the accompanying converter and FFmpeg libraries. The same files are
available separately as the Microsoft Visual C++ Redistributable.
"""


def runtime_name(name, platform):
    major = {"avcodec": 61, "avformat": 61, "avutil": 59, "avfilter": 10,
             "avdevice": 61, "swscale": 8, "swresample": 5}[name]
    if platform.startswith("darwin-"):
        return f"lib{name}.{major}.dylib"
    if platform.startswith("linux-"):
        return f"lib{name}.so.{major}"
    return f"{name}-{major}.dll"


def expected_runtime_paths(platform):
    if platform not in PLATFORMS:
        raise ValueError(f"unknown platform: {platform}")
    root = "" if platform.startswith("windows-") else "lib/"
    return {root + runtime_name(name, platform) for name in ffmpeg.LIBRARIES}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def _json(data):
    return json.loads(data.decode("utf-8"), object_pairs_hook=_unique_pairs)


def _unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _safe_path(name):
    return (isinstance(name, str) and name and not name.startswith("/") and
            "\\" not in name and all(segment not in ("", ".", "..") for segment in name.split("/")))


def _source_files(platform):
    names = {"native/sources/ffmpeg-7.1.5.tar.xz", "native/sources/BUILD.txt",
             "native/licenses/ffmpeg/COPYING.LGPLv2.1", "native/licenses/ffmpeg/LICENSE.md",
             "native/ffmpeg-build.json"}
    if platform.startswith("windows-"):
        names |= {"native/sources/zlib-1.3.1.tar.gz", "native/licenses/ffmpeg/LICENSE.zlib",
                  "native/licenses/Microsoft-Visual-C++-Runtime.txt"}
    return names


def _tar_member(archive, member):
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        matches = [entry for entry in tar if entry.name == member and entry.isfile()]
        if len(matches) != 1:
            raise ValueError(f"missing upstream source member: {member}")
        return tar.extractfile(matches[0]).read()


def validate_entries(payload: dict[str, bytes], platform: str) -> None:
    """Check complete payload without executing binaries or invoking platform tools."""
    if platform not in PLATFORMS:
        raise ValueError(f"unknown platform: {platform}")
    if not all(_safe_path(name) and isinstance(data, bytes) for name, data in payload.items()):
        raise ValueError("invalid payload path or bytes")
    cli = "tsrct-conv.exe" if platform.startswith("windows-") else "tsrct-conv"
    fixed = {cli, "native/manifest.json"} | expected_runtime_paths(platform) | _source_files(platform)
    optional = VC_RUNTIME_DLLS if platform.startswith("windows-") else set()
    if not fixed <= payload.keys() or not payload.keys() <= fixed | optional:
        raise ValueError(f"missing or unapproved native payload files: {sorted(payload.keys() ^ fixed)}")
    if not all(payload[name] for name in {cli} | expected_runtime_paths(platform)):
        raise ValueError("empty native executable or library")
    manifest = _json(payload["native/manifest.json"]) 
    if set(manifest) != {"schemaVersion", "platform", "ffmpegVersion", "files"} or (manifest["schemaVersion"], manifest["platform"], manifest["ffmpegVersion"]) != (1, platform, ffmpeg.VERSION):
        raise ValueError("invalid native manifest identity")
    expected = {name: digest(data) for name, data in payload.items() if name != "native/manifest.json"}
    if manifest["files"] != expected:
        raise ValueError("native payload checksum mismatch")
    source = payload["native/sources/ffmpeg-7.1.5.tar.xz"]
    if digest(source) != ffmpeg.SOURCE_SHA256:
        raise ValueError("FFmpeg source checksum mismatch")
    for name in ("COPYING.LGPLv2.1", "LICENSE.md"):
        if payload[f"native/licenses/ffmpeg/{name}"] != _tar_member(source, f"ffmpeg-{ffmpeg.VERSION}/{name}"):
            raise ValueError(f"FFmpeg {name} does not match pinned source")
    identity = _json(payload["native/ffmpeg-build.json"])
    if not isinstance(identity, dict) or identity.get("version") != ffmpeg.VERSION or identity.get("sourceSha256") != ffmpeg.SOURCE_SHA256:
        raise ValueError("FFmpeg build identity mismatch")
    options = identity.get("options")
    if not isinstance(options, list) or not all(isinstance(option, str) for option in options):
        raise ValueError("invalid FFmpeg build configuration")
    required = set(ffmpeg.COMMON_OPTIONS) | set(ffmpeg.PLATFORM_OPTIONS[{"darwin": "Darwin", "linux": "Linux", "windows": "Windows"}[platform.split("-")[0]]])
    if len(options) != len(required) or set(options) != required:
        raise ValueError("unapproved FFmpeg build configuration")
    if identity.get("architecture") not in ({"arm64", "aarch64"} if platform == "darwin-arm64" else {"x86_64", "AMD64"}):
        raise ValueError("FFmpeg architecture mismatch")
    if identity.get("deploymentTarget") != ffmpeg.DEPLOYMENT_TARGET or identity.get("zlib") != (ffmpeg.ZLIB_VERSION if platform.startswith("windows-") else "system"):
        raise ValueError("FFmpeg build policy mismatch")
    instructions = payload["native/sources/BUILD.txt"].decode("utf-8")
    if ffmpeg.SOURCE_URL not in instructions or ffmpeg.SOURCE_SHA256 not in instructions or "make install" not in instructions or not all(option in instructions for option in required) or any(flag in instructions for flag in ("--enable-gpl", "--enable-nonfree", "--enable-version3")):
        raise ValueError("incomplete FFmpeg rebuild instructions")
    if platform.startswith("windows-"):
        zlib = payload["native/sources/zlib-1.3.1.tar.gz"]
        if digest(zlib) != ffmpeg.ZLIB_SHA256 or payload["native/licenses/ffmpeg/LICENSE.zlib"] != _tar_member(zlib, "zlib-1.3.1/README"):
            raise ValueError("zlib source or license mismatch")
        if ffmpeg.ZLIB_URL not in instructions or ffmpeg.ZLIB_SHA256 not in instructions:
            raise ValueError("incomplete zlib rebuild instructions")
        notice = payload["native/licenses/Microsoft-Visual-C++-Runtime.txt"].decode("utf-8")
        if "Microsoft Visual C++ Runtime" not in notice or "https://visualstudio.microsoft.com/license-terms/" not in notice:
            raise ValueError("missing Visual C++ runtime notice")


def entries(bundle: Path, platform: str) -> dict[str, tuple[bytes, int]]:
    """Read a bundle, rejecting symbolic links and unexpected directory entries."""
    if platform not in PLATFORMS or bundle.is_symlink() or not bundle.is_dir():
        raise ValueError("invalid bundle root or platform")
    result = {}
    def walk(directory):
        for path in directory.iterdir():
            if path.is_symlink():
                raise ValueError(f"symlink in bundle: {path}")
            if path.is_dir():
                walk(path)
            elif path.is_file():
                result[path.relative_to(bundle).as_posix()] = (path.read_bytes(), path.stat().st_mode & 0o777)
            else:
                raise ValueError(f"unsupported bundle entry: {path}")
    walk(bundle)
    validate_entries({name: data for name, (data, _) in result.items()}, platform)
    return result


def output(*args):
    return subprocess.check_output(args, text=True).strip()


def verify_linkage(path, library_dir, *, relocated):
    names = "|".join(ffmpeg.LIBRARIES)
    for line in output("otool", "-L", str(path)).splitlines()[1:]:
        dependency = line.strip().split(" (", 1)[0]
        if dependency.startswith(("/usr/lib/", "/System/Library/")):
            continue
        name = Path(dependency).name
        if not re.fullmatch(rf"lib({names})(\.[0-9]+)*\.dylib", name):
            raise RuntimeError(f"unapproved native dependency: {dependency}")
        expected = f"@executable_path/lib/{name}" if relocated else str(library_dir / name)
        if dependency != expected or not (library_dir / name).is_file():
            raise RuntimeError(f"dependency outside bundled FFmpeg: {dependency}")


def relocate_macos(paths, prefix, library_dir):
    """Rewrite every controlled FFmpeg load, including libraries unused by the CLI."""
    names = "|".join(ffmpeg.LIBRARIES)
    approved = {runtime_name(name, "darwin-arm64") for name in ffmpeg.LIBRARIES}
    for path in paths:
        changes = []
        for index, line in enumerate(output("otool", "-L", str(path)).splitlines()[1:]):
            dependency = line.strip().split(" (", 1)[0]
            if dependency.startswith(("/usr/lib/", "/System/Library/")):
                continue
            name = Path(dependency).name
            if name not in approved and not re.fullmatch(rf"lib({names})(\.[0-9]+)*\.dylib", name):
                raise RuntimeError(f"unapproved native dependency: {dependency}")
            # The original install-name may use the full library version, while
            # the bundle contains only the major-version dylib.
            match = re.fullmatch(rf"lib({names})\.[0-9]+(?:\.[0-9]+)*\.dylib", name)
            if match is None:
                raise RuntimeError(f"invalid FFmpeg install-name: {dependency}")
            bundled = runtime_name(match.group(1), "darwin-arm64")
            if dependency != str(prefix / "lib" / name) or not (library_dir / bundled).is_file():
                raise RuntimeError(f"dependency outside controlled FFmpeg prefix: {dependency}")
            if index == 0 and path != paths[0] and bundled == path.name:
                continue  # otool includes this dylib's own install-name first.
            changes.extend(("-change", dependency, f"@executable_path/lib/{bundled}"))
        if path != paths[0]:
            changes.extend(("-id", f"@executable_path/lib/{path.name}"))
        if changes:
            subprocess.run(["install_name_tool", *changes, str(path)], check=True)


def windows_dependencies(path):
    listing = output("dumpbin", "/nologo", "/dependents", str(path))
    return listing.split("dependencies:", 1)[1].split("Summary", 1)[0].split()


def verify_windows_linkage(path, directory):
    names = "|".join(ffmpeg.LIBRARIES)
    for dependency in windows_dependencies(path):
        lower = dependency.lower()
        if re.fullmatch(rf"({names})-[0-9]+\.dll", lower) or lower in VC_RUNTIME_DLLS:
            if not (directory / dependency).is_file():
                raise RuntimeError(f"missing bundled DLL: {dependency}")
        elif not (lower.startswith(SYSTEM_DLL_PREFIXES) or (ffmpeg.SYSTEM_DIRECTORY / dependency).is_file()):
            raise RuntimeError(f"unapproved DLL: {dependency}")


def verify_linux_linkage(path, directory):
    versions = re.findall(r"GLIBC_([0-9]+)\.([0-9]+)", output("readelf", "--version-info", str(path)))
    if any(tuple(map(int, version)) > (2, 35) for version in versions):
        raise RuntimeError(f"glibc newer than 2.35: {path}")
    names = "|".join(ffmpeg.LIBRARIES)
    for name in output("patchelf", "--print-needed", str(path)).splitlines():
        if re.fullmatch(rf"lib({names})\.so\.[0-9]+", name):
            if not (directory / name).is_file():
                raise RuntimeError(f"missing bundled ELF library: {name}")
        elif name not in LINUX_SYSTEM_LIBRARIES:
            raise RuntimeError(f"unapproved ELF dependency: {name}")


def vc_runtime_library(name):
    redist = os.environ.get("VCToolsRedistDir")
    if not redist:
        raise RuntimeError("VCToolsRedistDir is unset")
    candidates = sorted(Path(redist, "x64").glob(f"Microsoft.VC*.CRT/{name}"))
    if not candidates:
        raise RuntimeError(f"missing VC redistributable: {name}")
    return candidates[-1]


def build(binary: Path, prefix: Path, platform: str, bundle: Path):
    if platform not in PLATFORMS:
        raise ValueError(f"unknown platform: {platform}")
    system = {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}.get(host_platform.system())
    arch = {"aarch64": "arm64", "arm64": "arm64", "AMD64": "x86_64", "x86_64": "x86_64"}.get(host_platform.machine())
    if platform != f"{system}-{arch}":
        raise RuntimeError("native build platform does not match requested bundle")
    ffmpeg.validate(prefix)
    if bundle.exists():
        raise RuntimeError(f"output already exists: {bundle}")
    binary = binary.resolve(strict=True)
    if binary.name != ("tsrct-conv.exe" if system == "windows" else "tsrct-conv"):
        raise RuntimeError("unexpected converter binary name")
    bundle.mkdir(parents=True)
    cli = bundle / binary.name
    shutil.copy2(binary, cli)
    library_dir = bundle if system == "windows" else bundle / "lib"
    library_dir.mkdir(exist_ok=True)
    libraries = []
    for name in ffmpeg.LIBRARIES:
        source = ffmpeg.shared_library(prefix, name)
        filename = runtime_name(name, platform)
        if system == "linux" and output("patchelf", "--print-soname", str(source)) != filename:
            raise RuntimeError(f"wrong FFmpeg SONAME: {source}")
        target = library_dir / filename
        shutil.copy2(source, target)
        libraries.append(target)
    if system == "windows":
        binaries = [cli, *libraries]
        runtime = {dependency.lower() for path in binaries for dependency in windows_dependencies(path) if dependency.lower() in VC_RUNTIME_DLLS}
        binaries += [Path(shutil.copy2(vc_runtime_library(name), bundle / name)) for name in sorted(runtime)]
        for path in binaries:
            verify_windows_linkage(path, bundle)
        (bundle / "native/licenses").mkdir(parents=True)
        (bundle / "native/licenses/Microsoft-Visual-C++-Runtime.txt").write_text(VC_RUNTIME_NOTICE.format(files=", ".join(sorted(runtime)), version=os.environ.get("VCToolsVersion", "unknown")))
    elif system == "linux":
        for path in [cli, *libraries]:
            verify_linux_linkage(path, library_dir)
            subprocess.run(["patchelf", "--set-rpath", "$ORIGIN/lib" if path == cli else "$ORIGIN", str(path)], check=True)
    else:
        relocate_macos([cli, *libraries], prefix, library_dir)
        for path in [*libraries, cli]:
            if arch not in output("lipo", "-archs", str(path)).split():
                raise RuntimeError(f"wrong architecture: {path}")
            verify_linkage(path, library_dir, relocated=True)
            ffmpeg.validate_macos_target(path)
            subprocess.run(["codesign", "--force", "--sign", "-", str(path)], check=True)
    native = bundle / "native"
    (native / "sources").mkdir(parents=True)
    (native / "licenses/ffmpeg").mkdir(parents=True)
    for name in (f"ffmpeg-{ffmpeg.VERSION}.tar.xz", "BUILD.txt") + ((f"zlib-{ffmpeg.ZLIB_VERSION}.tar.gz",) if system == "windows" else ()):
        shutil.copy2(prefix / "sources" / name, native / "sources" / name)
    for name in ("COPYING.LGPLv2.1", "LICENSE.md"):
        shutil.copy2(prefix / "licenses" / name, native / "licenses/ffmpeg" / name)
    if system == "windows":
        shutil.copy2(prefix / "zlib/LICENSE.zlib", native / "licenses/ffmpeg/LICENSE.zlib")
    shutil.copy2(prefix / "build.json", native / "ffmpeg-build.json")
    files = {path.relative_to(bundle).as_posix(): digest(path.read_bytes()) for path in bundle.rglob("*") if path.is_file()}
    (native / "manifest.json").write_text(json.dumps({"schemaVersion": 1, "platform": platform, "ffmpegVersion": ffmpeg.VERSION, "files": files}, sort_keys=True, indent=2) + "\n")
    entries(bundle, platform)
    print(bundle)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--ffmpeg-prefix", required=True, type=Path)
    parser.add_argument("--platform", required=True, choices=sorted(PLATFORMS))
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    build(args.binary, args.ffmpeg_prefix.resolve(), args.platform, args.output)


if __name__ == "__main__":
    main()
