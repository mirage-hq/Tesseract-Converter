"""Offline release routing and immutable-identity tests."""
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

import release_plan

SHA = "a" * 40
BEFORE = "b" * 40


class ReleasePlanTests(unittest.TestCase):
    def test_push_increase_and_manual(self):
        with patch.object(release_plan, "git"), patch.object(release_plan, "version_at", return_value="0.1.0"):
            self.assertEqual(release_plan.decide("push", "refs/heads/main", SHA, BEFORE, "0.2.0", "new"), "release")
            self.assertEqual(release_plan.decide("workflow_dispatch", "refs/heads/main", SHA, None, "0.2.0", "new"), "release")
            self.assertEqual(release_plan.decide("workflow_dispatch", "refs/heads/main", SHA, None, "0.2.0", "existing"), "skip")

    def test_unchanged_decreased_and_bad_baseline(self):
        with patch.object(release_plan, "git"), patch.object(release_plan, "version_at", return_value="0.2.0"):
            for version in ("0.2.0", "0.1.9"):
                self.assertEqual(release_plan.decide("push", "refs/heads/main", SHA, BEFORE, version, "new"), "skip")
            with self.assertRaisesRegex(ValueError, "baseline"):
                release_plan.decide("push", "refs/heads/main", SHA, "0" * 40, "0.3.0", "new")
            with self.assertRaisesRegex(ValueError, "main"):
                release_plan.decide("workflow_dispatch", "refs/heads/dev", SHA, None, "0.3.0", "new")

    def test_force_push_fails_closed(self):
        from subprocess import CalledProcessError
        with patch.object(release_plan, "git", side_effect=CalledProcessError(1, "git")):
            with self.assertRaises(CalledProcessError):
                release_plan.decide("push", "refs/heads/main", SHA, BEFORE, "0.3.0", "new")

    def test_release_identity(self):
        names = [f"Tesseract-Converter-0.2.0-{platform}.zip{suffix}"
                 for platform in release_plan.PLATFORMS for suffix in ("", ".sha256")]
        ref = {"object": {"type": "commit", "sha": SHA}}
        release = {"tag_name": "v0.2.0", "target_commitish": SHA, "draft": False,
                   "prerelease": False, "assets": [{"name": name, "size": 1} for name in names]}
        with patch.object(release_plan, "api", side_effect=lambda path: ref if path.startswith("git/") else release):
            self.assertEqual(release_plan.release_identity("v0.2.0", SHA, "0.2.0"), "existing")
        for broken in (None, {**release, "assets": release["assets"][:-1]},
                       {**release, "target_commitish": BEFORE}, {**release, "prerelease": True}):
            with patch.object(release_plan, "api", side_effect=[ref, broken]):
                with self.assertRaisesRegex(ValueError, "identity"):
                    release_plan.release_identity("v0.2.0", SHA, "0.2.0")
        with patch.object(release_plan, "api", side_effect=[None, None]):
            self.assertEqual(release_plan.release_identity("v0.2.0", SHA, "0.2.0"), "new")

    def test_repository_api_url_has_no_trailing_slash(self):
        with patch.dict(os.environ, {"GITHUB_REPOSITORY": "mirage-hq/conv-test", "GH_TOKEN": "token"}):
            with patch.object(release_plan, "urlopen", return_value=io.BytesIO(b'{"default_branch":"main"}')) as request:
                self.assertEqual(release_plan.api("")["default_branch"], "main")
                self.assertEqual(request.call_args.args[0].full_url,
                                 "https://api.github.com/repos/mirage-hq/conv-test")

    def test_api_distinguishes_absence_from_permission_failure(self):
        def error(code):
            return HTTPError("https://api.github.test", code, "failure", {}, None)

        with patch.dict(os.environ, {"GITHUB_REPOSITORY": "owner/repo", "GH_TOKEN": "token"}):
            with patch.object(release_plan, "urlopen", side_effect=error(404)):
                self.assertIsNone(release_plan.api("releases/tags/v1.0.0"))
            with patch.object(release_plan, "urlopen", side_effect=error(403)):
                with self.assertRaises(HTTPError):
                    release_plan.api("releases/tags/v1.0.0")


class ReleasePlanIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.repo = Path(self.temporary.name)
        subprocess.run(["git", "init", "-b", "main"], cwd=self.repo, check=True, capture_output=True)
        subprocess.run(["git", "config", "user.email", "release-test@example.com"], cwd=self.repo, check=True)
        subprocess.run(["git", "config", "user.name", "Release Test"], cwd=self.repo, check=True)
        self.original_cwd = Path.cwd()

    def tearDown(self):
        os.chdir(self.original_cwd)
        self.temporary.cleanup()

    def commit(self, version, message):
        manifest = self.repo / "apps/tesseract-conv/Cargo.toml"
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text(f'[package]\nname = "tsrct-conv"\nversion = "{version}"\n')
        subprocess.run(["git", "add", "."], cwd=self.repo, check=True)
        subprocess.run(["git", "commit", "--allow-empty", "-m", message], cwd=self.repo, check=True, capture_output=True)
        return subprocess.run(["git", "rev-parse", "HEAD"], cwd=self.repo, check=True,
                              text=True, capture_output=True).stdout.strip()

    def run_main(self, *, event, ref, sha, before=None, default_branch="main", api=None):
        event_file = self.repo / "event.json"
        event_file.write_text(json.dumps({"before": before}))
        output = self.repo / "output"
        environment = {"GITHUB_SHA": sha, "GITHUB_EVENT_NAME": event, "GITHUB_REF": ref,
                       "GITHUB_EVENT_PATH": str(event_file), "GITHUB_OUTPUT": str(output),
                       "GITHUB_REPOSITORY": "owner/repo", "GH_TOKEN": "token"}
        api_mock = api or (lambda path: {"default_branch": default_branch} if path == "" else None)
        os.chdir(self.repo)
        with patch.dict(os.environ, environment, clear=False), patch.object(sys, "argv", ["release_plan.py", "--phase", "validate"]), patch.object(release_plan, "api", side_effect=api_mock):
            release_plan.main()
        return output.read_text(), api_mock

    def test_multi_commit_push_uses_event_before_not_head_parent(self):
        before = self.commit("0.1.0", "baseline")
        self.commit("0.2.0", "version bump")
        head = self.commit("0.2.0", "follow-up in same push")
        output, _ = self.run_main(event="push", ref="refs/heads/main", sha=head, before=before)
        self.assertIn("action=release\n", output)
        self.assertIn("version=0.2.0\n", output)

    def test_unchanged_push_avoids_tag_queries(self):
        before = self.commit("0.2.0", "baseline")
        head = self.commit("0.2.0", "unrelated change")
        calls = []

        def api(path):
            calls.append(path)
            return {"default_branch": "main"}

        output, _ = self.run_main(event="push", ref="refs/heads/main", sha=head, before=before, api=api)
        self.assertIn("action=skip\n", output)
        self.assertEqual(calls, [""])

    def test_manual_requires_exact_checkout_main_and_main_default(self):
        head = self.commit("0.2.0", "release")
        output, _ = self.run_main(event="workflow_dispatch", ref="refs/heads/main", sha=head)
        self.assertIn("action=release\n", output)
        with self.assertRaisesRegex(ValueError, "checkout"):
            self.run_main(event="workflow_dispatch", ref="refs/heads/main", sha="a" * 40)
        with self.assertRaisesRegex(ValueError, "main"):
            self.run_main(event="workflow_dispatch", ref="refs/heads/dev", sha=head)
        with self.assertRaisesRegex(ValueError, "default branch"):
            self.run_main(event="workflow_dispatch", ref="refs/heads/main", sha=head,
                          default_branch="develop")

    def test_push_rejects_missing_baseline(self):
        head = self.commit("0.2.0", "release")
        with self.assertRaisesRegex(ValueError, "baseline"):
            self.run_main(event="push", ref="refs/heads/main", sha=head, before="0" * 40)


if __name__ == "__main__":
    unittest.main()
