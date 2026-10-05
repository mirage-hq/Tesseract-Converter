# Independently authored Group effect-domain controls

These public-safe controls were authored in Adobe AE **26.5x89** on 2026-10-03
through typed `headless_adobe.Client.run_jsx`, then rendered through
`Client.render_aep`. No converter-created project, cached donor, private project,
external media, direct Adobe transport or alternate runner authored the controls.
The trusted ES3 function-body recipes are retained beside the immutable AEPs;
run them only through the managed API with `output_kind="aep"`.

## Sources and rights

Authored for this repository under its MIT license. Content consists only of
new geometric stripes/solid colors and the public string `BOUNDARY CONTROL` in
explicit native `ArialMT`; fonts are referenced, not embedded. No customer
content/assets or personal labels. Byte-level UTF-8/UTF-16LE/UTF-16BE scans found
no `/Users/`, `/Volumes/`, Windows user paths, author account name or private task
labels. Native projects contain generated solids/precompositions/Text only and
have no external media dependencies. Normal Adobe GUIDs/timestamps are retained.

| Source | SHA-256 | Native targets |
|---|---|---|
| `inline_add_mask.aep` | `a5e19d66f4e66891f3e9c40d2c06329e04dfcebdf9c2d608bce5d4b3d9e10422` | `mask_wide` 103; `mask_crop` 217 |
| `logical_bulge.aep` | `3d32c8076c57d70f8b72c871353fd19ccf2b10145f2e4697c780db6ed9c3435b` | `logical_wide` 230; `logical_crop` 471 |

Authoring recipe hashes:
- `inline-mask-crop-v1.jsx`: `7aa2bf5f8000d11cda843b40137cc0a2aff2886717236bba174a4517ffa01325`.
- `bulge-logical-plane-v1.jsx`: `f0e93ec7f9d214008b6ca8c57abc4907af72bd0305f49939656d7c1ae5fac986`.

Requests: `aep49-inline-mask-independent-crop-v2` and
`aep49-bulge-logical-input-v1`. Successful authoring/render calls verified cleanup
and fresh native READY. The first mask authoring attempt failed because AE rejects
assigning an empty Shape vertex array; its failure returned verified READY. The
corrected recipe and new request above were used, not an automatic replay.

## Native controls and measured geometric support

All native compositions retain square pixels, **30fps**, and full **one second**.
No source bytes/FPS were changed. Each independently rendered RGBA MOV has 30
frames; decoded RGBA hashes are constant within each static control, so the
reported frame differences apply to all 30 frames. No source/video is relabeled,
resampled, substituted or committed as media.

### Static inline rounded hard Add mask

4096x3072 output; logical mask 1920x1080 with radius76, zero feather/expansion,
uninverted full-opacity Add. Boundary-crossing stripes and Arial Text are visible.
The wide source is4096x3072 at(-1024,-1024); cropped source is1920x1080 at(0,0).
The same native mask path and source-space translation are retained.

Both outputs have alpha support exactly x1024..2943, y1024..2103: **zero pixels
outside the mask's control hull**. Wide/crop renders differ in74 RGBA bytes, max
**1/255**;25 alpha pixels differ by1/255. This establishes finite support, **not
pixel-exact equivalence**.

Native RGBA MOV SHA-256:
- wide: `98f9535047d87ddf7e4fad23bc59d06ad810e4f39867858cfc3934fb1319e3c8`.
- crop: `518d88d2f9804085719c3271de71c5a27ea78cdbd6c596586142e879ec86441a`.

### Fixed logical-plane Bulge

1920x1080 output and logical input plane. Native Bulge radii(1632,1026), logical
center(960,518.4), height0.08, pinningfalse. The wide source is6144x4096 at
(-2048,-1536); cropped source is1920x1080 at(0,0). A native hard input mask in the
wide control supplies the same declared input clip as the cropped plane. Bulge
pixel controls remain fixed; only center coordinates subtract the source origin.
Output placement(400,267) exposes expanded effect boundaries.

The two outputs differ in62 RGBA bytes, max **1/255**;10 alpha pixels differ by
1/255. Both nonzero-alpha supports begin at x379/y254: native Bulge output extends
21px left and13px above the input rectangle. **Input crop and effect output reach
are separate.** Outside the Bulge ellipse the operator is identity; the converter
uses the logical rectangle plus ellipse control hull for conservative output
support, not guessed glyph bounds or blur padding.

Native RGBA MOV SHA-256:
- wide: `53791a5a6cf1c32f6e89faa7a6abc87bf6a06932180971c955cff9ab113a5792`.
- crop: `4f45b1d17a9a3480fe63cce32c0e32830153608a6bdbe645c53f40caecbb14f8`.

## Executable checks and proof boundaries

`export_document::hierarchy::root_viewport_tests` retains:
- `native_control_fixtures_pin_the_declared_group_effect_planes`: immutable source
  hashes, target/source identity, native logical Bulge pixel controls and mask
  presence. This reads native sources; it does not exercise our writer.
- `inline_hard_add_mask_encloses_nested_text_without_dropping_root_or_sibling`:
  source-derived editable regression, exact finite hull and retained root/Text/
  vector sibling. Failed before the fix (job1025); passed after (job1029).
- `inline_mask_support_refuses_soft_or_spatial_text_input`.
- `group_bulge_keeps_declared_logical_plane_and_all_editable_children`:1920x1080
  and640x360 root variants, native source dimensions and editable native controls.
- `known_bounds_group_bulge_uses_root_plane_even_inside_a_smaller_parent_capture`.
- `group_bulge_normalization_preserves_logical_point_keys_across_capture_origin`:
  affine translation of existing supported native Point base/Linear keys.
- `logical_bulge_source_rejects_unproved_spatial_text_inputs_and_bypass`.

These supplementary generated-output assertions are not independent rendered
converter-fidelity comparisons. The independently authored controls prove the
bounded native input/crop profiles above; they do not prove arbitrary Bulge
parameters, animated crop support, pre-clip spatial Text effects, Drop Shadow,
font substitution, full title fidelity or broad corpus safety.

Import implementation/proof is unchanged and was not freshly measured by this
export repair. Export structural checks and independent native crop controls ran;
fresh converter-vs-native RGB/alpha fidelity remains **unmeasured** here. Reference
MP4 generation, long-term Asset publication/fresh download and hash verification
remain **incomplete**. No feature is marked a full bidirectional fidelity pass,
and the 1-byte native crop differences are not hidden by changing a threshold.
