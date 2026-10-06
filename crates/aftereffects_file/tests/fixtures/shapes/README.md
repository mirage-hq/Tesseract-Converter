# Native shape and gradient sources

Unchanged Adobe-native fixtures and independent AE-exported JSON sidecars from
MIT `forticheprod/py-aep` at commit
`e12a451c35bacd3f34a080090265f9370e66162b`. `provenance.json` pins source
paths, SHA-256 and byte counts.

- `shape_basic` exercises closed/open straight and cubic path storage through
  native `om-s/omks/shap/shph` and `lhd3/ldat` records.
- `gradient` exercises static shape Gradient Fill/Stroke `GCst/GCky/Utf8`
  color and alpha stop data, gradient axes, stroke geometry and dashes.
- `gradient_animated` supplements detection of multiple native gradient stop
  values. Upstream has no JSON sidecar for this source.
- `shape_misc` includes a native two-key path value used in source-local path
  animation assertions. Additional immutable native value-at-time references
  live under `../path-animation/`.

This is structural/editable evidence only: no Adobe UI inspection, new
fixture authoring, render comparison, alpha proof or fidelity scoring was
performed. Animated gradient-stop conversion remains unverified/limited. Path
morphing is omitted by this converter's requested scope with diagnostics;
older script-based results are historical supplements, not current editable
Path animation evidence.
Ordered paint ownership and shared modifier tests are supplemental structural
checks, not independent Adobe paint-order or rendering proof.

## Supplemental toggle and channel regressions

The same pinned py-aep `binary/property_chunks.py` and `parsers/property.py`
define group-level `tdgp/tdsb` enable bit0 in byte3 (collapse is bit1); this is
separate from leaf `tdbs` metadata. Synthetic headers test disabled source,
modifier, paint and group ownership. There is no independent disabled-operation
native/render oracle in this panel.

Native Star match names spell `Inner/Outer Roundess` (no second `n`), as pinned
in `synthesis/property.py`. The source-control regression checks static values;
relabelled native scalar key records from `property_rotation.aep` check Star
roundness and Gradient Stroke Miter Limit animation dispatch. These are
supplemental dispatch tests, not native Star/gradient-animation render evidence.

## Direction, traversal and shape blend semantics

- AE Shape Direction value 3 reverses explicit paths; it is preserved in the
  exported Rectangle/Ellipse/Star direction field. Values 1/2 are normal.
- Rectangle traversal starts at the upper-right/right-edge anchor in either
  direction, including nonzero roundness. Closed explicit paths retain their
  first anchor; open paths swap endpoints, with cubic handles reversed.
- Shape blend ordinals are separate from AEP layer-record blend bytes (e.g. 1
  Normal, 4 Multiply, 10 Screen, 15 Overlay). The importer maps them to existing
  FX modes for each paint and visual group; non-Normal tests are supplemental,
  not Adobe blend-render proof.

## libpag AE exporter reference

Pinned Tencent/libpag revision `380e5cddcdacd3d15be785cb2e2dccd2f0c8f002`:
- `exporter/src/export/data/Shape.cpp` reads `ADBE Vector Composite Order`;
  `exporter/src/export/stream/StreamValue.cpp` delegates to
  `exporter/src/utils/AEDataTypeConverter.cpp`, lines244–249: native 2 is Above,
  otherwise Below. The importer accepts only 1/2 and diagnoses unknown values.
- `AEDataTypeConverter.cpp`: 20,896 bytes, SHA-256
  `595ba24ae6a4739ddce706a49eb29d010b56ee69c3fa9ca3fabc8e002700db11`.

The fractional Star port and its dedicated license copy were removed at the
user's request. Star/Polygon outlines now use the existing integer construction:
point counts are clamped to 3..1000 and floored. Source values/keyframes remain
editable; fractional/animated counts are diagnosed during import. The fallback
checks do not claim fractional Star fidelity.

These are implementation-source and supplemental numeric/graph checks, not an
independent Adobe rendering comparison. The script-free converter retains a single intermediate Round radius as
static per-point geometry on a straight explicit contour when possible; its
animation and live source-edit linkage are not preserved. Other intermediate
Round stages across transformed scopes are omitted with diagnostics. Earlier Round-stage cases describe
historical behavior, not a current native FX or Adobe fidelity pass.
