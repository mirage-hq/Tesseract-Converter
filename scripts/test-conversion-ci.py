#!/usr/bin/env python3
"""Execute the standalone CI inventory policy against an isolated fake checkout."""

import contextlib
import json
import re
import runpy
import sys
import tempfile
import textwrap
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
            (root / "scripts").mkdir()
            (root / "scripts/conversion-export-files.json").write_text(json.dumps(["Cargo.toml"]))
            names = ["tsrct-conv", "aftereffects_file", "premiere_file", "fx_conv",
                     "fx_keyframe_bake", "tesseract_file", "fx_schema", "media_transcode"]
            packages = [{"name": name, "publish": [],
                         "manifest_path": str(root / name / "Cargo.toml")} for name in names]
            metadata = root / "metadata.json"

            def execute():
                metadata.write_text(json.dumps({"packages": packages}))
                with contextlib.chdir(root), patch.object(sys, "argv", ["-", str(metadata)]), \
                        patch("subprocess.check_output", return_value=b"Cargo.toml\0"):
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


class WorkflowIntegrityTests(unittest.TestCase):
    ACTION_PINS = {
        "ilammy/msvc-dev-cmd": ("0b201ec74fa43914dc39ae48a89fd1d8cb592756", "v1"),
        "msys2/setup-msys2": ("ec48f7c5447b3140e2b088413ae3a55687bccb6e", "v2"),
        "actions/checkout": ("11d5960a326750d5838078e36cf38b85af677262", "v4"),
        "actions/setup-python": ("a26af69be951a213d495a4c3e4e4022e16d87065", "v5"),
        "actions/upload-artifact": ("ea165f8d65b6e75b540449e92b4886f43607fa02", "v4"),
        "actions/download-artifact": ("d3f86a106a0bac45b974a628896c90dbdf5c8093", "v4"),
        "dtolnay/rust-toolchain": ("ffaa7fb73f2b6e3c49bc425913220fa3d71c3ee5", "1.92.0"),
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


class SourceInputBoundaryTests(unittest.TestCase):
    @contextlib.contextmanager
    def fixture(self, extra_manifest='', rust=''):
        with tempfile.TemporaryDirectory() as temporary:
            root = (Path(temporary) / 'public').resolve()
            (root / 'src').mkdir(parents=True)
            (root / 'scripts').mkdir()
            (root.parent / 'private.rs').write_text('private input')
            (root / 'Cargo.toml').write_text(
                '[package]\nname = "demo"\nversion = "0.1.0"\n' + extra_manifest)
            (root / 'src/lib.rs').write_text(rust)
            (root / 'scripts/conversion-export-files.json').write_text(
                json.dumps(['Cargo.toml', 'src/lib.rs']))
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
            inventory = root / 'scripts/conversion-export-files.json'
            inventory.write_text(json.dumps(json.loads(inventory.read_text()) +
                                            ['optional/Cargo.toml', 'nested/Cargo.toml']))
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
            inventory.write_text(json.dumps(['Cargo.toml', 'src/lib.rs', 'optional/Cargo.toml']))
            with self.assertRaisesRegex(ValueError, 'unlisted dependency manifest'):
                audit()

    def test_inventoried_and_source_symlinks_are_rejected(self):
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
            inventory = root / 'scripts/conversion-export-files.json'
            inventory.write_text(json.dumps(json.loads(inventory.read_text()) +
                                            ['nested/Cargo.toml', 'nested/optional/Cargo.toml']))
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
            inventory = root / 'scripts/conversion-export-files.json'
            inventory.write_text(json.dumps(json.loads(inventory.read_text()) + ['optional/Cargo.toml']))
            with self.assertRaisesRegex(ValueError, 'escaping or missing workspace'):
                audit()
            manifest.write_text(manifest.read_text().replace('workspace = "../.."', 'workspace = ".."'))
            with self.assertRaisesRegex(ValueError, 'missing workspace dependency'):
                audit()
            public = root / 'Cargo.toml'
            public.write_text(public.read_text() + '\n[workspace.dependencies]\nprivate = "1"\n')
            audit()

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

    def test_unlisted_build_script_and_symlink_are_rejected(self):
        with self.fixture() as (root, audit):
            (root / 'build.rs').write_text('fn main() {}')
            with self.assertRaisesRegex(ValueError, 'source input'):
                audit()
        with self.fixture(rust='include_str!("../secret");') as (root, audit):
            (root / 'secret').symlink_to(root.parent / 'private.rs')
            with self.assertRaisesRegex(ValueError, 'symlink'):
                audit()


if __name__ == "__main__":
    unittest.main()
