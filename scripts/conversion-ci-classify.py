#!/usr/bin/env python3
"""Decide whether a PR needs the strict conversion video reference gate."""

import sys

EXACT_INPUTS = {
    ".github/workflows/conversion-render-t4.yml",  # Renamed workflow; classify deletions too.
    ".github/workflows/conversion-render.yml",
    "Cargo.toml",
    "Cargo.lock",
    "Makefile",
    "opensource/conv/scripts/conversion-ci-classify.py",
    "scripts/ffmpeg-version.env",
}
PREFIX_INPUTS = (
    ".github/actions/setup-ffmpeg/",
    ".github/actions/setup-linux-gpu-ffmpeg/",
    "opensource/conv/",
    "apps/public_cli/",
    "apps/utils/",
    "crates/validation/",
    "patches/",
)


def should_run(files: list[str]) -> bool:
    # A missing/truncated list must not silently exempt a PR from the gate.
    return not files or len(files) >= 3000 or any(
        path in EXACT_INPUTS or path.startswith(PREFIX_INPUTS) for path in files
    )


def self_test() -> None:
    assert should_run([])
    assert should_run(["docs/unrelated.md"]) is False
    assert should_run(["docs/unrelated.md", "opensource/conv/crates/premiere_file/src/lib.rs"])
    assert all(should_run([path]) for path in EXACT_INPUTS)
    assert all(should_run([f"{prefix}fixture"]) for prefix in PREFIX_INPUTS)
    # The API supplies previous_filename for renames and filename for deletions.
    assert should_run(["docs/new.md", "opensource/conv/tests/old.json"])
    assert should_run(["opensource/conv/tests/deleted.json"])
    assert should_run(["crates/unrelated/src/lib.rs"]) is False
    assert should_run(["docs/unrelated.md"] * 2999) is False
    assert should_run(["docs/unrelated.md"] * 3000)


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    elif len(sys.argv) == 1:
        files = [line.strip() for line in sys.stdin if line.strip()]
        print(f"run={str(should_run(files)).lower()}")
    else:
        raise SystemExit("usage: conversion-ci-classify.py [--self-test]")
