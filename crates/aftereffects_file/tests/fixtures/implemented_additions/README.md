# Independently Adobe-authored implemented-feature sources

Eight native AEPs / 62 compositions were authored in Adobe After Effects 26.5x89.
These complement the original 62-source / 415-composition corpus; they do not
replace or rewrite any original source or reference.

| Native source | Compositions | Canvas | Controls |
|---|---:|---|---|
| `native_static_paints_and_geometry.aep` | 8 | 640×360 | Rectangle Fill/Stroke joins, Ellipse, Polygon, Star, static cubic Path |
| `native_transform_keys.aep` | 7 | 960×540 | Anchor, Position, Scale, Rotation, Opacity, Rectangle Size/Roundness; Hold/Linear/Bezier and spatial tangents |
| `native_boolean_operations.aep` | 4 | 640×360 | Union, Subtract, Intersect, Exclude with two operands and one Fill |
| `native_layer_ranges_and_order.aep` | 2 | 720×1280 | Leaf in/out ranges; mixed Solid/Rect/Shape/Boolean layer order |
| `native_parametric_key_channels.aep` | 14 | 640×360 | Isolated Ellipse size/position and Star/Polygon position, points, rotation, radii and roundness key channels |
| `native_export_edit_controls.aep` | 8 | 640×360 | Explicit source/geometry edits, signed/nonuniform scale, paint/layer opacity, combined 2D Transform and separated position keys |
| `native_boolean_operand_structures.aep` | 10 | 640×360 | Path/Rect and nested Boolean operands, all four operations, subtraction order |
| `native_clock_and_3d_controls.aep` | 9 | 640×360 | One supporting animated source, four Time Remap controls, four static/keyed Anchor Z/Scale Z omission inputs |

The supporting clock composition is not a separate supported feature. Anchor Z
and Scale Z are diagnostic inputs, not new destination-engine capabilities.
Converter-only behaviors (filesystem publication, preflight limits, and FX wrapper
normalization) are identified separately in `native_source_coverage.json`; an AEP
alone cannot establish those behaviors. This inventory does not claim conversion
or visual fidelity.

## Fixture inventory

- `native_source_coverage.json`: native source/composition associations for the
  implementation inventory, with operational and unsupported boundaries explicit.
- [`../aep_video_references.json`](../aep_video_references.json): source hashes,
  composition IDs/settings and reference-video publication records.

All sources are 24fps and require no external footage or fonts. The AEPs can be
opened directly in Adobe; no authoring script or sidecar is required. Temporary
JSX, script copies, readbacks and creation logs are not included as deliverables.
Their removal does not regenerate or modify the AEPs or reference videos.

The authoring/publication workflow is documented in the
[After Effects support ledger](../../../../../docs/after-effects-support.md).
The retained inventory is not an independent editable-value readback or a
successful conversion result.

## Expected MP4 references

Composition-by-composition Asset records live in
[`../aep_video_references.json`](../aep_video_references.json). Adobe renders
scratch copies at **30fps output**, full duration and original canvas; source FPS
is not altered. MP4s are published with `long_term` retention and fresh-download
SHA verification. Local video/contact files are ignored under
`../render/.expected-30fps/`; videos must never be committed to Git.

Publication integrity (format/decode/hash checks and sampled source-content
inspection) is not a converter test or AEP/FX render comparison. No tests,
builds, lint, converter validation, FX rendering or scoring were run during
this source work. The existing 20 failed tests are untouched. These new inputs
have not been exercised through the converter; nonempty FX→AEP output still has
no independent Adobe acceptance/render proof. RGB MP4s do not prove alpha or
audio fidelity.

The first 21 references finished uploading before the request to pause uploads.
Further uploads were paused until all eight AEPs had been saved. Only then was
the remaining 41-reference batch started and completed. All 62 new references
are published, bringing the combined corpus to 70 AEPs / 477 references.
Published Assets are not replaced.
