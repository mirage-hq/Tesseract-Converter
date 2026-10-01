#!/bin/sh
# Installs a complete binary release from the Tesseract-Converter repository.
set -eu
command -v python3 >/dev/null 2>&1 || { echo 'Python 3 is required' >&2; exit 1; }
python3 -c 'import sys; raise SystemExit(sys.version_info < (3, 9))' || {
  echo 'Python 3.9 or newer is required' >&2; exit 1;
}
command -v curl >/dev/null 2>&1 || { echo 'curl is required' >&2; exit 1; }
exec python3 - "$@" <<'PY'
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile

FFMPEG_VERSION = '7.1.5'
FFMPEG_LIBRARIES = {
    'avcodec': 61, 'avformat': 61, 'avutil': 59, 'avfilter': 10,
    'avdevice': 61, 'swscale': 8, 'swresample': 5,
}
NATIVE_PROVENANCE = {
    'native/ffmpeg-build.json', 'native/sources/ffmpeg-7.1.5.tar.xz',
    'native/sources/BUILD.txt',
    'native/licenses/ffmpeg/COPYING.LGPLv2.1',
    'native/licenses/ffmpeg/LICENSE.md',
}


def fail(message):
    raise ValueError(message)


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            fail('duplicate JSON key: ' + key)
        value[key] = item
    return value


def parse_json(data, label):
    try:
        return json.loads(data.decode('utf-8'), object_pairs_hook=unique_object)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f'invalid {label}: {error}')


def fetch(url, accept='application/vnd.github+json'):
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != 'https':
        fail('non-HTTPS download refused')
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None

    headers = {'Accept': accept, 'User-Agent': 'Tesseract-Converter-installer'}
    request = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.build_opener(NoRedirect).open(request, timeout=30) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        if error.code not in (301, 302, 303, 307, 308):
            fail(f'download failed (HTTP {error.code})')
        target = error.headers.get('Location', '')
        redirected = urllib.parse.urlsplit(target)
        if redirected.scheme != 'https' or redirected.hostname not in (
                'release-assets.githubusercontent.com', 'objects.githubusercontent.com'):
            fail('untrusted download redirect')
        return fetch(target)  # HTTPS redirects stay within GitHub's asset hosts.


def choose_platform(system=None, machine=None):
    system = system or platform.system()
    machine = machine or platform.machine()
    arch = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'x86_64', 'AMD64': 'x86_64'}.get(machine)
    if (system, arch) not in (('Darwin', 'arm64'), ('Darwin', 'x86_64'), ('Linux', 'x86_64')):
        fail('unsupported platform; Windows users should install the ZIP manually')
    return ('darwin' if system == 'Darwin' else 'linux') + '-' + arch


def runtime_paths(platform_name):
    root = 'lib/'
    suffix = (lambda name, major: f'lib{name}.{major}.dylib') if platform_name.startswith('darwin-') else (
        lambda name, major: f'lib{name}.so.{major}')
    return {root + suffix(name, major) for name, major in FFMPEG_LIBRARIES.items()}


def verify_native(payload, platform_name):
    manifest = parse_json(payload['native/manifest.json'], 'native manifest')
    if set(manifest) != {'schemaVersion', 'platform', 'ffmpegVersion', 'files'}:
        fail('invalid native manifest fields')
    if (manifest['schemaVersion'], manifest['platform'], manifest['ffmpegVersion']) != (
            1, platform_name, FFMPEG_VERSION):
        fail('invalid native manifest identity')
    required = {'tsrct-conv'} | runtime_paths(platform_name) | NATIVE_PROVENANCE
    native_names = {name for name in payload if name == 'tsrct-conv' or name.startswith('lib/')
                    or name.startswith('native/')}
    if native_names != required | {'native/manifest.json'} or any(not payload[name] for name in required):
        fail('missing or unapproved native libraries, provenance, or licenses')
    expected = {name: hashlib.sha256(payload[name]).hexdigest() for name in required}
    if manifest['files'] != expected:
        fail('native payload checksum or file coverage mismatch')
    identity = parse_json(payload['native/ffmpeg-build.json'], 'FFmpeg build identity')
    if identity.get('version') != FFMPEG_VERSION or not re.fullmatch(
            r'[0-9a-f]{64}', identity.get('sourceSha256', '')):
        fail('invalid FFmpeg build provenance')


def verify_archive(data, checksum, stem, version, platform_name):
    name = stem + '.zip'
    digest = hashlib.sha256(data).hexdigest()
    try:
        checksum_text = checksum.decode('ascii')
    except UnicodeDecodeError:
        fail('invalid release checksum')
    if checksum_text != f'{digest}  {name}\n':
        fail('release checksum mismatch')
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        members = archive.infolist()
        names = [entry.filename for entry in members]
        required = {stem + '/' + item for item in (
            'tsrct-conv', 'build.json', 'README.md',
            'THIRD_PARTY_NOTICES.md', 'native/manifest.json')}
        if len(set(names)) != len(names) or not required.issubset(names):
            fail('duplicate or missing archive entries')
        for entry in members:
            mode = entry.external_attr >> 16
            if (not entry.filename.startswith(stem + '/') or '\\' in entry.filename
                    or any(part in ('', '.', '..') for part in entry.filename.split('/'))
                    or entry.is_dir() or stat.S_IFMT(mode) != stat.S_IFREG):
                fail('unsafe archive entry')
        payload = {entry.filename[len(stem) + 1:]: archive.read(entry) for entry in members}
        if not payload['THIRD_PARTY_NOTICES.md'].strip() or (
                'LICENSE' in payload and not payload['LICENSE'].strip()):
            fail('missing license notices')
        if not payload['tsrct-conv'] or not (archive.getinfo(stem + '/tsrct-conv').external_attr >> 16) & 0o111:
            fail('missing executable')
        provenance = parse_json(payload['build.json'], 'release provenance')
        if provenance != {'version': version, 'platform': platform_name,
                           'source_sha': provenance.get('source_sha')} or not re.fullmatch(
                               r'[0-9a-f]{40}', provenance.get('source_sha', '')):
            fail('invalid release provenance')
        verify_native(payload, platform_name)
        if archive.testzip() is not None:
            fail('corrupt archive')


def managed_link_target(link, base):
    try:
        target = link.readlink()
    except OSError:
        return False
    absolute = target if target.is_absolute() else link.parent / target
    normalized = Path(os.path.abspath(absolute))
    expected_base = Path(os.path.abspath(base))
    return (normalized.name == 'tsrct-conv'
            and normalized.parent.parent == expected_base
            and re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+-(darwin-(arm64|x86_64)|linux-x86_64)',
                             normalized.parent.name) is not None)


def install(args):
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', args.repo) or '..' in args.repo:
        fail('invalid repository')
    if args.version and not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', args.version):
        fail('invalid version')
    platform_name = choose_platform()
    api = 'https://api.github.com/repos/' + args.repo + '/releases/'
    endpoint = 'tags/v' + args.version if args.version else 'latest'
    release = parse_json(fetch(api + endpoint), 'release response')
    version = args.version or release['tag_name'].removeprefix('v')
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version) or release['tag_name'] != 'v' + version or release.get('draft') or release.get('prerelease'):
        fail('unexpected release identity')
    stem = f'Tesseract-Converter-{version}-{platform_name}'
    assets = {}
    for item in release['assets']:
        if item.get('name') in assets:
            fail('duplicate release asset: ' + str(item.get('name')))
        assets[item.get('name')] = item

    def asset(name):
        item = assets.get(name)
        if not item or not isinstance(item.get('id'), int):
            fail('release asset missing: ' + name)
        return fetch(api.replace('/releases/', '/releases/assets/') + str(item['id']),
                     'application/octet-stream')

    data = asset(stem + '.zip')
    verify_archive(data, asset(stem + '.zip.sha256'), stem, version, platform_name)
    prefix = Path(args.prefix).expanduser().absolute()
    base = prefix / 'share' / 'Tesseract-Converter'
    destination = base / f'{version}-{platform_name}'
    link = prefix / 'bin' / 'tsrct-conv'
    if destination.exists() or destination.is_symlink():
        fail('installation destination already exists; refusing overwrite')
    if (link.exists() or link.is_symlink()) and (not link.is_symlink() or not managed_link_target(link, base)):
        fail('unrelated command entry exists; refusing overwrite')
    base.mkdir(parents=True, exist_ok=True)
    link.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix='.install-', dir=base))
    link_staging = None
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            for entry in archive.infolist():
                relative = entry.filename[len(stem) + 1:]
                target = staging / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(entry) as source, target.open('xb') as output:
                    shutil.copyfileobj(source, output)
                target.chmod((entry.external_attr >> 16) & 0o777)
        result = subprocess.run([str(staging / 'tsrct-conv'), '--version'], check=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=30)
        if result.stdout != f'tsrct-conv {version}\n':
            fail('installed binary reported unexpected version')
        # Defer cancellation across the atomic activation: never roll back the
        # bundle after its command link has already become visible.
        previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
        try:
            staging.rename(destination)
            try:
                link_staging = Path(tempfile.mkdtemp(prefix='.tsrct-conv-link-', dir=link.parent))
                temporary_link = link_staging / 'tsrct-conv'
                temporary_link.symlink_to(destination / 'tsrct-conv')
                os.replace(temporary_link, link)
            except BaseException:
                destination.rename(staging)
                raise
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
    finally:
        if link_staging is not None:
            shutil.rmtree(link_staging, ignore_errors=True)
        if staging.exists():
            shutil.rmtree(staging)
    print(f'Installed {destination}; add {link.parent} to PATH if necessary')


def interrupted(_signum, _frame):
    raise KeyboardInterrupt


if __name__ == '__main__':
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser()
    parser.add_argument('--version')
    parser.add_argument('--repo', default='mirage-hq/Tesseract-Converter')
    parser.add_argument('--prefix', default='~/.local')
    try:
        install(parser.parse_args())
    except (ValueError, KeyError, TypeError, OSError, zipfile.BadZipFile,
            subprocess.SubprocessError) as error:
        print(f'Installation failed: {error}', file=sys.stderr)
        sys.exit(1)
PY
