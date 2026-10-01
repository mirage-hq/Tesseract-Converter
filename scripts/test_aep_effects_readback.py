import importlib.util
from pathlib import Path

import pytest

spec = importlib.util.spec_from_file_location(
    "readback", Path(__file__).with_name("aep-effects-readback-check.py")
)
readback = importlib.util.module_from_spec(spec)
spec.loader.exec_module(readback)


@pytest.fixture
def oracle():
    return {
        "effect": "Test",
        "enabled": True,
        "controls": [{
            "name": "Value",
            "keys": [[0, 20], [1, 50]],
            "segments": ["linear"],
            "valueAtTime": [[0.5, 35]],
        }],
    }


@pytest.fixture
def actual():
    return {
        "effect": "Test",
        "enabled": True,
        "width": 320,
        "height": 180,
        "duration": 2,
        "frameRate": 24,
        "layers": 1,
        "pixelAspect": 1,
        "controls": [{
            "name": "Value",
            "value": [20],
            "keys": [
                {"time": 0, "value": [20], "inCode": "6612", "outCode": "6612"},
                {"time": 1, "value": [50], "inCode": "6612", "outCode": "6612"},
            ],
            "samples": [{"time": 0.5, "value": [35]}],
        }],
    }


@pytest.fixture
def receipt(actual):
    return {
        "mode": "inspect",
        "version": "26.5x89",
        "build": 89,
        "completed": True,
        "cases": [actual],
    }


def test_complete_inspection_envelope(receipt, actual):
    assert readback.checked_receipt(receipt) == [actual]


@pytest.mark.parametrize(
    "changes",
    [
        pytest.param({"completed": False}, id="incomplete-receipt"),
        pytest.param({"error": "Unsafe project state after inspection"}, id="late-batch-error"),
        pytest.param(
            {"failures": [{"name": "other", "error": "missing owner"}]},
            id="failed-case-in-completed-batch",
        ),
        pytest.param({"mode": "author"}, id="authoring-is-not-inspection"),
        pytest.param({"version": "other"}, id="wrong-adobe-version"),
        pytest.param({"build": 90}, id="wrong-adobe-build"),
    ],
)
def test_invalid_inspection_receipt_is_rejected(receipt, changes):
    receipt.update(changes)
    with pytest.raises(ValueError):
        readback.checked_receipt(receipt)


def test_matching_native_controls(oracle, actual):
    assert readback.compare(oracle, actual)["status"] == "passed"


def test_missing_keys_and_samples_are_not_passes(oracle, actual):
    actual["controls"][0].update(keys=[], samples=[])
    assert readback.compare(oracle, actual)["status"] == "failed"


def test_wrong_raw_interpolation_is_not_mislabeled(oracle, actual):
    actual["controls"][0]["keys"][1]["inCode"] = "6614"
    assert readback.compare(oracle, actual)["status"] == "failed"


@pytest.mark.parametrize(
    ("collection", "field", "value"),
    [
        pytest.param("samples", "value", [float("nan")], id="nonfinite-value"),
        pytest.param("samples", "time", float("nan"), id="nonfinite-sample-time"),
        pytest.param("keys", "time", float("nan"), id="nonfinite-key-time"),
    ],
)
def test_nonfinite_native_data_fails(oracle, actual, collection, field, value):
    actual["controls"][0][collection][0][field] = value
    assert readback.compare(oracle, actual)["status"] == "failed"


def test_duplicate_controls_fail(oracle, actual):
    actual["controls"].append(actual["controls"][0].copy())
    assert readback.compare(oracle, actual)["status"] == "failed"


def test_missing_owner_is_not_native_acceptance(oracle, actual):
    actual["layers"] = 0
    assert readback.compare(oracle, actual)["status"] == "failed"
