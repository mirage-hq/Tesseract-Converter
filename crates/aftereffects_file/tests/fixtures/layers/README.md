# Native layer-structure fixtures

All `.aep` files in this directory are unchanged source fixtures from
`forticheprod/py-aep` at commit
`e12a451c35bacd3f34a080090265f9370e66162b`, covered by `../LICENSE` (MIT).
`provenance.json` pins original repository paths, byte counts, SHA-256 hashes and
associated independent AE-exported JSON sidecars. Files were retrieved via
GitHub's LFS media endpoint at that exact commit and verified against the
committed LFS pointer's size and SHA-256 before staging. The unit test rechecks
every committed AEP hash.

These are **source-only structural cases**, not a render-reference manifest.
External media referenced by them is not staged, opened, uploaded or rendered;
current conversion creates non-rendering leaf placeholders. No Adobe UI or
native render was invoked for this import work. Do not use these files as
runtime templates or claim render/alpha/audio fidelity from these tests.

## Cases and exact composition identity

`expected.json` retains the source IDs and names of **every item and composition**
in each sidecar case. Tests select by ID, never by potentially ambiguous names.

| Source basename | Main purpose |
|---|---|
| `type.aep` | AV null, camera, text and shape envelopes; comp IDs 1, 42, 16, 29 |
| `lightType.aep` | Light subtype envelopes |
| `parametric_meshes.aep` | Six mesh layer envelopes in comp 1; fractional FPS/duration |
| `three_d_model_layer.aep` | Model layer 14 in comp 2; external file-source classification |
| `avlayer_flags.aep` | Layer switches across distinct single-feature compositions |
| `layer_misc.aep` | Names/labels/comments; parent comp 44, child 59 → parent 57 |
| `layer_timing.aep` | Start offset and positive/negative stretch; comps 1, 16, 30, 44 |
| `inPoint.aep`, `outPoint.aep` | Trim endpoints |
| `outPoint_clamp.aep` | Native precomp source 1 used in comps 13, 26, 39, 52, including stretch and offset |
| `track_matte_yes.aep` | Comp 1: layer 17 uses explicit matte layer 15 |
| `folder.aep` | Nested folder ancestry, compositions and file sources |
| `complex_comp.aep` | Additional pinned imported-source structure; no companion AE JSON at this ref |

Repeated/nested/cyclic precomp stress graphs in tests are explicitly marked
**synthetic supplementary tests** derived from the native precomp case. They are
not independent new Adobe render or semantic-fidelity cases. Native
`outPoint_clamp.aep` independently demonstrates the source comp reused from four
different containing compositions, not all synthetic graph shapes.

## Independent expectation projection

The full original sidecars total about 11 MB, mostly properties outside this
milestone. `expected.json` is a deterministic field projection of those sidecars,
**not a dump of the Rust/Python AEP parser's output**:

1. Keep each sidecar's `items` array in order.
2. For items, retain present `id`, `name`, `itemType`, `parentFolderId`, `width`,
   `height`, `duration`, `frameRate`, `pixelAspect`, `displayStartTime`; retain
   `mainSource.sourceType` as `sourceType` when provided.
3. Keep each composition's layer array in order; retain IDs, name, `layerType`,
   `matchName`, `sourceId`, `parentIndex`, `startTime`, `inPoint`, `outPoint`,
   `stretch`, label, enabled/audio/effects/solo/guide/null/adjustment/3D/shy/locked/
   collapse/motion-blur/frame-blending/per-character/environment/transparency
   switches, matte/blend modes and auto-orient.
4. Resolve sidecar `parentIndex` through the **sidecar's own** 1-based layer array
   to add `parentId` (zero when absent). Do not infer it using our binary reader.
5. Wrap each source as `{file, items}`. No expected values are recomputed from
   `.aep` bytes. Original sidecar paths, hashes and sizes remain in provenance.

Some sidecars identify text/shape/model only as `Layer`; tests use their independent
`matchName` for those cases. Missing sidecar `sourceId` (notably the model case)
is not invented as zero. Tests assert only sidecar-provided values for that field.

### Retained mismatch: reverse timing

For `layer_timing.aep`, comp 30 / layer 43, raw stored start/in/out are
`0`, `0`, `1474560/24576 = 60` seconds with stretch `-1`. Raw conversion gives
composition endpoints `0`, `-60`. The immutable AE sidecar gives
`-0.00033333333333`, `-60.0003333333333`: one **1/3000-second** difference.
The test explicitly asserts and retains this discrepancy; it does not widen the
general comparison tolerance or replace the reference. See `AE-TIMING` in
[`docs/after-effects-support.md`](../../../../../docs/after-effects-support.md).
This case is not claimed to have exact AE boundary fidelity.
