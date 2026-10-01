"""Offline installer security, rollback, and fake-network integration tests."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest import mock
import urllib.error
import zipfile

scope = {'__name__': 'installer_test'}
source = (Path(__file__).resolve().parents[1] / 'install.sh').read_text()
exec(source.split("<<'PY'\n", 1)[1].rsplit('\nPY', 1)[0], scope)


class Response:
    def __init__(self, data):
        self.data = data

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def read(self):
        return self.data


class InstallerTests(unittest.TestCase):
    VERSION = '1.2.3'
    PLATFORM = 'linux-x86_64'

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.prefix = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    def entries(self, version=VERSION, binary_version=None):
        binary_version = binary_version or version
        binary = f'#!/bin/sh\nprintf "tsrct-conv {binary_version}\\n"\n'.encode()
        entries = {
            'tsrct-conv': binary,
            'README.md': b'readme',
            'LICENSE': b'project license',
            'build.json': json.dumps({'version': version, 'platform': self.PLATFORM,
                                      'source_sha': 'a' * 40}).encode(),
            'THIRD_PARTY_NOTICES.md': b'notices',
            'MP4-LICENSE': b'mp4 license',
            'native/ffmpeg-build.json': json.dumps(
                {'version': '7.1.5', 'sourceSha256': 'b' * 64}).encode(),
            'native/sources/ffmpeg-7.1.5.tar.xz': b'pinned source',
            'native/sources/BUILD.txt': b'build instructions',
            'native/licenses/ffmpeg/COPYING.LGPLv2.1': b'lgpl',
            'native/licenses/ffmpeg/LICENSE.md': b'ffmpeg license',
        }
        for name, major in scope['FFMPEG_LIBRARIES'].items():
            entries[f'lib/lib{name}.so.{major}'] = (name + '-runtime').encode()
        manifest_names = {name for name in entries if name == 'tsrct-conv' or name.startswith('lib/')
                          or name.startswith('native/')}
        manifest_files = {name: hashlib.sha256(entries[name]).hexdigest() for name in manifest_names}
        entries['native/manifest.json'] = json.dumps({
            'schemaVersion': 1, 'platform': self.PLATFORM,
            'ffmpegVersion': '7.1.5', 'files': manifest_files,
        }).encode()
        return entries

    def archive(self, entries=None, duplicate=None, symlink=None):
        stem = f'Tesseract-Converter-{self.VERSION}-{self.PLATFORM}'
        entries = entries or self.entries()
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, 'w') as archive:
            for name, value in entries.items():
                info = zipfile.ZipInfo(stem + '/' + name)
                info.create_system = 3
                mode = stat.S_IFLNK | 0o777 if name == symlink else stat.S_IFREG | (0o755 if name == 'tsrct-conv' else 0o644)
                info.external_attr = mode << 16
                archive.writestr(info, value)
            if duplicate:
                info = zipfile.ZipInfo(stem + '/' + duplicate)
                info.create_system = 3
                info.external_attr = (stat.S_IFREG | 0o644) << 16
                archive.writestr(info, b'duplicate')
        data = buffer.getvalue()
        checksum = f'{hashlib.sha256(data).hexdigest()}  {stem}.zip\n'.encode()
        return data, checksum, stem

    def verify(self, data, checksum, stem):
        return scope['verify_archive'](data, checksum, stem, self.VERSION, self.PLATFORM)

    def release_fetch(self, data, checksum, duplicate_assets=False):
        stem = f'Tesseract-Converter-{self.VERSION}-{self.PLATFORM}'
        assets = [{'name': stem + '.zip', 'id': 1}, {'name': stem + '.zip.sha256', 'id': 2}]
        if duplicate_assets:
            assets.append({'name': stem + '.zip', 'id': 3})
        release = json.dumps({'tag_name': 'v' + self.VERSION, 'draft': False,
                              'prerelease': False, 'assets': assets}).encode()

        def fake_fetch(url, *_args):
            if url.endswith('/latest') or url.endswith('/tags/v' + self.VERSION):
                return release
            if url.endswith('/1') or url.endswith('/3'):
                return data
            if url.endswith('/2'):
                return checksum
            raise AssertionError(url)
        return fake_fetch

    def run_install(self, data, checksum, **patches):
        args = argparse.Namespace(version=None, repo='mirage-hq/Tesseract-Converter', prefix=str(self.prefix))
        fetch = patches.pop('fetch', self.release_fetch(data, checksum))
        with mock.patch.dict(scope, {'fetch': fetch, 'choose_platform': lambda: self.PLATFORM, **patches}):
            scope['install'](args)

    def test_platforms(self):
        choose = scope['choose_platform']
        self.assertEqual(choose('Darwin', 'arm64'), 'darwin-arm64')
        self.assertEqual(choose('Darwin', 'x86_64'), 'darwin-x86_64')
        self.assertEqual(choose('Linux', 'x86_64'), 'linux-x86_64')

    def test_unsupported_platforms(self):
        for system, machine in [('Windows', 'AMD64'), ('Linux', 'aarch64')]:
            with self.assertRaises(ValueError):
                scope['choose_platform'](system, machine)

    def test_valid_archive(self):
        self.verify(*self.archive())

    def test_checksum_corruption(self):
        data, checksum, stem = self.archive()
        with self.assertRaisesRegex(ValueError, 'checksum'):
            self.verify(data, b'0' * 64 + checksum[64:], stem)

    def test_missing_required_library(self):
        entries = self.entries()
        del entries['lib/libavcodec.so.61']
        data, checksum, stem = self.archive(entries)
        with self.assertRaisesRegex(ValueError, 'coverage|libraries'):
            self.verify(data, checksum, stem)

    def test_manifest_hash_mismatch(self):
        entries = self.entries()
        entries['lib/libavcodec.so.61'] = b'tampered'
        data, checksum, stem = self.archive(entries)
        with self.assertRaisesRegex(ValueError, 'checksum'):
            self.verify(data, checksum, stem)

    def test_invalid_manifest_identity(self):
        entries = self.entries()
        manifest = json.loads(entries['native/manifest.json'])
        manifest['platform'] = 'darwin-arm64'
        entries['native/manifest.json'] = json.dumps(manifest).encode()
        data, checksum, stem = self.archive(entries)
        with self.assertRaisesRegex(ValueError, 'identity'):
            self.verify(data, checksum, stem)

    def test_missing_native_license(self):
        entries = self.entries()
        del entries['native/licenses/ffmpeg/LICENSE.md']
        data, checksum, stem = self.archive(entries)
        with self.assertRaisesRegex(ValueError, 'coverage|licenses'):
            self.verify(data, checksum, stem)

    def test_unapproved_native_file(self):
        entries = self.entries()
        entries['native/extra'] = b'extra'
        data, checksum, stem = self.archive(entries)
        with self.assertRaisesRegex(ValueError, 'unapproved'):
            self.verify(data, checksum, stem)

    def test_private_archive_without_project_license(self):
        entries = self.entries()
        del entries['LICENSE']
        data, checksum, stem = self.archive(entries)
        self.verify(data, checksum, stem)
        self.run_install(data, checksum)
        self.assertFalse((self.prefix / 'share/Tesseract-Converter/1.2.3-linux-x86_64/LICENSE').exists())

    def test_empty_project_license_rejected_when_present(self):
        entries = self.entries()
        entries['LICENSE'] = b''
        with self.assertRaisesRegex(ValueError, 'missing license notices'):
            self.verify(*self.archive(entries))

    def test_missing_dependency_notices(self):
        entries = self.entries()
        del entries['THIRD_PARTY_NOTICES.md']
        with self.assertRaisesRegex(ValueError, 'missing'):
            self.verify(*self.archive(entries))

    def test_dependency_specific_notice_is_not_required_after_dependency_removal(self):
        entries = self.entries()
        del entries['MP4-LICENSE']
        self.verify(*self.archive(entries))

    def test_duplicate_zip_entry(self):
        data, checksum, stem = self.archive(duplicate='README.md')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            self.verify(data, checksum, stem)

    def test_path_escape(self):
        entries = self.entries()
        entries['../outside'] = b'bad'
        data, checksum, stem = self.archive(entries)
        with self.assertRaisesRegex(ValueError, 'unsafe'):
            self.verify(data, checksum, stem)

    def test_real_symlink_entry(self):
        data, checksum, stem = self.archive(symlink='README.md')
        with self.assertRaisesRegex(ValueError, 'unsafe'):
            self.verify(data, checksum, stem)

    def test_successful_fake_network_install_and_exact_version(self):
        data, checksum, _stem = self.archive()
        self.run_install(data, checksum)
        destination = self.prefix / 'share/Tesseract-Converter/1.2.3-linux-x86_64'
        link = self.prefix / 'bin/tsrct-conv'
        self.assertTrue((destination / 'native/manifest.json').is_file())
        self.assertEqual(link.resolve(), (destination / 'tsrct-conv').resolve())
        self.assertEqual(scope['subprocess'].check_output([link, '--version'], text=True),
                         'tsrct-conv 1.2.3\n')

    def test_upgrade_replaces_managed_link_and_preserves_old_install(self):
        old = self.prefix / 'share/Tesseract-Converter/1.0.0-linux-x86_64'
        old.mkdir(parents=True)
        (old / 'tsrct-conv').write_text('old')
        link = self.prefix / 'bin/tsrct-conv'
        link.parent.mkdir(parents=True)
        link.symlink_to(old / 'tsrct-conv')
        data, checksum, _stem = self.archive()
        self.run_install(data, checksum)
        self.assertTrue(old.exists())
        self.assertIn('1.2.3-linux-x86_64', str(link.resolve()))

    def test_unrelated_link_rejected_including_prefix_trick(self):
        outside = self.prefix / 'share/Tesseract-Converter-evil/1.0.0-linux-x86_64'
        outside.mkdir(parents=True)
        (outside / 'tsrct-conv').write_text('unrelated')
        link = self.prefix / 'bin/tsrct-conv'
        link.parent.mkdir(parents=True)
        link.symlink_to(outside / 'tsrct-conv')
        data, checksum, _stem = self.archive()
        with self.assertRaisesRegex(ValueError, 'unrelated'):
            self.run_install(data, checksum)
        self.assertEqual(link.resolve(), (outside / 'tsrct-conv').resolve())

    def test_wrong_binary_stdout_rolls_back(self):
        data, checksum, _stem = self.archive(self.entries(binary_version='9.9.9'))
        with self.assertRaisesRegex(ValueError, 'unexpected version'):
            self.run_install(data, checksum)
        self.assertFalse((self.prefix / 'share/Tesseract-Converter/1.2.3-linux-x86_64').exists())

    def test_activation_failure_preserves_prior_link(self):
        old = self.prefix / 'share/Tesseract-Converter/1.0.0-linux-x86_64'
        old.mkdir(parents=True)
        (old / 'tsrct-conv').write_text('old')
        link = self.prefix / 'bin/tsrct-conv'
        link.parent.mkdir(parents=True)
        link.symlink_to(old / 'tsrct-conv')
        data, checksum, _stem = self.archive()
        with self.assertRaises(OSError):
            self.run_install(data, checksum, os=mock.Mock(**{'path.abspath': os.path.abspath,
                                                             'replace.side_effect': OSError('fail')}))
        self.assertEqual(link.resolve(), (old / 'tsrct-conv').resolve())
        self.assertFalse((self.prefix / 'share/Tesseract-Converter/1.2.3-linux-x86_64').exists())

    def test_interrupt_during_smoke_preserves_prior_install(self):
        old = self.prefix / 'share/Tesseract-Converter/1.0.0-linux-x86_64'
        old.mkdir(parents=True)
        (old / 'tsrct-conv').write_text('old')
        link = self.prefix / 'bin/tsrct-conv'
        link.parent.mkdir(parents=True)
        link.symlink_to(old / 'tsrct-conv')
        data, checksum, _stem = self.archive()
        runner = mock.Mock(side_effect=KeyboardInterrupt)
        with self.assertRaises(KeyboardInterrupt):
            self.run_install(data, checksum, subprocess=mock.Mock(run=runner))
        self.assertEqual(link.resolve(), (old / 'tsrct-conv').resolve())
        self.assertFalse((self.prefix / 'share/Tesseract-Converter/1.2.3-linux-x86_64').exists())

    def test_cancellation_during_activation_never_leaves_a_broken_link(self):
        data, checksum, _stem = self.archive()
        signal = scope['signal']
        previous_handler = signal.signal(signal.SIGTERM, scope['interrupted'])
        replace = os.replace

        def activate(source, destination):
            replace(source, destination)
            os.kill(os.getpid(), signal.SIGTERM)

        try:
            with mock.patch.object(os, 'replace', side_effect=activate):
                with self.assertRaises(KeyboardInterrupt):
                    self.run_install(data, checksum)
        finally:
            signal.signal(signal.SIGTERM, previous_handler)
        link = self.prefix / 'bin/tsrct-conv'
        self.assertTrue(link.is_file())
        self.assertEqual(scope['subprocess'].check_output([link, '--version'], text=True),
                         'tsrct-conv 1.2.3\n')
        self.assertFalse(list((self.prefix / 'bin').glob('.tsrct-conv-link-*')))

    def test_duplicate_release_assets_rejected(self):
        data, checksum, _stem = self.archive()
        fetch = self.release_fetch(data, checksum, duplicate_assets=True)
        with self.assertRaisesRegex(ValueError, 'duplicate release asset'):
            self.run_install(data, checksum, fetch=fetch)

    def test_redirect_requests_have_no_auth_headers(self):
        requests = []
        redirect = urllib.error.HTTPError(
            'https://api.github.com/x', 302, 'redirect',
            {'Location': 'https://objects.githubusercontent.com/asset'}, io.BytesIO())

        class Opener:
            def open(self, request, timeout):
                requests.append(request)
                if len(requests) == 1:
                    raise redirect
                return Response(b'asset')

        with mock.patch.object(scope['urllib'].request, 'build_opener', return_value=Opener()):
            self.assertEqual(scope['fetch']('https://api.github.com/x',
                                             'application/octet-stream'), b'asset')
        self.assertEqual(len(requests), 2)
        self.assertTrue(all(request.get_header('Authorization') is None for request in requests))
        redirect.close()

    def test_default_repository_is_production_distribution(self):
        self.assertIn("parser.add_argument('--repo', default='mirage-hq/Tesseract-Converter')", source)
        data, checksum, _stem = self.archive()
        urls = []
        fake_fetch = self.release_fetch(data, checksum)
        def fetch(url, *args):
            urls.append(url)
            return fake_fetch(url, *args)
        self.run_install(data, checksum, fetch=fetch)
        self.assertTrue(urls)
        self.assertTrue(all(url.startswith('https://api.github.com/repos/mirage-hq/Tesseract-Converter/')
                            for url in urls))

    def test_sigterm_handler_raises_for_cleanup(self):
        with self.assertRaises(KeyboardInterrupt):
            scope['interrupted'](scope['signal'].SIGTERM, None)

    def test_non_https_fetch_rejected(self):
        with self.assertRaisesRegex(ValueError, 'non-HTTPS'):
            scope['fetch']('http://api.github.com/x')


if __name__ == '__main__':
    unittest.main()
