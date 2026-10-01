"""Public fixed Adjustment export case identities shared with the private Adobe adapter.

These IDs identify CPU records in the shared JSONL; they are not evidence that
Adobe accepted or rendered any exported project.
"""

CASE_SPECS: tuple[tuple[str, int], ...] = (
    ("scope", 1),
    ("stack-order", 22),
    ("effect-order", 44),
    ("keys", 64),
    ("disabled", 84),
    ("span", 106),
    ("nested", 146),
    ("masks", 163),
    ("matte", 183),
    ("parent", 205),
)

ADJUSTMENT_EXPORT_CASE_IDS = frozenset(
    f"fx-export-adjustment-{slug}" for slug, _ in CASE_SPECS
)
