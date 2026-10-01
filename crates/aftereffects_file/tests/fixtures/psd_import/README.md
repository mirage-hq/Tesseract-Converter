# PSD footage → PNG import checkpoint

This is an **import-only**, bounded implementation checkpoint, not completed
Adobe fidelity proof. PDF-compatible AI and FX → AEP were explicitly deferred.
See the [support ledger](../../../../../docs/after-effects-support.md#psd-footage-import)
for the format limits and the separate implementation/proof status.

## Native provenance and complete target registry

[`evidence.json`](evidence.json) pins the source/media hashes, both composition
IDs, Adobe control readback, exact executable test symbols, local reference
hashes, critical samples and missing evidence. Neither source target is omitted:

| Case | Composition | Native footprint | Executable import assertion |
|---|---:|---|---|
| `aep-psd-merged-v2-c2` | 2, `PSD merged` | footage item 1, 64×48 | `adapter::tests::psd::adobe_psd_merged_import_packages_png_and_preserves_editable_image` |
| `aep-psd-layers-v2-c16` | 16, `PSD selected layers` | Blue item 30 (PSD ID 202/index 1), 32×20; Red item 28 (ID 101/index 0), 20×16 | `adapter::tests::psd::adobe_psd_selected_layers_import_distinct_cropped_pngs_and_native_placement` |

`psd_sources_v2.aep` was independently authored through Adobe After Effects
26.5x89 scripting, not the converter's writer. Native source remains **24fps**,
2 seconds, 64×48. A fresh, owned Adobe process imported the same PSD first as
`FOOTAGE`, then as `COMP_CROPPED_LAYERS`; the second composition's duration and
FPS were set to 2s/24fps, and the merged footage was centered at `[32,24]`.
Adobe script-read controls, not manual UI inspection, establish the assertions.

`two_layers_v2.psd` is **specification-built media, not Photoshop-authored**:
PSD v1, RGB8, four raw planes, two records, opacity 255/normal blend, no masks or
resources. Record 0 is Red/ID 101, bounds `(top=8,left=10,bottom=24,right=30)`,
RGBA `[255,32,16,255]`; record 1 is Blue/ID 202, bounds `(20,24,40,56)`,
RGBA `[16,64,255,128]`. Adobe imports Blue above Red. The stored composite uses
that order with straight-alpha source-over; overlap pixel `(26,22)` is
`[135,48,136,255]`. Negative layer count establishes merged transparency.
This does not prove arbitrary Photoshop-produced documents are supported.

Tests freshly read the pinned AEP. Packaging tests relink only `alas.fullpath`
in a disposable copy to the **identical hash-pinned media**, without changing
native selectors, geometry or timing. Malformed-media variants are explicitly
supplementary tests, not independent native proof. Full-canvas selected-layer
output and PackBits RLE are CPU-tested only; the native selected-layer fixture
uses cropped bounds and raw compression.

## Selector contract

Adobe-authored `sspc` records use format `8BPS`. At offsets 188/192 they carry
big-endian PSD persistent layer ID and record index. Offset 196 is `0` for these
individual-layer sources; the merged source has both selectors `0xffffffff`
and offset 196 equal to `1`. Short, inconsistent or unknown selectors are
rejected rather than guessing by name or substituting a composite. This is an
AE26 observed subset, not a claim about every Adobe version/import option.

## Validation and remaining proof

Run from the standalone conv workspace:

```sh
make test-aftereffects-file filter=psd
make test-aftereffects-file filter=media
```

Independent `aerender` produced both full-duration **30fps MP4s**, using native
source frame indices 0–47 and the settings pinned in `evidence.json`. Both decode
to 60 frames at 64×48. Samples 0, 15, 30 and 59 inspect background, red, blue and
overlap regions. Expected RGB reference videos remain ignored local files,
**never Git assets**.

**Blocked/incomplete:** long-term Asset publication and fresh remote download
verification have not run (publication authorization pending). Fresh FX-render
comparison is **unmeasured**; no similarity score or visual pass is claimed.
Independent alpha proof is **unverified**: RGB MP4s cannot establish alpha.
The CPU tests establish PNG alpha bytes, not Adobe alpha equivalence.

An initial specification-built PSD had inconsistent stored composite/layer order.
It was discarded as an oracle; its hashes and reason are retained in
`evidence.json`, with original bytes kept locally. V2 is a new source revision,
not an overwritten or weakened reference.
