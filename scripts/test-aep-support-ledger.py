#!/usr/bin/env python3
"""Keep AEP diagnostic documentation checked without a private crate dependency."""

from pathlib import Path
import re
import unittest


class AepSupportLedgerTests(unittest.TestCase):
    def test_every_emitted_code_has_a_committed_approximation_entry(self):
        root = Path(__file__).resolve().parent.parent
        source = (root / "crates/aftereffects_file/src/diagnostic.rs").read_text()
        notes = (root / "docs/after-effects-support.md").read_text()
        codes = set(re.findall(r'"(AE-[A-Z-]+)"', source))
        self.assertTrue(codes, "diagnostic code inventory must not be empty")
        for code in sorted(codes):
            with self.subTest(code=code):
                self.assertIn(f"| `{code}` |", notes)


if __name__ == "__main__":
    unittest.main()
