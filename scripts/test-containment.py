#!/usr/bin/env python3
"""Offline regression checks for conversion tooling ownership and cwd handling."""
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

CONV = Path(__file__).resolve().parents[1]
ROOT = CONV.parents[1]
# These root-owned integration targets require binaries and tools outside this workspace.
PRIVATE_INTEGRATION_TARGETS = {
    'build-conversion-score', 'aep-score-build', 'aep-score-run',
    'aep-workflow-build', 'adobe-test', 'aep-audio-e2e-build',
    'check-prproj-score', 'check-prproj-render-ci', 'prproj-download',
    'regenerate-prproj-expected-video', 'conv-require-cases',
}
# These internal entry points belong outside the public source tree entirely.
PRIVATE_SCRIPTS = {
    'scripts/aep-workflow.py',
    'scripts/aep_workflow.py',
    'scripts/aep_workflow_adobe.py',
    'scripts/aep_workflow_batch.py',
    'scripts/test_aep_workflow.py',
    'scripts/test_aep_workflow_adobe.py',
    'scripts/test_aep_workflow_docs.py',
    'scripts/aep-evaluate-expressions.py',
    'scripts/test_aep_evaluate_expressions.py',
}


class ContainmentTests(unittest.TestCase):
    def test_root_does_not_duplicate_public_conversion_tools(self):
        if not (ROOT / 'apps/public_cli/Cargo.toml').is_file():
            self.skipTest('enclosing checkout not present')
        targets = set(re.findall(r'^([\w-]+):', (ROOT / 'Makefile').read_text(), re.MULTILINE))
        self.assertTrue(PRIVATE_INTEGRATION_TARGETS.issubset(targets))
        self.assertFalse([name for name in targets - PRIVATE_INTEGRATION_TARGETS if re.search(
            r'aep|adobe|prproj|conversion|aftereffects|tesseract-file', name)])
        inventory = json.loads((CONV / 'scripts/conversion-export-files.json').read_text())
        public_scripts = {Path(path).name for path in inventory if path.startswith('scripts/')}
        # Private native-format adapters may live at the root; public tools must not be duplicated.
        self.assertFalse([path.name for path in (ROOT / 'scripts').iterdir()
                          if path.name in public_scripts])

    def test_fixture_corpus_lives_directly_under_tests(self):
        for name in ('manifest.json', 'README.md'):
            with self.subTest(name=name):
                self.assertTrue((CONV / 'tests' / name).exists())
        self.assertFalse((CONV / 'tests/conversion').exists())
        self.assertFalse((CONV / 'tests/evidence').exists())

    def test_export_excludes_run_reports_and_includes_legal_evidence(self):
        inventory = set(json.loads((CONV / 'scripts/conversion-export-files.json').read_text()))
        self.assertFalse(any(path.startswith(('tests/evidence/', 'docs/after-effects-history/'))
                             for path in inventory))
        for relative in inventory:
            self.assertTrue((CONV / relative).is_file(), relative)
        self.assertFalse(any(path.startswith('licenses/') for path in inventory))
        self.assertIn('about.toml', inventory)
        self.assertIn('about.hbs', inventory)

    def test_fixture_readme_relative_links_resolve(self):
        readme = CONV / 'tests/README.md'
        for target in re.findall(r'\]\(([^)]+)\)', readme.read_text()):
            if '://' in target or target.startswith('#'):
                continue
            with self.subTest(target=target):
                self.assertTrue((readme.parent / target.split('#', 1)[0]).exists())

    def test_helpers_are_explicitly_inventoried(self):
        inventory = json.loads((CONV / 'scripts/conversion-export-files.json').read_text())
        published = set(inventory)
        self.assertEqual(len(inventory), len(published))
        self.assertFalse(PRIVATE_SCRIPTS.intersection(published),
                         'private Adobe workflow helpers must not enter the public export')
        for relative in PRIVATE_SCRIPTS:
            self.assertFalse((CONV / relative).exists(), relative)
        for private_doc in ('docs/after-effects-workflow.md', 'docs/after-effects-development.md'):
            self.assertNotIn(private_doc, published)
            self.assertFalse((CONV / private_doc).exists(), private_doc)
        for path in (CONV / 'scripts').iterdir():
            if path.is_file() and path.suffix in {'.py', '.mjs', '.jsx', '.json', '.sh'}:
                relative = path.relative_to(CONV).as_posix()
                self.assertTrue(relative in published, relative)

    def test_random_expression_sources_and_attribution_are_exported(self):
        published = set(json.loads((CONV / 'scripts/conversion-export-files.json').read_text()))
        for name in ('random.js', 'random_tests.rs', 'RANDOM-APIS.md'):
            relative = f'crates/aftereffects_file/src/expression_eval/{name}'
            with self.subTest(path=relative):
                self.assertTrue(relative in published, f'missing export source: {relative}')
                self.assertTrue((CONV / relative).is_file())

    def test_tracked_public_tree_matches_export_inventory(self):
        result = subprocess.run(['git', 'ls-files', '-z', '--', '.'], cwd=CONV,
                                capture_output=True, text=True, timeout=15)
        if result.returncode != 0 or not result.stdout:
            self.skipTest('source tree is not tracked in a Git checkout')
        # Allow an unstaged deletion during local development; CI sees committed files.
        tracked = {path for path in result.stdout.rstrip('\0').split('\0')
                   if (CONV / path).is_file()}
        published = set(json.loads((CONV / 'scripts/conversion-export-files.json').read_text()))
        self.assertEqual(tracked - published, set(), 'unreviewed files remain in the public tree')
        self.assertEqual(published - tracked, set(), 'export inventory includes untracked files')

    def test_public_docs_do_not_link_to_private_local_files(self):
        inventory = set(json.loads((CONV / 'scripts/conversion-export-files.json').read_text()))
        for relative in inventory:
            if not relative.endswith('.md') or relative.startswith(('licenses/', 'patches/')):
                continue
            source = CONV / relative
            for target in re.findall(r'\]\(([^)]+)\)', source.read_text(errors='replace')):
                path = target.split('#', 1)[0]
                if not path or '://' in path or path.startswith(('mailto:', 'data:')):
                    continue
                destination = (source.parent / path).resolve()
                message = f'{relative} links to missing or unexported {path}'
                self.assertTrue(destination.is_relative_to(CONV), message)
                target = destination.relative_to(CONV).as_posix()
                self.assertTrue(target in inventory or any(
                    entry.startswith(target.rstrip('/') + '/') for entry in inventory), message)

    def test_cli_imports_and_core_make_work_without_parent_checkout(self):
        with tempfile.TemporaryDirectory() as directory:
            standalone = Path(directory) / 'conv'
            shutil.copytree(CONV / 'scripts', standalone / 'scripts',
                            ignore=shutil.ignore_patterns('__pycache__'))
            shutil.copyfile(CONV / 'Makefile', standalone / 'Makefile')
            for script in ('aep_test.py', 'check-prproj-scores.py', 'adobe-test.py',
                           'conversion-fixtures.py', 'aep-test.py', 'aep-feature-proof.py'):
                result = subprocess.run(['python3', f'scripts/{script}', '--help'],
                                        cwd=standalone, capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
            core = subprocess.run(['make', '-n', 'build', 'test'], cwd=standalone,
                                  capture_output=True, text=True, timeout=15)
            self.assertEqual(core.returncode, 0, core.stderr)
            self.assertNotIn(str(ROOT), core.stdout)
            help_output = subprocess.run(['make', 'help'], cwd=standalone,
                                         capture_output=True, text=True, timeout=15)
            self.assertEqual(help_output.returncode, 0, help_output.stderr)
            help_targets = {line.split()[0] for line in help_output.stdout.splitlines() if line.strip()}
            for target in ('aep-workflow', 'aep-workflow-test',
                           'aep-scale-probe', 'aep-scale-probe-build'):
                self.assertNotIn(target, help_targets)
                result = subprocess.run(['make', '-n', target], cwd=standalone,
                                        capture_output=True, text=True, timeout=15)
                self.assertNotEqual(result.returncode, 0, target)
            for target in PRIVATE_INTEGRATION_TARGETS:
                result = subprocess.run(['make', '-n', target], cwd=standalone,
                                        capture_output=True, text=True, timeout=15)
                self.assertNotEqual(result.returncode, 0, target)
                self.assertNotIn(target, help_targets)

    def test_conversion_aggregates_retain_support_ledger_guard(self):
        for target in ('test-conversion', 'test-aftereffects-file'):
            with self.subTest(target=target):
                result = subprocess.run(['make', '-n', target], cwd=CONV,
                                        capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn('python3 scripts/test-aep-support-ledger.py', result.stdout)

    def test_ci_uses_contained_entrypoints(self):
        workflow_path = ROOT / '.github/workflows/conversion-render.yml'
        if not workflow_path.is_file():
            self.skipTest('enclosing CI not present')
        workflow = workflow_path.read_text()
        self.assertIn('make check-prproj-render-ci', workflow)
        self.assertIn('/opensource/conv/scripts/conversion-ci-classify.py', workflow)
        self.assertIn('opensource/conv/scripts/pair-prproj-evidence.sh', workflow)


if __name__ == '__main__':
    unittest.main()
