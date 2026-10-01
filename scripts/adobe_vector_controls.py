"""Validate captured Adobe vector controls; never infer proof from a receipt status."""
from __future__ import annotations

import math
from typing import Any

SAMPLE_TIMES = (0.0, 0.5, 29 / 30, 1.0, 31 / 30, 59 / 30)


def _number(actual: Any, expected: Any) -> bool:
    try:
        return (
            isinstance(actual, (int, float)) and not isinstance(actual, bool)
            and isinstance(expected, (int, float)) and not isinstance(expected, bool)
            and math.isfinite(actual) and math.isfinite(expected)
            and math.isclose(actual, expected, rel_tol=0, abs_tol=1e-6)
        )
    except OverflowError:
        return False


def _values(actual: Any, expected: Any) -> bool:
    return (isinstance(actual, list) and isinstance(expected, list)
            and len(actual) == len(expected)
            and all(_number(a, e) for a, e in zip(actual, expected)))


def compare(case_name: str, expected: dict[str, Any], readback: dict[str, Any],
            sample_oracle: dict[str, Any] | None = None) -> list[str]:
    """Compare controls/keys to manual expectations, samples to independent Adobe.

    AE valueAtTime quantizes even Linear interpolation. Its independently authored
    source samples, not ideal floating-point interpolation, are the export oracle.
    Without sample_oracle this validates source sample structure/endpoints only.
    1e-6 covers f32 control narrowing, not a visual-fidelity tolerance.
    Malformed evidence raises ValueError; callers must fail closed.
    """
    failures = []
    if (readback.get("case") != case_name or readback.get("completed") is not True
            or readback.get("ownedProjectClosed") is not True or readback.get("error")):
        failures.append("native readback was not completed in an owned project")
    composition = readback["composition"]
    for name, value in (("width", 320), ("height", 180), ("duration", 2),
                        ("frameRate", 24), ("pixelAspect", 1), ("layers", 1)):
        if not _number(composition[name], value):
            failures.append(f"composition {name} differs")
    if composition["name"] != case_name or readback["layer"]["name"] != expected["layerName"]:
        failures.append("composition/layer name differs")
    if not readback.get("adobeVersion") or not readback.get("adobeBuild"):
        failures.append("Adobe build identity is absent")
    controls = readback["controls"]
    if not isinstance(controls, list) or not isinstance(expected["properties"], list):
        raise ValueError("control arrays are required")
    by_name = {control["name"]: control for control in controls}
    wanted = {prop["name"] for prop in expected["properties"]}
    if len(by_name) != len(controls) or set(by_name) != wanted or not wanted:
        failures.append("native control set differs or contains duplicates")
    for prop in expected["properties"]:
        name = prop["name"]
        actual = by_name.get(name)
        if actual is None:
            continue
        keys = prop.get("keys", [])
        if keys:
            if len(keys) != 2 or not _number(keys[0][0], 0) or not _number(keys[1][0], 1):
                raise ValueError("this vector panel requires two Linear keys at 0/1 seconds")
            actual_keys = actual["keys"]
            if len(actual_keys) != len(keys):
                failures.append(f"{name}: key count differs")
            for key, oracle in zip(actual_keys, keys):
                if not _number(key["time"], oracle[0]) or not _values(key["value"], oracle[1:]):
                    failures.append(f"{name}: native key time/value differs")
                if key["inCode"] != "6612" or key["outCode"] != "6612":
                    failures.append(f"{name}: native interpolation is not Linear")
        else:
            if actual["keys"] or not _values(actual["value"], prop["value"]):
                failures.append(f"{name}: static value or key count differs")
        samples = actual["samples"]
        if len(samples) != len(SAMPLE_TIMES):
            failures.append(f"{name}: critical sample count differs")
        oracle_samples = None
        if sample_oracle is not None:
            oracle_control = next((c for c in sample_oracle["controls"] if c["name"] == name), None)
            if oracle_control is None:
                raise ValueError("independent source control is missing")
            oracle_samples = oracle_control["samples"]
            if len(oracle_samples) != len(SAMPLE_TIMES):
                raise ValueError("independent source sample count differs")
        for index, (sample, time) in enumerate(zip(samples, SAMPLE_TIMES)):
            values = prop.get("value")
            if oracle_samples is not None:
                if not _number(oracle_samples[index]["time"], time):
                    raise ValueError("independent source sample schedule differs")
                values = oracle_samples[index]["value"]
            elif keys:
                if time == 0:
                    values = keys[0][1:]
                elif time >= 1:
                    values = keys[1][1:]
                else:
                    values = sample["value"]
                    if len(values) != len(keys[0]) - 1:
                        failures.append(f"{name}: native source sample dimensions differ")
            if not _number(sample["time"], time) or not _values(sample["value"], values):
                failures.append(f"{name}: critical valueAtTime differs at {time}")
    return failures
