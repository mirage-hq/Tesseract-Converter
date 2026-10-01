#!/usr/bin/env python3
"""Build and verify standalone binary ZIPs without publishing a release."""

from __future__ import annotations

import argparse
from contextlib import nullcontext
from io import BytesIO
import hashlib
import json
from pathlib import Path
import re
import secrets
import subprocess
import tempfile
import tomllib
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

import native_bundle

PLATFORMS = (
    "darwin-arm64",
    "darwin-x86_64",
    "windows-x86_64",
    "linux-x86_64",
)
SHA_PATTERN = re.compile(r"[0-9a-f]{40}\Z")


def portable_text(path: Path) -> bytes:
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"expected a nonempty regular text file: {path}")
    data = path.read_bytes()
    if not data.strip():
        raise ValueError(f"expected a nonempty regular text file: {path}")
    # Windows checkouts may use CRLF; keep every ZIP's text entries identical.
    return data.replace(b"\r\n", b"\n")


def legal_text(root: Path, relative: str) -> bytes:
    path = Path(relative)
    if path.is_absolute() or "\\" in relative or ".." in path.parts or path.as_posix() != relative:
        raise ValueError(f"unsafe legal artifact path: {relative}")
    for parent in [path, *path.parents]:
        if (root / parent).is_symlink():
            raise ValueError(f"expected regular text file; symlinked legal artifact path: {relative}")
    portable_text(root / path)  # Enforce the regular, nonempty text-file checks.
    return (root / path).read_bytes()


def validate_identity(root: Path, version: str, source_sha: str) -> None:
    if not SHA_PATTERN.fullmatch(source_sha):
        raise ValueError("source_sha must be a full lowercase Git SHA")
    with (root / "apps/tesseract-conv/Cargo.toml").open("rb") as file:
        package_version = tomllib.load(file)["package"]["version"]
    if version != package_version:
        raise ValueError(f"release version {version!r} differs from Cargo {package_version!r}")


def archive_stem(version: str, platform: str) -> str:
    if platform not in PLATFORMS:
        raise ValueError(f"unsupported platform: {platform}")
    return f"Tesseract-Converter-{version}-{platform}"


def archive_name(version: str, platform: str) -> str:
    return f"{archive_stem(version, platform)}.zip"


def archive_entries(version: str, platform: str, source_sha: str, root: Path, binary: Path,
                    native_runtime: Path | None = None, notices: Path | None = None):
    folder = archive_stem(version, platform)
    binary_name = "tsrct-conv.exe" if platform.startswith("windows-") else "tsrct-conv"
    if binary.name != binary_name or not binary.is_file() or binary.is_symlink():
        raise ValueError(f"expected a regular {binary_name} executable: {binary}")
    runtime = native_bundle.entries(native_runtime or binary.parent, platform)
    if runtime[binary_name][0] != binary.read_bytes():
        raise ValueError("requested executable differs from the native runtime bundle")
    provenance = {"version": version, "source_sha": source_sha, "platform": platform}
    entries = {
        **{f"{folder}/{path}": entry for path, entry in runtime.items()},
        f"{folder}/README.md": (portable_text(root / "README.md"), 0o644),
        f"{folder}/build.json": (json.dumps(provenance, sort_keys=True).encode() + b"\n", 0o644),
        f"{folder}/THIRD_PARTY_NOTICES.md": (portable_text(notices or root / "target/THIRD_PARTY_NOTICES.md"), 0o644),
    }
    license_file = root / "LICENSE"
    if license_file.exists():
        if not license_file.is_file() or license_file.is_symlink():
            raise ValueError("LICENSE must be a regular file")
        entries[f"{folder}/LICENSE"] = (portable_text(license_file), 0o644)
    return entries


def build(root: Path, version: str, source_sha: str, platform: str, binary: Path, output: Path,
          native_runtime: Path | None = None, notices: Path | None = None) -> Path:
    validate_identity(root, version, source_sha)
    entries = archive_entries(version, platform, source_sha, root, binary, native_runtime, notices)
    output.mkdir(parents=True, exist_ok=True)
    buffer = BytesIO()
    with ZipFile(buffer, "w", compression=ZIP_DEFLATED, compresslevel=9) as archive:
        for path, (data, mode) in sorted(entries.items()):
            info = ZipInfo(path, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (0o100000 | mode) << 16
            info.compress_type = ZIP_DEFLATED
            archive.writestr(info, data, compress_type=ZIP_DEFLATED, compresslevel=9)
    content = buffer.getvalue()
    digest = hashlib.sha256(content).hexdigest()
    name = archive_name(version, platform)
    zip_path = output / name
    checksum_path = output / f"{name}.sha256"
    if zip_path.exists() or checksum_path.exists():
        raise ValueError(f"refusing to overwrite existing release artifact: {name}")
    with zip_path.open("xb") as file:
        file.write(content)
    try:
        with checksum_path.open("x") as file:
            file.write(f"{digest}  {name}\n")
    except OSError:
        zip_path.unlink()
        raise
    verify_archive(zip_path, root, version, source_sha, platform, require_license=False)
    return zip_path


def verify_archive(zip_path: Path, root: Path, version: str, source_sha: str, platform: str, require_license: bool) -> None:
    if not zip_path.is_file() or zip_path.is_symlink():
        raise ValueError(f"missing expected release archive: {zip_path}")
    digest = hashlib.sha256(zip_path.read_bytes()).hexdigest()
    name = archive_name(version, platform)
    if zip_path.name != name:
        raise ValueError(f"unexpected archive name: {zip_path.name}")
    checksum_path = zip_path.parent / f"{name}.sha256"
    if not checksum_path.is_file() or checksum_path.is_symlink():
        raise ValueError(f"missing regular checksum file for {name}")
    checksum = checksum_path.read_text()
    if checksum != f"{digest}  {name}\n":
        raise ValueError(f"invalid checksum for {name}")
    folder = archive_stem(version, platform)
    binary = "tsrct-conv.exe" if platform.startswith("windows-") else "tsrct-conv"
    expected = {f"{folder}/{path}" for path in ("README.md", "build.json", "THIRD_PARTY_NOTICES.md")}
    license_entry = f"{folder}/LICENSE"
    with ZipFile(zip_path) as archive:
        actual = set(archive.namelist())
        if not expected.issubset(actual) or any(not path.startswith(folder + "/") for path in actual):
            raise ValueError(f"unexpected ZIP entries in {name}: {sorted(actual ^ expected)}")
        runtime_names = actual - expected - {license_entry}
        native_bundle.validate_entries({path[len(folder) + 1:]: archive.read(path)
                                        for path in runtime_names}, platform)
        for path in runtime_names:
            mode = archive.getinfo(path).external_attr >> 16
            if mode & 0o170000 != 0o100000:
                raise ValueError(f"non-regular runtime ZIP entry: {path}")
        if len(archive.namelist()) != len(actual) or (
            require_license and license_entry not in actual
        ):
            raise ValueError(f"duplicate ZIP entries or missing LICENSE/notices in {name}")
        provenance = json.loads(archive.read(f"{folder}/build.json"))
        if provenance != {"version": version, "source_sha": source_sha, "platform": platform}:
            raise ValueError(f"invalid build provenance in {name}")
        if archive.read(f"{folder}/README.md") != portable_text(root / "README.md"):
            raise ValueError(f"README differs from checkout in {name}")
        if not archive.read(f"{folder}/THIRD_PARTY_NOTICES.md").strip():
            raise ValueError(f"empty third-party notices in {name}")
        if require_license and archive.read(license_entry) != portable_text(root / "LICENSE"):
            raise ValueError(f"LICENSE differs from checkout in {name}")
        if not archive.read(f"{folder}/{binary}"):
            raise ValueError(f"empty executable in {name}")
        mode = archive.getinfo(f"{folder}/{binary}").external_attr >> 16
        if not platform.startswith("windows-") and mode & 0o111 != 0o111:
            raise ValueError(f"non-executable POSIX bundle: {name}")
        if archive.testzip() is not None:
            raise ValueError(f"corrupt ZIP: {name}")


def verify_set(root: Path, version: str, source_sha: str, output: Path, require_license: bool) -> None:
    validate_identity(root, version, source_sha)
    actual = {path.name for path in output.iterdir()}
    archives = {}
    for platform in PLATFORMS:
        stem = archive_stem(version, platform)
        candidates = [name for name in actual if name == f"{stem}.zip"]
        if len(candidates) != 1:
            raise ValueError(f"release artifacts missing or unexpected for {platform}: {sorted(candidates)}")
        archives[platform] = output / candidates[0]
    expected = {name for path in archives.values() for name in (path.name, f"{path.name}.sha256")}
    if actual != expected:
        raise ValueError(f"release artifacts missing or unexpected: {sorted(actual ^ expected)}")
    license_file = root / "LICENSE"
    if require_license and (
        not license_file.is_file() or license_file.is_symlink() or not license_file.read_bytes().strip()
    ):
        raise ValueError("approved nonempty project LICENSE is required before publishing")
    for platform in PLATFORMS:
        verify_archive(archives[platform], root, version, source_sha, platform, require_license)


def publish_cdn(
    root: Path, version: str, source_sha: str, output: Path, skill_output: Path | None = None
) -> str:
    """Upload only a verified release set to a new immutable public CDN prefix."""
    verify_set(root, version, source_sha, output, require_license=True)
    prefix = f"jerboa/Tesseract-Converter/v{version}/{secrets.token_hex(16)}"
    base = f"gs://captions-public-assets/{prefix}"
    url = f"https://captions-cdn.xyz/{prefix}/"
    if skill_output is not None:
        # Reserve the destination and render before any irreversible public upload.
        # Keep the skill hidden until the full upload succeeds.
        skill_output.mkdir(parents=True, exist_ok=False)
    try:
        with (tempfile.TemporaryDirectory(dir=skill_output) if skill_output is not None else nullcontext()) as staged:
            if skill_output is not None:
                write_skill(version, source_sha, output, url, Path(staged) / "skill")
            for path in sorted(output.iterdir()):
                content_type = "application/zip" if path.suffix == ".zip" else "text/plain"
                subprocess.run(
                    [
                        "gcloud", "storage", "cp", "--if-generation-match=0",
                        "--predefined-acl=publicRead",
                        "--cache-control=public, max-age=31536000, immutable",
                        f"--content-type={content_type}", str(path), f"{base}/{path.name}",
                    ],
                    check=True,
                    stdout=subprocess.DEVNULL,
                )
            if skill_output is not None:
                (Path(staged) / "skill" / "SKILL.md").replace(skill_output / "SKILL.md")
    except Exception:
        if skill_output is not None:
            skill_output.rmdir()
        raise
    return url


def write_skill(version: str, source_sha: str, output: Path, base_url: str, destination: Path) -> None:
    """Generate a portable skill only for successfully published, verified binaries."""
    rows = []
    for platform in PLATFORMS:
        stem = archive_stem(version, platform)
        files = list(output.glob(f"{stem}.zip"))
        if len(files) != 1:
            raise ValueError(f"expected exactly one archive for {platform}")
        path = files[0]
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if path.name != archive_name(version, platform):
            raise ValueError(f"unexpected archive name: {path.name}")
        rows.append(f"| {platform} | `{path.name}` | `{digest}` | {base_url}{path.name} |")
    text = f"""---
name: tsrct-conv
description: Install and use the pinned Tesseract converter CLI to import or export editable Premiere, After Effects and .tsrct projects.
---

# Tesseract converter

Version `{version}`; exact source commit `{source_sha}`. This build was published at the following CDN URLs. Verify the full SHA-256, not only the version.

| Platform | Archive | SHA-256 | Download URL |
| --- | --- | --- | --- |
{chr(10).join(rows)}

Keep the entire extracted directory together: the CLI loads its bundled FFmpeg libraries. Do not copy just the executable. The default library backend needs no separately installed FFmpeg. Linux supports copy/remux, PCM and ProRes preparation but has no bundled H.264 encoder; use an explicitly selected external encoder for new H.264 encoding there.

Pick the archive matching the host platform. Download its exact URL with `curl -fL -o <archive.zip> '<URL>'`, then check `echo '<SHA-256>  <archive.zip>' | shasum -a 256 -c -` (or compare `Get-FileHash <archive.zip> -Algorithm SHA256` on Windows). Stop on any mismatch. Unzip it and use the bundled `tsrct-conv` executable (`tsrct-conv.exe` on Windows). Do not execute an unverified download.

Run `tsrct-conv --help` for current options. Typical usage: `tsrct-conv convert input.prproj --to tesseract --output converted`; `tsrct-conv convert input.aep --to tesseract --output converted-ae --composition 1`; `tsrct-conv convert input.tsrct --to premiere --output exported`. An output directory must not already exist. Keep originals: import and export are best-effort and can omit unsupported controls or media; review diagnostics and visually inspect results. A successful conversion is not proof of Adobe render fidelity.
"""
    destination.mkdir(parents=True, exist_ok=False)
    (destination / "SKILL.md").write_text(text)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for command in ("build", "verify", "publish-cdn"):
        args = commands.add_parser(command)
        args.add_argument("--version", required=True)
        args.add_argument("--source-sha", required=True)
        args.add_argument("--output", type=Path, required=True)
        args.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
        if command == "build":
            args.add_argument("--platform", required=True, choices=PLATFORMS)
            args.add_argument("--binary", required=True, type=Path)
            args.add_argument("--native-bundle", required=True, type=Path)
            args.add_argument("--notices", required=True, type=Path)
        elif command == "verify":
            args.add_argument("--require-license", action="store_true")
        else:
            args.add_argument("--skill-output", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "build":
            print(build(args.root, args.version, args.source_sha, args.platform, args.binary, args.output,
                        args.native_bundle, args.notices))
        elif args.command == "verify":
            verify_set(args.root, args.version, args.source_sha, args.output, args.require_license)
            print("verified all four archives and checksums")
        else:
            print(publish_cdn(args.root, args.version, args.source_sha, args.output, args.skill_output))
    except (OSError, ValueError, RuntimeError, KeyError, json.JSONDecodeError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release archive: {error}\n")


if __name__ == "__main__":
    main()
