# Native point-text fragments

These are unchanged top-level Text / Vector Motion component records and their
parameters from an Adobe-native Premiere project, plus the original records that
define each referenced `BinaryHash`. No media or complete production project is
included. The reader regression grafts these records into the existing minimal
test placement on a 1080 x 1920 canvas. That surrounding placement is a test
harness; its timing is not claimed as Adobe-native evidence.

Source SHA-256: `e80cca0275ee14286e6de71cff289b1624e62e39eebed4e345d83b9e3f85001f`.
Selected source sequence UID: `84f4bbb6-d3c8-44e4-87b1-9de28c0d7a37`.

| Fragment | Native occurrence | Components | Source Text parameter | Native active seconds |
| --- | --- | --- | --- | --- |
| 421.xml | 421 | 1617 | 4009, payload definition 2558 | 0 to 1.266666667 |
| 422.xml | 422 | 1619 | 4031, payload definition 2590 | 1.266666667 to 4.4 |
| 423.xml | 423 | 1621, 1622 | 4059, payload definition 2622 | 4.4 to 7 |
| 424.xml | 424 | 1624, 1625 | 4087, payload definition 2654 | 7 to 10.833333333 |

The Source Text document stores center alignment (slot 5 = 1). Uniform titles
store leading 0. The opening title has two styles separated by a paragraph
break. Independent existing Adobe frames at source frames 0, 73, 145 and 219
show the opening title and uniform-title baselines. The unchanged reference MP4
SHA-256 is `0d35899a6b0f257344edc22fe63c087138ae1ee96ad57436bee2430a244ada9b`.
It is retained outside Git. The opening title's exact font substitution was not
independently inspected; typography remains diagnostic. These fixtures assert
editable structure and numerical baseline placement, not a visual equality gate.

Extraction used the original top-level record text, including IDs, Source Text
bytes, empty hash references, normalized coordinates, scale, rotation and
opacity. The reader's real hash resolver supplies the payload definitions.
