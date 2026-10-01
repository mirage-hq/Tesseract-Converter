#!/usr/bin/env python3
"""Check external Adobe script receipts against explicit CPU-emitted oracles.

This is native semantic readback, NOT pixel/alpha proof. Rejected/missing cases
fail; unknown or duplicate receipt entries are rejected. No Adobe execution here.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path

CODES = {"linear": "6612", "cubic": "6613", "hold": "6614"}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def compare(oracle, actual, point_tolerance=0.001):
    failures = []
    if actual.get("effect") != oracle["effect"] or actual.get("enabled") != oracle["enabled"]:
        failures.append("effect identity/enabled mismatch")
    controls = {c["name"]: c for c in actual.get("controls", [])}
    if len(controls) != len(actual.get("controls", [])):
        failures.append("duplicate native controls")
    max_delta = 0.0

    def values(expected, observed, label):
        nonlocal max_delta
        if not isinstance(observed, list) or len(expected) != len(observed):
            failures.append(f"{label}: missing/wrong component count")
            return
        # Source-relative Point evaluation has measured subpixel solver error;
        # this tolerance is not reused for colors/scalars or pixel comparisons.
        tolerance = point_tolerance if len(expected) in (2, 3) else 1e-7
        for index, (a, b) in enumerate(zip(expected, observed)):
            if not isinstance(b, (float, int)) or not math.isfinite(b):
                failures.append(f"{label}[{index}]: non-finite/nonnumeric")
                continue
            delta = abs(a - b)
            max_delta = max(max_delta, delta)
            if delta > tolerance:
                failures.append(f"{label}[{index}]: expected {a}, got {b}")

    def timestamp(expected, observed, label):
        if not isinstance(observed, (float, int)) or not math.isfinite(observed) or abs(expected - observed) > 1e-7:
            failures.append(f"{label}: missing/non-finite/wrong time")

    for expected in oracle["controls"]:
        name = expected["name"]
        got = controls.get(name)
        if got is None:
            failures.append(f"{name}: missing control")
            continue
        if "value" in expected:
            values(expected["value"], got.get("value"), name)
            if got.get("keys"):
                failures.append(f"{name}: invented animation")
        else:
            keys = got.get("keys", [])
            if len(keys) != len(expected["keys"]):
                failures.append(f"{name}: expected {len(expected['keys'])} keys, got {len(keys)}")
            for index, (key, observed) in enumerate(zip(expected["keys"], keys)):
                timestamp(key[0], observed.get("time"), f"{name}/{index}")
                values(key[1:], observed.get("value"), f"{name}/{index}")
                if index:
                    code = CODES[expected["segments"][index - 1]]
                    if str(keys[index - 1].get("outCode")) != code or str(observed.get("inCode")) != code:
                        failures.append(f"{name}/{index}: wrong native interpolation")
            samples = got.get("samples", [])
            desired = expected.get("valueAtTime", [])
            if len(samples) != len(desired):
                failures.append(f"{name}: missing critical samples")
            for index, (sample, observed) in enumerate(zip(desired, samples)):
                timestamp(sample[0], observed.get("time"), f"{name}/sample{index}")
                values(sample[1:], observed.get("value"), f"{name}/sample{index}")
    if (actual.get("width"), actual.get("height"), actual.get("duration"), actual.get("frameRate"), actual.get("layers"), actual.get("pixelAspect")) != (320, 180, 2, 24, 1, 1):
        failures.append("wrong native canvas/duration/source FPS/owner count")
    return {"status": "passed" if not failures else "failed", "failures": failures, "maximum_control_delta": max_delta}


def checked_receipt(receipt, version="26.5x89", build=89):
    if receipt.get("completed") is not True or receipt.get("error") or receipt.get("failures"):
        raise ValueError("Adobe inspection did not complete cleanly; partial captures are not passing receipts")
    if receipt.get("mode") != "inspect" or receipt.get("version") != version or receipt.get("build") != build:
        raise ValueError("Adobe inspection mode/version/build differs from the pinned producer")
    cases = receipt.get("cases")
    if not isinstance(cases, list) or not cases:
        raise ValueError("Adobe receipt has no inspected cases")
    return cases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, action="append", required=True)
    parser.add_argument("--case", action="append", required=True, help="Explicit case names; no implicit directory sweep")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--adobe-version", default="26.5x89")
    parser.add_argument("--adobe-build", type=int, default=89)
    args = parser.parse_args()
    if len(set(args.case)) != len(args.case):
        parser.error("duplicate case selection")
    actual = {}
    for path in args.receipt:
        receipt = json.loads(path.read_text())
        try:
            inspected = checked_receipt(receipt, args.adobe_version, args.adobe_build)
        except ValueError as error:
            parser.error(f"{path}: {error}")
        for case in inspected:
            if case["name"] in actual:
                parser.error("duplicate native receipt case")
            actual[case["name"]] = case
    results = []
    for name in args.case:
        if not name or any(char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for char in name):
            parser.error("unsafe case name")
        oracle_path = args.inputs / (name + ".expected.json")
        oracle = json.loads(oracle_path.read_text())
        result = compare(oracle, actual[name]) if name in actual else {"status": "failed", "failures": ["No successful independent native readback; inspect rejection/render logs"]}
        result.update(case=name, oracle_sha256=digest(oracle_path), input_sha256=digest(args.inputs / (name + ".fx.json")), aep_sha256=digest(args.inputs / (name + ".aep")))
        results.append(result)
    report = {"scope": "Independent Adobe scripted control readback, not manual UI inspection or pixel/alpha fidelity", "receipts": [{"name": p.name, "sha256": digest(p)} for p in args.receipt], "cases": results}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print(f"{sum(r['status']=='passed' for r in results)}/{len(results)} native readback cases passed")
    return int(any(r["status"] != "passed" for r in results))


if __name__ == "__main__":
    raise SystemExit(main())
