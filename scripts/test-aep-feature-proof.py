#!/usr/bin/env python3
"""Focused tests for the local AE feature-proof helper.

These tests are authored for later execution; the helper implementation task
must not run them, Adobe, uploads, conversion, or repository test commands.
"""

import argparse
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import aep_feature_proof as proof
import aep_feature_proof_publish as publish


SOURCE = "crates/aftereffects_file/tests/fixtures/compositing/example.aep"
CASE_ID = "aep-compositing-example-c7"


def sample_case(**updates):
    value = {
        "case_id": CASE_ID,
        "direction": "import",
        "feature": "Native opacity keyframe preserves layer-clock timing",
        "requirement": "required",
        "supported_by": [],
        "source_path": SOURCE,
        "composition_id": 7,
        "critical_frames": ["0", "1/2", "47/24"],
        "tests": [
            {
                "path": "crates/aftereffects_file/src/structure_document/tests/example.rs",
                "symbol": "imports_native_opacity",
                "assertions": [
                    {
                        "description": "Opacity key at one half second remains editable at 50 percent",
                        "source_anchor": "assert_eq!(opacity, 0.5);",
                    }
                ],
            }
        ],
        "execution": {"status": "UNRUN"},
        "visual": {"status": "unmeasured"},
        "limitations": ["Actual-vs-expected visual comparison is not implemented."],
    }
    value.update(updates)
    return value


def expression_samples(workspace, source_sha256):
    path = workspace / "fixtures/expression_samples.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({
        "version": 2,
        "source_sha256": source_sha256,
        "sample_interval_ms": 1,
        "capture_scope": {"mode": "selected_composition", "root_composition_id": 7},
        "properties": [],
        "errors": [],
    }))
    return {
        "path": "fixtures/expression_samples.json",
        "bytes": path.stat().st_size,
        "sha256": proof.sha256_file(path),
    }


def sample_manifest(reference_path="references/example.mp4"):
    return {
        "schema_version": 1,
        "scope": "Independent native AE references; publication is not conversion proof.",
        "reference_settings": {"output_fps": 30},
        "sources": [
            {
                "source_path": SOURCE,
                "source_sha256": "a" * 64,
                "source_bytes": 10,
                "media_dependencies": [],
                "compositions": [
                    {
                        "composition_id": 7,
                        "composition_name": "EXAMPLE",
                        "status": "verified",
                        "expected_frame_count": 60,
                        "reference": {
                            "path": reference_path,
                            "fps": 30,
                            "frame_count": 60,
                        },
                        "verification": {
                            "native_render": True,
                            "full_decode": True,
                            "fx_render_comparison": "not_run",
                        },
                    }
                ],
            }
        ],
    }


class SchemaTests(unittest.TestCase):
    def test_checked_in_registry_has_stable_ids_and_valid_support_links(self):
        workspace = Path(__file__).resolve().parent.parent
        registry = proof.load_json(
            workspace / "crates/aftereffects_file/tests/fixtures/aep_feature_cases.json"
        )
        proof.validate_registry(registry)

    def test_minimal_case_schema_is_explicitly_import_unrun_unmeasured(self):
        proof.validate_case_schema(sample_case())

    def test_fx_render_comparison_requires_explicit_not_run(self):
        composition = sample_manifest()["sources"][0]["compositions"][0]
        self.assertEqual(proof._reference_problems(composition), [])
        verification = composition["verification"]
        verification["fx_render_comparison"] = "passed"
        self.assertIn("manifest unexpectedly claims an FX render comparison",
                      proof._reference_problems(composition))
        del verification["fx_render_comparison"]
        self.assertIn("manifest unexpectedly claims an FX render comparison",
                      proof._reference_problems(composition))

    def test_export_cannot_reuse_import_proof(self):
        with self.assertRaisesRegex(proof.ProofError, "never establishes export proof"):
            proof.validate_case_schema(sample_case(direction="export"))

    def test_bogus_execution_or_visual_pass_is_rejected(self):
        with self.assertRaises(proof.ProofError):
            proof.validate_case_schema(sample_case(execution={"status": "passed"}))
        with self.assertRaises(proof.ProofError):
            proof.validate_case_schema(sample_case(visual={"status": "passed"}))

    def test_case_id_is_derived_from_source_path_and_composition(self):
        bad = sample_case(case_id="aep-unrelated-c7")
        with self.assertRaisesRegex(proof.ProofError, "case_id must be"):
            proof.validate_case_schema(bad)

    def test_supporting_case_requires_real_required_consumer(self):
        supporting = sample_case(
            case_id="aep-compositing-support-c8",
            source_path="crates/aftereffects_file/tests/fixtures/compositing/support.aep",
            composition_id=8,
            requirement="supporting",
            supported_by=[CASE_ID],
        )
        proof.validate_registry(
            {"schema_version": 1, "scope": "Import cases with explicit support roles.", "cases": [sample_case(), supporting]}
        )
        supporting["supported_by"] = ["aep-compositing-missing-c9"]
        with self.assertRaisesRegex(proof.ProofError, "dangling"):
            proof.validate_registry(
                {"schema_version": 1, "scope": "Import cases with explicit support roles.", "cases": [sample_case(), supporting]}
            )

    def test_required_case_cannot_delegate_its_coverage(self):
        with self.assertRaisesRegex(proof.ProofError, "evade coverage"):
            proof.validate_case_schema(sample_case(supported_by=["aep-compositing-other-c8"]))

    def test_signed_url_or_secret_fields_are_rejected(self):
        case = sample_case(limitations=["https://storage.googleapis.com/x?X-Goog-Signature=secret"])
        with self.assertRaisesRegex(proof.ProofError, "signed-URL"):
            proof.validate_case_schema(case)

    def test_expression_samples_descriptor_is_strict_and_optional(self):
        descriptor = {
            "path": "fixtures/expression_samples.json",
            "bytes": 42,
            "sha256": "c" * 64,
        }
        proof.validate_case_schema(sample_case(expression_samples=descriptor))
        with self.assertRaises(proof.ProofError):
            proof.validate_case_schema(sample_case(expression_samples=None))
        invalid = (
            {**descriptor, "extra": True},
            {**descriptor, "path": "../expression_samples.json"},
            {**descriptor, "path": "/tmp/expression_samples.json"},
            {**descriptor, "path": "fixtures\\expression_samples.json"},
            {**descriptor, "path": "fixtures/expression_samples.txt"},
            {**descriptor, "bytes": 0},
            {**descriptor, "bytes": proof.MAX_EXPRESSION_SAMPLES_BYTES + 1},
            {**descriptor, "sha256": "C" * 64},
        )
        for value in invalid:
            with self.subTest(value=value), self.assertRaises(proof.ProofError):
                proof.validate_case_schema(sample_case(expression_samples=value))

    def test_duplicate_reference_paths_are_rejected(self):
        manifest = sample_manifest()
        duplicate = json.loads(json.dumps(manifest["sources"][0]))
        duplicate["source_path"] = "crates/aftereffects_file/tests/fixtures/compositing/two.aep"
        duplicate["source_sha256"] = "c" * 64
        duplicate["compositions"][0]["composition_id"] = 8
        manifest["sources"].append(duplicate)
        with self.assertRaisesRegex(proof.ProofError, "reused"):
            proof.validate_manifest(manifest)


class LinkAndReportTests(unittest.TestCase):
    def make_workspace(self, root):
        workspace = Path(root).resolve()
        (workspace / "Cargo.toml").write_text("[workspace]\n")
        (workspace / "scripts").mkdir()
        (workspace / "crates/aftereffects_file/src/structure_document/tests").mkdir(parents=True)
        source = workspace / SOURCE
        source.parent.mkdir(parents=True, exist_ok=True)
        source.write_bytes(b"0123456789")
        reference = workspace / "references/example.mp4"
        reference.parent.mkdir(parents=True, exist_ok=True)
        reference.write_bytes(b"independent Adobe reference")
        test = workspace / "crates/aftereffects_file/src/structure_document/tests/example.rs"
        test.write_text(
            "#[test]\nfn imports_native_opacity() {\n    let opacity = 0.5;\n    assert_eq!(opacity, 0.5);\n}\n"
        )
        return workspace

    def test_optional_missing_local_reference_is_diagnostic_not_required_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            target = ({}, {"reference": {"path": "tests/references/aep/missing.mp4"}})
            optional = sample_case(requirement="optional")
            required = sample_case(requirement="required", case_id="another-case")
            errors = proof._verify_references(
                [optional, required], {(SOURCE, 7): target}, Path(directory)
            )
        self.assertEqual(len(errors), 2)
        self.assertFalse(errors[0][1])
        self.assertTrue(errors[1][1])

    def test_import_check_does_not_require_export_oracles_in_import_registry(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = self.make_workspace(directory)
            manifest = sample_manifest()
            source = manifest["sources"][0]
            source["source_sha256"] = proof.sha256_file(workspace / SOURCE)
            export_source = copy.deepcopy(source)
            export_source["source_path"] = "crates/aftereffects_file/tests/fixtures/panel/native/fx-export-example.aep"
            export_source["compositions"][0]["reference"]["path"] = "references/export.mp4"
            manifest["sources"].append(export_source)
            registry = {"schema_version": 1, "scope": "Import cases only", "cases": [sample_case()]}
            proof.write_json(workspace / "registry.json", registry)
            proof.write_json(workspace / "manifest.json", manifest)
            args = argparse.Namespace(workspace=str(workspace), registry="registry.json",
                                      manifest="manifest.json", require="structural",
                                      verify_references=False)
            with mock.patch("builtins.print") as output:
                self.assertEqual(proof.command_check(args), 0)
            self.assertIn("required link-checked cases: 1/1", str(output.call_args_list))
            self.assertNotIn("coverage: missing registry case", str(output.call_args_list))

    def test_link_check_is_not_test_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = self.make_workspace(directory)
            manifest = sample_manifest()
            manifest["sources"][0]["source_sha256"] = proof.sha256_file(workspace / SOURCE)
            targets = proof.validate_manifest(manifest)
            state = proof.evaluate_case(sample_case(), targets, workspace, verify_files=True)
            self.assertEqual(state.level, "links_checked")

    def test_expression_samples_file_is_hash_verified_and_source_bound(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = self.make_workspace(directory)
            manifest = sample_manifest()
            source_sha = proof.sha256_file(workspace / SOURCE)
            manifest["sources"][0]["source_sha256"] = source_sha
            targets = proof.validate_manifest(manifest)
            case = sample_case(expression_samples=expression_samples(workspace, source_sha))
            self.assertEqual(
                proof.evaluate_case(case, targets, workspace, verify_files=True).level,
                "links_checked",
            )

            sidecar = workspace / case["expression_samples"]["path"]
            sidecar.write_text(json.dumps({"source_sha256": "f" * 64, "properties": []}))
            case["expression_samples"]["bytes"] = sidecar.stat().st_size
            case["expression_samples"]["sha256"] = proof.sha256_file(sidecar)
            state = proof.evaluate_case(case, targets, workspace, verify_files=True)
            self.assertEqual(state.level, "pending")
            self.assertTrue(any("source_sha256" in problem for problem in state.problems))

    def test_expression_samples_symlink_escape_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            workspace = self.make_workspace(directory)
            manifest = sample_manifest()
            source_sha = proof.sha256_file(workspace / SOURCE)
            manifest["sources"][0]["source_sha256"] = source_sha
            targets = proof.validate_manifest(manifest)
            external = Path(outside) / "expression_samples.json"
            external.write_text(json.dumps({"source_sha256": source_sha, "properties": []}))
            link = workspace / "fixtures/expression_samples.json"
            link.parent.mkdir(parents=True, exist_ok=True)
            link.symlink_to(external)
            case = sample_case(
                expression_samples={
                    "path": "fixtures/expression_samples.json",
                    "bytes": external.stat().st_size,
                    "sha256": proof.sha256_file(external),
                }
            )
            state = proof.evaluate_case(case, targets, workspace, verify_files=True)
            self.assertEqual(state.level, "pending")
            self.assertTrue(any("symlink" in problem or "escapes workspace" in problem for problem in state.problems))

    def test_missing_assertion_anchor_stays_pending(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = self.make_workspace(directory)
            manifest = sample_manifest()
            manifest["sources"][0]["source_sha256"] = proof.sha256_file(workspace / SOURCE)
            targets = proof.validate_manifest(manifest)
            case = sample_case()
            case["tests"][0]["assertions"][0]["source_anchor"] = "assert_eq!(missing, true);"
            state = proof.evaluate_case(case, targets, workspace, verify_files=True)
            self.assertEqual(state.level, "pending")
            self.assertTrue(any("anchor" in problem for problem in state.problems))

    def test_report_labels_comparison_unmeasured(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = self.make_workspace(directory)
            registry = {"schema_version": 1, "scope": "Import proof registry for test.", "cases": [sample_case()]}
            manifest = sample_manifest()
            targets = proof.validate_manifest(manifest)
            report = proof.build_report(registry, manifest, targets, workspace, "1" * 64, "2" * 64)
            self.assertIn("Visual comparison is not implemented", report)
            self.assertIn("unmeasured", report)
            self.assertIn("UNRUN", report)

    def test_stale_check_rejects_modified_report_even_with_current_input_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = self.make_workspace(directory)
            manifest = sample_manifest()
            manifest["sources"][0]["source_sha256"] = proof.sha256_file(workspace / SOURCE)
            registry = {
                "schema_version": 1,
                "scope": "Import proof registry for stale-report test.",
                "cases": [sample_case()],
            }
            registry_path = workspace / "registry.json"
            manifest_path = workspace / "manifest.json"
            proof.write_json(registry_path, registry)
            proof.write_json(manifest_path, manifest)
            targets = proof.validate_manifest(manifest)
            report = proof.build_report(
                registry,
                manifest,
                targets,
                workspace,
                proof.sha256_file(registry_path),
                proof.sha256_file(manifest_path),
            )
            report_path = workspace / "report.md"
            report_path.write_text(report + "\nmodified without changing the embedded hashes\n")
            args = argparse.Namespace(
                workspace=str(workspace),
                registry="registry.json",
                manifest="manifest.json",
                check_stale="report.md",
            )
            self.assertEqual(proof.command_report(args), 1)


class AuthoringSafetyTests(unittest.TestCase):
    def test_generic_or_session_controlling_jsx_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory).resolve()
            (workspace / "Cargo.toml").write_text("[workspace]\n")
            (workspace / "scripts").mkdir()
            (workspace / "crates/aftereffects_file").mkdir(parents=True)
            feature = workspace / "feature.jsx"
            feature.write_text(
                "function buildAep(project) { app.quit(); }\n"
                "function readbackAep(project) { return {}; }\n"
            )
            paths = proof.Paths(workspace, workspace / "registry.json", workspace / "manifest.json")
            args = argparse.Namespace(
                feature_jsx="feature.jsx",
                wrapper_jsx="wrapper.jsx",
                readback="readback.json",
                receipt="receipt.json",
            )
            with self.assertRaisesRegex(proof.ProofError, "may not quit"):
                publish.author_source(paths, sample_case(source_path="source.aep"), args)

    def test_template_is_emitted_but_never_runs_adobe(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory).resolve()
            (workspace / "Cargo.toml").write_text("[workspace]\n")
            (workspace / "scripts").mkdir()
            (workspace / "crates/aftereffects_file").mkdir(parents=True)
            (workspace / "feature.jsx").write_text(
                "function buildAep(project) { project.items.addComp('feature', 16, 16, 1, 1, 24); }\n"
                "function readbackAep(project) { return { item_count: project.numItems }; }\n"
            )
            paths = proof.Paths(workspace, workspace / "registry.json", workspace / "manifest.json")
            args = argparse.Namespace(
                feature_jsx="feature.jsx",
                wrapper_jsx="wrapper.jsx",
                readback="readback.json",
                receipt="receipt.json",
            )
            with mock.patch("subprocess.Popen") as popen:
                publish.author_source(paths, sample_case(source_path="source.aep"), args)
            popen.assert_not_called()
            wrapper = (workspace / "wrapper.jsx").read_text()
            self.assertIn("existing project/file/dirty/nonempty state", wrapper)
            self.assertNotIn("app.quit", wrapper)
            self.assertIn("ownedProject.close", wrapper)


class JournalSafetyTests(unittest.TestCase):
    def test_local_reference_recording_copies_without_asset_or_reference_hash_pin(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory).resolve()
            source_file = workspace / SOURCE
            source_file.parent.mkdir(parents=True)
            source_file.write_bytes(b"native-aep")
            source_sha = proof.sha256_file(source_file)
            manifest = sample_manifest()
            source = manifest["sources"][0]
            source["source_sha256"] = source_sha
            source["source_bytes"] = source_file.stat().st_size
            composition = source["compositions"][0]
            composition.update(status="pending", reference=None, verification=None)
            paths = proof.Paths(workspace, workspace / "registry.json", workspace / "manifest.json")
            proof.write_json(paths.manifest, manifest)
            case = sample_case()
            video = publish._scratch(paths, case) / "expected.mp4"
            video.write_bytes(b"native-video")
            publish._write_journal(paths, case, {
                "case_id": case["case_id"], "source_sha256": source_sha,
                "composition_id": 7, "inspection": {"note": "critical frames inspected"},
                "reference": {"sha256": proof.sha256_file(video), "bytes": video.stat().st_size,
                              "fps": 30, "frame_count": 60}, "output_name": video.name,
            })
            (workspace / "external").mkdir()
            (workspace / "tests").mkdir()
            alias = workspace / "tests/references"
            alias.symlink_to(workspace / "external", target_is_directory=True)
            with self.assertRaisesRegex(proof.ProofError, "unsafe symlink"):
                publish.publish_reference(paths, case, source, composition)
            alias.unlink()
            publish.publish_reference(paths, case, source, composition)
            target = proof.load_json(paths.manifest)["sources"][0]["compositions"][0]
            self.assertEqual(target["reference"], {
                "fps": 30, "frame_count": 60,
                "path": f"tests/references/aep/{case['case_id']}.mp4",
            })
            self.assertEqual((workspace / target["reference"]["path"]).read_bytes(), video.read_bytes())
            with self.assertRaisesRegex(proof.ProofError, "already|unpublished"):
                publish.publish_reference(paths, case, source, target)

    def test_journal_rejects_signed_url_material(self):
        paths = mock.Mock()
        case = sample_case()
        with mock.patch.object(publish, "_journal_path", return_value=Path("unused")):
            with self.assertRaisesRegex(proof.ProofError, "signed-URL"):
                publish._write_journal(paths, case, {"upload_url": "https://example.invalid"})

    def test_owned_timeout_terminates_process_group(self):
        process = mock.Mock(pid=42, returncode=None)
        process.communicate.side_effect = [subprocess.TimeoutExpired("tool", 1), (b"", b"")]
        with mock.patch("subprocess.Popen", return_value=process), mock.patch("os.killpg") as killpg:
            with self.assertRaises(subprocess.TimeoutExpired):
                publish._run_owned(["tool"], 1)
        killpg.assert_called_once_with(42, publish.signal.SIGTERM)


if __name__ == "__main__":
    unittest.main()
