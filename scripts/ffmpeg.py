#!/usr/bin/env python3
"""Build and validate the standalone converter LGPL FFmpeg package."""
import argparse
import contextlib
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import tarfile
import urllib.request

VERSION = "7.1.5"
SOURCE_SHA256 = "de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f"
SOURCE_URL = f"https://ffmpeg.org/releases/ffmpeg-{VERSION}.tar.xz"
DEPLOYMENT_TARGET = "14.0"
# zlib backs FFmpeg's PNG codec. macOS links the system copy; Windows builds
# this pinned release statically with the MSVC toolchain (zlib license).
ZLIB_VERSION = "1.3.1"
ZLIB_SHA256 = "9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23"
# The upstream release asset; zlib.net has served a mismatching body to hosted runners.
ZLIB_URL = f"https://github.com/madler/zlib/releases/download/v{ZLIB_VERSION}/zlib-{ZLIB_VERSION}.tar.gz"
LIBRARIES = ("avcodec", "avformat", "avutil", "avfilter", "avdevice", "swscale", "swresample")
HOST = platform.system()
# Windows' own DLL directory; bundles may import from it without shipping copies.
SYSTEM_DIRECTORY = Path(os.environ.get("SystemRoot", r"C:\Windows")) / "System32"
# The LGPL-only core is shared; each platform adds its media framework. Windows
# enables x86 assembly because software decode is its only decode path, and the
# MSVC toolchain so the DLLs match the CLI's C runtime.
COMMON_OPTIONS = (
    "--enable-shared", "--disable-static", "--disable-autodetect",
    "--disable-gpl", "--disable-nonfree", "--disable-version3",
    "--disable-doc", "--disable-debug", "--disable-ffplay", "--disable-network",
    "--disable-indevs", "--disable-outdevs",
)
PLATFORM_OPTIONS = {
    "Darwin": ("--disable-x86asm", "--enable-videotoolbox", "--enable-audiotoolbox",
               "--enable-zlib", "--cc=/usr/bin/clang"),
    "Linux": ("--enable-x86asm", "--enable-zlib"),
    "Windows": ("--toolchain=msvc", "--arch=x86_64", "--enable-x86asm",
                "--enable-mediafoundation", "--enable-d3d11va", "--enable-dxva2", "--enable-zlib"),
}
OPTIONS = COMMON_OPTIONS + PLATFORM_OPTIONS.get(HOST, ())
# Media-framework H.264 on Mac/Windows; Linux has no bundled H.264 encoder.
REQUIRED_ENCODERS = {
    "Darwin": ("h264_videotoolbox", "prores_ks", "aac"),
    "Windows": ("h264_mf", "prores_ks", "aac"),
    "Linux": ("prores_ks", "aac"),
}


def output(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()


def validate_license(license_name, configuration):
    if license_name != "LGPL version 2.1 or later":
        raise RuntimeError(f"public FFmpeg must be LGPL 2.1+: {license_name}")
    flags = set(shlex.split(configuration))
    if flags & {"--enable-gpl", "--enable-nonfree", "--enable-version3"}:
        raise RuntimeError("public FFmpeg contains prohibited configure flags")
    if not set(OPTIONS).issubset(flags):
        raise RuntimeError("public FFmpeg does not match the controlled build configuration")


def validate_macos_target(path):
    """Reject dependencies that silently raise the supported macOS minimum."""
    commands = output("/usr/bin/otool", "-l", str(path))
    versions = re.findall(
        r"cmd LC_BUILD_VERSION\s+cmdsize \d+\s+platform (?:1|macos)\s+minos ([\d.]+)", commands)
    versions += re.findall(
        r"cmd LC_VERSION_MIN_MACOSX\s+cmdsize \d+\s+version ([\d.]+)", commands)
    if not versions:
        raise RuntimeError(f"cannot determine macOS deployment target: {path}")
    maximum = tuple(map(int, DEPLOYMENT_TARGET.split("."))) + (0,)
    for version in versions:
        required = (tuple(map(int, version.split("."))) + (0, 0))[:3]
        if required > maximum:
            raise RuntimeError(
                f"{path} requires macOS {version}, above the public target {DEPLOYMENT_TARGET}. "
                "Rebuild or replace this dependency; do not raise the bundle minimum automatically.")


def executable(name):
    """Helper executable name on this platform (`ffmpeg` or `ffmpeg.exe`)."""
    return name + (".exe" if HOST == "Windows" else "")


def shared_library(prefix, name):
    """The versioned shared library for `name` on the current platform."""
    if HOST == "Windows":
        candidates = sorted((prefix / "bin").glob(f"{name}-*.dll"))
    elif HOST == "Linux":
        candidates = sorted((prefix / "lib").glob(f"lib{name}.so.*"))
    else:
        candidates = sorted((prefix / "lib").glob(f"lib{name}.*.dylib"))
    if not candidates:
        raise RuntimeError(f"public FFmpeg is missing {name}")
    return candidates[0]


def validate(prefix, *, check_programs=True):
    if HOST == "Darwin":
        binaries = {path.resolve() for path in (prefix / "lib").glob("*.dylib")}
        if check_programs:
            binaries.update(prefix / "bin" / name for name in ("ffmpeg", "ffprobe"))
        for path in sorted(binaries):
            validate_macos_target(path)
    # On Windows the FFmpeg DLLs depend on each other; make their directory a
    # DLL search location while they are loaded.
    dll_directory = os.add_dll_directory(str(prefix / "bin")) if HOST == "Windows" else contextlib.nullcontext()
    with dll_directory:
        for name in LIBRARIES:
            library = ctypes.CDLL(str(shared_library(prefix, name)))
            license_fn = getattr(library, f"{name}_license")
            license_fn.restype = ctypes.c_char_p
            config_fn = getattr(library, f"{name}_configuration")
            config_fn.restype = ctypes.c_char_p
            validate_license(license_fn().decode(), config_fn().decode())
    if check_programs:
        for name in ("ffmpeg", "ffprobe"):
            version = output(str(prefix / "bin" / executable(name)), "-version")
            if not version.startswith(f"{name} version {VERSION} "):
                raise RuntimeError(f"unexpected {name} version: {version.splitlines()[0]}")
        ffmpeg = str(prefix / "bin" / executable("ffmpeg"))
        encoders = output(ffmpeg, "-hide_banner", "-encoders")
        for name in REQUIRED_ENCODERS[HOST]:
            if not any(len(line.split()) > 1 and line.split()[1] == name for line in encoders.splitlines()):
                raise RuntimeError(f"public FFmpeg lacks required encoder {name}")
        if "libx264" in encoders or "libx265" in encoders:
            raise RuntimeError("public FFmpeg contains GPL video encoders")
        decoders = output(ffmpeg, "-hide_banner", "-decoders")
        for name in ("png", "h264", "hevc", "aac"):
            if not any(len(line.split()) > 1 and line.split()[1] == name for line in decoders.splitlines()):
                raise RuntimeError(f"public FFmpeg lacks required decoder {name}")


def fetch_source(sources, work, name, url, sha256):
    """Download (once) and verify a release tarball, extract it under `work`, return its directory."""
    archive = sources / name
    if not archive.exists():
        temporary = archive.with_suffix(".download")
        urllib.request.urlretrieve(url, temporary)
        temporary.rename(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != sha256:
        archive.unlink()  # never let a bad download survive into a cached prefix
        raise RuntimeError(f"source checksum mismatch: {archive} (removed; rerun to download again)")
    with tarfile.open(archive) as tar:
        tar.extractall(work, filter="data")
    return work / name.removesuffix(".tar.gz").removesuffix(".tar.xz")


def build_zlib(sources, work, prefix, env):
    """Build zlib statically with MSVC; returns the configure flags that point FFmpeg at it."""
    source = fetch_source(sources, work, f"zlib-{ZLIB_VERSION}.tar.gz", ZLIB_URL, ZLIB_SHA256)
    subprocess.run(["nmake", "-f", "win32/Makefile.msc", "zlib.lib"], cwd=source, env=env, check=True)
    staging = prefix / "zlib"
    shutil.rmtree(staging, ignore_errors=True)
    (staging / "include").mkdir(parents=True)
    (staging / "lib").mkdir()
    shutil.copy2(source / "zlib.h", staging / "include" / "zlib.h")
    # The tarball's zconf.h tests `#ifdef HAVE_UNISTD_H`; FFmpeg's config.h
    # defines it as 0 under MSVC, which still counts as defined and drags in
    # <unistd.h>. vcpkg applies the same one-line fix.
    zconf = (source / "zconf.h").read_text()
    guard = "#ifdef HAVE_UNISTD_H"
    if guard not in zconf:
        raise RuntimeError("zconf.h layout changed; review the HAVE_UNISTD_H patch")
    (staging / "include" / "zconf.h").write_text(
        zconf.replace(guard, "#if defined(HAVE_UNISTD_H) && HAVE_UNISTD_H", 1))
    # FFmpeg's MSVC toolchain spells `-lz` as `zlib.lib`, the name nmake produces.
    shutil.copy2(source / "zlib.lib", staging / "lib" / "zlib.lib")
    shutil.copy2(source / "README", staging / "LICENSE.zlib")
    print("staged zlib:", sorted(path.name for path in (staging / "lib").iterdir()),
          sorted(path.name for path in (staging / "include").iterdir()))
    # Mixed-form paths (D:/a/...) survive the MSYS2 shell and are accepted by cl/link.
    include = output("cygpath", "-m", str(staging / "include"))
    lib = output("cygpath", "-m", str(staging / "lib"))
    return [f"--extra-cflags=-I{include}", f"--extra-ldflags=-LIBPATH:{lib}"]


def configure_failure_excerpt(lines):
    """The probes that mention an external library (with their compiler/linker output) plus the tail."""
    interesting = set()
    for index, line in enumerate(lines):
        if line.startswith(("check_lib", "check_pkg_config", "require")) or "zlib" in line.lower():
            interesting.update(range(index, min(index + 25, len(lines))))
    interesting.update(range(max(0, len(lines) - 40), len(lines)))
    return "\n".join(f"{index + 1}: {lines[index]}" for index in sorted(interesting))


def msys_tool(name):
    """Windows path of an MSYS2 tool (`bash`, `make`) when running under its shell."""
    return output("cygpath", "-w", f"/usr/bin/{name}")


def toolchain_identity():
    """Compiler and SDK versions that must match for a cached package to be reused."""
    if HOST == "Windows":
        # `cl` prints its banner on stderr and exits non-zero without input.
        banner = subprocess.run(["cl"], capture_output=True, text=True).stderr.strip().splitlines()
        return {"compiler": banner[0] if banner else "", "sdk": os.environ.get("WindowsSDKVersion", "")}
    if HOST == "Linux":
        return {"compiler": output("cc", "--version"), "libc": output("ldd", "--version")}
    return {"compiler": output("/usr/bin/clang", "--version"), "sdk": output("xcrun", "--show-sdk-version")}


def build_identity(prefix):
    if HOST not in PLATFORM_OPTIONS:
        raise RuntimeError("the public FFmpeg package supports macOS, Windows, and Linux")
    if VERSION.split(".")[0] != "7":
        raise RuntimeError("converter FFmpeg must remain on the reviewed major version")
    return {"version": VERSION, "sourceSha256": SOURCE_SHA256, "options": list(OPTIONS),
            "architecture": platform.machine(), "deploymentTarget": DEPLOYMENT_TARGET,
            "zlib": ZLIB_VERSION if HOST == "Windows" else "system",
            **toolchain_identity(), "prefix": str(prefix)}


def build(prefix):
    identity = build_identity(prefix)
    manifest = prefix / "build.json"
    if manifest.is_file() and json.loads(manifest.read_text()) == identity:
        validate(prefix)
        return
    prefix.mkdir(parents=True, exist_ok=True)
    sources = prefix / "sources"
    sources.mkdir(exist_ok=True)
    work = prefix / "build"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir()
    source = fetch_source(sources, work, f"ffmpeg-{VERSION}.tar.xz", SOURCE_URL, SOURCE_SHA256)
    env = os.environ | {"MACOSX_DEPLOYMENT_TARGET": DEPLOYMENT_TARGET, "PKG_CONFIG_PATH": "",
                        "CFLAGS": "", "CPPFLAGS": "", "LDFLAGS": ""}
    # Machine-specific flags stay outside OPTIONS: the license gate checks that
    # OPTIONS is a subset of the recorded configuration, not equal to it.
    extra_flags = build_zlib(sources, work, prefix, env) if HOST == "Windows" else []
    if HOST == "Windows":
        # Run from an MSYS2 shell with the MSVC tools on PATH: configure is a
        # shell script and the MSVC toolchain needs `cl`/`link` to win over
        # MSYS's `link`. Paths are POSIX inside that shell. The tools are named
        # explicitly: a bare `bash` resolves to the WSL launcher in System32.
        posix_prefix = output("cygpath", "-u", str(prefix))
        configure = [msys_tool("bash"), "./configure", f"--prefix={posix_prefix}", *OPTIONS, *extra_flags]
        make = msys_tool("make")
    else:
        configure = ["./configure", f"--prefix={prefix}", *OPTIONS, *extra_flags]
        make = "make"
    if subprocess.run(configure, cwd=source, env=env).returncode != 0:
        log = source / "ffbuild/config.log"
        if log.is_file():
            print(configure_failure_excerpt(log.read_text(errors="replace").splitlines()))
        raise RuntimeError("FFmpeg configure failed; see ffbuild/config.log excerpt above")
    subprocess.run([make, f"-j{os.cpu_count() or 2}"], cwd=source, env=env, check=True)
    subprocess.run([make, "install"], cwd=source, env=env, check=True)
    if HOST == "Windows":
        # The MSVC toolchain installs import libraries beside the DLLs;
        # ffmpeg-sys-next links from FFMPEG_DIR/lib.
        for library in (prefix / "bin").glob("*.lib"):
            shutil.copy2(library, prefix / "lib" / library.name)
    if HOST == "Linux":
        # RUNPATH belongs to each ELF object (it is not inherited by children).
        # No LD_LIBRARY_PATH wrapper: external encoders must keep their own ABI.
        for path in {p.resolve() for p in (prefix / "lib").glob("*.so.*")}:
            subprocess.run(["patchelf", "--set-rpath", "$ORIGIN", str(path)], check=True)
        for name in ("ffmpeg", "ffprobe"):
            subprocess.run(["patchelf", "--set-rpath", "$ORIGIN/../lib", str(prefix / "bin" / name)], check=True)
    licenses = prefix / "licenses"
    licenses.mkdir(exist_ok=True)
    for name in ("COPYING.LGPLv2.1", "LICENSE.md"):
        shutil.copy2(source / name, licenses / name)
    (sources / "BUILD.txt").write_text(
        f"Source: {SOURCE_URL}\nSHA-256: {SOURCE_SHA256}\nUnmodified upstream source.\n"
        f"Environment: MACOSX_DEPLOYMENT_TARGET={DEPLOYMENT_TARGET}; empty CFLAGS, CPPFLAGS, LDFLAGS, PKG_CONFIG_PATH.\n"
        + shlex.join(configure) + "\nmake -j2\nmake install\n"
        + (f"zlib {ZLIB_VERSION} built statically from {ZLIB_URL} (SHA-256 {ZLIB_SHA256}).\n"
           "Run nmake -f win32/Makefile.msc zlib.lib using the unmodified zlib source.\n"
           "In the staged copy of zconf.h used to build FFmpeg, replace the first "
           "'#ifdef HAVE_UNISTD_H' with '#if defined(HAVE_UNISTD_H) && HAVE_UNISTD_H' "
           "to match MSVC's FFmpeg config.h (which defines HAVE_UNISTD_H=0).\n"
           "Stage zlib.h, patched zconf.h and zlib.lib as recorded by the configure "
           "--extra-cflags/--extra-ldflags.\n"
           "Packaging copies the DLLs beside the executable.\n" if HOST == "Windows" else
           "ELF RUNPATH: $ORIGIN for libraries, $ORIGIN/../lib for build-prefix helpers, "
           "$ORIGIN/lib for the packaged converter.\n" if HOST == "Linux" else
           "Packaging rewrites Mach-O library paths and applies ad-hoc signatures.\n"))
    validate(prefix)
    manifest.write_text(json.dumps(identity, indent=2) + "\n")
    shutil.rmtree(work)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("prefix", type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--cache-key", action="store_true", help="Print the build identity hash without building")
    parser.add_argument("--libraries-only", action="store_true")
    args = parser.parse_args()
    if args.libraries_only and not args.check:
        parser.error("--libraries-only requires --check")
    if args.cache_key:
        identity = json.dumps(build_identity(args.prefix.resolve()), sort_keys=True).encode()
        print(hashlib.sha256(identity).hexdigest())
    elif args.check:
        validate(args.prefix.resolve(), check_programs=not args.libraries_only)
    else:
        build(args.prefix.resolve())
