"""Offline receipt-shape guards for the non-CI AE ABI generator."""
import importlib.util
from pathlib import Path

import pytest

SPEC = importlib.util.spec_from_file_location(
    "generate_aep_effect_definitions", Path(__file__).with_name("generate-aep-effect-definitions.py")
)
GENERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GENERATOR)


@pytest.mark.parametrize(
    ("kind", "value"),
    [(10, None), (5, [1, 0, 0]), (6, [1]), (1, True), (2, float("nan")), (10, float("inf"))],
)
def test_numeric_kinds_require_finite_correct_arity(kind, value):
    with pytest.raises(ValueError):
        GENERATOR.numeric_defaults(kind, value)


@pytest.mark.parametrize("kind, value", [(10, 1.25), (6, [60, 40]), (5, [0.2, 0.4, 0.6, 1])])
def test_numeric_scalar_point_and_color_are_preserved(kind, value):
    assert GENERATOR.numeric_defaults(kind, value) == (value if isinstance(value, list) else [value])


def test_non_numeric_kind_has_no_defaults():
    assert GENERATOR.numeric_defaults(0, None) == []
