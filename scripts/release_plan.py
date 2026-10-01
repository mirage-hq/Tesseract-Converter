#!/usr/bin/env python3
"""Fail-closed routing for immutable converter releases."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
from urllib.error import HTTPError
from urllib.request import Request, urlopen

PLATFORMS = ("darwin-arm64", "darwin-x86_64", "linux-x86_64", "windows-x86_64")


def git(*args):
    return subprocess.run(["git", *args], check=True, text=True, capture_output=True).stdout.strip()


def version_at(ref):
    data = git("show", f"{ref}:apps/tesseract-conv/Cargo.toml")
    version = tomllib.loads(data)["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("Cargo version must be numeric major.minor.patch")
    return version


def api(path):
    repo = os.environ["GITHUB_REPOSITORY"]
    token = os.environ["GH_TOKEN"]
    url = f"https://api.github.com/repos/{repo}" + (f"/{path}" if path else "")
    request = Request(url, headers={
        "Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28"})
    try:
        with urlopen(request, timeout=15) as response:
            return json.load(response)
    except HTTPError as error:
        if error.code == 404:
            return None
        raise


def release_identity(tag, sha, version):
    ref = api(f"git/ref/tags/{tag}")
    release = api(f"releases/tags/{tag}")
    if ref is None and release is None:
        return "new"
    if ref is None or release is None:
        raise ValueError("partial release identity; investigate tag/release manually")
    target = ref["object"]
    if target["type"] == "tag":
        target = api(f"git/tags/{target['sha']}")["object"]
    expected = {f"Tesseract-Converter-{version}-{platform}.zip{suffix}"
                for platform in PLATFORMS for suffix in ("", ".sha256")}
    assets = [asset["name"] for asset in release["assets"]]
    if (target["type"] != "commit" or target["sha"] != sha or
            release["tag_name"] != tag or release["draft"] or release["prerelease"] or
            release["target_commitish"] != sha or len(assets) != len(expected) or
            set(assets) != expected or any(asset["size"] <= 0 for asset in release["assets"])):
        raise ValueError("conflicting or incomplete release identity; investigate manually")
    return "existing"


def decide(event, ref, sha, before, version, identity):
    if ref != "refs/heads/main":
        raise ValueError("release must run from main")
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("invalid source SHA")
    if event == "push":
        if not re.fullmatch(r"[0-9a-f]{40}", before or "") or before == "0" * 40:
            raise ValueError("missing push baseline")
        git("merge-base", "--is-ancestor", before, sha)
        previous = version_at(before)
        if tuple(map(int, version.split("."))) <= tuple(map(int, previous.split("."))):
            return "skip"
    elif event != "workflow_dispatch":
        raise ValueError("unsupported release event")
    return "skip" if identity == "existing" else "release"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--phase", choices=("validate", "publish"), required=True)
    args = parser.parse_args()
    sha = os.environ["GITHUB_SHA"]
    if git("rev-parse", "HEAD") != sha:
        raise ValueError("checkout does not match event SHA")
    repository = api("")
    if repository is None:
        raise ValueError("repository metadata unavailable; check repository read access")
    if repository["default_branch"] != "main":
        raise ValueError("repository default branch must be main")
    version = version_at(sha)
    tag = f"v{version}"
    event = os.environ["GITHUB_EVENT_NAME"]
    before = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text()).get("before")
    if args.phase == "publish":
        if release_identity(tag, sha, version) != "new":
            raise ValueError("release identity appeared during build; refusing overwrite")
        action = "release"
    else:
        # Avoid remote tag queries for non-version-changing pushes.
        action = decide(event, os.environ["GITHUB_REF"], sha, before, version, "new")
        if action == "release":
            action = decide(event, os.environ["GITHUB_REF"], sha, before, version,
                            release_identity(tag, sha, version))
    if args.phase == "validate":
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            for key, value in (("action", action), ("version", version), ("source_sha", sha), ("tag", tag)):
                output.write(f"{key}={value}\n")
    print(f"release routing: {action} {tag}")


if __name__ == "__main__":
    main()
