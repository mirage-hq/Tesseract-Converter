#!/usr/bin/env python3
"""Compare pinned Premiere/FX audio artifacts without rendering or alignment."""

from __future__ import annotations

import argparse
import json
import sys
import uuid
from pathlib import Path

from aep_audio_test import AudioTestError, compare, require_hash, validate_policy


def load_case(path: Path) -> tuple[dict, dict[str, Path]]:
    case = json.loads(path.read_text())
    fields = {"version", "id", "direction", "sequence_uid", "native_project",
              "fx_project", "media", "actual", "reference", "policy"}
    if not isinstance(case, dict) or set(case) != fields or type(case["version"]) is not int or case["version"] != 1:
        raise AudioTestError("unknown Premiere audio case version/fields")
    if not isinstance(case["id"], str) or not case["id"] or case["direction"] not in ("import", "export"):
        raise AudioTestError("case requires an ID and import/export direction")
    try:
        uuid.UUID(case["sequence_uid"])
    except (ValueError, TypeError, AttributeError) as error:
        raise AudioTestError("case requires a Premiere sequence UID") from error
    validate_policy(case["policy"])
    if not isinstance(case["media"], list):
        raise AudioTestError("media must be a list of pinned files")

    def pinned(item: dict, label: str) -> Path:
        if not isinstance(item, dict) or set(item) != {"path", "sha256"} or not isinstance(item["path"], str):
            raise AudioTestError(f"{label}: expected path and SHA-256")
        resolved = (path.parent / item["path"]).resolve()
        require_hash(resolved, item["sha256"], label)
        return resolved

    files = {key: pinned(case[key], key) for key in
             ("native_project", "fx_project", "actual", "reference")}
    for index, item in enumerate(case["media"]):
        pinned(item, f"media {index}")
    if files["actual"].samefile(files["reference"]):
        raise AudioTestError("actual and reference must be separate artifacts")
    return case, files


def run(case_path: Path, output: Path) -> bool:
    """Retain the raw scorer result separately from input identity and status."""
    case, files = load_case(case_path)
    output.mkdir(parents=True, exist_ok=False)
    # Record resolved paths unambiguously; the source case stays unchanged.
    (output / "inputs.json").write_text(json.dumps({
        "case": case, "case_file": str(case_path.resolve()),
        "resolved_files": {key: str(value) for key, value in files.items()},
        "roles": {"actual": "FX render" if case["direction"] == "import" else "Premiere export render",
                  "reference": "Premiere source render" if case["direction"] == "import" else "edited FX render"},
        "evidence_limit": "Pins bind supplied files, not their claimed renderer or conversion history. Retain native and conversion receipts separately.",
    }, indent=2, allow_nan=False) + "\n")
    try:
        result = compare(files["actual"], files["reference"], case["policy"], report_duration_failure=True)
    except (AudioTestError, OSError, ValueError) as error:
        (output / "status.json").write_text(json.dumps({
            "case": case["id"], "direction": case["direction"],
            "measurement": "unmeasured", "status": "blocked", "error": str(error),
        }, indent=2) + "\n")
        raise
    # Do not add metadata to or change the scorer's comparison fields.
    (output / "comparison.json").write_text(json.dumps(result, indent=2, allow_nan=False) + "\n")
    (output / "status.json").write_text(json.dumps({
        "case": case["id"], "direction": case["direction"], "measurement": "measured",
        "status": "passed" if result["passed"] else "failed_score",
    }, indent=2) + "\n")
    return result["passed"]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", type=Path, required=True, help="Pinned case JSON; relative paths use its directory")
    parser.add_argument("--output", type=Path, required=True, help="New result directory; existing results are never overwritten")
    args = parser.parse_args(argv)
    try:
        passed = run(args.case, args.output)
        print(f"{'PASS' if passed else 'FAIL'}: {args.output / 'comparison.json'}")
        return 0 if passed else 1
    except (AudioTestError, OSError, ValueError, TypeError) as error:
        print(f"Premiere audio comparison blocked: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
