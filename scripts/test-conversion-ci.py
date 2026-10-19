#!/usr/bin/env python3
"""Exercise standalone CI publication policy and public source boundaries."""

import contextlib
import json
import re
import runpy
import sys
import tempfile
import textwrap
import tomllib
import unittest
from pathlib import Path
from unittest.mock import patch

SOURCE_ROOT = Path(__file__).resolve().parents[1]


class ConversionCiPolicyTests(unittest.TestCase):
    def test_all_workspace_packages_are_accepted_but_publication_is_rejected(self):
        workflow = (SOURCE_ROOT / ".github/workflows/ci.yml").read_text()
        marker = '          python3 - "$RUNNER_TEMP/metadata.json" <<\'PY\'\n'
        policy = textwrap.dedent(workflow.split(marker, 1)[1].split("          PY\n", 1)[0])
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            names = ["tsrct-conv", "aftereffects_file", "premiere_file", "fx_conv",
                     "fx_keyframe_bake", "tesseract_file", "fx_schema", "media_transcode"]
            packages = [{"name": name, "publish": [],
                         "manifest_path": str(root / name / "Cargo.toml")} for name in names]
            metadata = root / "metadata.json"

            def execute(tracked=b'100644 source-hash 0\tCargo.toml\0'):
                metadata.write_text(json.dumps({"packages": packages}))
                with contextlib.chdir(root), patch.object(sys, "argv", ["-", str(metadata)]), \
                        patch('subprocess.check_output', return_value=tracked):
                    exec(compile(policy, "standalone-ci-policy", "exec"), {})

            execute()
            packages[4]["publish"] = None
            with self.assertRaisesRegex(AssertionError, "publication is not disabled"):
                execute()
            packages[4]["publish"] = []
            packages.append({"name": "unreviewed", "publish": [],
                             "manifest_path": str(root / "unreviewed/Cargo.toml")})
            with self.assertRaisesRegex(AssertionError, "publication is not disabled"):
                execute()
            packages.pop()
            for relative in ('README.md', 'tests/unreferenced.bin'):
                with self.subTest(symlink=relative), self.assertRaisesRegex(
                        AssertionError, 'symlink in public source'):
                    execute(f'120000 source-hash 0\t{relative}\0'.encode())


class WorkflowIntegrityTests(unittest.TestCase):
    ACTION_PINS = {
        "ilammy/msvc-dev-cmd": ("0b201ec74fa43914dc39ae48a89fd1d8cb592756", "v1"),
        "msys2/setup-msys2": ("ec48f7c5447b3140e2b088413ae3a55687bccb6e", "v2"),
        "actions/checkout": ("11d5960a326750d5838078e36cf38b85af677262", "v4"),
        "actions/setup-python": ("a26af69be951a213d495a4c3e4e4022e16d87065", "v5"),
        "actions/upload-artifact": ("ea165f8d65b6e75b540449e92b4886f43607fa02", "v4"),
        "actions/download-artifact": ("d3f86a106a0bac45b974a628896c90dbdf5c8093", "v4"),
        "dtolnay/rust-toolchain": ("fb523f40495206854c36b250850817ae0f83e3d1", "1.99.0"),
        "Mozilla-Actions/sccache-action": ("7d986dd989559c6ecdb630a3fd2557667be217ad", "v0.0.9"),
        "Swatinem/rust-cache": ("6323deb102c322ba6fcbdcafc7e3dddab59af2b6", "v2"),
        "actions/cache/restore": ("0057852bfaa89a56745cba8c7296529d2fc39830", "v4"),
        "actions/cache/save": ("0057852bfaa89a56745cba8c7296529d2fc39830", "v4"),
        "taiki-e/install-action": ("f7e5d7c961414b23f5b25b2da9294395d08513ad", "v2"),
    }

    def workflows(self):
        return (SOURCE_ROOT / ".github/workflows").glob("*.yml")

    def assert_reviewed_action_lines(self, text, source):
        uses_key = re.compile(r"^\s*(?:-\s*)?uses\s*:")
        uses_value = re.compile(r"^\s*(?:-\s*)?uses\s*:\s*(\S+?)(?:\s+#\s*(\S+))?\s*$")
        seen = set()
        for line in text.splitlines():
            if not uses_key.match(line):
                continue
            match = uses_value.match(line)
            self.assertIsNotNone(match, f"unreviewed uses syntax in {source}: {line.strip()}")
            reference, comment = match.groups()
            if reference.startswith("./"):
                continue
            action, separator, revision = reference.rpartition("@")
            self.assertTrue(separator and action and revision,
                            f"unpinned nonlocal action in {source}: {reference}")
            self.assertIn(action, self.ACTION_PINS, f"unreviewed action in {source}: {action}")
            self.assertEqual((revision, comment), self.ACTION_PINS[action],
                             f"unpinned or mislabeled action in {source}: {line.strip()}")
            seen.add(action)
        return seen

    def test_external_actions_are_pinned_to_reviewed_commits(self):
        seen = set()
        for workflow in self.workflows():
            seen.update(self.assert_reviewed_action_lines(workflow.read_text(), workflow))
        self.assertEqual(seen, set(self.ACTION_PINS))

    def test_unpinned_named_step_is_rejected(self):
        workflow = "steps:\n  - name: Checkout\n    uses: actions/checkout@v4\n"
        with self.assertRaisesRegex(AssertionError, "unpinned or mislabeled action"):
            self.assert_reviewed_action_lines(workflow, "named-step.yml")

    def test_notices_are_generated_for_distribution_not_vendored(self):
        ci = (SOURCE_ROOT / ".github/workflows/ci.yml").read_text()
        release = (SOURCE_ROOT / ".github/workflows/release.yml").read_text()
        self.assertNotIn("check-third-party-notices", ci + release)
        self.assertIn("make generate-third-party-notices", release)
        self.assertIn("--notices target/THIRD_PARTY_NOTICES.md", release)
        self.assertFalse((SOURCE_ROOT / "licenses").exists())


class WorkflowStructureTests(unittest.TestCase):
    def job(self, name):
        workflow = (SOURCE_ROOT / '.github/workflows/ci.yml').read_text()
        match = re.search(r'^  ' + re.escape(name) + r':\n(.*?)(?=^  [\w-]+:\n|\Z)',
                          workflow, re.MULTILINE | re.DOTALL)
        self.assertIsNotNone(match, f'missing CI job: {name}')
        return match.group(1)

    def test_lightweight_checks_do_not_build_native_dependencies(self):
        checks = self.job('test')
        self.assertIn('make fmt', checks)
        self.assertIn('make test-release-archive', checks)
        self.assertIn('audit-dependency-boundary.py', checks)
        self.assertNotIn('scripts/ffmpeg.py', checks)
        self.assertNotIn('cargo build', checks)
        self.assertNotRegex(checks, r'(?m)^\s*run: make (?:clippy|test)$')

    def test_all_platforms_build_and_smoke_independently_of_checks(self):
        build = self.job('platform-build')
        self.assertEqual(set(re.findall(r'- runner: (\S+)', build)),
                         {'ubuntu-22.04', 'macos-15', 'macos-15-intel', 'windows-2022'})
        self.assertNotRegex(build, r'(?m)^    needs:')
        self.assertIn('fail-fast: false', build)
        smoke = build.split('      - name: Build and smoke CLI\n', 1)[1].split(
            '\n      - ', 1)[0]
        self.assertNotIn('if: runner.os', smoke)
        self.assertIn('cargo build --locked -p tsrct-conv', smoke)
        self.assertIn('"$binary" --version', smoke)
        self.assertIn('"$binary" --help', smoke)

    def test_platform_caches_are_standalone_and_keep_native_validation(self):
        build = self.job('platform-build')
        for setting in ('RUSTC_WRAPPER: sccache', 'CARGO_INCREMENTAL: "0"',
                        'SCCACHE_GHA_ENABLED: "true"', 'SCCACHE_GHA_VERSION: converter-v1',
                        "SCCACHE_GHA_RW_MODE: ${{ github.ref == 'refs/heads/main' && 'READ_WRITE' || 'READ_ONLY' }}",
                        'workspaces: . -> target', 'shared-key: converter-ci',
                        "save-if: ${{ github.ref == 'refs/heads/main' }}",
                        'path: target/ci-ffmpeg', "hashFiles('scripts/ffmpeg.py')",
                        'steps.ffmpeg-identity.outputs.key'):
            self.assertIn(setting, build)
        self.assertIn('${{ steps.ffmpeg-identity.outputs.image }}', build)
        self.assertIn('image=${ImageVersion:?Hosted runner image version is required}', build)
        self.assertIn('version: v0.18.0', build)
        self.assertNotIn('opensource/conv', build)
        self.assertNotIn('restore-keys:', build)
        rust_cache = build.split('      - name: Restore Rust build cache\n', 1)[1].split('\n      - ', 1)[0]
        self.assertIn('shared-key: converter-ci-${{ steps.ffmpeg-identity.outputs.image }}-', rust_cache)
        self.assertNotIn('\n          key:', rust_cache)
        self.assertIn('steps.ffmpeg-identity.outputs.key', rust_cache)
        self.assertIn('hashFiles(', rust_cache)
        markers = ('name: Set up MSVC compiler', 'name: Set up optional compiler cache',
                   'name: Compute FFmpeg cache identity', 'name: Restore Rust build cache',
                   'name: Restore pinned FFmpeg', 'name: Build pinned inspection libraries (Linux)',
                   'name: Build pinned inspection libraries (Windows)',
                   'name: Build pinned inspection libraries (macOS)', 'name: Save validated FFmpeg',
                   'name: Build and smoke CLI')
        positions = [build.index(marker) for marker in markers]
        self.assertEqual(positions, sorted(positions))
        save = build.split('      - name: Save validated FFmpeg\n', 1)[1].split('\n      - ', 1)[0]
        self.assertIn("github.ref == 'refs/heads/main'", save)
        self.assertIn("steps.ffmpeg-cache.outputs.cache-hit != 'true'", save)
        self.assertIn('key: ${{ steps.ffmpeg-cache.outputs.cache-primary-key }}', save)
        self.assertNotIn('if: always()', save)
        for platform in ('Linux', 'Windows', 'macOS'):
            native = build.split(f'      - name: Build pinned inspection libraries ({platform})\n', 1)[1].split('\n      - ', 1)[0]
            self.assertNotIn('cache-hit', native)
        release = (SOURCE_ROOT / '.github/workflows/release.yml').read_text()
        def probe(source):
            return source.split('      - name: Verify compiler cache or fall back\n', 1)[1].split('\n      - ', 1)[0]
        # The release tests execute this same fallback body with fake compilers.
        self.assertEqual(probe(build), probe(release))

    def test_linux_build_precedes_clippy_and_tests_with_one_ffmpeg_setup(self):
        build = self.job('platform-build')
        setup = build.split('      - name: Build pinned inspection libraries (Linux)\n', 1)[1].split(
            '\n      - ', 1)[0]
        self.assertIn("if: runner.os == 'Linux'", setup)
        self.assertEqual(setup.count('python3 scripts/ffmpeg.py "$prefix"'), 1)
        prerequisites = build.split('      - name: Install controlled FFmpeg build prerequisites\n', 1)[1].split('\n      - ', 1)[0]
        self.assertIn('patchelf', prerequisites)
        self.assertLess(build.index('name: Install controlled FFmpeg build prerequisites'),
                        build.index('name: Compute FFmpeg cache identity'))
        self.assertIn('CARGO_BUILD_JOBS=2', setup)
        self.assertLess(build.index('name: Build and smoke CLI'), build.index('name: Clippy'))
        self.assertLess(build.index('name: Clippy'), build.index('name: Rust tests'))
        for name, command in (('Clippy', 'make clippy'), ('Rust tests', 'make test')):
            step = build.split(f'      - name: {name}\n', 1)[1].split('\n      - ', 1)[0]
            self.assertIn("if: runner.os == 'Linux'", step)
            self.assertIn(f'run: {command}', step)


class SourceInputBoundaryTests(unittest.TestCase):
    @contextlib.contextmanager
    def fixture(self, extra_manifest='', rust=''):
        with tempfile.TemporaryDirectory() as temporary:
            root = (Path(temporary) / 'public').resolve()
            (root / 'src').mkdir(parents=True)
            (root.parent / 'private.rs').write_text('private input')
            (root / 'Cargo.toml').write_text(
                '[package]\nname = "demo"\nversion = "0.1.0"\n' + extra_manifest)
            (root / 'src/lib.rs').write_text(rust)
            metadata = {'resolve': {}, 'packages': [
                {'name': 'demo', 'source': None, 'manifest_path': str(root / 'Cargo.toml')}]}
            audit = runpy.run_path(str(SOURCE_ROOT / 'scripts/audit-dependency-boundary.py'))['audit']
            yield root, lambda: audit(root, metadata)

    def test_excluded_optional_path_closure(self):
        with self.fixture('\n[dependencies]\noptional = { path = "optional", optional = true }\n') as (root, audit):
            optional = root / 'optional'
            nested = root / 'nested'
            optional.mkdir()
            nested.mkdir()
            (optional / 'Cargo.toml').write_text('[package]\nname = "optional"\nversion = "0.1.0"\n'
                                                 '[dependencies]\nnested = { path = "../nested" }\n')
            (nested / 'Cargo.toml').write_text('[package]\nname = "nested"\nversion = "0.1.0"\n'
                                               '[dependencies]\noptional = { path = "../optional" }\n')
            audit()  # A valid nested chain with a cycle terminates.
            (nested / 'Cargo.toml').write_text('[package]\nname = "nested"\nversion = "0.1.0"\n'
                                               '[dependencies]\nprivate = { path = "../../private" }\n')
            with self.assertRaisesRegex(ValueError, 'escaping or missing path dependency'):
                audit()
            (nested / 'Cargo.toml').write_text('[package]\nname = "nested"\nversion = "0.1.0"\n'
                                               '[dependencies]\nprivate = { path = "../link" }\n')
            (root / 'link').symlink_to(root.parent, target_is_directory=True)
            with self.assertRaisesRegex(ValueError, 'symlink'):
                audit()
            (nested / 'Cargo.toml').write_text('[package]\nname = "nested"\nversion = "0.1.0"\n'
                                               'build = "../../private.rs"\n')
            with self.assertRaisesRegex(ValueError, 'source input'):
                audit()

    def test_source_and_manifest_symlinks_are_rejected(self):
        with self.fixture() as (root, audit):
            (root / 'other.rs').write_text('safe')
            (root / 'src/lib.rs').unlink()
            (root / 'src/lib.rs').symlink_to('../other.rs')
            with self.assertRaisesRegex(ValueError, 'symlink'):
                audit()
        with self.fixture(rust='include_str!("../alias");') as (root, audit):
            (root / 'alias').symlink_to('Cargo.toml')
            with self.assertRaisesRegex(ValueError, 'symlink'):
                audit()
        with self.fixture() as (root, audit):
            (root / 'Cargo.toml').rename(root / 'real.toml')
            (root / 'Cargo.toml').symlink_to('real.toml')
            with self.assertRaisesRegex(ValueError, 'symlink'):
                audit()

    def test_inherited_metadata_paths(self):
        for key in ('readme', 'license-file'):
            with self.subTest(key=key), self.fixture(f'{key}.workspace = true\n') as (root, audit):
                manifest = root / 'Cargo.toml'
                manifest.write_text(manifest.read_text() + f'\n[workspace.package]\n{key} = "../private.rs"\n')
                with self.assertRaisesRegex(ValueError, 'source input'):
                    audit()
                manifest.write_text(manifest.read_text().replace('../private.rs', 'src/lib.rs'))
                audit()

    def test_excluded_optional_nested_workspace(self):
        with self.fixture('\n[dependencies]\noptional = { path = "nested/optional", optional = true }\n') as (root, audit):
            nested = root / 'nested'
            optional = nested / 'optional'
            optional.mkdir(parents=True)
            (nested / 'Cargo.toml').write_text('[workspace]\n[workspace.package]\nreadme = "../../private.rs"\n'
                                                 '[workspace.dependencies]\nlocal = "1"\n')
            manifest = optional / 'Cargo.toml'
            manifest.write_text('[package]\nname = "optional"\nversion = "0.1.0"\nreadme.workspace = true\n'
                                '[dependencies]\nlocal = { workspace = true }\n')
            with self.assertRaisesRegex(ValueError, 'source input'):
                audit()
            (nested / 'Cargo.toml').write_text((nested / 'Cargo.toml').read_text().replace(
                '../../private.rs', '../src/lib.rs'))
            audit()
            (nested / 'Cargo.toml').write_text('[workspace]\n')
            with self.assertRaisesRegex(ValueError, 'missing workspace dependency'):
                audit()

    def test_excluded_optional_external_workspace(self):
        with self.fixture('\n[dependencies]\noptional = { path = "optional", optional = true }\n') as (root, audit):
            optional = root / 'optional'
            optional.mkdir()
            manifest = optional / 'Cargo.toml'
            manifest.write_text('[package]\nname = "optional"\nversion = "0.1.0"\nworkspace = "../.."\n'
                                '[dependencies]\nprivate = { workspace = true }\n')
            with self.assertRaisesRegex(ValueError, 'escaping or missing workspace'):
                audit()
            manifest.write_text(manifest.read_text().replace('workspace = "../.."', 'workspace = ".."'))
            with self.assertRaisesRegex(ValueError, 'missing workspace dependency'):
                audit()
            public = root / 'Cargo.toml'
            public.write_text(public.read_text() + '\n[workspace.dependencies]\nprivate = "1"\n')
            audit()

    def test_checked_in_workspace_source_inputs_are_auditable(self):
        audit_inputs = runpy.run_path(
            str(SOURCE_ROOT / 'scripts/audit-dependency-boundary.py'))['audit_inputs']
        workspace = tomllib.loads((SOURCE_ROOT / 'Cargo.toml').read_text())['workspace']
        manifests = {SOURCE_ROOT / 'Cargo.toml'}
        manifests.update(SOURCE_ROOT / member / 'Cargo.toml' for member in workspace['members'])
        audit_inputs(SOURCE_ROOT, manifests)

    def test_public_literal_inputs_are_accepted(self):
        with self.fixture(rust='include_str!("../Cargo.toml");') as (_, audit):
            audit()

    def test_manifest_entry_points_cannot_escape(self):
        declarations = [f'{key} = "../private.rs"\n'
                        for key in ('build', 'readme', 'license-file')]
        declarations += ['\n[lib]\npath = "../private.rs"\n']
        declarations += [f'\n[[{kind}]]\nname = "demo"\npath = "../private.rs"\n'
                         for kind in ('bin', 'test', 'example', 'bench')]
        for declaration in declarations:
            with self.subTest(declaration=declaration), self.fixture(declaration) as (_, audit):
                with self.assertRaisesRegex(ValueError, 'source input'):
                    audit()

    def test_rust_inputs_cannot_escape_even_when_inactive(self):
        inputs = [f'#[cfg(any())] {macro}!("../../private.rs");'
                  for macro in ('include', 'include_str', 'include_bytes')]
        inputs += ['#[path = "../../private.rs"] mod private;']
        for source in inputs:
            with self.subTest(source=source), self.fixture(rust=source) as (_, audit):
                with self.assertRaisesRegex(ValueError, 'source input'):
                    audit()

    def test_computed_include_requires_review(self):
        with self.fixture(rust='include!(concat!("..", "/private.rs"));') as (_, audit):
            with self.assertRaisesRegex(ValueError, 'computed Rust include'):
                audit()

    def test_new_public_files_and_build_scripts_are_accepted(self):
        with self.fixture(rust='include_bytes!("../new.bin");') as (root, audit):
            with self.assertRaisesRegex(ValueError, 'missing source input'):
                audit()
            (root / 'new.bin').write_bytes(b'public fixture')
            (root / 'build.rs').write_text('fn main() {}')
            audit()

    def test_external_input_symlink_is_rejected(self):
        with self.fixture(rust='include_str!("../secret");') as (root, audit):
            (root / 'secret').symlink_to(root.parent / 'private.rs')
            with self.assertRaisesRegex(ValueError, 'symlink'):
                audit()


if __name__ == "__main__":
    unittest.main()
