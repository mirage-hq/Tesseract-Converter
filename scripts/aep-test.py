#!/usr/bin/env python3
"""Offline AEP audio E2E and legacy manual artifact comparator."""

import argparse
import json
import sys
from pathlib import Path

from aep_audio_test import AudioTestError, compare, require_hash, validate_policy
from aep_audio_e2e import load_case, run as run_e2e

CONV = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = CONV / "crates/aftereffects_file/tests/fixtures/aep_audio_test_cases.json"


def load_manifest(path: Path) -> dict:
    manifest = json.loads(path.read_text())
    if set(manifest) != {"version", "policy", "cases"} or manifest["version"] != 1:
        raise AudioTestError("unknown audio case manifest version/fields")
    if not isinstance(manifest["cases"], list):
        raise AudioTestError("cases must be a list")
    ids = set()
    for case in manifest["cases"]:
        if set(case) != {"id", "direction", "oracle_source_sha256", "composition_id",
                         "reference_sha256", "edited_fx_sha256", "status"}:
            raise AudioTestError("case fields missing or unknown")
        if case["id"] in ids or case["direction"] not in ("import", "export"):
            raise AudioTestError("duplicate case ID or invalid direction")
        if case["status"] != "UNRUN" or (case["composition_id"] is not None and (
                not isinstance(case["composition_id"], int) or isinstance(case["composition_id"], bool)
                or case["composition_id"] <= 0)):
            raise AudioTestError("case status/target invalid; results must be recorded separately")
        if case["direction"] == "import" and case["composition_id"] is None:
            raise AudioTestError("import case must select a native composition")
        ids.add(case["id"])
    return manifest


def main(argv=None) -> int:
    if argv is None:
        argv = sys.argv[1:]
    if argv and argv[0] == "list":
        parser = argparse.ArgumentParser(description="List registered real audio E2E cases (no execution)")
        parser.add_argument("--manifest", type=Path,
                            default=CONV / "crates/aftereffects_file/tests/fixtures/audio_e2e/cases.json")
        args = parser.parse_args(argv[1:])
        try:
            manifest = json.loads(args.manifest.read_text())
            rows = []
            for item in manifest["cases"]:
                _, case, root = load_case(args.manifest, item["id"], item["directions"][0])
                require_hash(root / case["source"]["path"], case["source"]["sha256"], "native source")
                for media in case["primary"]:
                    require_hash(root / media["path"], media["sha256"], "primary media")
                if "export" in case["directions"]:
                    require_hash(root / "fx" / (case["id"] + ".json"),
                                 case["fx_input"]["document_sha256"], "explicit edited FX")
                rows.append({"case": case["id"], "directions": case["directions"],
                             "composition_id": case["source"]["composition_id"],
                             "reference": "pinned" if case["reference"] else "missing pin"})
            print(json.dumps({"cases": rows, "execution": "UNRUN by this listing"}, indent=2))
            return 0
        except (AudioTestError, OSError, ValueError, KeyError, TypeError) as exc:
            print(f"aep audio registry invalid: {exc}", file=sys.stderr)
            return 1
    if not argv or argv[0] not in ("check", "compare"):
        parser = argparse.ArgumentParser(description="Fresh AEP audio import/export render and comparison")
        parser.add_argument("--manifest", type=Path, default=CONV / "crates/aftereffects_file/tests/fixtures/audio_e2e/cases.json", help="E2E case manifest")
        parser.add_argument("--case", required=True, help="Exact feature-case slug or all registered cases")
        parser.add_argument("--direction", choices=("import", "export", "both"), required=True)
        parser.add_argument("--work", type=Path, required=True, help="New empty result directory; never reused")
        parser.add_argument("--converter", default=str(CONV / "target/debug/tsrct-conv"))
        parser.add_argument("--tsrct", default=str(CONV.parents[1] / "target/debug/tsrct"))
        parser.add_argument("--preparer", default=str(CONV / "target/debug/examples/audio_e2e"))
        parser.add_argument("--timeout", type=int, default=900)
        args = parser.parse_args(argv)
        try:
            result = run_e2e(args)
            print(json.dumps(result, indent=2, allow_nan=False))
            return 0
        except (AudioTestError, OSError, ValueError, KeyError, TypeError, RuntimeError) as exc:
            print(f"aep audio E2E BLOCKED/FAILED: {exc}", file=sys.stderr)
            return 1
    parser = argparse.ArgumentParser(description="Legacy manual audio artifact comparator")
    parser.add_argument("command", choices=("check", "compare"))
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--case", help="Exact case ID")
    parser.add_argument("--direction", choices=("import", "export"))
    parser.add_argument("--source", type=Path, help="Pinned native oracle AEP (identity check only)")
    parser.add_argument("--fx-input", type=Path, help="Explicit edited FX input for export (identity check only)")
    parser.add_argument("--actual", type=Path, help="Separately produced FX/import or Adobe/export audio artifact")
    parser.add_argument("--reference", type=Path, help="Separately produced native/oracle audio artifact")
    args = parser.parse_args(argv)
    try:
        manifest = load_manifest(args.manifest)
        if args.command == "check":
            if any((args.case, args.direction, args.source, args.fx_input, args.actual, args.reference)):
                raise AudioTestError("check accepts only --manifest")
            # Validate policy even with no artifacts (fixture registration is not proof).
            validate_policy(manifest["policy"])
            print(json.dumps({"cases": len(manifest["cases"]), "status": "UNRUN", "artifact_comparisons": 0}))
            return 0
        if not all((args.case, args.direction, args.source, args.actual, args.reference)):
            raise AudioTestError("compare requires --case --direction --source --actual --reference")
        matches = [case for case in manifest["cases"] if case["id"] == args.case
                   and case["direction"] == args.direction]
        if len(matches) != 1:
            raise AudioTestError("case/direction not registered")
        case = matches[0]
        if case["composition_id"] is None:
            raise AudioTestError("native target identity pending")
        require_hash(args.source, case["oracle_source_sha256"], "native oracle source")
        if args.direction == "export":
            if args.fx_input is None:
                raise AudioTestError("export requires explicit --fx-input")
            require_hash(args.fx_input, case["edited_fx_sha256"], "edited FX input")
        elif args.fx_input is not None:
            raise AudioTestError("import does not accept --fx-input")
        require_hash(args.reference, case["reference_sha256"], "independent reference")
        if args.actual.resolve() == args.reference.resolve():
            raise AudioTestError("actual and reference must be separate artifacts")
        result = compare(args.actual, args.reference, manifest["policy"])
        print(json.dumps({"case": args.case, "direction": args.direction,
                          "reference_sha256": case["reference_sha256"], **result}, indent=2, allow_nan=False))
        return 0 if result["passed"] else 1
    except (AudioTestError, OSError, ValueError, KeyError, TypeError) as exc:
        print(f"aep audio test BLOCKED/FAILED: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
