#!/usr/bin/env python3
"""Generate `color_warm_tint.cube`, the 17-point 3D LUT used by the colour fixtures.

The table is a deterministic, visibly warm tint of the identity: red is lifted
(`0.1 + 0.9 r`), green is unchanged and blue is scaled to `0.8 b`. Values are
printed with six decimals so regeneration is byte-identical.

Usage: python3 color_warm_tint_cube.py [output.cube]
"""
from pathlib import Path
import sys

SIZE = 17


def main() -> None:
    output = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("color_warm_tint.cube")
    lines = ['TITLE "color_warm_tint"', f"LUT_3D_SIZE {SIZE}", "DOMAIN_MIN 0.0 0.0 0.0", "DOMAIN_MAX 1.0 1.0 1.0"]
    step = SIZE - 1
    # .cube order: red varies fastest, then green, then blue.
    for b in range(SIZE):
        for g in range(SIZE):
            for r in range(SIZE):
                red, green, blue = r / step, g / step, b / step
                lines.append(f"{0.1 + 0.9 * red:.6f} {green:.6f} {0.8 * blue:.6f}")
    output.write_text("\n".join(lines) + "\n", encoding="ascii")


if __name__ == "__main__":
    main()
