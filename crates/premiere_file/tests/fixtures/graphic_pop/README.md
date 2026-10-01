# Native two-sided graphic Pop fragments

`native.xml` contains unchanged top-level records for four consecutive native
point-text placements (421–424), their synthetic graphic source/clip/component
links, and three Film Impact Pop transitions (425–427), components 761–763 and
all 43 parameters per component, Text/Vector Motion records and their original
BinaryHash definitions. These text records also occur in the existing
`../point_text_lines/` fragments. No video, fonts or full production project is
included. The test supplies a minimal sequence/track wrapper; that wrapper is a
harness, not an Adobe-authored project.

Source project SHA-256:
`e80cca0275ee14286e6de71cff289b1624e62e39eebed4e345d83b9e3f85001f`.
Source sequence UID: `84f4bbb6-d3c8-44e4-87b1-9de28c0d7a37`.
Native sequence: 1080 × 1920, 30 fps. Original placement ranges and generator
InPoint/OutPoint values are retained.

| Transition | Outgoing / incoming | Range seconds | Cut seconds | Half frames |
| --- | --- | --- | --- | --- |
| 425 | 421 / 422 | 1.066666667–1.466666667 | 1.266666667 | 6 / 6 |
| 426 | 422 / 423 | 4.2–4.6 | 4.4 | 6 / 6 |
| 427 | 423 / 424 | 6.766666667–7.233333333 | 7 | 7 / 7 |

All controls equal the already measured bounded default Pop profile. The
existing independent native reference MP4 (kept outside Git) has SHA-256
`0d35899a6b0f257344edc22fe63c087138ae1ee96ad57436bee2430a244ada9b`.
Yellow title bounds at native frames 129–138 and 207–217 fit the normalized
sampled scale curve independently on each half; both cut frames have no title.
Native antialiasing, early blur/fade, fitted scale and the ink-center pivot remain
approximate. The converter uses the authored block origin (~3 px peak vertical
difference in this reference), without inventing font metrics or equating the
plugin blur control to an engine shutter.

`native_two_sided_graphic_pop_owns_whole_blocks_and_merges_disjoint_windows`
imports this source freshly and asserts editable common ownership, local key
clocks, collapse at cuts and merged disjoint head/tail windows. Additional tests
mutate the parsed model to exercise guards and other geometry/clocks; those
mutations are semantic tests, not new native evidence. This fixture establishes
structural import coverage, not a scored visual gate, new Adobe playback proof
or an editable Film Impact plugin exporter.
