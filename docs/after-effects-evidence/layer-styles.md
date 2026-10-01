# AE Layer Styles: implementation and independent evidence

Scope: all nine native Layer Style identities representable by existing FX
`LayerEffect` values, in both directions. The user approved implementation before
native fixture/reference work and accepts documented AE/FX approximations. This
is **not** an exact-render or complete-animation claim. The authoritative
[approximation ledger](../after-effects-support.md#layer-styles--current-implementation-first-checkpoint)
includes Pattern Overlay's diagnosed omission and every shared normalization.

## Independent import source

- Source: [`styles_static_adobe.aep`](../../crates/aftereffects_file/tests/fixtures/layer_styles/styles_static_adobe.aep),
  635,693 bytes, SHA-256 `0160adcfd09c70f4f13b7eedbb539b57e855312fbb0c7b2b44513aef8f418273`.
- Authored independently in Adobe After Effects **26.5x89 / build 89**. Each style
  was applied through the real Layer → Layer Styles menu in an exclusively owned
  new GUI session, then nondefault controls were set and read through Adobe's
  scripting API. No converter-generated project was used as this source.
- [`styles_static_adobe.readback.json`](../../crates/aftereffects_file/tests/fixtures/layer_styles/styles_static_adobe.readback.json)
  pins enabled native identities, all explicitly authored values, composition and
  layer IDs, application/build and the executed authoring-wrapper hash. The
  machine-specific session wrapper is not shipped; the values and provenance are.
- Each composition is **320×180, square pixels, 2 seconds, 24fps**, containing one
  96×64 blue solid. Native bytes/FPS were not changed to manufacture references.
- References were rendered independently by `aerender`, source frames 0–47,
  `Use this frame rate: 30; Quality: Best; Resolution: Full`, and
  `H.264 - Match Render Settings - 40 Mbps`. All nine outputs are **60 frames at
  30fps**, fully decoded and inspected at frames **0, 30, 59**. The static samples
  show the intended style rather than a blank/offline/error slate.
- All nine MP4s were published with **long_term** retention and freshly downloaded
  with byte-count and SHA verification. Videos remain ignored local files and
  Assets, never Git. Exact video hashes/metadata are in
  [`aep_video_references.json`](../../crates/aftereffects_file/tests/fixtures/aep_video_references.json).

| Style / native composition ID | Case ID suffix | Long-term reference Asset |
| --- | --- | --- |
| Drop Shadow / 1 | `c1` | `bNFMuFOWcwVv6fFwF4c7_vid` |
| Inner Shadow / 16 | `c16` | `0nodZNzL3Y8WFhAdhoKU_vid` |
| Outer Glow / 30 | `c30` | `x7ZL66kQwfk9oX1SXG3o_vid` |
| Inner Glow / 44 | `c44` | `yhmbXbm89u89WKjkP5sy_vid` |
| Bevel & Emboss / 58 | `c58` | `4M0YJHKv9VcdpI8LTdoi_vid` |
| Satin / 72 | `c72` | `TTyJRBEybmw55ikf4zGT_vid` |
| Color Overlay / 86 | `c86` | `Lqcj84koZZrysuqjbZ9C_vid` |
| Gradient Overlay / 100 | `c100` | `deLzWbqPbm5nR3GpPiZn_vid` |
| Stroke / 114 | `c114` | `L6mOxQOugBDYHtKRbLy9_vid` |

The full case prefix is `aep-layer-styles-styles-static-adobe-`. All nine targets
have concrete editable assertions registered in
[`aep_feature_cases.json`](../../crates/aftereffects_file/tests/fixtures/aep_feature_cases.json).
There are no unregistered supporting compositions in this source. Registry
`UNRUN`/`unmeasured` fields are historical schema placeholders: actual task-local
execution below is separate, not inferred from registration or publication.

## Executable editable assertions

`structure_document::tests::native_layer_styles` pins the source hash/byte count,
freshly imports the selected composition, preserves the owner, requires exactly
one typed native editable FX style and excludes generated scripts. Its nine
feature-specific tests assert:

- `adobe_drop_shadow_imports_nondefault_typed_controls`: RGB/opacity, 135°/12px
  offset, 9px Size and 20% Spread → FX sigma/dilation formulas.
- `adobe_inner_shadow_imports_nondefault_typed_controls`: RGB/opacity,
  145°/8px offset, size12 and choke0.15.
- `adobe_outer_glow_imports_nondefault_typed_controls`: cyan/alpha0.85,
  size14, spread0.25, range0.75.
- `adobe_inner_glow_imports_nondefault_typed_controls`: color/alpha0.7,
  size12, choke0.1, range0.7 and Center source.
- `adobe_bevel_emboss_imports_nondefault_typed_controls`: Inner Bevel,
  depth1.6, size10, soften2, angle120/altitude35 and independent highlight/shadow RGBA.
- `adobe_satin_imports_nondefault_typed_controls`: color/alpha0.55,
  25°/13px offset, size8 and invert=false.
- `adobe_color_overlay_imports_as_constant_gradient_overlay`: two equal RGBA
  stops, original opacity carried in alpha0.65, editable native FX gradient payload.
- `adobe_gradient_overlay_imports_native_stops_size_angle_and_offset`: native
  white/black defaults, opacity0.85, 35°/80% local-axis approximation centered on
  **[10,-5]**, not zero. This exposed the native Point-descriptor reader bug.
- `adobe_stroke_imports_nondefault_typed_position`: blue/alpha0.9, width8, Inside.

`generated_layer_style_tdsb_flags_match_the_native_adobe_fixture` also compares
fresh generated enabled/disabled groups with exact native flag values. Our
reader's permissive enable-bit test alone had missed the native writer defect.

These Layer Styles tests are ordinary, non-ignored CPU tests and no longer
require a Cargo feature. To select them without running the unrelated ignored
proof backlog:

```sh
make -C opensource/conv test-aftereffects-file filter=layer_styles
```

Before the user's stop-testing instruction, the scoped panel passed **22 tests**
(928 filtered out), formatting passed, and the native gradient grammar regression
passed separately. One import regression was subsequently strengthened with Size
keys starting at zero and Spread20; that final test edit has **not been rerun**.
No further tests, builds or final export regeneration were performed after the
stop instruction. Earlier normal-suite/check/Clippy results are checkpoints, not
validation of the final tree. Source publication itself is not a test pass.

## Export: explicit edited FX inputs and Adobe proof

[`layer_styles/`](../../crates/aftereffects_file/tests/fixtures/layer_styles/)
contains a separate `<Style>.edited.fx.json` input for each style. These specify
current editable FX values, not hidden original AEP data: colors, alpha, sizes,
offsets, direction, overlay colors/opacity and Stroke position differ from the
native source. Each was freshly exported from a validated `.tsrct` archive.

Adobe independently opened and read back **seven** exported styles after the
native enable-flag repair: Drop Shadow, Inner Shadow, Outer Glow, Inner Glow,
Bevel & Emboss, Satin and Color Overlay. Changed numeric/color values and enabled
identities were returned by Adobe, not by our reader. For example, edited Outer
Glow read back color `[0.8,0.1,0.9]`, opacity65, size18, spread40 and range60;
edited Bevel read back depth220, Down, size12, soften3, angle75 and altitude50.

At the first export checkpoint, Gradient Overlay's generated gradient payload
caused Adobe open/readback to time out. Its shared gradient serialization was
repaired against independent native grammar and its targeted CPU test passed;
Adobe acceptance after that repair remains unverified. The original failure is
not relabeled a pass. Stroke's export readback and additional export rendering
were not reached. A subsequently active Adobe GUI session was not owned by this
task; it was not attached to, closed or killed. Additional Adobe operations remain
blocked while that session's ownership is unknown.

Native control readback is **not** a cross-renderer visual score. The source
reference is the original native setting, not an oracle for intentionally edited
export values. All nine fresh-import RGB comparisons completed before the stop
instruction (540 native frames; 549 comparator samples including endpoints);
descriptive per-frame results are preserved in
[`layer-styles-results.json`](layer-styles-results.json), not a fidelity pass.
No edited-export MP4, alpha proof, manual inspection of every Adobe UI control,
animated-style native reference or arbitrary multi-style ordering proof is claimed.
The seven successful export readbacks are historical, hash-pinned artifacts;
final-output hash equivalence was not rerun and is not claimed.

## Defects found rather than hidden by round trips

1. Enabled native style/master groups use `tdsb=1`; generated `tdsb=3` had been
   accepted by our reader but produced **zero enabled styles in Adobe**. Fixed
   to match independently authored native flags; Adobe then returned the edited
   controls for seven styles.
2. Native Gradient Offset has the two-component Point descriptor's integer
   marker, despite continuous coordinate values. Generic numeric decoding rejected
   it and incorrectly fell back to zero. The source-based nonzero-offset test
   stayed failing until the decoder used the proper Point path.
3. Native gradient mode ordinals include Angle/Conic3 and Reflected4. These now
   retain the existing FX modes instead of falsely reporting no native equivalent.
4. Shared gradient plist grammar and zero-static-spread Drop Shadow blur animation
   were repaired. Coupled spread/blur tracks retain static values with diagnostics;
   affine owner clocks rebase keys, while unsupported nonlinear style timing keeps
   the owner/siblings and static style. Read the execution limits above.

## Limits and unmeasured work

- Pattern Overlay is genuinely unsupported by the current FX style representation;
  it is diagnosed and omitted while retaining owner/siblings.
- Color/vector/coupled-polar, stop/endpoint and enum animations remain outside the
  implemented scalar subset; some are unfinished converter mappings, not absent
  FX capabilities. No native animated-style proof was produced.
- Kernels, bounds-dependent gradient geometry, contour/noise/texture/global-light
  semantics, non-Normal/Multiply/Screen blends, duplicate/interleaved stacks and
  finite generated-precomposition effect expansion remain approximations.
- Existing shader/runtime limitations are not repaired by changing FX or the
  renderer. Source MP4 RGB cannot establish alpha or exact compositing parity.
- No broad local GPU suite, full opt-in Adobe corpus, CI change or required-check
  change was performed. Missing visual measurements remain **unmeasured**.
