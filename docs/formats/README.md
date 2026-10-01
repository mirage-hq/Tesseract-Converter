# Format support

`tsrct-conv` converts standalone editable documents through Tesseract
(`.tsrct`). It is not a lossless native-project archiver or a video transcoder.

| Format | Import to `.tsrct` | Export from `.tsrct` | Details |
| --- | --- | --- | --- |
| Premiere Pro (`.prproj`) | Partial | Partial | [Features and losses](premiere.md) |
| After Effects (`.aep`) | Partial / best effort | Partial / experimental | [Features and losses](after-effects.md) |

Direct Premiere ↔ After Effects and same-format conversions are not supported.
An intermediate conversion does not remove either format's limitations.

## Detailed support tables

- [Premiere: timelines, media, motion, effects, text and audio](premiere.md)
- [After Effects: layers, timing, geometry, text, masks and audio](after-effects.md)
- [After Effects: individual effect controls](after-effects-effects.md)

Each page separates import from export and records constraints, approximation or
loss, failure/fallback behavior and the available evidence. Follow the source,
test and ledger links for exact control combinations. These tables describe the
current converter, not an exhaustive catalog of every feature in each native
application. An unlisted feature is **not a promise of support**.

## Reading support status

- **Supported**: the stated feature and constraints have an implemented mapping;
  this alone does not claim visual equality.
- **Partial**: only a subset maps, or conversion may approximate or omit content.
- **Unsupported**: no supported mapping; affected content can be omitted, retained
  at an initial/static value, or rejected as described for that feature.
- **Experimental**: an implemented path with incomplete native-application
  compatibility/fidelity evidence.
- **Known failure**: an implemented path has a recorded failure for the stated
  case. Its presence in code is not successful support for that case.
- **Unverified**: evidence is missing. This is separate from implementation status
  and must not be read as a pass.

No current Adobe direction is fully or losslessly supported. A supported static
value does not imply support for its animation, and supported import does not
imply an equivalent export. Check the individual control and direction, not just
the feature-family name.

## Data loss and errors

Imports preserve the *convertible editable subset*, not every native record.
Exports use the current edited `.tsrct` document, not hidden original project
bytes. Unsupported effects, properties, layers, metadata and relationships may
not survive a round trip. A valid `.tsrct` may use features that a target format
cannot represent. Full application projects containing additional timelines or
application-level data are outside the standalone-document boundary.

Conversion warnings on stderr describe diagnosed omissions and approximations.
A successful exit can still mean data loss; warnings are not an exhaustive audit
of every field in the original file. Keep the source and inspect the result in
the destination application. Malformed or unsafe input, invalid media/archive
structure, invalid output paths and I/O failures can stop conversion instead of
producing a partial result. `--check` checks conversion/validation, not rendered
fidelity or future filesystem write success.

## Evidence

Implementation, editable-structure tests, native-application acceptance/control
readback, and independent render comparisons are different forms of evidence.
The detailed pages identify these separately: a source mapping, a named test,
an Adobe control measurement and a rendered comparison cannot substitute for
one another. Test references identify the applicable assertions; they do not
mean every opt-in test was run or passed in the current release.
A parser test or internal round trip does not prove that Adobe accepts or renders
an exported file correctly. Selected comparisons do not establish general
compatibility, and RGB video comparisons do not establish alpha or audio parity.
See the per-format evidence links; `make test` does not run Adobe render checks.
