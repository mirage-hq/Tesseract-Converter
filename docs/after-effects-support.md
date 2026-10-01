# After Effects ↔ editable FX: current support and limitation ledger

This is the **current feature × direction** index, not a claim of general
conversion or fidelity. Import selects one composition; export creates a **new**
AEP from the edited FX document, not a patch or replay of source bytes. `--check`
can succeed with diagnosed omissions. An empty output, a placeholder, our own
reader accepting our writer, an encoded video, or an RGB score is not proof of
editable Adobe semantics. Native Shape/Mask Path keys have the bounded
Linear/Hold/zero-speed-Bezier mapping described below in both directions;
compound Shape Path export splits authored-order contours into independent
native tracks, including held births/disappearances via native one-vertex paths. Unsupported topology/easing/clocks are diagnosed and Shape raster
fidelity remains below the strict gate. No import-generated `JsScript`, AE expression runtime,
per-frame flattening, or hidden source-byte restoration is implemented.

## Adjacent collected footage — inspection and import path correction

**Direction: import/inspection only; export unchanged.** When the authored local
path and its native alias relocation are missing, resolve the exact adjacent
`(Footage)/<native project folder ancestry>/<authored basename>` path. A native
folder-alias PNG sequence adds each native frame filename below that directory.
No recursive search, cross-platform path guessing, implicit transcode, schema or
renderer change is introduced. Existing authored/alias files and operational I/O
errors retain priority. Media-map hashes bind to the selected collected original;
linked import freshness retains every missing higher-priority path.

Unsafe or cyclic folder ancestry, conflicting distinct sources targeting one
collected path, and symlinks escaping the adjacent collection are not selected.
File-alias image-sequence collection layouts remain unsupported by this fallback;
existing authored/native alias resolution is unchanged. Repeated item identities
for the same authored source may share a collected file. The fallback diagnoses
its inferred identity: the absent original cannot be hash-verified, and renamed
or differently organized collected folders are not guessed. Visual error from a
wrongly substituted collected file is unbounded. Human inspection uses
`PATH UNRESOLVED`; JSON keeps `missing` and includes the resolution reason.
Successfully resolved unsupported video is `requires_transcode`, and ordinary
Check/Write now reject it instead of omitting it as unavailable footage.

**Evidence:** `adapter::media::tests` has synthetic resolver/precedence, conflict,
sequence and path-safety assertions. The native-derived
`adapter::tests::collected::collected_video_inspection_check_write_and_media_map_agree`
asserts inspection, blocked Check/Write, source-bound replacement and packaged
editable Video content; codec-mutated MOV bytes are admission-only evidence, not
QTRLE decoding proof. Local unchanged `Play_OFintro.aep` (SHA-256
`1876484cd2b9870d0b1ad0c4a511a284db5d7d7117680542f2a77a02283d3fca`,
composition `705`) reproduced nine falsely unresolved references before the fix.
The source stays private/local, not a public fixture. Fresh post-fix text/JSON
inspection of composition 705 resolved all nine references: seven supported and
two QTRLE MOVs (`Tunnel-Circle.mov`, `Tunnel-Diamond.mov`) requiring preparation.
Check rejected the reached unsupported video before publication; source AEP and
all seven distinct audio/video files retained their SHA-256 hashes. Resolver,
native-derived packaging/media-map and CLI presentation tests passed. These are
path/admission/packaging results, not decoding or render evidence. Adobe readback/
render, alpha/audio fidelity and original-to-collected media identity remain
unverified. No new export mapping or proof claim.

## Explicit video admission and prepared-media import

This import-only media workflow supersedes historical warning-and-omit handling
for used unsupported video. Check and Write must reject incompatible reached
video before publication, including linked pictures; unrelated missing-file,
still and effect approximation policies remain distinct. Native reachability
includes disabled/off-range and nested sources before FX omission. Inspection
reports failures using read-only libavformat metadata inspection, without decoding,
transcoding or launching FFmpeg processes. A source-bound `--media-map` substitutes
only exact resolved originals and reruns admission; original files are unchanged.

The separate explicit transcoder processes one media file into one output file,
with library and external-command backends; it never reads this project's other
assets or generates a media map. New inspection admission covers audio/video only,
not image-specific descriptor validation. Ordinary image import remains unchanged.
Opaque SDR H.264, guarded ProRes4444 alpha and standalone PCM audio preparation
have defined preservation checks, not a general decoder/fidelity guarantee.
Unsupported Flash rendering and unverified timing/color/layout profiles remain
blocked with reasons. A generated alpha file is not proof of Premiere import support. No automatic import-time encoding, new FX
schema, renderer feature or Adobe exporter behavior is introduced. See the
[complete workflow and limitations](media-preparation.md).

**Evidence:** shared map identity tests and AE/Premiere import/CLI regressions are
structural/software-contract evidence. Codec-mutated `stsd` tests do not establish
real QTRLE decoding. Synthetic SWF tags distinguish classification paths, not
Adobe raster fidelity. Independent Adobe readback/render, alpha/color/audio
comparisons and Asset publication for this workflow are **unrun/unmeasured**.
Generated backend smoke evidence is separate and must not be promoted to Adobe
proof. **Export:** unchanged, no new FX → AE proof or added supported mappings.

**Hybrid implementation update:** ordinary Premiere CLI export now automatically
packages required editable AEP root scopes, and ordinary import resolves linked
occurrences using the actual format converters.
Parallel assessment/encoded-observation/source-wire proof machinery was removed.
Standalone library converters remain available. Current validation status, both directions,
remaining scope restrictions and missing independent Adobe proof are recorded in
the [hybrid implementation ledger](hybrid-adobe-export.md).

**Evidence notation:** S = checked-in structural assertion (not necessarily
executed in this checkout); A = independently opened/script-read controls;
R = independently measured RGB frames. “Unrun” means no execution receipt;
“unmeasured” means no score; a measured mismatch is **not a pass**. Adobe
script readback is **not manual UI inspection**. Unless a case below explicitly
says otherwise, independent Adobe open/control readback, alpha, audio, and RGB
fidelity are **unverified** for that feature/direction. For exact fixture IDs,
source hashes, receipts, test symbols and historical execution state, consult
[case results](after-effects-test-results.md), [Effects controls](formats/after-effects-effects.md),
[Adjustment evidence](../crates/aftereffects_file/tests/fixtures/adjustment/README.md),
[Layer Styles evidence](after-effects-evidence/layer-styles.md), and the
pinned fixture provenance. This index does
not promote historical `UNRUN` cases or supersede recorded failures.

## Modern matte selector and still-image visibility — import-only correction

The [bounded import correction](#mattes-still-lifetimes-frame-fades-and-still-geometry2--bounded-import-correction)
below states the same two rules with other synthetic evidence.

| Feature | Native semantics | Replacement | Impact and limits |
|---|---|---|---|
| Zero matte selector | A 164-byte layer record stores the selected matte layer ID. Zero means no matte layer, even when the record keeps its last Alpha/Luma mode. Only the 160-byte legacy layout uses the preceding layer. | Zero now gives no matte. A nonzero ID still selects that layer, with the existing missing, self-reference and cycle checks. Camera normalization, Set Matte eligibility and the Adjustment matte note use the same rule. | Formerly the stored mode consumed the layer above. This hid visible layers and made false cycles that dropped genuine explicit mattes. |
| Still image before its layer start | A still image has no media clock. Its native parent span (`start + in × stretch` to `start + out × stretch`) is its visibility, including a negative source-time prefix. | Still-image content uses that parent span without an affine source clock. Occurrence Transform, effect and mask keys keep their clock. Video, image sequences, precompositions, solids, text and shapes are unchanged. | Formerly the content started at the layer start time. Fractional-millisecond endpoints are still rounded, with the existing Timing diagnostic. |

**Evidence (S, executed):** `compositing::tests::modern_zero_selector_on_a_provider_keeps_its_explicit_consumer_matte`,
`compositing::tests::synthetic_matte_modes_and_invalid_edges_are_explicit` (corrected; it asserted the old rule),
`compositing::tests::generated_camera_used_as_legacy_matte_provider_is_not_normalized_away` (now uses a real
160-byte record), `tests::media::still_image_keeps_its_native_parent_span_before_the_layer_start` and
`tests::media::video_with_the_same_clock_keeps_its_nonnegative_media_clock`. The tests patch public
`compositing/trackMatteType.aep` and `media/audioEnabled.aep` records to field states read from a private
source: zero selector with Alpha mode, and clock `11000/23976` start with `-11000/23976` in point. The three
changed-behavior tests fail with the old rule and pass with the new rule. Patched records are not independent
Adobe proof. A private-source structural probe (not committed) shows the same change on the original
composition. Adobe render, alpha and visual comparison are **unmeasured**. **Export:** unchanged.

## Authored Time Remap end values, one-key constants and the two-sided cycle — import-only correction

The authored remap stays inside the existing lifetime gate (native parent span, `Inactive` outside).

| Feature | Native semantics | Replacement | Impact and limits |
|---|---|---|---|
| Remap outside its keys | A keyed AE property holds its first and last values outside its keys. | Authored remap keys use `Hold` before and after. | Formerly `Inactive`: the layer was absent between its lifetime start and its first key. |
| One authored key | One key is a constant source time for the whole layer. | Two equal-value keys at the lifetime gate endpoints, `Hold` both sides. They share the gate's budget reservation and rollback. Zero keys still keep the affine clock. | Formerly rejected (`TimeRemap requires at least two keyframes`), with affine playback. The FX structure has two keys where AE has one; a diagnostic records this. |
| `loopIn() + loopOut() - value` | Both default `cycle` calls repeat all authored keys on their own side; `value` cancels the doubled keyed value. | **Approximation.** Exact text match after whitespace and one final `;`. Authored keys use `Loop` before and after, with a contextual Timing warning on each admitted layer. | Other syntax, arguments, comments or operand order stay diagnosed and keep the affine clock. FX starts the next cycle at the final authored key, so a frame exactly there can show the first key's source time where AE `loopOut()` returns the last. Native boundary fidelity is not established. |

**Evidence (S, executed):** `tests::authored_remap_holds_end_values_inside_its_lifetime_gate`,
`tests::one_authored_remap_key_is_constant_over_the_layer_lifetime`,
`tests::exact_two_sided_cycle_expression_loops_the_authored_keys` and
`tests::other_remap_expressions_stay_diagnosed_without_authored_keys`. The tests patch the native Time Remap of public
`layers/avlayer_flags.aep` (composition 125) to record states from a private source: a lifetime that begins before the first
key, one key at source 1.833333s with lifetime `55/24` to `98/24`s, and the cycle expression on keys at local -1s and 0s.
The first three fail with the earlier rule and pass with the new rule. The fourth guards the rejection, and only its wording
changed. The cycle test also asserts the contextual loop warning. A read-only evaluation with the FX runtime
`TimeRemapChain` (not committed) matched a computed native expectation at 23 of 24 samples of one positive-stretch case;
the miss is the loop's final-key instant. That expectation is computed, not Adobe output: fresh Adobe boundary proof is
**unrun**, and render and visual comparison are **unmeasured**. **Export:** unchanged; the exporter does not read import
extrapolation.

## Exact unit-stretch source clock rounding — import-only correction

The affine source clock maps the layer window to source time with two Linear keys. FX times are integer milliseconds.

| Feature | Native semantics | Replacement | Impact and limits |
|---|---|---|---|
| Exact unit stretch | Composition time is `start + source × stretch`. A stretch fraction whose numerator equals its nonzero denominator (for example `1/1` or `2/2`) plays the source at exactly 1×. | The importer keeps the two keys, the window, the rounded source start, the clipping at parent and source zero, the animation-budget checks and the diagnostics. The end key value is now the rounded source start plus the rounded window duration, so the source span equals the window span. | Formerly the importer rounded each source end on its own. With fractional-millisecond endpoints, a 1× clock could become a near-1× rate, for example 100/101× over a 101 ms window. FX playback treats that rate as a speed change: below 1×, a layer with frame blending requests a blended frame. The integer source start is still an approximation: a constant offset of at most 1 ms, with the existing Timing diagnostic. |
| Other stretches | Nonunit, near-unit (for example `1000001/1000000`) and reverse stretches have their own authored rates. | Unchanged: the importer still rounds both source ends on their own. There is no tolerance snap to 1×. | Rounding can still change these rates slightly. A reverse 1× stretch (`-1/1`) is not corrected. |
| Authored Time Remap, still images, Adjustment layers | These layers do not use the affine source clock. | Unchanged. A remap with zero authored keys keeps the affine clock, so this correction applies to it. | — |

**Evidence (S, executed):** `tests::authored_unit_stretch_keeps_an_exact_1x_source_clock_after_rounding` patches the
precomposition layer record of public `layers/outPoint_clamp.aep` (composition 13): start, in and out points in 0.1 ms units
and the stretch fraction. Five synthetic states cover `1/1`, an unreduced `2/2`, both rounding directions, a negative start
clipped at parent zero and a negative in point clipped at source zero. All five fail with the earlier rule, where only the
end key value differs, and pass with the new rule. Each case also checks the editable JSON readback.
`tests::nonunit_near_unit_and_reverse_stretches_keep_independently_rounded_source_keys` (`2/1`, `1000001/1000000`, `-1/1`)
passes with both rules. `tests::one_authored_remap_key_is_constant_over_the_layer_lifetime` asserted the earlier rule for its
zero-key affine clock: its end value changes from 3625 ms (1792/1791×) to 3624 ms (1×). Its authored gate, constant and budget
assertions do not change. A diagnostic copy of one private-source conversion (not committed) changed only this end key on a
frame-blended video layer. At one exact frame time, that isolated layer changed from black to nonblack with the FX renderer.
This change alone removed that black frame. It does not separate the rate change from the sub-millisecond phase change, and
it is not render fidelity: render, visual and audio comparison of a corrected conversion are **unmeasured**. **Export:**
unchanged.

## Unsupported Roto Brush — import-only best-effort fallback

| Role | Native semantics | Replacement | Impact and limits |
|---|---|---|---|
| Ordinary paint with an active Roto Brush (`ADBE Samurai`: effect enabled, layer effect switch on, layer visible) | Only the segmented foreground is painted; the rest is transparent. | Roto Brush has no FX mapping. The occurrence's source visuals are hidden, not deleted. The Group, Transform, masks, other effects, audio, children's parent transforms and siblings are kept. | Formerly the unsegmented full frame painted opaquely and covered lower layers. Now the isolated foreground and its occlusion are **lost**. This restores lower content; it is not Roto Brush support. |
| Track-matte provider whose sampled alpha needs that cutout: directly, through a nested precomposition, or through the provider's own track matte | The consumer is masked by the segmented alpha. | The consumer is left unmasked, with contextual TrackMatte diagnostics on the provider and consumer. The provider sample copy is hidden and unlinked. Only drawn content counts: an undrawn ordinary branch (eye off, guide, solo-hidden) inside the sample does not make it unavailable. The provider's own eye switch never stops matte sampling. | Formerly a direct provider passed raw full-frame alpha as if segmented. With hidden paint, a nested provider would have masked the consumer to nothing. Correct masking is **lost**. |
| Disabled effect, layer effect switch off, hidden layer, Source-stage Set Matte sample | AE renders raw pixels. | Unchanged. | — |

The decision uses only the native effect match name, its switches and the layer's paint or matte role. It never uses project, layer or source names or IDs.

**Not covered:** Roto Brush controls (view mode, invert, refine edge) are not decoded. Set Matte and Preserve Underlying Transparency do not check for omitted cutouts. Other keyers are unchanged. Keylight 906 has the separate bounded approximation below.

**Evidence (S, executed):** `tests::media::active_roto_brush_hides_only_its_unsegmented_painted_footage`,
`tests::media::matte_alpha_from_an_omitted_roto_brush_leaves_consumers_unmasked`,
`tests::media::unavailable_cutout_alpha_propagates_through_matte_chains` and
`tests::media::only_drawn_nested_cutouts_make_a_matte_sample_unavailable`. These are supplementary mutations of the native
`media/audioEnabled.aep` footage record, with a writer-generated effect envelope renamed to `ADBE Samurai`. They are not
Adobe-authored proof. They cover the parented child, plate sibling, disabled effect and switch, an ordinary matte that is
kept, nested and chained providers, an undrawn nested branch, and the provider eye switch. Each fails with its earlier
rule and passes with the new rule. A private-source structural probe (not committed) shows the same change on the
original composition. Adobe render, alpha and visual comparison are **unmeasured**. **Export:** unchanged.

## Keylight 906 bounded profile — import-only CustomShader approximation

This checkpoint is **AEP → editable FX only**. A native `Keylight 906` (Keylight 1.2)
instance in the supported static profile becomes one editable `customShader`
([`effects/keylight.rs`](../crates/aftereffects_file/src/effects/keylight.rs),
[`keylight.wgsl`](../crates/aftereffects_file/src/effects/keylight.wgsl)). The shader stays
on the owner Group at its native Effect Parade position. Its enable state is the effect
switch and the layer Effects switch. Owners, siblings, masks, transforms, mattes and clocks
do not change. No schema, renderer, runtime, `JsScript`, frame bake or media is added.

| Feature | Native semantics | Replacement | Impact and limits |
|---|---|---|---|
| Supported static profile | View Final Result, Soft Colour with the neutral Replace Colour, Source Alpha Normal, Unpremultiply off, neutral biases, Clip Black 0, no pre-blur, rollback, shrink/grow, softness or despot, mask Invert off, both colour corrections off, full source crops. | Editable `screen.colorR/G/B`, `screenGain`, `screenBalance` and `clipWhite` in native units. Group markers, buttons and controls of disabled features are inert and ignored. | Approximation; see the pixel model. Every Keylight 906 control has one classification in the profile table. |
| Sparse plugin defaults | AE can store an instance with an empty `parT` and only its authored controls. The project `EfdG` and other instances hold declarations, not this instance's values. | Per control: explicit record, else this instance's declaration, else the recorded Keylight 906 profile default. Profile defaults apply only when the instance's own table is missing or readable. A diagnostic lists the controls that use profile defaults. Values are never copied from another instance, and `EfdG` is not read. | The profile records one plugin build's declarations. Another build with the same match name is not established. |
| Unsupported, malformed or ambiguous input | Another value, animation or an enabled expression on a render-relevant control; an undecodable or duplicated record or declaration, even when a valid explicit value exists; a duplicated declaration table or one with malformed names; an unknown control; a declaration type that differs from the profile; a Screen Colour without a unique dominant channel. | The instance is omitted with a contextual `AE-PROPERTIES` reason. The owner and other effects are retained, and no effect identity is consumed. | Animated Keylight is not converted, even when only one control moves. |
| Inside/Outside Mask selectors | Type-12 path controls. | Admitted only when absent, or declared with an all-zero default payload and no explicit record. | An explicit or undecodable selector omits the instance. |
| Unset type-12 path declarations (all effects) | An all-zero default payload selects no path (Adobe readback of `fill_isolated.aep`: Fill Mask 0). | The native decoder records the unset path and no longer reports "unsupported effect default kind" for such a row. | The numeric value stays undecoded, and a mapped effect still reports the control as unmapped. This also removes the line for an unset Fill Mask (`ADBE Fill-0001`) declaration. |
| Declaration table state (all effects) | An instance can store no `parT`, one, or a duplicated or malformed one. | The decoder records the state. A duplicated table is now reported as duplicated, not as missing. Other effects keep their explicit values and catalog fallback. | Only the Keylight profile rejects an unreadable table or declaration. |

**Pixel model.** For straight colour `C` and Screen Colour `S`, `p` is the strictly
dominant channel of `S`, and Screen Balance `b` weights the smaller (`b`) and the larger
(`1 − b`) other channel of `S`. The raw matte is `k0 = clamp(1 − gain·D(C)/D(S))`, Clip White
remaps it linearly to `k`, the screen colour is subtracted as `clamp(C − (1 − k0)·S, 0, k0)`,
and Soft Colour adds `(k − k0)` times the Rec. 601 luminance of `C`. The output is
`(A·Q, A·k)`, so incoming coverage never rises. A pixel with another dominant channel is
unchanged. Degenerate runtime parameters pass every pixel through. Red, green and blue
screens use the same model. These equations are implementation hypotheses from the Keylight
user guide (Screen Colour, Screen Balance, Clip, Replace Method and Source Alpha), not
Keylight's unpublished algorithm. The shader compares the renderer's display-referred SDR
working values directly. `CustomShader` has no colour-space input, so HDR and colour-managed
equivalence is unsupported and unverified.

**Evidence (S, executed):** 15 tests in `effects::tests::keylight` use fabricated generic
records through the real native decoder and structural import. They cover sparse defaults,
explicit and declared precedence with integer-flagged fractional sliders, per-occurrence
defaults, red/green/blue and tied screens, every required control, declared ranges,
duplicate, malformed, unknown, incompatibly declared and animated controls, mask selectors,
inert controls, unknown match names, renamed and reordered project items, parade order,
enable switches, identity commitment, the emitted uniform layout, and duplicated or malformed
declaration tables and declarations behind valid explicit values. Also:
`effects::keylight::tests::*` (profile table and balance rule),
`effects::native::tests::adobe_native_unset_mask_path_declaration_is_decoded` (Adobe-authored
`fill_isolated.aep` and its Adobe readback) and
`effects::native::tests::unreadable_declarations_keep_the_catalog_fallback_for_other_effects`.
`sparse_keylight_without_declarations_uses_plugin_defaults` failed before this change (Keylight
omitted as unmapped) and passes after it.
`unreadable_or_ambiguous_declarations_are_omitted_despite_explicit_values` failed before the
declaration guard (each of its five inputs was admitted) and passes after it. Local mutations of
the unset-path rule, the duplicate rule and identity commitment each failed these tests.

A private-source conversion (not committed) converted two real instances: one with a full
local declaration table keeps its authored Gain/Balance/Clip White values without rounding,
and one with an empty table gets 100/50/100. The exact emitted WGSL ran through the pinned
renderer preview on 72 flat RGBA patches with 11 parameter sets and an effect-free control.
It matched an independent float64 model of these equations within 2/255 RGB and 1/255 alpha.
Alpha never increased, zero alpha stayed transparent black, pixels with another primary or a
tie were unchanged, red and blue screens on channel-rotated patches matched the rotated green
result (alpha always, RGB without Soft Colour) and degenerate parameters passed through. This
is shader-execution evidence, not Adobe proof.

**Not established:** no Adobe-authored Keylight fixture, native Adobe render, alpha
reference, Asset or AE scoring exists for this feature. Exact matte, despill and Soft Colour
equality, edge resampling, HDR behaviour and native alpha are unverified. **Export:**
unchanged. The existing `CustomShader` diagnostic omits the shader, so FX → AEP Keylight
export and its Adobe proof are missing.

## Text selector control expressions — import-only correction

An enabled expression on a Range Selector field or a Text Animator property is lowered only when it is an exact affine function of at most one same-layer Slider curve. Nothing is executed or sampled. The result is a static value or the Slider's own keys, in the same layer clock (no rebasing).

| Feature | Native semantics | Replacement | Impact and limits |
|---|---|---|---|
| Slider references and sums | `effect(name)(1)` reads the Slider value, as does its quoted value parameter. Numbers, earlier `var` bindings, `+` and `-` combine values. | A static value, or the Slider keys with mapped values and temporal speeds. Influence, interpolation and key times stay. Index `1` resolves only `ADBE Slider Control-0001`, never the hidden `-0000` control or Compositing Options. The Scale, sibling Slider, Rectangle component and Source Text percent links use the same reference grammar and accept the same index. A same-effect alias does not: an index reference stays a non-candidate there. | Formerly the stored value was used, which AE does not show while the expression is on. Editing the Slider does not update the lowered value. |
| `linear(t, tMin, tMax, v1, v2)` | AE clamps `t` to the input range, then interpolates linearly. | Static bounds with `tMin < tMax` only. A static input is clamped exactly. A curve input needs Linear/Hold keys inside the range, so the clamp never applies and the map is exact. | A curve that leaves the range, a Bezier input (it can overshoot between keys), an animated bound or width, and other arities are rejected. The two-binding completion/width rig is admitted for any consistent binding and control names. |
| Same-text Range Selector alias | `text.animator(A).selector(S).start`, `end`, `offset`, `advanced.easeHigh` and `advanced.easeLow` read another selector's value. | The target's resolved value: its own lowered expression, or its authored values/keys when it has no enabled expression. A missing target field uses the importer's native default (End 100%, the others 0). Names must be unique over all animators and selectors, including disabled ones; placeholder names never match. | The target must be a percentage Range Selector. Absent or ambiguous names, Index units, other fields and cycles are rejected. At most 16 alias resolutions per property. |
| Vector animator properties | `var x = value[0]; var y = effect(name)(1); [x, y]` | A static vector when every component is static. `value[i]` reads the stored value of a static owner. | Animated components are rejected. |
| Rejected forms | Malformed or extra statements, unused or duplicate bindings, reserved names, comments, adjacent signs, legacy octal and non-finite numbers, absent or non-Slider controls (including pseudo-effect conditionals), sibling-layer references, Wiggly and Expression Selectors. Bounded work: nested `linear()`, an expression over 4096 bytes and more than eight bindings are rejected before evaluation, for the owner and for every aliased expression. | The stored/keyed value is used, with a contextual `control link not lowered (reason)` diagnostic. | Expression Selectors are still omitted, and their motion is approximated. The exception is the alternating `textIndex` form in the next section. A pseudo effect's sparse default is not guessed. |

The fully selected and fully deselected states are exact. Mid-transition character weights can still differ: the existing FX Ramp Up/Down shapes are linear, Ease High/Low apply only to Triangle, and Smoothness has no destination field. The lowered Ease values are kept as editable data.

**Evidence (S, executed):** `tests::text_control_links::renamed_control_rig_lowers_both_selectors_to_ordinary_editable_curves`,
`tests::text_control_links::unprovable_offset_links_keep_stored_values_with_a_reason`,
`tests::text_control_links::disabled_offset_expression_keeps_authored_keys_for_itself_and_its_alias`,
`tests::text_control_links::animated_span_lowers_start_but_rejects_its_linear_bound` and
`tests::text_control_links::animation_budget_denial_keeps_resolved_static_selector_values`. They mutate the native text layer of public
`text/import_selector_animation.aep` (composition 79): its keyed 0→100 curve becomes a Slider value and its Position storage gets an
expression. The Slider envelopes, expressions and names are synthetic, so this is not Adobe-authored proof. These five tests fail
with the earlier rule and pass with the new rule. `text::expression_links::tests` (seven grammar, linear(), alias and vector tests)
and `control_links::tests::numeric_slider_index_one_selects_only_the_value_parameter` cover the rejection rules.
`control_links::tests::slider_index_one_reaches_source_text_percent_and_default_named_sliders` pins the Source Text percent grammar
and a default-named Slider. `control_links::effect_alias::tests::non_reference_expressions_are_not_candidates` and
`…::chained_index_references_are_not_pure_aliases` pin the same-effect alias exclusion, directly and in a chain. The Rectangle
component path uses the tested Scale grammar and resolver; it has no separate index test.
`text::expression_links::tests::deeply_nested_linear_falls_back_without_exhausting_the_stack`,
`…::nested_linear_and_oversized_expressions_are_rejected`, `…::bindings_beyond_the_budget_are_rejected` and
`…::bounds_apply_to_aliased_expressions` cover the work bounds. Without them, the deep case overflowed the stack and aborted.
A private-source structural probe (not committed) shows ordinary offset keys −1→1 on both selectors of each occurrence, and the FX
runtime selector weights are zero for every character after completion. No other document field changes. Adobe render, visual
comparison and alpha are **unmeasured**. **Export:** unchanged.

## Alternating `textIndex` Expression Selector — import-only lowering

The converter expands an admitted animator before the shared text conversion. The original animator, the correction and all later
siblings then get their identifiers, values and tracks from the same path. Nothing is executed or sampled.

| Feature | Native semantics | Replacement | Impact and limits |
|---|---|---|---|
| Alternating sign | An enabled Expression Selector directly after the animator's only Range Selector, with Amount `if(textIndex%2 == 0){ selectorValue; }else{ -selectorValue; }`. `selectorValue` is the weight `w` of the selector above. `textIndex` is one-based, and Characters counts spaces. Even indices keep `w`; odd indices get `-w`. | The animator keeps Position `P` on its Range Selector, without the Expression Selector. A new editable animator, `Animator N alternating position`, follows it with Position `-2P`. Its Index/Square Add gates `[0,1]`, `[2,3]`, … select each odd one-based character. A copy of the Range Selector (same values and keys, Mode Intersect) multiplies them by `w`. Odd characters move `-P*w` and even ones `P*w`. A negative Amount cannot do this: FX skips weights at or below zero. | All controls are ordinary editable data, but the expression is not live. A later text-length, Range Selector or Position edit can break the alternation; reimport to regenerate it. Ramp Up Ease High/Low keep the linear approximation of the previous section. |
| Admitted profile | One static Source Text document in one paragraph and one character-style run. | 1–256 printable ASCII characters, spaces included, after the existing terminal-return normalization. The animator has only a static, finite 2D Position with zero Z, and exactly the selectors Range, Expression. The Range Selector is Add, Characters and not randomized, with a static Amount from 0 to 100%. It has no malformed field and no expression that the previous section does not lower. The Expression Selector has only its Amount and, optionally, Based On Characters. | Every other source keeps the omission, with `alternating textIndex Expression Selector not lowered (reason)`. This includes other modes and bases, randomized order, a negative, animated or over-100% Amount, keyed or scripted Position, other animator properties, Unicode, tabs, inner returns, styled runs and keyed or expression-driven Source Text. Any other expression text is not attempted. Subtract is a counterexample: for raw weight 0.25 the identity gives `+0.25P`, not `-0.75P`. |
| Bounds and atomicity | — | At most 256 characters and 130 expanded selectors per text (at 256 characters: one Range Selector, 128 gates and the copy), checked before allocation. Every selector visits every character, so the bound is 33,280 selector weights per frame. Identifiers are reserved before allocation. All tracks are admitted before any is committed. Each keyed Range Selector field must become a track on both copies. | A denied track, a keyed field without a destination track, or exhausted identifiers rejects the whole expansion. The unexpanded import and its diagnostics stay, with the reason. The bound limits work; it is not a latency measurement. The animation allowance has a quota only in tests. |

**Evidence (S, executed):** `tests::text_control_links::alternating_selector_moves_odd_characters_opposite_to_even_ones` fails
on the earlier rule (`500 ms: character 0 ('E') axis 1 moved 60 instead of -60`) and passes now. It evaluates the emitted
editable controls with a model of the FX selector contract at range start, middle and completion. The space in
`Editable Selector` (one-based index 9) moves like an odd letter. `…::alternating_correction_is_an_editable_animator_beside_untouched_siblings`
pins the editable structure, the cloned keys and the later sibling's target.
`…::alternation_follows_text_length_position_span_and_clock` uses the native `Animate Me` Source Text of public
`text/text_animator.aep` (even length, with a space), a nonzero X, Span 25 and a 0.25s layer start.
`…::alternation_outside_the_admitted_profile_keeps_the_omission` covers the importer fallback for a Subtract range, a keyed field
without a destination track, styled runs (`AVAWAY` of `text/text_ranges.aep`), the basis and the parity.
`…::denied_or_unaddressable_expansion_rolls_back_to_the_omission` covers the budget and identifier rollback.
`text::alternating::tests` covers the grammar, every text and animator admission rule and the bound. These tests mutate public `text/import_selector_animation.aep` (composition 79). Their expression, controls and names are
synthetic, so they are not Adobe-authored proof.

**Evidence (documentation and file inspection):** Adobe's *Animating text* help defines `selectorValue` as the input from the
selector above and the default Amount as `selectorValue * textIndex/textTotal`. Adobe's *variable font axes* help defines
`textIndex` as one-based. A read-only inspection of the 41 Expression Selectors in the AE 2026 text presets found only Based On
and Amount, and no Mode, while their Range Selectors store Mode. `Alternating Characters In.ffx` contains this exact expression.
This is not an Adobe execution receipt. Adobe render, native calibration, visual comparison and alpha are **unmeasured**.
**Export:** unchanged; export of the expanded animators is not verified.

## Compact Source Text font-table import repair

**Import only (reported AEP → FX task).** The reader accepts both Adobe's
nested CoolTypeFont name, which the writer now also emits (see *Source Text font
identity*), and the compact direct-name table written by earlier writer versions.
Unreadable/empty entries retain their original slots rather than shifting
later document font indices. Missing identities still use the existing diagnosed
fallback; font files are not discovered or bundled automatically. Export behavior
is unchanged and no new export/Adobe-acceptance claim is made.

The local reported `project.aep` (SHA-256
`92b3d645468f74168ecbb140b0ee45fe89d7d72e46bb2c71c96524b533909b28`,
composition **1**, 1080×1920, 30fps, approximately 21.8s) contains the compact
`Bungee-Regular` identity. Before this repair it became `sans/serif` in all
218 editable text layers. The source's compact payload matches the earlier
writer; it is **not independent Adobe-authored proof**. The private project and media
are not redistributed. Its 17 MOV and two WAV dependencies must accompany the
AEP in `media/`; missing external footage is not embedded in the AEP and cannot
be recovered from its project bytes.

`structure_document::text::tests::compact_source_text_font_table_preserves_editable_identity`
failed before the fix (`sans` instead of `Bungee`) and passed afterward.
`source_text_font_table_keeps_indices_across_unreadable_entries` checks mixed
native/compact entries and diagnosed holes. All 19 tests in
`structure_document::text::tests` passed, including the existing independent
native `text_ranges.aep` editable-font regression. The two new COS tests are
structural/synthetic regressions, not new native feature fixtures.

A fresh CLI import recovered `Bungee/Regular` for all 218 text layers, with no
font-identity fallback diagnostics. All 19 packaged media hashes matched the
supplied local files; document geometry, timing and media references were
unchanged by the repair. For local delivery, Bungee was explicitly embedded from
the user's original `.tsrct`, not discovered or substituted by the converter.
Six local FX preview samples (0.5, 2, 6, 10, 15 and 20 seconds) showed the cats;
the 216×384 diagnostic tiles are not a full-resolution fidelity gate.

The requested local Adobe/FX side-by-side is **blocked**, not a visual pass.
A separate AE **26.5x89** `aerender` process attempted the unchanged source copy,
composition 1, source frames 0–653, full resolution/Best, 30fps output and
`H.264 - Match Render Settings - 40 Mbps`. It returned zero but logged
`After Effects error: Error reading the text layer. Skipping the text layer.`
and produced **no MP4**. The interactive project was not reused; this is failed
Adobe reading/rendering evidence, not a successful open or editable-control proof.
The source writer's native text acceptance remains outside this import-only repair.

The fresh imported FX document, with the explicitly supplied font embedded, did
export locally: **720×1280, 30fps, 654 decoded frames / 21.8s**, 30,347,131 bytes,
SHA-256 `558e00ef1d4e82bfc0182e41166b21434a0f388e8e3b04e2489a2220959ecb6b`.
Decoded samples at the six times above show cat footage. An audio stream exists,
but audible fidelity was not measured. A prior 1080p attempt was terminated at a
15-minute supervisor limit and produced no deliverable; the completed 720p run
had no time limit. Videos and original media remain local/ignored, not published.
No Adobe reference was substituted with FX output. Independent native 30fps
reference/long-term Asset proof, RGB/alpha/audio comparisons, Adobe control readback
and full-project fidelity remain **missing or unverified/unmeasured**.

## Canonical windowed clocks — revision 67 migration

Video, Audio and Group retain their existing kinds and now carry required
windowed `playback`; Video and Audio retain independent `sourceRange` selection.
The visibility window does not replace the authored mapping or offset. This is
an FX schema migration, not new Adobe fidelity evidence.

| Direction / semantics | Implementation and limits | Current-checkout evidence |
| --- | --- | --- |
| Import: native trim/stretch and authored remap | Existing importers construct the canonical playback directly, retaining editable remap keys. No historical archive migration or additional layer kinds. | `native_precomp_trim_and_stretch_use_content_clock_not_active_range_offset` and `authored_time_remap_uses_parent_visibility_when_affine_source_is_negative` passed against the existing native sources. These are structural assertions, not new render comparisons. |
| Export: independent affine window/mapping/offset | Exact mapped endpoints are computed separately from source selection. Fractional-millisecond endpoints or mapped values outside the authored source selection are diagnosed rather than rounded or silently retimed. | Six hierarchy-clock and three media-clock unit tests passed. New Adobe open/control inspection and render comparison remain **unrun/unmeasured**. |
| Export: remap input offset | Native signed key times subtract the shifted window origin; occurrence visibility, key values and easing are retained. Existing guard-key, exact native key-time, source-domain and animated occurrence-transform restrictions still apply. Unsupported cases remain diagnosed omissions, not identity playback. | `remap_offsets_shift_native_key_times_without_changing_values_or_visibility` passed for positive and negative offsets. This is synthetic writer-plan evidence only; independent Adobe proof is **missing**. |
| Export: explicit Audio remap gain keys | A zero-offset two-key Audio remap that spans its window exactly keeps an affine native stretch record. Gain on an explicit remap samples the mapped source clock. The writer can only rebase occurrence-local Audio Levels keys, and that rebase applies the mapping again: a 750 ms source key moves from parent 1.5 s to 1.75 s. Thus multi-key gain on a canonical explicit remap omits its Audio owner with the `Time Remap cannot drive occurrence-owned Audio Levels` diagnostic. Static gain, ordinary Linear Audio keys and convertible siblings are retained. | `audio_export_rejects_explicit_remap_source_clock_gain_keys_and_retains_its_sibling` failed before the guard (owner kept, switch at 1.75 s) and passes after it. `audio_export_explicit_remap_keeps_trim_and_stretch_but_rejects_gain_keys` and `canonical_keyed_audio_keeps_affine_records_and_full_source_control_hull_guards` pin the boundary. Synthetic export evidence only; Adobe open/inspection and audible comparison are **unrun/unmeasured**. |
| Archive transport | Current-format windows, mappings, offsets and independent source selections survive `.tsrct` writing/reopening. The archive reader also accepts historical Video/Audio/Group `activeRange` clocks with positive exact ranges: video preserves affine trim/stretch, implicit audio stays at 1x, groups start at local zero, and explicit remap keys/extrapolation are retained. Canonical schema validation stays strict; conflicting clocks and unsupported legacy shapes still reject. Original archive and checkout bytes are preserved. | `tesseract_file::file_roundtrip`, `file_security`, and `legacy_timing` cover archive integrity, clock normalization, unknown-field preservation and rejection. This reader compatibility is not native render proof. |

Self-review corrected Group identity classification for keyed maps with a
nonzero `inputOffsetMs`, including the mask-flattening admission check. The
`identity_keys_with_input_offset_preserve_source_clock` regression fails without
the fix (the native remap disappears) and passes with it; all seven hierarchy-clock
unit tests passed at this checkpoint. This is export-plan evidence, not Adobe
open/inspection or render proof. Import implementation/proof is unchanged.

No native fixture bytes, pinned source hashes or independent reference media were
replaced for this migration. These checks do not establish pixel, alpha or audio
fidelity, nor coverage of every converter feature. Existing broader native-panel
and export evidence limitations below remain in force.

## Ordinary Intro import corrections — draft checkpoint

**Import corrections plus bounded Radial Wipe export (follow-up below).** The
other corrections are not new export mappings or a full-project bidirectional
fidelity claim. No FX schema/evaluator/renderer/editor changes, generated JS or
frame baking. Noise/Grain reproduction is explicitly excluded.

| Feature / original semantics | Editable replacement and limits | Executed evidence / remaining proof |
| --- | --- | --- |
| Nonzero native Path temporal speed | Normalize each Bezier handle as `x=influence/100`, `y=x*speed`; no duration/path-distance multiplier. Admit only finite handles in `[0,1]`; unsupported easing retains diagnosed initial outlines. | Independent Adobe readback: 5,520 mask-coordinate checks, maximum error `0.00010161850536860584` px (below float32 `1/4096` px bound). Pinned local regression passed; 5s capsule geometry visually restored. Not an export extension or broad Path fidelity claim. |
| Coupled Vector2 ease roundoff | Eight unit-scale floating-point epsilon tolerance preserves numerically equivalent Size/Anchor easing; genuinely distinct/nonfinite curves still reject. | Native Dark_Masker rectangle source assertion failed with exact comparison and passed with tolerance. |
| Animated Rect begins at zero | Nonnegative dimensions may start at zero if a later native key has positive width and height; static/all-zero, negative and invalid cases retain fallback. | Native Black_box source assertion failed with old positive-only guard and passed after fix. 10s outer size and inner black square visually restored. |
| Source-stage Set Matte / copied provider | Source samples omit only owner opacity/masks/effects/styles, preserving source/paint alpha and transforms. AllEffects provider copies retain the existing bounded Set Matte pass. Only the complete eight-row native Alpha default profile is newly decoded. | Scoped matte tests and pinned native Alpha-profile test passed. Unknown/default overrides and recursive/unsupported dependencies remain diagnosed. Independent copies are not live-linked. |
| Signed sibling Rotation | Bounded `-sibling.rotation +/- constant` becomes editable values/keys with signed temporal speeds and unchanged influences. Existing addition grammar, clocks and identity checks remain. | Native wall controls independently read at 8s/10s; pinned source and parser tests passed. 8s ring/background geometry visually restored. |
| Set Matte plus ordinary track matte | Preserve an already-resolved native Alpha binding inside the existing independent Set Matte wrapper. Allow a native-matted provider only for the bounded Source/Alpha sample. AllEffects providers and unresolved copied-provider bindings still reject. | Synthetic and pinned source regression failed before the fix, passed after; 15 scoped matte tests passed, four ignored. At 10s, the fresh-import comparison against the Noise-disabled Adobe frame restores the diagonal square segmentation. RGB/alpha scoring is unmeasured. |
| 50% hard Radial Wipe | Experimental source-local half-plane Shape guide + PathMask and editable Rotation. Static completion 50, direction 1, feather 0, static center, matching square-pixel precomp plane, no authored masks, only Fill/Tint predecessors. Unsupported expressions never use stale angle storage. Sparse Point defaults are decoded as signed 16:16 **percentages** locally to this mapping; explicit coordinates remain pixels. | New Adobe-authored fixture below proves 0° retains left and 90° top. Fresh-import structural test and licensed Intro center/Rotation-key test passed, including center red/green. The corrected-center full 858-frame comparison, including 4/5/8/10s, was delivered and visually accepted. Formal neighbor/smoke scoring and general import rejection coverage remain incomplete. Exact FX antialiasing is unproved. |

Licensed source identity: SHA-256
`75bb7d70238e23ffaafdeacf952217de1fcded8f86875bee39c2e91bda7804d9`,
master composition 705 / SH01 composition 3. Sources, footage, readbacks and
comparison images remain local and are **not redistributed**. Local pinned tests
use `AEP_INTRO_IMPORT_SOURCE` and remain ignored without that licensed file:
`pinned_intro_rectangles_keep_zero_start_and_coupled_size_keys`,
`pinned_intro_alpha_defaults_match_independent_adobe_readback`,
`pinned_intro_wall_rotations_counterrotate_halfs_01`,
`pinned_intro_noise_consumers_keep_both_alpha_gates`, and
`pinned_intro_radial_wipe_center_uses_native_pixel_coordinates`.
The local native reference is 24fps, 858 frames, 35.75s, 3840x1600—not a public
30fps Asset proof. Two qtrle clips (native items953/969) use the previously
approved ProRes4444 compatibility workaround; all other archive entries and
editable project JSON remain byte-identical. This is lossy media normalization,
not a codec fix or exact-alpha pass.

The redistributable Radial Wipe source is
[`radial_wipe_half_plane.aep`](../crates/aftereffects_file/tests/fixtures/effects/radial_wipe_half_plane.aep),
SHA-256 `672afb582fa532bff7c4a3c610891da762d962bd66bd22845f147b45cfeb9f6a`.
Main composition 22 is the tested Wipe target; composition 1 supplies its four
colored quadrants and is a supporting target, not an additional completed case.
Its executable assertion is
`radial_wipe_native_fresh_import_has_editable_half_plane_not_omission`.
Adobe26.5x89 independently rendered 30fps/60 frames/2s at96x64, plus lossless
alpha frames at0/1s. Fresh FX interior samples agree on retained quadrants;
strict RGB/alpha comparison is **unmeasured**. See the adjacent
[provenance record](../crates/aftereffects_file/tests/fixtures/effects/radial_wipe_half_plane.provenance.json).
The independent MP4 is now long-term Asset `dvKvnfJwundie9BarwZN_vid`; a fresh
13,306-byte download matched SHA-256
`5453522d0e1266a4f4b822b6059b0c077eefcd5d0399446b838d0ace4d484b92`.
This publication does not establish import pixel/alpha equality or full Intro
fidelity. The separate export evidence below does not promote those claims.

### Radial Wipe export follow-up — edited native controls and independent proof

**Export implemented:** a generic direct-child unpainted half-plane Shape /
Add PathMask on an identity-clock 2D Group becomes native `ADBE Radial Wipe`.
Completion remains 50%, Wipe 1, Feather 0; edited static center and bounded
Rotation values/keys become native controls. Recognition uses geometry, never
names, IDs or retained AEP bytes. The finite rectangle must cover the entire
source canvas at every angle. Native Center is rebased by the source origin;
Wipe precedes owner effects to preserve FX mask-before-effect ordering. The guide
is excluded from painted source bounds only after guarded recognition; failed
bounds/control checks restore ordinary mask lowering. Other guide transforms,
geometry/center animation, unsupported mask controls, source clocks and shared
non-mask uses reject this specialization with the existing diagnosed fallback.
No completion/feather control is invented in FX, and arbitrary edited masks are
not called Radial Wipes. The new canonical parameter definition is regenerated
from the independent source's global `EfDf` plus a fresh Adobe default readback;
its fractional Point defaults are distinct from sparse instance percentages.

| Direction | Implementation and executed proof | Remaining limits |
| --- | --- | --- |
| Adobe → FX | All three pinned native sources freshly import to editable Shape/PathMask, correct center, exact angle values/times and arriving Hold easing. `radial_wipe_native_fresh_import_has_editable_half_plane_not_omission` and `radial_wipe_edited_adobe_source_imports_current_center_and_hold_angles` passed. Supporting composition 1 is exercised through each target22. | Interior import samples agree; strict full-frame FX RGB/alpha equality remains unmeasured. No general Wipe modes, feather or expression runtime. |
| Edited FX → Adobe | `edited_quadrant_half_plane_exports_current_center_and_hold_angles` specifies four editable FX Rects plus the guide, center **[41,17]**, and Hold angles **30°/120° at 0/1s**. It failed with export recognition disabled, then passed. Adobe26.5x89 independently opened the fresh AEP and read the actual editable effect, Center, Completion, direction, feather and angle keys, with no expression. Native render matched the independently Adobe-authored edited oracle: **all 60 full-resolution RGB24 frames exactly equal**, and **all 6,144 RGBA pixels exactly equal at both 0s/1s**. | Equality is for this discriminating 96×64, 2s case, not all Intro content. Source-origin rebasing, Linear keys, effect order and rejection cases have structural tests, not separate independent Adobe render claims. |

Export oracle case `aep-radial-wipe-edited-c22` uses
[`radial_wipe_edited.aep`](../crates/aftereffects_file/tests/fixtures/effects/radial_wipe_edited.aep),
SHA-256 `8bb3d334d53dbff0c9fd75cf376582d1ee5a99c11ea1915f9db2f9407c9b9f26`.
The native source remains 24fps; Adobe rendered its full canvas/duration at30fps,
not a retagged FX render. Its reference is long-term Asset `1Fhrc3qyE7O2pfUTDei8_vid`:
15,185 bytes, SHA-256 `5a2c76eb45696d3f2a209143beeb9849357b79dcfbd503b2ef5b57cda22d8413`,
freshly downloaded and verified. Critical control/video samples include
0, 0.5, 29/30, 1, 31/30, 1.5 and 59/30 seconds. Committed independent TIFFs
retain the native premultiplied/matted RGBA samples; the Adobe output-module
convention was read independently. This is exact case-local RGB/RGBA comparison,
not an enrollment in the general AE scoring gate. See the complete
[edited-source/export proof record](../crates/aftereffects_file/tests/fixtures/effects/radial_wipe_edited.provenance.json)
for input/export hashes, Adobe control readback, source/supporting targets,
comparison counts, tests and limitations. UI clicks were not inspected; the
editable controls were read through Adobe's API. No licensed source or video
binary was committed.

A third independently Adobe-authored source,
[`radial_wipe_keyed.aep`](../crates/aftereffects_file/tests/fixtures/effects/radial_wipe_keyed.aep),
replaces the expression with direct native Angle keys. Its source SHA-256 is
`7df3d2c575ec1d67fb5ea6076e3133b03c1d4fbafbf2bb0cd404854fd8fd32d7`;
composition 22 is the case, composition 1 its supporting source.
`radial_wipe_native_direct_angle_keys_import_as_editable_hold_rotation` failed
before the direct-key rejection was removed, then passed through the existing
atomic numeric clock/easing/budget checks. Fresh exported Angle keys also pass
an editable reimport assertion. Adobe independently read the direct keys and
rendered a full30fps reference, now long-term Asset `8Viy5rfoulEYgKluf1uI_vid`
(15,185 bytes, fresh SHA-256 verification
`3e9c6030707f00dfb9f415c3f8e13df38ab09758797c5f0e7b10f158648e8d94`).
All60 RGB frames and both full RGBA samples again matched the fresh export.
See its [direction-specific evidence](../crates/aftereffects_file/tests/fixtures/effects/radial_wipe_keyed.provenance.json).
Direct native Angle keys are now supported alongside static angles and bounded
sibling Rotation expressions; unsupported easing and clocks still reject.

## Hybrid staging and linked import — incomplete Adobe proof

`AfterEffects::stage_picture_layers` borrows a contiguous selected root range and
uses the existing picture-only writer. Converted audio owners/media remain, but
native audio switches are disabled throughout generated compositions. The
coordinator retains native Premiere sound rather than routing it through AEP.
File-level writer SHA and package freshness/rollback checks remain; these are
integrity checks, not semantic certification.

`ResolvedAfterEffectsComposition::import_editable_picture` converts an actual
file/GUID selection into typed editable parts with caller-reserved numeric IDs
and asset names, for a caller-supplied Premiere importer: it is the same
conversion, media preflight and destination rules as `import_picture` (below),
with a root Group that its host re-parents. Its media guard remains alive through
final archive writing; the freshness of its assets is its caller's. Picture
expansion is muted using the existing AE audio-switch handling; native Premiere
audio is imported independently. Linked audio remains unsupported.

No new Adobe operation, reference Asset, oracle replacement or fidelity result
is supplied. The [hybrid ledger](hybrid-adobe-export.md) records the current
implementation, bounded CPU verification, unsupported cases and unmeasured
open/render/alpha/audio/edit/relocation proof. Deleted observer tests' historical
receipts are not current validation.

## Glow Operation — export-only Normal approximation

**Export:** all fresh FX `glow` Effect Parade occurrences now explicitly author native
`ADBE Glo2-0006` Glow Operation **Normal (2)** instead of the catalog's Add (3).
FX composites glow premultiplied-over its source; native Add can wash out
colored source interiors. The native instance enabled switch, threshold/radius/
intensity values and keys, other controls, and editable owner remain intact.
A contextual export diagnostic records the substitution. This is **not full
fidelity**: native halo color and interior brightness may still differ, including
on other exported Glows affected by the generic choice. **Import is unchanged**:
native Add (and other operation settings) remains outside the current FX model;
no import preservation or new import proof is claimed. Catalog defaults,
FX renderer/schema and Layer Styles are unchanged.

`export_document::tests::effects::glow_export_uses_normal_without_changing_keyed_controls_or_native_source`
checks the existing independent Adobe-authored `effects_coverage/native_static_controls.aep`
composition 183 (source Operation Add 3) separately from an explicitly edited
FX input. Freshly exported native controls have Normal 2 with the expected keyed
threshold and unaffected static radius/intensity; the adjacent edited-static
regression checks the disabled occurrence stays disabled. The targeted regression
failed before the repair (exported Operation 3 vs 2). These are own-reader
structural checks, **not independent Adobe control readback of this export**.
A private isolated sun probe found a colored gradient interior with Normal
rather than near-white with Add; its
halo stayed gray versus FX's colored halo, and interior brightness still
mismatched. The scratch source, render and samples are not public feature
fixtures or long-term Assets. A freshly built converter independently reproduced
the isolated Normal result. An integration build additionally applied the separate
Group-matte repair (PR #4587) without including that repair in this change, and
freshly exported the unchanged private Showreel. All 159 native Glow Operation
values changed from Add to Normal; other native bytes were identical to the
matte-only export. A 13-frame full-resolution Adobe render at 7.8–8.2s decoded,
but its 8s PNG was byte-identical to the old Add render and did not establish the
fix. A separate half-resolution diagnostic at 8s showed the colored striped
sun and dark floor restored. After explicit user approval, the AE26.5 disk
render cache was cleared with no Adobe process running. The same unchanged AEP
was rerendered at **1920×1080/30fps, frames 234–246**; all 13 frames decoded and
frame 240 showed the yellow/orange/pink striped sun and dark floor restored.
The full-resolution PNG changed from SHA-256
`39035e1ab8969e5c7977a06ea745dd83b1f88e76f60e5cfad8b10675100bd509`
to `adffc1318a1ca06a1c1a570e96524fca85c09fdae88243b6631af4dd374b3927`.
A subsequent full-duration integration render decoded all 450 frames at
1920×1080/30fps. Its frame timestamps and the original FX video's timestamps
matched n/30 within 2µs; the silent labeled comparison is 3840×1128, 15 seconds.
Inspected frames 15, 180, 240 and 447 show colored suns, the striped sun at 8s,
and restored multicolor end-card lettering; glow/particle and other differences
remain. The focused effects panel passed 19 tests with 78 pre-existing proof
backlog tests ignored; these are not 97 passes. Converter formatting, strict
workspace/all-target Clippy, support-ledger validation and CLI build passed.
This supersedes the stale-render observation, not the remaining halo/brightness
limitations or missing pixel-equivalence proof. Public independently authored feature case,
30fps reference/Asset/hash verification, editable Adobe inspection, automated
RGB score and alpha proof remain **unrun/unmeasured**. This section does not
promote the historical Glow import cases or prove general Glow fidelity.

## Single-paint leaf opacity — export normalization

**Export only:** a 2D Shape with one static Normal paint can now transfer an
otherwise overrange paint-opacity factor into its static or bounded scalar
layer Opacity, including the cubic extension below. Static products and keyed
endpoints must remain in 0..100; an animated native property uses its keys, not
an unused static base. The current FX leaf tessellator
multiplies these factors before drawing: paint `100` × layer `0.999%` produces
`99.9%` coverage for opaque paint. Previously export clamped the paint first and
left only `0.999%`, making the reported finale's tagline and chrome faces nearly
invisible. This is not a global reinterpretation of low percentage values.

The uniform paint factor becomes 100% and the layer carries the combined
factor. Existing solid-alpha folding may additionally move color alpha into
paint Opacity; gradient stops remain unchanged. Colors, path geometry, matte
references, clocks and other transforms remain editable. A contextual
`AE-EXPORT` diagnostic records this control normalization. It does not distribute Group opacity or
change the FX renderer/model/schema. **Import is unchanged and not new proof.**

The bounded repair excludes unsupported or upper-overshooting layer-opacity curves, independent
paint opacity or color animation, multiple paints, non-Normal paint/layer blends,
3D leaves, and leaves with their own effects or masks. Animated normalization
also excludes skew and forced-vector transform paths that reconstruct keys from
original FX dynamics. Static normalization on these vector paths is unchanged. Products above native 100% retain the
existing diagnosed saturation fallback rather than clipping animated values or
inventing new semantics. These other combinations remain approximation/proof
limitations, not newly supported fidelity cases.

`export_document::tests::paint_opacity::single_paint_preserves_showreel_leaf_opacity_product`
uses explicit reduced FX inputs with the reported paint/layer factors and chrome
stops, not an Adobe-authored oracle. It failed before the repair (native layer
alpha `0.00999` instead of `0.999`). Adjacent regressions cover ordinary low
opacity, consumed matte visibility/reference, independent parent opacity,
animated/multiple-paint fallback, nested vector/skew transforms, and rejection
without mutation. The initial bounded CPU panel passed **86 tests**: 16 opacity tests,
three matte-visibility tests and 67 hierarchy tests. One pre-existing ignored
Adobe-proof test remains unrun. Formatting, ledger validation and CLI build
passed. Scoped strict Clippy is blocked by existing unused/dead-code, visibility
and style findings outside this repair; no clean lint claim is made.

The unchanged private source was freshly exported before and after the repair.
A separate local RIFX inspector found all **1,670 native layer records** retained;
all **16 tagline glyphs and four chrome faces** changed from native alpha
`0.00999` to `0.999`. The generic rule also normalized other matching leaves:
76 layer-opacity payloads and two stroke-opacity payloads changed overall.
Both AEP files are 39,941,944 bytes; every non-opacity native leaf payload is
byte-identical, including paths, gradients, timing and matte flags/references.
The original project JSON hash remained
`ad1d95d21abc87be576914cd2ff373668de80ea289f7c027f58c59330599a982`.
Original private artwork/media and local inspector outputs are not published
as fixtures. These structural checks alone are **not Adobe proof**.

### Initial private Showreel Adobe diagnostic — remaining visual mismatch

The user subsequently authorized a local side-by-side. AE **26.5x89** separately
opened/rendered the fresh exported AEP, composition **1**, at 1920×1080/30fps for
all **450 frames / 15 seconds**. The owned `aerender` process used `Best Settings`,
full-comp/full-resolution 30fps override and a `Lossless` MOV, then an H.264
transcode; it did not reuse the interactive AE session. The unmodified original
TSRCT was separately rendered by the matching Tesseract FX renderer, not by
reimporting the AEP. Both MP4s were fully decoded and their frame timestamps
matched the 30fps grid within two microseconds.

| Local artifact | SHA-256 |
| --- | --- |
| Original TSRCT archive | `8f3a72f3751de991b29d3d4cc38d0487cf5d0b668d999bfb77ed804c6b1abf66` |
| Fresh AEP (comp 1) | `dafd6468d99bd0baf84b9d662ca392ab32ca2a0b58d627bae4a86935c7c687a4` |
| Original-TSRCT FX MP4 | `d74bd2784ea1be7ba6ce3afe43d0b9ef5eeb0b2d05ba09d240e6fe0cdd265342` |
| Fixed-AEP Adobe MP4 | `14bf3e79b6dcd08ab833e77f4c054c659d1789e7eeb13cca560f718a7c25335f` |

The silent labeled side-by-side is **3840×1128, 30fps, 450 frames / 15 seconds**
(SHA-256 `4e9c8e8a095db3567b5719a6561919ef978dbf0578d2c19369c156f0c689aa05`).
A common integer 1/30 timebase is used only for the diagnostic encoding after
verifying the original frame grids; neither source video is rewritten and no
frames are omitted. Inspected samples cover **0.5, 7.5, 13.2, 13.6, 14.2 and
14.9 seconds** (frames 15, 225, 396, 408, 426 and 447).

At **14.9s / frame 447**, Adobe visibly retains `PHOTOS. NO LIMITS.`. However,
**the SPAM faces render white instead of the original multicolor gradient**,
and the tagline pill/background/horizon also differ. Thus restoring the static
opacity product does **not** complete the reported visual repair. These remaining
gradient, pill and background differences are open blockers, not
fidelity passes or proof that all 76 normalized leaves render equivalently.
Independent editable Adobe control readback, automated RGB scoring, alpha/audio
comparison and public native-reference/long-term Asset proof remain unverified
or unmeasured. The private side-by-side is a silent, labeled visual diagnostic;
source artwork and videos remain in ignored local storage, not Git.

### Animated pill follow-up — bounded Linear/Hold opacity

**Export:** the same factor now scales native scalar Linear/Hold opacity keys,
whose stored values are fractions rather than static percentages. Every key is
validated before either paint or keys change; out-of-range, non-finite, cubic,
non-scalar and spatial tracks retain the prior fallback. No clocks, easing,
geometry, independent scale animation or FX renderer behavior are changed.
**Import:** unchanged. Reduced synthetic FX geometry is structural evidence, not
an independently Adobe-authored feature fixture.

`export_document::tests::paint_opacity::single_paint_preserves_animated_showreel_pill_opacity`
failed before the extension: final native alpha was `0.00999`, not `0.999`.
It now passes for fill and rim using the reported incoming-easing sequence:
`(0 ms, 0, Linear)`, `(960 ms, 0, Hold)`, `(1010 ms, 0.999, Linear)` (rim final
key: 1020 ms). It verifies the complete opacity key metadata and unchanged
960–1130 ms cubic ScaleX animation. Regressions also verify atomic rejection of
unsupported tracks and the static-skew, animated-skew and forced-vector fallback.
The bounded panel passed **89 tests**: 19 opacity, three matte-visibility and
67 hierarchy tests; the pre-existing Adobe-proof test stays ignored. Build and
format checks passed; strict Clippy remains blocked by the existing 50 lib / 30
lib-test errors, not a clean lint gate.

A fresh export of the unchanged private source is still **39,941,944 bytes**,
SHA-256 `2e1358bfef96b2c00b32f51ee97ed360caed8113fa57691dcc04e3fcca0b4de5`.
Against the initial fix above, only **four of 790,371 native leaf payloads**
change: opacity-key values for pill **70053**, rim **70054** and matching Hold
caret **70052**, plus rim paint opacity `100 → 55` to retain its original solid
alpha. Each native key's clock, interpolation, flags, speeds and influences is
byte-identical; all non-opacity payloads remain byte-identical. This is private
native structure verification, not an independent editable-control or RGB pass.

A second isolated AE **26.5x89** render of comp **1** completed at
1920×1080/30fps, **450 frames / 15s**. Its MP4 SHA-256 is
`a98a4e7f312d260c7f399f918b3f1b917a3620631b8091474a03ea224058930b`.
The unchanged original-FX video above was reused, with both full timestamp grids
verified again. The new silent 3840×1128 comparison SHA-256 is
`2aa23dbc5168be704fe0daa758e272689bf0c392795031d3e3194fc5036da9fa`.
Inspected frames **15, 405, 407, 408, 426, 447** cover 0.5s and the pill's entrance
through 14.9s. **The dark rounded pill and pink rim are now visible in Adobe**,
including the developing tagline at 13.6s and complete text at 14.9s. This
supersedes the missing-pill observation in the initial diagnostic, not its other
limitations. Opening-scene mask leakage, SPAM gradient and horizon/background
mismatches remain open. Automated RGB, alpha/audio, independent editable Adobe
control readback and public native-fixture/Asset proof are still unmeasured or
missing; this private comparison is not a general fidelity pass.

### HUD, particle and tile-border follow-up — export only

**Import remains unchanged.** Export now preserves the keyed single-paint
product even when an unused static base would exceed 100%. Cubic key values
scale uniformly; native temporal speeds scale with them while clocks and easing
influences remain unchanged. The entire track is validated before mutation.
Non-finite/unsupported metadata, spatial tracks and upper-overshooting control
hulls retain the diagnosed fallback, as do the existing blend/effect/mask and
skew/forced-vector exclusions. Finite negative cubic interiors are permitted:
FX runtime opacity and Adobe opacity both clip below zero, and multiplication
by a positive factor commutes with that lower clipping. Curves are not flattened,
resampled or adjusted to remove their negative tails.

The private clock digits and scene labels previously had native alpha `0.01`
where the source leaf product was `1.0`. Particle curves also used separate XY
Position followers: their real Opacity keys lived on a native **2D Transform
sidecar**, while the inner Shape received identity opacity and no keys. The
same normalization now updates that live sidecar only, leaving inner opacity
at identity and Position followers unchanged. True 3D remains excluded.
All 40 private sparks now report normalization and retain keyed native opacity.
Reduced regression inputs cover keyed base-100 opacity, cubic
values/speeds/influences/clocks, negative tails, 2D sidecar ownership,
true-3D exclusion and atomic rejection.
These are explicit FX-input structural tests, not independent Adobe-native
fixtures or editable-control readback.

Separately, later paints in a native vector paint scope now use **Above Previous
(2)** rather than **Below Previous (1)**. FX fill/stroke array order and widths
are retained; nested and intervening rendered groups delimit paint stacking.
This fixes a fill covering the inner half of its later stroke, not a linewidth
or transform-scale error. The private isolated tile at 400% scale showed about
30 px of border in FX versus 15 px in Adobe; changing only Stroke Composite Order
to 2 restored about 29 px with unchanged outer geometry. Writer and fresh-export
regressions assert ordinals as well as paint order for solid/gradient and
multiple paints and scope boundaries. Native fixture
`shapes/import_shape_flags_order.aep`, compositions 122/137, supports the native
ordinal interpretation; its historical nondiscriminating reference samples are
not promoted to new export-fidelity proof.

A separate converter-generated white-on-gray probe was actually rendered in
Adobe 26.5x89: native opacity 100→0 over one second with cubic controls
`(1/3, 2, 2/3, 1.5)` has interpolated opacity −28% at 0.8s. At frame 0 the
foreground center was RGB `(255,255,255)` against `(128,128,128)`; at frame 24
both were `(128,128,128)`, confirming rendered lower clipping. This is an
isolated native-runtime diagnostic, not an independently Adobe-authored oracle.
**Executed CPU evidence:** 29 opacity tests and 32 writer Shape tests passed.
The two edited-paint export cases were explicitly run despite their proof-backlog
ignore markers and passed:
`export_document::tests::pr4442_vector_cases::edited_rect_gradient_fill_and_solid_stroke_export_in_owned_order`
and
`export_document::tests::pr4442_vector_cases::edited_shape_multiple_paints_keep_order_fill_rule_cap_opacity_and_stroke_keys`.
Opacity regressions include `keyed_digit_with_static_base_100_exports_unclamped_keys`,
`keyed_cubic_spark_preserves_native_speed_influence_and_clock`,
`keyed_spark_negative_trough_keeps_native_handles_and_scaled_values` and
`separated_xy_shape_normalizes_sidecar_opacity_without_replacing_position`
under `export_document::tests::paint_opacity`. Writer stacking is covered by
`writer::shapes::tests::native_paint_stack_preserves_fill_then_stroke_in_each_scope`
and `nested_paint_scopes_reset_without_moving_geometry_modifiers_or_groups`.
New regressions failed before their corresponding repairs. Converter formatting,
strict workspace Clippy, check, build and support-ledger validation passed.
The broader explicitly enabled `pr4442_vector_cases` backlog is **not green**:
12 passed and 8 failed. Repeating it on the unchanged branch base yielded
11 passed and 9 failed; all 8 remaining failures also occur on that base.
One opacity proof-backlog test remains ignored. This is not a full-suite pass.

**Private Adobe render evidence:** the unchanged source archive hash is
`8f3a72f3751de991b29d3d4cc38d0487cf5d0b668d999bfb77ed804c6b1abf66`.
The converter repair is based on `9e8ddf405`; the unchanged original-FX video
was rendered with `922372d4`. A temporary integration additionally applied the
independent Group-matte and Glow-operation fixes (`64bcce7da`, `6f08922df`); these are not dependencies
or code changes in this repair. The freshly exported integration AEP hash is
`3ea521629db2c89e36e9db17439aa7b7bb970d06897d493f1fa5f73eeea96ed9`.
An isolated Adobe 26.5x89 process rendered comp 1 at 1920×1080/30fps:
particles are present at frame **18 / 0.6s**, `08:25` and `05 PRODUCT UI` are
present at **265 / 8.833333s**, and tile borders have their intended thickness
at **339 / 11.3s**. A subsequent full **450-frame / 15s** native render matches
all three reviewed stills byte-for-byte in decoded RGBA. Its SHA-256 is
`5218897678a165e29d5cebd7b1b6c24eeed78f33322cc9f008bb7f87e711a2e5`.
The silent labeled comparison is Tesseract left, actual Adobe right,
3840×1128/30fps/450 frames, SHA-256
`42fea1c62896daac4436aae22b9bc6e870c3e05d98311bc6a2683435991f89e3`.
All timestamp grids match within one microsecond and the comparison fully decodes.
No additional Adobe cache deletion was needed.

These observations resolve the three reported missing/thin details, **not general
pixel fidelity**: Glow/grid brightness and some tile-motion differences remain.
Independent editable Adobe control readback, public native feature fixtures/Asset
references, quantitative RGB/alpha/audio fidelity and untested paint/group
combinations remain missing or unmeasured. Original private artwork and videos
stay in ignored local storage and are not published.

## Rust code-quality audit — identity, timing and malformed-input regressions

This repair does not add a conversion feature or widen the native writer profile.
The regressions are CPU-only safety/editable-structure evidence, not new Adobe
open/control, RGB, alpha or audio proof. No native fixtures or reference Assets
were changed. Independent Adobe proof for these repairs remains **unrun/unmeasured**.

| Finding / direction | Original behavior → repair | Evidence and limitations |
| --- | --- | --- |
| Generated identity capture, export | Auxiliary backgrounds/helpers could reuse dangling parent/matte/guide/animation IDs, silently binding them to unrelated generated content. Reserve typed references as well as owned IDs; unresolved references keep the existing validation behavior. | `references::tests::review_audit_all_referenced_layer_ids_are_reserved_even_when_dangling` and `review_regressions::review_audit_generated_background_does_not_capture_dangling_{matte,animation}`. Explicit edited FX inputs, not native-render evidence. |
| Non-finite derived Source Text, export | Individually finite box coordinates/size or auto-leading could overflow into invalid COS numbers. Validate derived box edges and automatic leading before encoding; invalid Text is omitted with the existing layer-context diagnostic, retaining valid siblings. | `writer::text_document::tests::finite_derived_text_numbers_*` and `review_regressions::review_audit_overflowing_text_is_omitted_without_losing_siblings`. No rounding/clamping or new text approximation. |
| Preserve Transparency provider clocks, import | Raw source-local in/out points were compared across layers. Compare parent-composition intervals using start/stretch, reject inactive clocks, and restrict overlap to the composition span. | `preserve_transparency::tests::review_audit_preserve_transparency_*`. Synthetic interval/provider assertions; independent native Preserve Transparency fidelity remains unverified. Existing best-effort fallback for unsupported stacks remains. |
| Ambiguous Effect Parade roots, import | Duplicate roots silently selected the first parade. Omit the ambiguous parade with an explicit duplicate-root diagnostic, including the Set Matte path; unrelated layers/content remain convertible. | `native::tests::review_audit_duplicate_effect_parades_are_diagnosed_not_selected` and `set_matte::tests::review_audit_duplicate_set_matte_parades_are_rejected`. Deliberately malformed structure, not a supported Adobe feature. |
| Failed media identity, import | A failed lookup cached only its ID, hiding later path/selector conflicts. Retain the complete failed request; conflicting reuse fails just as for a successfully resolved source, while identical failures retain one diagnostic. | `adapter::media::tests::review_audit_missing_media_keeps_identity_for_conflict_detection`. Filesystem/identity regression only. |
| Host-invalid media filename, import | Host filename errors aborted the complete import. Diagnose `local source filename is invalid on this host; media content omitted` per asset; genuine permissions/I/O failures remain fatal. | `adapter::media::tests::review_audit_overlong_media_component_is_an_item_local_omission` (Unix). No new path-length quota or path substitution. |
| Legacy record mutation, internal native schema | Applying modern export options to a decoded 160-byte legacy record indexed beyond its bytes. Return a typed layout error rather than panic or silently upgrading the record. | `layer_records::tests::review_audit_legacy_export_options_return_an_error_without_panicking`. Current fresh exports use modern records; no external-file-to-panic path was established. |

Import and export proof statuses above are separate. These fixes do not complete
outstanding native-feature proof elsewhere in this ledger. See the
regression test symbols above for CPU coverage; this is not independent Adobe proof.

## Static legacy inline mask loops — export closure repair

**Export:** a validated static legacy inline mask whose final LineTo/CubicTo
endpoint exactly equals its initial MoveTo now receives explicit native closure.
FX fills this geometric loop, but AE ignored its exported open mask. Native
encoding merges the duplicate terminal vertex into the first, retaining its
incoming handle. A contextual `AE-EXPORT` diagnostic records normalization.
This is mask-only: ordinary Shape paths, guide-layer geometry (including animated
paths), already explicit Close, noncoincident endpoints and two-command loops
are unchanged. No tolerance-based snapping, general open-path reinterpretation,
FX renderer/model/schema change or additional animation support is introduced.
**Import:** unchanged; no new import implementation or proof claim.

`export_document::tests::pr4442_scene_cases::legacy_inline_closed_loop_exports_closed_native_group_mask`
uses reduced synthetic FX geometry, not the private artwork or an Adobe-native
oracle. It failed before the fix with native `shph` header `b3de0209` (open),
then passed with `b3de0201` (closed). It verifies translated/source-normalized
bounds, mask ownership and retained grid content. The adjacent
`export_document::masks::tests::inline_mask_loop_closure_is_exact_and_does_not_change_other_topologies`
checks straight/cubic returns, unchanged control points, explicit closure,
noncoincident endpoints, two-command loops, Add mode and translation.
The scoped panel passed **42 mask tests and 19 opacity tests**; **12 mask and
one opacity proof-backlog tests remain ignored/unrun**. Existing animated-mask
and native-mask controls tests ran in that panel. Build, formatting and ledger
validation passed. After integrating the remote branch's main/audit merge,
the same panel passed again, the fresh AEP remained byte-identical, and strict
scoped Clippy (`aftereffects_file --lib --tests -- -D warnings`) passed. That
updated baseline supersedes the earlier Clippy-blocked observations above.

The unchanged private Showreel contains exactly one matching inline loop:
Group **10230**, mask **100033**, composition **1**. Fresh export changes only
**three of 790,371 leaf payloads**, all inside that mask's `ADBE Mask Shape`:
closed header, vertex-list allocation and vertex triples. The file becomes
39,941,920 bytes (24 bytes smaller); every other native leaf payload, including
pill opacity, mask mode/controls and clocks, remains byte-identical. The final
build reproduced the same AEP bytes after diagnostic wording was clarified.

A fresh isolated AE **26.5x89** render completed **450 frames / 15s**,
1920×1080/30fps; the original FX video above was reused byte-for-byte. Both full
frame grids were verified within 2µs of n/30. Samples **0, 15, 52, 90, 408, 447**
cover the opening, transition and restored pill. At **0.5s and 1.733s**, sky and
grid are visibly contained inside the front rounded white frame; the earlier
large outside spill is gone. Late pill/rim and tagline remain visible. This
supersedes the opening-mask leakage observation above for this private case,
not the remaining gradient, glow/color, particles/timecode or horizon differences.

| Latest local artifact | SHA-256 |
| --- | --- |
| Fresh mask-fixed AEP (comp 1) | `1c8681358c53f116e150938c8522c80ee4f0c4451201073e2dc55e57a837fb77` |
| Direct Adobe MP4 | `0b71ecf25fa46f68f38dda0e8dadafde2ccad8c12a29de801f8e2306b0b8011c` |
| Silent original-FX / Adobe comparison | `4abf8a482ed89e650f3c095b5ea80a7483647405d31ea548b4c00194d208f046` |

The comparison remains a **private visual diagnostic**, not a general fidelity
pass or public feature-case delivery. Independent editable Adobe control
readback, automated RGB/alpha/audio scores and public pinned Adobe-native source
and immutable Asset proof remain missing/unmeasured. Private artwork, videos
and inspection reports are not committed or uploaded.

## Track-matte provider display — export visibility repair

**Export:** FX consumes referenced track-matte layers instead of painting them
independently. The exporter previously disabled that independent display only
for Adjustment owners. Ordinary owners therefore retained a visible matte
provider, which could paint an opaque rectangle over otherwise retained content.
All referenced matte providers now use the same native display switch, without
changing their sampled opacity, source geometry, keys, references or clocks.
Shared providers stay editable/sampleable; children inside a provider's source
precomposition remain enabled. Unreferenced layers keep their display state.
This adds no approximation or omission. **Import:** unchanged by this repair;
fresh imports below are supplementary test setup, not new import-fidelity proof.

CPU regressions in `export_document::tests::matte_visibility` all failed before
the fix and passed afterward:

| Test symbol | Contract / source and evidence |
| --- | --- |
| `consumed_rect_mattes_are_sampleable_without_painting_over_their_owners` | Explicit edited FX inputs exercise Alpha, inverted Alpha, Luma and inverted Luma; provider display off, opacity intact, owner/reference/mode intact, unrelated sibling visible. |
| `shared_group_matte_hides_only_the_occurrence_not_its_source_children` | Two owners share one editable Group provider; only its occurrence is hidden, not its source children. |
| `native_alpha_and_luma_matte_inputs_keep_hidden_sampleable_export_providers` | Pinned native `compositing/trackMatteType.aep`, SHA-256 `4a6580962e57b8d523a4bbf90afd7cdf1c942ae66ade4274ae1111385830c1c4`: comp 1/provider 15 and comp 18/provider 31 have disabled display. Freshly import each target, explicitly set FX display switches on, then freshly export and assert consumed providers are still sampled but not displayed. These edited inputs are supplementary structure regressions, not independent native-render comparisons. |

An unchanged private original was freshly converted in 141.61 seconds and Adobe
rendered all **450 frames, 1920×1080/30fps, 15 seconds**, fully decoded. The 3-second
upload card is visible again. Compared with the preceding export, the AEP differs
at exactly **15 bytes**, each solely clearing a referenced provider's display bit;
all other native bytes, including geometry, clocks, colors and camera, are equal.
Private source/video/screenshots remain local. Native pixel equivalence is not
claimed. Independent public fresh-export feature renders and long-term reference
Asset/fresh-download proof remain incomplete; existing native-fixture provenance
is in `crates/aftereffects_file/tests/fixtures/compositing/README.md`. The bounded
CPU panel passed **71 tests** (three new tests, 67 hierarchy tests, and the existing
Adjustment matte test explicitly run with `--ignored`); this is not a full-suite
or Adobe-fidelity pass. Formatting, ledger validation and the CLI build passed.
Scoped strict Clippy remains blocked by existing unused/dead-code and style
findings outside this change; no clean lint claim is made.

### Animated multichild Group matte — export follow-up

**Export:** A full-span Group with multiple animated vector children could be
lowered to a native Null parent when a sibling consumed it as an inverted alpha
track matte. A Null does not composite its children into a sampleable alpha
source: the children painted independently and the owner retained its uncut
shape. Referenced Group matte providers now use the existing native
precomposition path, preserving their child keys and inverted matte link while
disabling only the provider's independent display. Unreferenced Groups retain
their prior Null-parent optimization. The editable FX input and geometry are
unchanged; no new approximation is introduced. **Import:** unchanged.

`export_document::tests::matte_visibility::animated_multichild_group_matte_retains_composited_source_and_inverted_link`
uses a reduced editable FX input with seven gaps and an animated first child.
It failed before the fix because the provider was a Null; after the fix it
asserts a disabled precomposition provider, enabled source children, and the
owner's inverted-alpha link. This is supplementary structural evidence, not an
independent Adobe-native render or editable-control inspection. The private
showreel source and videos are not committed. A fresh export of the unchanged
private source was rendered in an isolated Adobe After Effects process at
1920×1080/30fps, native frames 234–246 (7.8–8.2 seconds, 13 decoded frames).
Inspection of frame 240 (8 seconds) confirms horizontal cutouts in the sun
instead of independently painted protruding stripes. The sun's white gradient
and other color differences remain unresolved. The four matte-visibility and
67 hierarchy CPU tests passed; formatting, ledger validation and CLI build also
passed. This is targeted private visual evidence, not full-duration regression,
independent native-fixture proof, Adobe control readback or measured alpha/RGB
fidelity. Import is unchanged and gains no new proof.

## Unsupported video media — strict import preflight

**Import:** the user-approved policy now fails conversion for unsupported video
media, superseding the earlier warning-and-omit policy. Preflight checks the
`mp4`/`mov`/`m4v` filename contract and reads the actual video-track sample-entry
codec using the existing standalone workspace MP4 reader. Unknown codec families,
including `rle ` (QTRLE / QuickTime Animation), invalid container metadata and
containers without a video track fail before Check/Write publication. Unsupported
format/codec errors identify the logical asset and resolved path. No empty asset
ID, successful omission or partial final archive is substituted. Operational I/O
errors remain fatal. Existing missing-file and unsupported-effect policies are
unchanged; this strict gate applies to video format/codec admission.

Recognized sample-entry families mirror native `VideoCodec::from_stsd_fourcc`:
H.264, HEVC, VP9, the six ProRes variants (including alpha), MPEG-4 Part 2, AV1
and APV. Metadata admission is not full frame decoding, codec-profile validation,
WebCodecs portability or a guarantee of decoder availability in every downstream
FFmpeg build. An ffmpeg-based transcoding preprocessing stage is planned by the
user but is **not implemented here**; SWF and QTRLE are not transcoded or rendered.

**Export:** unchanged. Supplementary regressions
`unsupported_video_extensions_latch_a_fatal_error`,
`unsupported_video_codec_in_mov_and_invalid_container_are_fatal`,
`supported_video_containers_keep_their_original_bytes` and
`unsupported_video_fails_before_check_or_write_publication` cover extension and
sample-entry rejection, fatal error propagation, supported H.264 admission and
absence of output in both modes. The publication test relinks the existing
`pr4442_native/sources/media_video.aep` fixture to substitute media and a mutated
sample entry; it is not an independently Adobe-authored SWF/QTRLE fidelity case.
No new Adobe open/render, visual/alpha comparison or full Flibbertigibbet export
proof is claimed.

## Remaining-quota cleanup checkpoint — incomplete

- **Import:** RIFX reading/writing/cloning/destruction and COS parsing/destruction
  now use explicit traversal stacks instead of the former 48-level parser quotas.
  Essential overrides and property aliases are iterative; alias cycles still fail.
  Parent-scale/delayed-position and sampled-expression fitting no longer reject
  input solely at the former duration, source-key, fitted-key or evaluation counts;
  numerical tolerances and representability checks remain unchanged.
- **Export:** removed the 512 Boolean-operand/effect, 1 MiB gradient XML,
  1,024 EXR-attribute/RIFF-chunk and 32 KiB JavaScript source policy limits.
  OpenEXR's native 255-byte name maximum is retained (not a resource quota).
- **Remaining/blockers:** recursive Boolean geometry, nested Premiere sequences,
  dynamic-path limits and other inventoried recursive/algorithmic bounds remain.
  PDF Form evaluation retains a 16-level native-recursion safeguard and a
  100,000-operation Form-expansion budget; reaching either now interrupts the
  whole import instead of silently omitting that source. Flat page operators
  have no corresponding fixed count quota. Generated image-sequence paths retain
  their 64 KiB preallocation safeguard pending safe host-filename validation;
  this does not limit alias strings read from native files.
  A 4,096-level Premiere XML experiment aborted inside the XML parser; the
  attempted XML-limit removal was reverted, not declared stack-safe. Boa parser
  stack provisioning remains empirical, not isolation. Work-budget exhaustion is
  not yet uniformly propagated as a whole-conversion interruption.
- **Proof:** targeted CPU capacity/malformed-input checks only. No new Adobe
  open/inspection, native render comparison, alpha proof or successful fresh
  Flibbertigibbet end-to-end conversion is established for this checkpoint.

## Capacity and oversized masked-vector follow-up — incomplete render proof

This follow-up removes aggregate converter policy quotas; it does **not** remove
native wire widths, malformed-input checks, recursion/VM safeguards, or the
existing FX schema limits. The former internal capacity report recorded
changes, residual restrictions and an executed synthetic 10× probe: **16,570
layers, 38,540 tracks, 298,200 keys**, with every identity/key time/value checked,
326.04 seconds and 3,432,693,760 bytes maximum RSS. This is capacity evidence,
not Adobe fidelity or a claim that all arbitrary limits have been removed.

| Feature × direction | Implementation and evidence | Remaining limitation |
| --- | --- | --- |
| Aggregate capacity, import | AE chunk/text/numeric-key readers and Premiere readers accept the documented former policy boundaries; TSRCT project/metadata byte and asset-count quotas are removed; actual format, integrity and allocation constraints remain. Targeted boundary and malformed-input tests executed; native widths and validation remain. | The capacity probe is synthetic; residual quotas are listed separately. |
| Aggregate capacity, export | AE numeric/text/vector/script and Premiere output policy ceilings removed as listed in the capacity inventory. The pre-integration 10× fresh archive→AEP probe retained all checked content. | TSRCT writer and shared FX track byte quotas are now removed. The historical 10× probe has not been rerun for this cleanup and is not current-branch capacity proof. Native widths, recursive/algorithm safeguards and resource availability remain; see the capacity inventory. |
| Oversized 2D vector boundary, export | Finite oversized eligible vector subtrees use a native collapsed precomposition with an unchanged input geometry/effect stack, instead of subtree omission. No 3D scene, nonidentity clock, motion blur, skew helper, external matte/parent dependency, non-Normal owner blend or owner opacity animation is newly admitted. Fresh-export regression asserts the collapse switch, editable child and original 65,520 px geometry. | Bounded native capability proof is independent of the fresh exporter: public feature-case converter-generated AEP Adobe open/control inspection and native-render comparison remain **unrun/unmeasured**. Unsupported effects still produce their existing omission diagnostics. |
| Hard Add mask output support, export | A single static inline, non-inverted, unfeathered, unexpanded fully opaque Add mask certifies its output hull **after validating the full child input**. Child Glow is retained before masking; mask-owner effects disable this certificate. The ancestor may remain an ordinary camera-bearing 3D precomposition; input geometry is not clipped to the mask. | Other masks and owner effects retain full bounds. No general effect-radius estimate or input-crop rule is introduced. |
| Out-of-range mask opacity, export | Values above the native 0..1 range are explicitly clamped to 1 with a diagnostic, rather than allowing an invalid native mask record to discard content. | **Approximation:** source edge coverage may differ. This is not an opacity-fidelity pass. |
| Collapse and mask semantics, import | No new general collapse-import mapping is claimed. The pinned Adobe source and its native source relationships/switches are executable fixture assertions. | Existing import limitations remain; native parser assertions alone do not establish equivalent editable imported rendering. |

Independent native capability evidence is pinned in
[`collapsed_vectors`](../crates/aftereffects_file/tests/fixtures/collapsed_vectors/):
source SHA-256 `d520c45353d7a16dad19793fa0c52aa5fd3b3a34bd00f68e5f6e746eeabfd41f`,
Adobe 26.5x89. Targets 21/34/47 compare collapsed, uncollapsed-negative and direct
vector geometry; 94/121 discriminate child-Glow-before-mask from
owner-Glow-after-mask. The collapsed/direct stored RGBA samples are byte-identical
at frames **0,1,2,3,15,30,45,59**; the uncollapsed negative is blank at frame 30.
Mask samples at frames 0/30 retain an inside-edge Glow contribution and zero
outside alpha. This is stored-RGBA evidence, not an assertion about straight-alpha
encoding. Five independently Adobe-rendered 1920×1080, 30fps, 2-second MP4s were
fully decoded locally but remain unpublished: no publication receipt or long-term
Asset ID exists for these cases. No private project or render is committed.
At the initial post-PR checkpoint, a fresh private-project conversion succeeded (144.49 seconds;
1,777,582,080 bytes maximum RSS), and Adobe rendered its new AEP to
1920×1080/30fps/15 seconds; all 450 frames decoded and the original source hash
remained unchanged. **That checkpoint did not fix the missing scene:** visual
inspection still showed missing content. The masked-vector branch had finite
bounds, but a separate 3D corridor's projected extent exceeded the native canvas
and caused ancestor omission. The following repair addresses that separate cause;
Adobe acceptance alone was not treated as rendering correctness.

The subsequent **export-only 3D viewport investigation is not pixel-exact** and
is not a completed feature case. Independent Adobe-authored controls kept all 3D world
coordinates and camera position/zoom unchanged, used uncollapsed occurrences,
and compared stored RGBA at frames **0,1,2,3,15,30,45,59**:

- A height-only change (4096×4096 → 4096×2048) matched all eight samples exactly.
  Native readback also matched all seven source/camera property trees and keys.
  Source SHA-256: `c78c5e0aaffed87c52f14c270774820d0140b6c72fff859c73e2f7c03c55e9ba`.
- Changing both axes (8192×4096 → 4096×2048) **failed exact equality at every
  sample**: respectively 11, 2, 7, 15, 5, 6, 3 and 11 changed pixels, with a
  maximum channel difference of **1/255, including alpha**. A wrong-camera
  negative changed over 273,000 pixels at every sample; positive controls were
  nonblank. Source SHA-256:
  `00ee1a131dbeda4316f05c61791319ae5064cf65396a85ba17d085f1d15d7553`.

The narrower height-only result does not prove general viewport equivalence.
The exact-equality gate remains **failed**, not relaxed. Following explicit
approval to prioritize missing-content restoration while reporting the mismatch,
an oversized-only best-effort fallback uses a finite consumer output viewport.
It retains full geometry/near-plane checks, world-space 3D coordinates and camera
position/zoom; only planar placement and the occurrence anchor follow the source
origin. Already representable sources keep their existing canvas. Nonpointwise
owner/consumer effects, unsafe transforms/clocks/references, mixed native parent
chains and singular inverse scales retain full bounds and contextual rejection.
An already-omitted Chromatic Aberration control does not expand native demand;
its omission diagnostic remains. No child effect input is cropped, no 3D collapse
is enabled and no raster/media replacement is authored.

**Approximation:** changing the native raster domain can change edge/color/alpha
quantization (measured above), so every rescued source emits an explicit
`Oversized 3D source uses a finite consumer output viewport` diagnostic. A cached
private editable-input experiment retained the corridor's 113 owners plus camera;
Adobe rendered 61 frames covering 5–7 seconds at 1920×1080/30fps, all fully decoded.
Visual inspection at 5.0/5.5/6.0 seconds showed the formerly missing photo-card
tunnel and subsequent transition.

The final normal-build check freshly converted the unchanged original input in
**138.01 seconds**, with **1,814,478,848 bytes maximum RSS**. The three previously
omitted source groups now have 4096×4096 canvases; the corridor retains 113 owners
plus its camera. Fresh Adobe output at **1920×1080/30fps** covered native frames
150–210 (5–7 seconds inclusive): **61 frames fully decoded**, with visual checks
at 5.0, 5.5, 6.0, 6.433, 6.5 and 7.0 seconds showing the restored tunnel and following
transition. The source hash was unchanged. Private media is not published.
This is fresh-input missing-content restoration evidence, **not pixel-exact
fidelity or a full-duration visual regression pass**. The hierarchy CPU panel
passed **67 tests**; disabling the fallback reproduces subtree omission, and
reintroducing the incorrect rejection of planar separated-position sidecars
also fails its dedicated regression. Formatting, support-ledger validation and
CLI build passed. No clean Clippy/full-suite claim is made.

Experimental native sources/samples remain in local scratch, not a published
fixture corpus. Their 30fps MP4 publication, long-term Asset/fresh-download
verification and public feature-case fresh-export Adobe proof remain
**unrun/incomplete**. CPU consumer-viewport tests are supplementary structure and
safety regressions, not independent Adobe-native conversion proof. Import is
unchanged by this export repair.

Executable tests: `export_document::hierarchy::collapsed_tests` (native switch
and source identity, fresh editable export, oversized geometry, hard-mask output,
3D ancestor preservation and unsafe/soft/inverted-mask negatives), plus
`schema::layer_records::tests::fresh_collapse_switch_preserves_every_other_byte`.
The four initial behavior regressions fail with collapse/output-support behavior
disabled and pass with it enabled. Independent native capability evidence must
not be relabeled as an Adobe render of converter output.

## Intro frames108/603 follow-up: isolated import proof, master publication blocked

This import-only increment follows merged PR #4565. It changes only the AE
converter. **Export is unchanged and unproved.** The source/reference hashes,
licensed local provenance and no-Adobe/no-upload restrictions are the same as
those recorded in the historical section below. No new 30fps Adobe render,
long-term Asset publication/download verification, Adobe control inspection or
alpha proof was performed; those required proof steps remain incomplete.

| Local case / native target | Implementation and executed editable assertions | Visual evidence / remaining limits |
| --- | --- | --- |
| `ordinary-intro-108-keyed-cap-alias`: composition 596/layer 644; supporting composition 3/layer 472 | An exact enabled direct Transform alias supersedes valid stored destination keys. The native source X key is536 rather than stale destination300; the imported editable key is536 at4417 ms. Sparse Rotation resolves to zero; split-axis Scale replacement clears obsolete vector keys. `control_links::cross_comp::tests::local_frame108_keyed_cap_alias_uses_source_curve_not_stale_native_keys` passed, along with malformed/disabled/layout and clock guards. | Fresh isolated596 previews at107/108/109 show joined rounded caps and connector. Master backgrounds and central circle are not part of this isolated composition; this is not whole-frame equality. |
| `ordinary-intro-603-post-layer-geometry2`: composition 724/layers742,743,746; supporting source741 and controls729/730/733 | A sole native Geometry2 effect on a planar non-adjustment Shape becomes an editable Group **after** native layer and ancestor transforms. Native point normalization, Height/Width and Uniform Scale, static Rotation/Opacity, exact same-comp `toComp([0,0,0])` and same-effect Position alias are mapped. `geometry2::tests::local_external_source_restores_three_fold_geometry2_stages` pins three effect stages, native pivots/reflections, four editable point tracks without JS, and an analytical Center-origin sample. Disabling the new stage makes this pinned test fail. | Fresh isolated724 previews at master602/603/604 equivalents restore the detached reflected copies around the fold. Green/black regions, missing master color/grain context and unsupported transfer-mode22 remain; neither master geometry nor pixel/alpha fidelity is certified. |

**Defaults are not captured control evidence:** the valid sparse source Shape741
omits Position. Exact Position aliases now reuse the existing occurrence
converter's **source-composition center**, rather than stale destination Y534.71
or an invented zero/offset. The pinned copied-occurrence assertion failed with
`[1920,534.7096557617188]` and passed with `[1920,800]`. The rule excludes AV,
3D/null Shapes and malformed source groups. It does not newly resolve absent
Anchor/Scale/Opacity aliases or change sparse null anchors. Native control values
were not inspected in Adobe.

**Geometry2 approximation/omission ledger:** exact point expressions become
independent sparse linear X/Y keys; live linkage is lost. Existing analytical 2D
Transform chains are sampled on the complete 1 ms grid, with at most60s and a
200,000 combined sampling/refinement-work allowance, and fitted within0.01
composition units on that grid. This is not continuous-time or rendered-pixel
proof. Raster interpolation, clipping and shutter behavior remain approximate.
Mixed effects, masks/styles/preserve-transparency combinations, nonempty effect
compositing options, nondefault skew/shutter/sampling, unsupported scalar or
native point animation, unknown expressions/controls, duplicate controls and
unsafe owners/ancestors retain contextual omission diagnostics and owner content.
Source-stage matte helpers omit this occurrence effect; AllEffects helpers retain
it. Failed graph admission preserves the original Group and rolls back charges;
rollback is covered with a test-injected budget. No renderer/schema change, JS,
frame baking, native-source replay, JSON compaction or format-cap bypass is used.

**Executed validation:** 79 scoped test executions passed across overlapping
control-link, Geometry2, point-fit, effect and Set Matte filters; this is not79
unique tests or a full suite. The two pinned local cases ran explicitly.
Formatting and fresh CLI build passed. Strict all-target AE Clippy remains red:
untouched base `7a41fe07b` and this increment report the same55 error headers;
no lint exceptions or unrelated export/writer fixes were introduced. Six exact
neighbor previews were freshly rendered from two freshly imported archives.
Receipts, logs and coordinate traces are under ignored
`tmp/ordinary-intro/remaining/`; no licensed artifacts are committed/uploaded.

**Still blocked:** a fresh final composition 705 import again fails with
`invalid .tsrct file: project.json exceeds 67108864 bytes`. Main through
`4935dbc60` has no subsequent writer-limit change. The earlier size-policy merge
does not remove this format-v2 publication guard. Thus there is no fresh final
master archive, six-frame master regression panel or858-frame comparison video.
The historical fifth video below is not final-branch evidence. Further
converter-only storage work requires approval; this remains partial progress,
not a completed whole-project fidelity correction.

## Historical six-frame Intro correction (#4565)

The following records the evidence delivered in merged PR #4565. Its remaining
108/603 implementation gaps are updated by the follow-up above; its historical
video is not a fresh render of that follow-up.

This section supersedes the implementation limits and residual inventory of the
**earlier five-frame milestone below**. Scope is the user-approved **AEP → editable
FX import** correction, including original PNG sequences. **FX → AEP export is
unchanged and unproved by this work.** No shared FX model, schema, evaluation,
editor or renderer change, generated `JsScript`, general expression runtime,
frame baking or hidden native-source replay is used.

Native source SHA-256:
`28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d`;
composition 705, 3840×1600, 24fps, 858 frames. Existing independent Adobe reference
SHA-256: `80cdbfb0b0d79827d7d4f4c4d9f655386b92d688fc23788edaf51baa3d9ccebb`.
These licensed inputs and local comparison videos are **not redistributed**.
No Adobe operation, new 30fps reference, Asset upload, remote hash verification,
Adobe control inspection or independent export proof was performed. RGB video
comparison does not prove alpha fidelity.

| Import feature | Original semantics → replacement; bounds and impact |
| --- | --- |
| Cross-composition Transform aliases | Exact named composition/layer/member references copy supported editable values/curves, preserving positive compatible clocks and native temporal controls. Only the requested dependency is recursively resolved, with cycle/depth and unique-name guards. Unknown syntax, unsafe clocks and incompatible Anchor unit contracts retain diagnostics/fallbacks. Direct same-member X/Y Position, Rotation and Scale identities are recognized before sibling-reference parsing. Live cross-layer linkage is lost. |
| Cross-composition Path copies | Exact named source composition/layer/contents references can copy the bounded analytical Path mapping into independently editable destination keys. Destination active intervals are rebased to the source clock rather than fitting invisible source history. Arbitrary expressions, topology and unsupported clocks are not accepted. |
| Dynamic Path fitting | The former 2,048-key rejection, shared 1 MiB track cap and aggregate animation-byte quota are removed. Structural validation and checked accounting remain. The 60-second, 200,000-evaluation and dependency/depth bounds remain. Generated coordinates are rounded to four decimal places only when displacement is at most 0.0001 owner-local units; adaptive fitting and the complete 1 ms grid then compare those compact endpoints against **unrounded** analytical controls at the unchanged 0.01-unit tolerance. Opaque key IDs retain owner/time uniqueness with a shorter prefix; redundant straight closing segments use the existing Close command, while curved closing segments remain explicit. These latter two changes are lossless storage reductions, not relaxed limits. This is not a rendered-pixel or continuous-time guarantee. Budget/unsupported failures retain diagnosed original outlines. |
| Source-relative anchors | Explicit/keyed Solid and precomposition Anchor coordinates use physical source dimensions. Resolved constant aliases are normalized from their resolved values too, including ordinary occurrences and transform-only parent copies; reparsing the original expression alone would leave fractional values in pixel fields. Null render bounds remain zero; explicit null Solid anchors use physical dimensions, while sparse null anchors retain the existing zero default. The earlier investigative inference that every sparse null needs a source-center default was rejected. Shape/pixel-space anchors are not scaled. |
| Sibling Rotation | Both `constant + sibling.rotation` and `sibling.rotation + constant` use the existing bounded mapping; omitted native Rotation means zero. Other expression syntax is not executed. |
| Preserve Underlying Transparency | Overlapping prior ordinary paint and its matte dependencies form an independently editable, shared alpha helper for bounded supported stacks (128-provider/depth and dependency guards). Unsupported stacks retain diagnosed SourceOver. **This is an approximation:** masked SourceOver agrees on opaque underlying interiors but can gain alpha on antialiased/partially transparent edges; it is not exact AE SourceAtop. Underlying edits are not live-linked to the helper. |
| Set Matte sampling stage | `tdpi` source identity and `tdps` stage are separate. Source-stage helpers omit the provider occurrence's masks/effects/styles but retain nested source content and effects; AllEffects behavior remains. Static Alpha/default Alpha and explicit Luma are supported for eligible 2D shape/precomposition providers. Helpers are silent and their AudioVolume animations are removed; the visible provider is independent. Unknown controls/stages retain contextual omissions. |
| Leading Subtract mask | AE starts subtraction from full layer coverage, whereas the existing FX/PAG stack starts from the path. A first contributing, full-static-opacity Subtract becomes Add with toggled inversion. Later Subtract masks retain their order/mode. Non-unit or animated/expression opacity retains the diagnosed starting-shape approximation; runtime mask behavior is unchanged. |
| PNG sequences | Validated original PNG bytes become timed, editable ImageLayers in the fixed footage canvas, with source/frame-stable asset identities and contiguous rounded native/conform-clock boundaries. Missing/malformed frames remain blank only during their own interval. Untrusted padding and generated paths are bounded before allocation. No first-frame substitution or image rewriting is used. |
| Transform effect mapping at #4565 | Native `ADBE Geometry2` was diagnosed and omitted in that delivery, including composition 724/layers742/743/746. The bounded planar Shape mapping in the follow-up above supersedes this implementation gap; its independent master-render/control proof remains incomplete. |
| Mixed PNG dimensions | Original frames 0–56 are 7680×3200 and are imported. Frames 57–70 are 3840×1600 and remain **diagnosed omissions**: the existing master reference never exposes this transition and cannot establish whether AE stretches, pads or crops them. General mixed-size sequence support is incomplete; alpha fidelity is unverified. |

The exact reported samples are native frames **49 / 108 / 163 / 448 / 542 / 603**
(2.041667 / 4.5 / 6.791667 / 18.666667 / 22.583333 / 25.125 seconds), not rounded
integer-second substitutes. The latest inspected fifth integrated preview has:

- **49:** long diagonal extensions and curved protrusions removed; grain and a faint edge seam differ.
- **108:** central editable connector restored; detached rounded ends remain.
- **163 and 542:** major geometry restored/aligned; texture and tone differ.
- **448:** missing bars and angled occurrence restored after the leading-mask and
  precomposition-anchor corrections; texture/edge differences remain.
- **603:** background restored, but fold placement/shape remains incorrect.

These are **partial improvements, not six fidelity passes**. Earlier full imports
exposed aggregate animation-budget denials in later helper copies. The fifth full
composition-705 import emitted no animation-budget or dynamic-Path-lowering
denials after the bounded coordinate and lossless storage compaction; all six
exact-frame previews were rendered and inspected. The two substantial geometry
mismatches above remain even after those denials are removed.

**Main-integration blocker:** the successful fifth archive/previews were produced
before transfer onto main `1ddc4b609a9602f9d423b65a6786bf7c05b2dea8`. Its
`project.json` is 75,918,650 bytes. Fresh full-project import on the main-based
follow-up fails with `invalid .tsrct file: project.json exceeds 67108864 bytes`.
The starting local revision `8d692beda` had removed document caps, but #4554
actually merged head `8a2bb939`, whose later review fixes deliberately retain
format-v2 legacy-reader write limits. The 64 MiB total-document limit is separate
from the animation budget; those merged compatibility guards have not been
reverted or bypassed. Final-branch archive/render equivalence is therefore
**unproved**, despite 151 scoped test executions and the dev build passing.
That draft increment was subsequently merged as #4565; the final full-project
publication failure is reproduced by the follow-up above. Further converter-only
storage/admission work needs design and semantic validation; removing animation
or changing shared format limits is not an accepted workaround.

At frame 448 the complete native Time Remap selects **PNG frame 25**, not frame
22: master source time 0.8125s plus nested 0.25s is 1.0625s. The earlier frame-22
structural test proves that child's interval/bytes, not its selection at frame 448.

Focused regression evidence includes:
`local_ordinary_intro_opening_aliases_keep_curves_clocks_and_hierarchy`,
`local_external_source_restores_frame_108_cross_composition_connector`,
`local_external_source_lowers_noise_from_source_stage_luma`,
`leading_opaque_subtract_inverts_coverage_but_later_subtract_does_not`,
`local_external_source_leading_subtract_keeps_outside_the_native_mask`,
`precomposition_anchor_storage_is_normalized_to_source_pixels`,
`local_external_source_restores_rotated_precomposition_anchor`, and
`compact_generated_coordinates_remain_within_the_original_fit_tolerance`, and
`constant_anchor_aliases_use_pixels_in_occurrences_and_parent_copies`.
The constant-alias regression failed with `[0.5,0.25]` instead of `[50,25]`
and passed after resolved-value normalization (Solid/precomposition, ordinary
occurrence/parent copy, direct cross-composition alias/same-member identity).
The pinned leading-mask test failed before its normalization and passed after;
the precomposition-anchor regression failed with fractional coordinates in the
pixel field and passed after source scaling. Licensed tests remain local/ignored;
normal parser, unit, guard and serialization tests do not substitute for Adobe
feature-level render proof. Local receipts and comparison artifacts are under
`tmp/ordinary-intro/followup/` (ignored). No full rendering suite was run locally.

## Intro follow-up: parent Scale, sibling Rotation and expression-driven Paths

This corrective milestone concerns **AEP → editable FX import**. The user has
explicitly included bounded dynamic Path conversion in this follow-up; the former
exclusion in the historical Intro notes below is no longer the task boundary.
This does not authorize a general expression runtime, generated `JsScript`,
per-frame baking, or changes to the shared FX schema/evaluator/editor/renderer.
Existing native editable ShapePath animation is the destination representation.
FX → AEP export is unchanged by this follow-up; independent export/control/render
proof for these expression-derived results remains **unrun / unmeasured**.

The local native source remains SHA-256
`28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d`.
Licensed source/media are not redistributed. The five reported targets are:

| Target | Original semantics and prior replacement | Corrective mapping / evidence status |
| --- | --- | --- |
| 2s, comp 3 guides 20/32/33/34 | The stock parent-Scale reciprocal cancels visual scaling while retaining parent translation/rotation. Omitting it shrank descendants by the parent's initial 0.001% Scale. | Independent editable ScaleX/Y reciprocal curves; parent hierarchy remains intact. Finite direct 2D parents and equal positive clocks only. Animated values use bounded sparse analytical fitting with effective product error at most 0.01 percentage points on a 1 ms grid before FX clock quantization; live parent linkage is lost. Hash-pinned native guide/cancellation assertions passed. The 2s rendered sample closely matches the reference (diagnostic RGB MAE 0.031/255); other times are not implied proven. |
| 4s, comp 3 layer 392, source 391 | `90 + sibling.rotation` was replaced by identity Rotation, breaking the mirrored pair. | Copy the uniquely identified sibling's raw native scalar curve and add the constant to values only, preserving temporal speeds/influences and rebasing equal-stretch clocks. No recursive expression sources. Live sibling linkage becomes independent keys. Hash-pinned native offset/key assertions passed. The 4s rendered sample still differs in surrounding geometry (diagnostic RGB MAE 7.641/255); not a fidelity pass. |
| 8s, comp 3 circle 2209 / inverted matte 2210 | The circle exists; the matte's expression-driven four-point Path retained its initial 500×500 square and fully hid it. | Exact stock Layer-Control/coordinate-conversion mapping emits editable Path keys; signed scalar property aliases and equal-endpoint Bezier excursions resolve its controller dependencies. The pinned native target now has a changing, non-square four-vertex matte Path; the matte is not removed. Structural assertions passed, but the 8s rendered sample still lacks the large circular region visible in Adobe (diagnostic RGB MAE 40.272/255). This target remains visually unresolved. |
| 20s, comp 2420 connectors 2459/2466/2468 | A two-point Path follows circle anchors 2455/2456 through parent 2458. The retained horizontal initial line loses both direction and length; no connector Rotation expression exists. | All three connectors have editable two-vertex Path tracks. At 417 ms their endpoints match independently recorded CPU circle centers within 2 px (including clock rounding), and their vertical span exceeds 300 px. Existing stroke-width animation remains independent. Native structural/numeric assertions passed. The 20s rendered connector is diagonal and closely aligned with the reference; tone/noise differences remain (diagnostic RGB MAE 0.845/255), not exact pixel equivalence. |
| 25s, comp 724 paths 741/747/750 and copies 742/743/746 | Four-point coordinate-conversion paths and live path copies form the folds. Initial square outlines survive the earlier Set Matte correction, still hiding the circle and giving wrong geometry. | Stock Path construction and selector-validated sibling copies emit independent editable keys. The exact Position blend and Vert-minus-Vert-Offset Slider profiles resolve the fold rig; repeated controls and the stock self-vertex exclusion are preserved. All six native paths pass geometry assertions. Set Matte's separate sampling/default assumptions remain; other effects are not implied fixed. The 25s render still has incorrect fold placement/geometry and background tone (diagnostic RGB MAE 59.601/255). This target remains visually unresolved. |

Executed CPU checks: **47 control-link tests and 16 dynamic-Path tests passed**, including the hash-pinned native cases
`local_external_source_restores_comp3_guide_parent_scale_cancellation`,
`local_external_source_resolves_the_comp3_property_alias_chains`,
`local_external_source_restores_bounded_dynamic_path_families`, and
`local_external_source_resolves_fold_position_blends_and_transitive_rig`.
The source/reference remain local educational inputs, not redistributable fixtures.
The immutable existing Adobe reference is SHA-256
`80cdbfb0b0d79827d7d4f4c4d9f655386b92d688fc23788edaf51baa3d9ccebb`;
no new Adobe operation or Asset publication was performed.

Dynamic Paths fit analytical controls adaptively, not at video-frame cadence:
60-second interval, 2,048 keys, 200,000 evaluations, 256 transform dependencies
and depth 16. A 1 ms grid checks at most 0.01 **owner-local coordinate units**
before FX clock quantization, not rendered pixels or a continuous-time guarantee.
Native guide/controller hierarchy is retained, but expression relationships become
independently editable Path keys: editing a guide no longer drives the copied Path.
The Position-blend helper resolves Path dependencies; it does not install a general
Position expression evaluator or preserve that guide's live destination linkage.
Nested Path copies, unknown/executable expression variants, ambiguous selectors,
unsafe transforms/clocks, and over-budget fits retain diagnosed initial outlines.
In the native composition-3 run, targets 2137/2146–2150/2155–2156/2158/2199/2200
still exceed the sparse-key cap; 2157 and 474 retain unresolved-controller fallbacks.
These residuals are **not fidelity passes** and are not grounds to increase caps
silently or weaken the tolerance.

The aggregate RIFX chunk ceiling is 300,000 (previously 200,000): this unchanged
native input has 273,431 chunks, depth 16 and 10,376,814 bytes. Global byte/depth
bounds remain unchanged; synthetic acceptance and aggregate-bound rejection are
covered by **10 passing RIFX tests**, including over-limit rejection. Scoped
`aftereffects_file` all-target Clippy (`-D warnings`), formatting, the support-ledger
check, and a fresh converter build passed. No full workspace or GPU suite was run.
The first full-master publication failed the unchanged 64 MiB archive limit
because typed JSON serialization pretty-printed deeply nested tracks. The AEP
adapter now supplies compact JSON to the existing validated archive constructor:
no keys, geometry or hierarchy are removed, and no shared schema/reader/limit
changes. The archive-byte regression failed before this correction and passed
afterward; all **20 adapter tests passed**. A fresh complete composition-705
publication succeeded with **30,712,516 bytes** of `project.json`.
A full render at commit `a47e7ffeeee34885dd154f5947cd6be3b732e5f6` completed:
3840×1600, 24fps, 35.75 seconds, 858 frames, with successful full decoding.
The Adobe-left/FX-right comparison is 3840×800 and also decodes all 858 frames.
Local artifact: `tmp/ordinary-intro/fixed-five-bugs-a47e7ffee-render/side-by-side.mp4`.
The five critical samples were visually inspected; **4s, 8s and 25s retain
visible mismatches**. Reported MAEs compare encoded halves resized to 960×400;
they are diagnostics, not an approved fidelity threshold or alpha proof.
The previous comparison file was no longer available locally for numerical
before/after scoring. The subsequent archive-limit investigation and conservative transform-review
guards are not included in this render's source SHA; native structural tests were
rerun, but this comparison is not a final-HEAD rerender.

That historical integration retained 64 MiB project / 1 MiB metadata writer
caps and 256 MiB / 16 MiB reader caps. The subsequent quota cleanup removes
those caps, archive asset/count/path policies (except actual ZIP name width),
and the shared 1 MiB FX track cap. Declared lengths, SHA-256, CRC, schema and
regular-file checks remain. Older capped readers may reject newly admitted
output. The original internal capacity report is no longer distributed;
the original 27-test run is historical, not
validation of this cleanup. Browser runtime execution and fresh Adobe fidelity
comparisons are not established by a WASM compile check.

Analytical agreement, native structural assertions and video
decoding must not be reported as independent Adobe pixel/alpha fidelity.

## Export repair checkpoint — incomplete proof

### Latest WIP follow-up (supersedes older checkpoint results below)

The pending export-only clock, native WAVE source/alias, and narrowly eligible
source-less vector Group repairs are now included together as a WIP checkpoint.
Import is unchanged. This is not merge-ready or a claim that all repairs pass.

- Selected-rate dispatch reaches vector, footage, camera, mask, text and style
  writers. The comp-17 independent RGB comparison now passes all 60 frames at
  the unchanged 0.99 floor (minimum 0.995591; previously 0.904684). Remaining
  review findings include default-clock Time Remap planning validation and
  Gradient Colors descriptors; arbitrary fractional-rate compatibility remains
  unresolved.
- Exact WAVE metadata, native source/layer switches and explicit `./media/`
  aliases restore native audio in the fresh private export, without external
  muxing. The two-second waveform diagnostic has a constant 1,025-sample delay;
  its origin is unproved. Audio fidelity is not passed. Review still flags
  44.1 kHz duration compatibility, non-WAVE aliases, platform metadata and format
  coverage. Twelve footage tests passed. During main-branch integration the
  raw-path expectations were updated to the explicit `./media/` alias; all 21
  audio tests now pass. This does not resolve the remaining audio proof gaps.
- Narrow effectful direct-Shape Groups retain large editable geometry without
  an intermediate source canvas; three targeted structural tests pass. Native
  effect/alpha equivalence is unproved, and oversized ancestors remain omitted.
- Fresh original-Tesseract and current-AEP renders each decoded 450 frames at
  1920×1080/30fps over 15 seconds. Their labeled side-by-side still shows major
  background and color differences. Original CustomShader content is present
  only on the Tesseract side; not all differences are attributed to it.
- Private inputs and comparison videos remain local, not committed or uploaded.
  Final full validation, lint cleanup and independent proof remain incomplete;
  this checkpoint must not be treated as a clean-gate release.


This export-only increment does not add import mappings or change FX rendering.
The pinned [first-three-case checkpoint](../crates/aftereffects_file/tests/fixtures/export_repairs/export-checkpoint-v1.json)
records actual fresh-export Adobe readback and RGB results, not general fidelity.
Remaining repairs require final-code reverification; no full-project pass is claimed.
This is a **draft/WIP checkpoint, not merge-ready**. A private 15-second preview
was rendered after removing invalid generated audio records and separately muxing
the original audio. That workaround is neither a successful raw AEP export nor
editable native audio proof; the private source and preview are not published.

| Export feature | Original → replacement; reason and impact | Current evidence / limitation |
|---|---|---|
| Compound static Path | One multi-contour outline → editable native contours under the same paint/modifier scope, preserving order, closure and winding rather than omitting the layer. | Native comp 1: 60-frame RGB minimum **0.999625** at checkpoint v1. Hidden empty-path Trim guide 4 is still diagnosed as omitted; complete control linkage and alpha remain unverified. |
| Compound Boolean operand | Multiple contours remain **one** identity-transformed native Group with inner Merge mode 1, not separate outer operands or an inner Add. Subtract retains the existing FX-to-AE operand reversal. | [Independent native source](../crates/aftereffects_file/tests/fixtures/export_repairs/compound_boolean/provenance.json): disjoint/overlapping contours and a reverse-winding hole, Subtract, Intersect and Exclude. Native interior geometry checks passed. Source-backed scope regression failed before and passed after; six focused compound tests passed. Fresh-export Adobe comparison and long-term reference publication **pending**. Wrapper depth/work limits remain enforced. |
| Paired Scale tracks | Constant depth inherits compatible easing; independent axis knots are merged by exact curve subdivision, without dense resampling or weakened validation. | Comp 34: 60-frame RGB minimum **0.992457**. Comp 17: **failure**, minimum **0.904684** at frame 33. CPU millisecond readback hid the native timing error. |
| Composition FPS / property clocks | Generated compositions inherit selected FPS, including supplied records. | Adobe confirms all selected/generated comps at 30fps. Partial shared-clock repair now writes the comp-17 Solid Scale descriptor at 30,720 ticks/second with raw Hold times 7,680 / 33,792 / 50,688. The fresh export opens in Adobe and frame 33 renders; the unchanged reference has **not yet been rescored**. Shape/vector-program, footage, camera and layer-style paths retain legacy clocks. Unprobed fractional rates currently fail instead of preserving prior behavior: a compatibility blocker before merge. FPS propagation alone is not timing fidelity. |
| Constant ShapePath and hidden controls | Effective constant geometry replaces stale base geometry in supported consumers. Hidden Groups do not inflate rendering bounds; empty, effect/matte-free hidden control Groups can remain editable disabled Nulls. | Targeted CPU assertions passed. Native Path-key animation remains excluded; no new script or renderer capability is introduced. Complete native proof remains pending. |
| Audio captions flag | Explicit `captionsEnabled: false` follows the absent-flag path, instead of omitting otherwise supported audio. `true` and unsupported semantic controls remain diagnosed. | CPU variants passed, but a fresh audio-only export is **rejected by Adobe** with two skipped sections and missing data. Removing its generated Audio layer and file source permits the visual project to open. Native file-source records remain a blocker; audio range/alignment/fidelity are unverified. External preview audio muxing is not an exporter fix. |
| Static Stroke Dash/Gap descriptor (FX → AEP) | The previously emitted generic Scalar, missing native range records and redundant zero Offset could make a nonempty generated AEP fail to open. Dash/Gap and present Offset now use Adobe's vector-scalar descriptor and native 0–100 UI range records; zero Offset is omitted when unkeyed, preserving the same static phase. The editable stroke and supported positive patterns are retained, not flattened or dropped. This repairs native serialization, not a new source-feature mapping or a promise of visual equality. | `writer::dashes::tests::static_dash_descriptors_match_independent_adobe_source` pins an Adobe-authored DASH_GAP source (`shapes/import_stroke_dash_caps.aep`, composition 122) and failed before the correction, then passed; focused static dash tests passed. A private 77.9s source AEP initially failed Adobe 26.5x89 with one skipped section / missing data; removing the single dashed stroke avoided it. Fresh corrected full AEP opened and rendered its first 60 frames at 1920×1080/30fps. A separate **4.6s excerpt copy**, with the first scene, background and audio only, opened and rendered all 138 frames in Adobe 26.5x89 at native canvas/30fps. No independent reference comparison, alpha fidelity, native control UI readback, audio fidelity or full-length render is claimed; the excerpt copy is local/ignored, and other source omissions remain diagnosed. The historical audio-only failure above is not resolved by this mixed-content smoke. |
| Mosaic bounds | Enclosure includes lower-stack support **and** the explicit sampling canvas, rather than guessing from Adjustment guide geometry. | CPU bounds evidence only. Independent Mosaic grid, clipping and render equivalence remain **unmeasured**. Other unproved spatial effects retain diagnostics. |
| Static planar Group skew | Restricted nonsingular, static owner transforms without mapped effects or masks use a signed-SVD helper Null and ordinary precomp transform. Child order/mattes/effects and clocks are retained by the implementation; unsupported owners remain omitted contextually. | Targeted matrix, bounds, ID and clock tests passed. Review identified possible clipping of child effect expansion by geometry-only bounds. Short/offset skew Groups now use the existing source-clock normalization before spatial classification; the targeted regression failed before and four skew tests passed after, including child clocks, parenting and mattes. The previous full-span omission is gone. A subsequent 2D Null-anchor writer fix normalizes pixel anchors, key values and spatial tangents by the Null's 100 × 100 source size, as required by native AV storage; Z and times remain unchanged. Previously the helper's pixel anchor was interpreted at 100× displacement, moving its children off canvas. `writer::rects::hierarchy_writer_tests::null_anchor_uses_native_av_source_units` fails before and passes after; it pins the independent `effects/transform_probe.aep` AV-unit oracle (SHA-256 `d669d0ca3d505ebaca545d6866185f6f6e69fe00256024bd0cb9ee22ba1a67d7`, composition 1 / layer 15), and checks generated static/keyed Null anchors and tangents. A fresh private export (with the documented audio workaround) now visibly restores the wordmark at 14 seconds in Adobe. This is one-frame smoke evidence, **not** a scored native Null/skew oracle or full-animation fidelity proof. The follow-up `null_sidecar_anchor_uses_source_units_without_mutating_input` regression now also fails before / passes after normalizing Null Transform-sidecar anchors for planar and 3D records: static values, keyed XY and spatial tangents use the actual Null source dimensions, while Z, key time and caller input remain unchanged. This closes the serializer's corresponding sidecar omission structurally; fresh independent Adobe 3D/sidecar control and render proof remains pending. Independent Adobe controls/render proof remains pending; no general skew/3D fidelity claim. |
| Final-root output viewport | A static, canonical-identity Group at depth zero, with 2D or projected children, may use the final composition viewport instead of enclosing every offscreen projected pixel. Children and effect inputs remain uncropped; the existing root lens, clocks and support/near-plane validation remain. Owner effects, masks, motion blur, animated transforms, external image references and spatial root Adjustments reject this path. General/nested cropping is not enabled. | Structural regression failed with a 39,824 × 2,178 source for a 319 × 179 output, then passed with exact output dimensions, retained child and root camera (including odd dimensions). Focused rejection tests cover transforms, effects, motion blur, animation, matte consumers, spatial Adjustments and near-plane failures. A fresh private export restores the formerly blank spatial scene at one sampled Adobe frame; this is **partial smoke evidence**, not an independent pinned native feature/render/alpha proof. Pointwise Grain/Vignette retain their existing documented approximations. The additional `final_root_viewport_keeps_oversized_2d_child_and_clock_without_camera` regression is RED before / GREEN after: exact output rectangle, retained huge-scale child and occurrence clock, no added camera. A private Scene 4 sample remained blank after only the outer viewport repair because a nested source was still omitted. |
| Uniform Scale animation through fixed Group skew (export only) | Equal base XY scales and identical merged XY value/easing curves use a fixed signed-SVD basis. A zero base with uniform Scale keys is factored at unit scale, with the authored zero retained on the outer transform; nonzero bases retain the scalar-ratio path. Planar Position and Opacity tracks are retained alongside Scale. Original knots, interpolation and Z are retained; no animated SVD or frame resampling. Nonuniform Group curves and other unsupported animated transforms remain rejected. Already-unmapped owner effects are omitted with their existing contextual diagnostics before skew eligibility; mapped owner effects/styles and masks retain their phase guards. This does **not** implement Chromatic Aberration or export its controls/keys. | `uniform_scale_keys_preserve_static_skew_factorization` and `unmapped_owner_effect_does_not_discard_static_skew_content` each fail before / pass after. They check factored native keys, unchanged merged interpolation, helper parenting, matte/child Glow retention, and rejection of differing XY easing or mapped owner Glow. A fresh private Adobe root frame at 7.8s now visibly contains the readable skewed headline, unlike the preceding same-time export. This is visibility smoke evidence only: independent animated native-reference RGB/alpha fidelity is **unmeasured**, and descendant effect clipping remains a known limitation. The follow-up `zero_base_skew_keeps_flying_headline_position_opacity_and_uniform_scale` regression is RED before / GREEN after, and the flying headline is now visible in a fresh seven-frame private Adobe smoke at 7.3–7.5s. That combined candidate also includes pending vector-clock repairs; it is not isolated commit-level or independent native-reference fidelity proof. Deeper oversized backgrounds, other animated skew cases and native audio remain incomplete. Import is unchanged and was not revalidated. |
| Nonuniform Shape Scale / Opacity through fixed skew (export only) | Skewed Shape/Rectangle/Boolean paint programs can carry supported Scale and Opacity keys on the existing native vector-group Transform instead of rejecting their owner. Lowering rebuilds keys from the correct source owner through `vector_animation::group_animations`: vector Scale uses two percentage components and Opacity uses percentages, not AV-layer fractions/three-component Scale. Animated Anchor/Position/Rotation remain outside this narrow path; source-owned Skew/Skew Axis keys are covered by the separate row below. No spatial interpolation or unsupported key is silently rewritten. | `skewed_shockwave_keeps_nonuniform_scale_and_opacity_in_vector_group` is RED before / GREEN after parent integration, asserting exact values, arity, times and interpolation in the writer program. The combined seven-frame private Adobe smoke at 7.3–7.5s renders and visibly includes the restored headline and glowing outline. This is acceptance/visibility evidence, not independent Adobe control inspection or RGB/alpha fidelity. Descendant Glow crop bounds and overrange-paint saturation remain explicitly diagnosed and unproved. Import unchanged. |
| Source-owned vector Skew / Skew Axis keys (export only) | Eligible planar Shape, Rectangle and Boolean paint programs now retain authored Skew/Skew Axis tracks on their native vector Transform, including when both static base values are zero. Rectangles with an imported-solid hint are routed to a vector paint program rather than silently consuming these keys in a solid source. Duplicate owned Transform targets reject; geometry/paint and foreign-owner tracks remain separately owned. Native 3D and combined animated Anchor/Position/Rotation remain diagnosed rather than rewritten. | `source_owned_skew_keys_survive_fresh_vector_writer_with_zero_static_skew` was RED before parent dispatch integration and is GREEN after. Six focused vector-animation tests pass, including Rectangle/Boolean routing and rejection guards. `pinned_native_vector_group_skew_keys_identify_editable_property` checks independent native `shapes/import_group_transform_controls.aep` (SHA-256 `ea4ba57dc9d672d4e321208298541018464b88a2fce11c25a4ff9ff35d371ed5`), compositions 137/167. A separate **synthetic** fresh export opens and renders nine 320×180 Adobe frames at 30fps; inspected first/last frames visibly change the triangle's skew/axis and paint. This is acceptance/motion smoke evidence, **not** independent Adobe control readback or native-reference RGB/alpha fidelity; no new reference was published. It does not solve oversized ancestor omissions or Group-level animated skew. Import unchanged and not revalidated. |
| Nested owning-source clocks | Recursive lowering now uses the generated precomposition's exact source end and native duration, then restores the enclosing clock for siblings. A full-source descendant is no longer mistaken for a short root-timeline occurrence and forced into an unnecessary precomposition; existing eligibility rules still decide whether editable Null parenting is valid. | `nested_full_source_group_uses_owning_precomposition_clock` is RED before / GREEN after, checking retained children, parent links, unchanged local keys, source duration and a following root sibling's clock. One fresh private Adobe frame at 7.5s now contains background/fragment content rather than only the HUD. Scene 4 is **not restored completely**: oversized deeper content and unsupported skew/effect combinations remain omitted. No new nested crop was enabled; full-animation/native-reference fidelity remains unmeasured. |
| Oversized hierarchy / effects | General consumer-demand cropping, Chromatic Aberration and animated HSV saturation are not yet implemented/proved. No global canvas clamp, larger limit or silent substitution is introduced. | Fail-closed demand propagation is implemented and CPU-tested. Only the separately listed final-root spatial output case is enabled; oversized nested hierarchies and other unsupported scenes remain omitted. A native Chromatic channel-assembly candidate failed lossless RGB/alpha checks and is not a product mapping. Native Hue/Saturation can store Hold/Cubic composite Channel Range keys, but its color transfer differs from FX's multiplicative HSV saturation; keyability is **not** an exact mapping. CustomShader remains excluded. |

Source reference publication, source Adobe rendering, fresh-export structure,
fresh-export RGB, alpha and audio are separate gates. The new Boolean source has
not yet completed its publication or fresh-export gates. No thresholds or pinned
references were changed to turn a failure into a pass.

## Intro defects 5–7: bounded Rectangle, delayed Position and Scale-sum import

These are **import-only corrections**; FX → AEP export is unchanged and no new
export proof is claimed. Native source SHA-256 is
`28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d`.
The licensed source/reference remain local, unmodified and unpublished. No Adobe
operation, new video export, Asset publication or visual comparison was performed.
The prior comparison predates these fixes. RGB/alpha/audio and independent native
control inspection remain **unverified**.

- **33 seconds, comp 538/layer 539 (defect 5):** the exact Rigged Box Size,
  Position and Roundness formulas resolve to native editable Rect controls.
  Initial size is 45×35, roundness 20, with five size keys; Position/anchor curves
  remain editable. Independently timed axes may be combined only where inserted
  keys fall on constant segments; incompatible changing intervals retain the
  diagnosed static-outline fallback. One enabled paint plus disabled paints can
  use native Rect/Shape lowering without re-enabling a disabled fill or changing
  paint order. Sparse X Anchor slot 4 is **assumed zero for the recognized stock
  profile**, explicitly diagnosed; independent control readback is absent.
  Live pseudo-effect controller linkage becomes independent destination keys.
- **31 seconds, comp 873/layer 885 (defect 6):** this is the displaced circle
  with stored Position `[368,216,0]`, not the earlier guessed layer 882. The exact
  delayed `Master.position.valueAtTime(...)`/`easeOut`/two-key-Time-Remap family
  becomes sparse editable Position keys. X is restored to 1920. Layer/controller
  start and stretch, frame-duration delay, native temporal easing and straight
  spatial speed semantics are accounted for. Nonzero spatial tangents, unresolved
  controllers, ambiguous names and unsupported clocks retain diagnostics. Fitting
  is bounded to 60 seconds/128 keys/200,000 fitter evaluations; a 1 ms verification
  grid rejects errors above 0.01 px against the converter's analytical model.
  This is **not an Adobe pixel-error guarantee**: easeOut uses Lottie's compatible
  `(0.167,0.167,0.667,1)` curve, and millisecond FX clock quantization/independent
  native samples remain unverified. Controller edit linkage is not retained.
- **25 seconds, comp 724/layers 729 and 749 (defect 7, partial):** the exact
  same-layer `value[0] + Slider` repeated-Scale forms restore both controller and
  circle Scale animation, including numeric Slider index 1. Constant offsets
  preserve authored keys/easing; two animated inputs are fitted to sparse keys
  with a 0.0001 native-fraction (0.01 percentage-point) 1 ms model-error check and
  the same duration/key/work caps. This fixes lost Scale controls, **not the
  entire blank-frame report**. Layers 741/747/750 build paths with `createPath`,
  `toComp` and `fromCompToSurface`; 742/743/746 copy a live path. That dynamic Path
  mapping remains explicitly excluded. Cross-layer transform/property formulas,
  Geometry2 adjustment behavior and transfer-mode differences also remain.
  Actual FX CPU scene evaluation subsequently identified the blank-frame cause:
  layers 739/740 overpaint the canvas because their Set Matte3 gates were omitted.
  The bounded structural correction below is separate from excluded dynamic Path.

Pinned-source CPU assertions (explicitly ignored without separately supplied
licensed inputs; a skipped case is not a pass):

- `local_external_source_imports_comp538_layer539_as_rigged_box_rect`
  (`AEP_RIGGED_BOX_SOURCE`), including disabled fill and editable Rect keys;
- `local_external_source_restores_displaced_circle_and_neighbors`
  (`AEP_DELAYED_POSITION_SOURCE`), layers 885/882/883 and constant restored X;
- `local_external_source_restores_sh09_scale_curves`
  (`AEP_SCALE_OFFSET_SOURCE`), layers 729/749 and repeated native Scale curves.

All three have been run successfully against the hash above. Defect-5 native
proof failed on fallback geometry before correction; defect-6 native proof failed
on the strict fit check before the exact-Hold-boundary correction. Normal unit
regressions cover grammar rejection, constant-interval axis merging, percent
units, easing/clock handling, descending spatial speed and Hold boundaries.
No synthetic test or internal analytical comparison is presented as independent
Adobe-render proof. The whole requested defect set is **not complete** while
excluded defect-7 path semantics and independent visual proof remain missing.

### Defect 7: bounded Set Matte3 structural approximation

**Import:** consecutive sparse/default Set Matte3 instances on 2D shape layers
become nested editable alpha TrackMatte Groups. Native `tdpi` layer IDs, not the
zero-valued `cdat` placeholder, select independent direct-child shape samples.
Comp 724/layer 739 selects 743 then 742; layer 740 selects 741 then 746. The
ordinary provider paint copies remain visible and existing editable IDs/clocks
remain inside identity wrappers. This restores clipping rather than dropping
otherwise convertible content or allowing its full-canvas rectangles to overpaint.

**Approximations:** absent controls are assumed alpha, noninverted and composite
with original. Composition-space sampling approximates AE's pre-transform and
stretch-to-fit semantics; these defaults and coordinates have not been independently
inspected in Adobe. Sample copies lose live edit linkage to the ordinary paint
copies. Existing diagnosed initial path outlines remain; folding-path motion is
not implemented. No JS, raster flattening or shared renderer/schema changes.

**Bounds/fallbacks:** at most four gates; existing converter allocation, nesting,
shape and animation budgets apply. Missing/duplicate/self references, nondefault
sampling metadata, explicit other controls, mixed effect order, provider matte
chains, 3D/non-shape providers and consumers with existing mattes are rejected
with contextual diagnostics; existing best-effort content is retained. Native
effect-adapter omission diagnostics remain, supplemented by structural-lowering
diagnostics on accepted owners.

**Export:** unchanged, no Set Matte3 feature exporter or export proof added.
**Evidence:** the hash-pinned local-only test
`set_matte::tests::local_external_source_restores_two_intersection_matte_stacks`
asserts both stacks, source identities, editable wrappers and unconsumed paint
copies. Supplemental tests cover reference decoding, malformed references, sparse
controls, disabled effects and the stack bound. Final CPU validation passed:
four supplemental tests, the explicitly executed licensed-source test, and ten
existing compositing regressions; scoped clippy and support-ledger checks passed.
Adobe open/inspection and new render/alpha comparisons remain **unrun / unmeasured**. Licensed sources/media are not redistributed or uploaded.

## Large-graph import: generated occurrence count

**Import-only corrective change.** The former shared 10,000 generated-object
cutoff is removed from composition siblings, parent/matte/clock helpers, solids,
vector paints, masks, media, text controls, effects and layer styles. Finite
supported content is no longer omitted merely because its generated identifier
crosses 10,000. A shared reservation helper enforces the fixed safety ceiling of
1,000,000 generated identifiers across ordinary layers and all auxiliary mask,
shape, text, media/vector, effect and style objects. Multi-ID reservations are
atomic: a rejected reservation leaves the cursor unchanged and constructs no
corresponding output. Checked arithmetic also rejects integer overflow. Rejected
effects/styles and vector paints do not consume their provisional reservations.
Cycle detection, nesting depth, animation budgets and bounded media/shape parsing
remain unchanged. This does not provide process memory isolation: expanded shared
precompositions and copied parents can still consume substantial time and memory.

Structural evidence: `wide_native_graph_keeps_siblings_beyond_former_occurrence_limit`
failed before the fix (5,000 of 10,001 authored sibling occurrences survived) and
passed afterward. The native-derived graph mutation is a stress regression, not a
new Adobe-authored feature oracle. Targeted tests also exercise native mask guides,
gradient paints and Layer Styles at the old boundary, a valid Effect after crossing
it, text identifiers, an A/V pair, 5,001 editable PDF paints, numeric ID exhaustion,
cycles and depth. All 11 selected CPU tests passed; this is not a full-suite result.
The PDF stress input is specification-built, not Illustrator-authored evidence.

An independent, licensed production AEP was audited locally: count truncation
omitted 30 SH01 layers, the master background/audio and matte helpers. Its source
and Adobe reference remain private local educational artifacts, not published
fixtures or a registered fidelity case. A fresh post-fix import produced 12,795
stored layer nodes with no expansion-limit warnings and restored all 32 audited
missing source layers (including the 30 SH01 layers) with matching source-clock
ranges; the master background contains a Rect and the master mix contains Audio.
The 3840×1600, 24fps, 35.75s/858-frame FX export and labeled comparison both passed
full-video decoding. Selected visual samples show restored geometry but still
substantial scale, motion, matte and timing differences: this is not a fidelity
pass. The local import/export/comparison workflow took about 353s and reported
about 5.08 GB peak resident memory across the supervised run, so removing the
cutoff must not be described as a memory optimization. Alpha/audio fidelity and
remaining effects, expressions, path animation and media omissions are not
resolved by this change. FX → AEP implementation and
proof are unchanged and outside this import-only fix.

## Independent Scale axes and one-hop sibling Slider aliases

Import now lowers direct two-component Slider Scale vectors to separate editable
ScaleX/ScaleY values and tracks even when their key times or easing differ. Native
curves are not merged, resampled or baked. Scalar values and temporal speeds are
converted from Slider percent to native Scale fractions, then the existing FX
percentage mapping is applied. Static, parent and camera-normalized animation
paths support these import-only component markers; captured expressions do not
produce duplicate Scale tracks.

A Slider alias may additionally make **one** direct `thisComp.layer(...).effect(...)(...)`
hop to an unambiguous sibling in the same composition, only with identical finite
start time and positive stretch. Local aliases retain cycle detection; a second
cross-layer hop, different clock, arbitrary arithmetic and independently animated
three-component vectors remain unsupported. Raw AEP properties remain unchanged;
controller edit linkage becomes independent destination keys, explicitly diagnosed.
No JS, schema or renderer changes. Export is unchanged/outside this import fix.

CPU regression `independent_and_sibling_scale_curves_keep_separate_editable_axes`
covers distinct static/animated axes, pixel-independent Scale units, owner/parent/
camera tracks and ambiguous sibling rejection. The 13-test control-link panel
passes. The camera-path assertion initially failed and caught missing component
target mappings, which were added before the checkpoint. Local fresh Intro import
restores four previously absent tracks: comp 3 Rotater_Top (2162) / Rotater_Btm
(2166) ScaleY, 6750→7500 ms, 40→200.63912462477157%; Circle_R 2 (2186) ScaleX/Y,
6750→7458 ms, 230→0%. These tracks are absent in the prior archive. This establishes
specific lost Scale controls, **not resolution of every reported 7-second layout
difference**; cross-layer Position/Rotation references and other omissions remain.
No new video/Adobe test or visual fidelity pass was performed, per user instruction.
The same local source/reference provenance and unverified alpha/audio/public-native
proof limitations apply.

## Direct Slider offsets driving separated X Position — bounded import correction

The bounded scalar-offset resolver also accepts
`p = thisComp.layer("controller").effect("control")("Slider"); transform.xPosition +/- p;`.
It requires a static authored X base, an unambiguous non-expression Slider and
equal positive source stretch, reusing the Rotation resolver's start-time
rebasing and signed temporal easing. Pixel values/speeds are **not** converted
to Scale percentages. The expression's target must match the destination
property; other syntax/axes retain fallback. Controller edit linkage is lost and
diagnosed, with editable destination keys instead; no JS is run or emitted.

A CPU regression failed before the fix and now verifies pixel units plus owner/
transform-parent animation. The complete 12-test control-link panel passes.
Fresh local Intro import restores 11 X keys on Box_01–04 and tracks on 18 parent
copies. At composition 3250 ms, the native `Pos_x` offset is 133.9102 pixels:
Box_01/03 move from approximately 799.875 to 933.7852; Box_02/04 move from
approximately −800.1246 to −934.0348. The previous fallback retained only base
positions. Offline inspection found normal blend and no direct track matte on
the six rotating Box/Behind layers, so this missing translation is a concrete
remaining defect relevant to the reported side overlaps—not proof that every
overlap difference is resolved.

Per user instruction, no new preview, export or Adobe test was run. Visual overlap
fidelity remains **unverified**; existing local source/hash/reference and licensing
limitations apply. FX → AEP export is unchanged/out of scope for this import fix.

## Direct signed Position X and Slider-driven shape controls — bounded import correction

Separated X Position additionally accepts only a complete direct sibling Slider
reference, optionally prefixed by `+` or `-`, such as
`-thisComp.layer("controller").effect("Separation")("Slider")`. Unlike the
existing offset form, the authored Position base is not added. Ellipse Size
accepts the exact repeated-vector form
`temp = thisComp.layer("controller").effect("Size")("Slider"); [temp,temp]`
(with optional `var`), and Stroke Width accepts the exact scalar reference. The
resolver requires one unambiguous Slider control, a non-expression scalar curve,
finite clocks and equal positive stretch. It rebases controller-local key times,
preserves source interpolation/influence and signed temporal speeds, and copies
initial values plus native editable keys in pixel units. Arbitrary arithmetic,
property chains, executable suffixes and mismatched repeated variables retain the
existing diagnosed expression fallback.

The destination values/keys are independent copies: later controller edits are
not linked, and this approximation is diagnosed. No expression is executed or
emitted, no raw AEP chunks are rewritten, and no FX schema, evaluator, renderer
or export mapping changed. FX → AEP support and proof are unchanged.

CPU regressions cover direct signed Position without base addition, strict grammar,
and static plus animated Ellipse Size/Stroke Width mapping. The focused
`control_links` panel passes 15 tests. A fresh CPU import of hash-pinned local
`source.aep` (SHA-256
`28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d`),
composition 705 → nested composition 2420, restores Position X on layers 2455/2456
at 0/833/1458 ms with ±50/±250/0 pixels; Ellipse Size on 2455/2456 and Stroke Width
on 2459/2466/2468 at 0/1458/2083/2917 ms with 100/200/120/150 pixels. The fresh
archive contains no `JsScript` animator. This is editable structural evidence
only: no Adobe operation, render, export, RGB/alpha/audio comparison or visual
fidelity measurement was performed, so the 20-second visual result remains
**unverified**. Licensed source and reference media remain local.

## Slider-referenced Rectangle controls — bounded import correction

A native Rectangle whose enabled Size, Position or Roundness expression is
exactly one complete same-layer Slider reference, `effect("…")("…")`, or for
Size/Position a complete two-reference array, now imports as the existing
typed FX Rect instead of a frozen cached outline. References use the Scale
links' resolver: local Slider aliases with cycle detection and at most one
direct `thisComp.layer(...)` sibling hop whose layer has an identical start time
and an equal positive stretch. Slider values, keys, interpolation, influences
and temporal speeds stay in pixels (no Scale percentage conversion). Two
components merge only across compatible key times and interpolation (the Rigged
Box axis merger), never by resampling; unlinked properties keep their native
leaves or defaults. The resolved curves drive the initial Rect, its size
validity rules (including a zero start), the gradient-axis guard and the Size,
center-anchor, Position and Roundness tracks. For these Rects a center-anchor
track is emitted only for a Size component that can change: one whose every key
stores the same value with zero temporal speed keeps just its static half-size
anchor (equal endpoint values alone do not qualify, because a nonzero Bezier
speed overshoots). Position components keep both scalar tracks; a held axis
stays a flat editable track. Controller linkage becomes independent editable
keys, diagnosed per layer. Arithmetic, suffixes, `thisLayer`, wrong component
counts, missing/ambiguous/non-Slider controls, cycles, a second cross-layer hop,
clock mismatches and incompatible axis keys keep the diagnosed static-outline
fallback. An animated resolved Size or Position with a Gradient Fill also falls
back, for the stock Rigged Box too: its expression-backed leaf looked static, so
such a Rect previously kept static gradient axes while its geometry moved. No JS,
frame baking, schema, renderer or exporter change.

CPU regressions are **derived**, not Adobe-authored Rectangle proof: the native
Rectangle of `geometry/geometry_probe.aep` (composition 14, layer 40) with
relabelled native Bezier and linear keys:
`complete_slider_references_drive_a_typed_rect_and_its_sibling_copy`,
`slider_rect_references_outside_the_complete_form_keep_the_static_outline` and
`resolved_slider_rect_geometry_uses_the_typed_rect_guards` (which also covers a
stock Rigged Box Gradient Fill), each failing before the change;
`slider_rect_anchor_keys_follow_only_moving_size_axes_through_export` (fresh
import, then the unchanged exporter, for constant-Width, constant-Height and
two-axis cases), which failed before the anchor rule with `Spatial X/Y easing
differs`; `slider_linked_rect_position_and_roundness_keep_pixel_keys_through_export`
(fresh import, then the unchanged exporter, for constant-X, constant-Y and
two-axis Slider Position beside a static native Size and keyed Roundness),
which pins existing behavior; and the rule's mutation-checked decision test
`size_components_are_constant_only_with_unchanged_zero_speed_keys`. A private
production project (not redistributed) now imports its owner, one-hop sibling
copy and track-matte provider copy as typed Rects with the native keyed width
and a static height anchor. The exporter is unchanged; it pairs a keyed anchor
axis with an absent one using the keyed axis's ease, and writes independent
X/Y Position curves as separated native Position followers. Before the anchor
rule it omitted these Rects for differing X/Y anchor easings (as it previously
omitted the frozen rounded outlines for their unsupported Path keys); a fresh
export now keeps all three with keyed native Size, center and anchor. A
one-axis Slider Position needs no such rule: its fresh export keeps the Rect,
with the moving axis's Slider keys and ease on one separated follower and a
flat follower for the held axis, while two moving axes stay one native path.
Both outcomes are observed only by structural readback of our own output. No
independent Adobe-authored Rectangle-animation oracle exists, so native render,
RGB/alpha and export Adobe proof remain **blocked/unmeasured**.

## Source Text Slider percent expressions — bounded import correction

An enabled Source Text expression of exactly the form
`s = effect("…")("…"); Math.round(s).toLocaleString() + "%";` (optional `var`;
any one-letter binding, reused in `Math.round`) that reads the layer's own Slider
now imports as the existing editable Hold-segment TextLayers: one per native
Slider key, showing `N%` from that key's layer-local time (the first value also
before it), in the style of the single cached Source Text document. The Slider
must resolve unambiguously on the same layer (pure local aliases, no sibling
hop) and have a static value or finite, strictly increasing keys whose every key
but the last has outgoing Hold. Its values must be whole numbers 0–100, the only
ones whose `Math.round`/`toLocaleString` output needs no rounding, grouping,
decimal or signed-zero decision; digits are taken as ASCII, as in the reference
render, and locales with other digit systems are not modeled. Longer binding
names are rejected because AE can resolve them to layer or property attributes
(`value`, `time`, `index`, …). Nothing is executed, emitted as script or sampled
per frame; the live Slider linkage becomes independent editable text segments,
diagnosed. Any other enabled expression, one over keyed Source Text, or an
unresolvable, ambiguous, non-Slider, non-Hold or out-of-range Slider keeps the
authored documents with a contextual `[AE-PROPERTIES]` diagnostic (a Slider left
at its default is not stored as a leaf and does not resolve). A disabled
expression keeps them without one, as AE shows the authored value. Previously
every Source Text expression was ignored without a diagnostic.

Two shared corrections support it. The control-link resolver (Scale, Rectangle
component and this Source Text path) now matches an effect that keeps its AE
default name, stored as display name `-_0_/-`, by its descriptor `fnam`, the rule
Geometry2 already applied. A custom display name stays authoritative; the
placeholder without a valid `fnam`, or an effect without a display name (which
Geometry2 formerly named by `fnam`), names nothing. Held Source Text segments now
share one millisecond boundary per key: rounding each duration separately let
fractional-millisecond key times overlap by 1 ms (Slider ticks 11264 and 16384
of 30720 produced `[367,534)` and `[533,700)` ms). The boundary is the
millisecond at or before the key; see *Held Text, occurrence clocks and font
identity* below.

The expansion is admitted only as a whole, through the existing checked
identifier reservation and generated-animation accounting. Its segments take one
reservation of every TextLayer and text-control identity, and every copied
control track is reserved before anything is committed. If either is refused,
the expression is diagnosed as not lowered and the authored cached text is
imported instead: no partial percentage sequence is emitted, the provisional
identities and track reservations are released, and later layers keep what they
can afford. Admission is in import order, as for other content, so an admitted
expansion can still leave a later layer short. No count or size quota is added;
production accounting keeps its checked arithmetic without an aggregate quota.
Formerly the segments were committed one by one and their tracks admitted
afterwards: identity exhaustion left a percentage prefix without the cached text
and could abort the import for a later layer, and a refused track left a partial
animated sequence.

Evidence is structural only. CPU regressions use derived storage: unchanged
Adobe-authored parts of pinned sources combined in memory, namely the Text layer
of `pr4442_native/sources/text_document_point.aep`, the default-named Slider
Control of `geometry/geometry_probe.aep` layer 28, and the Hold keys of
`pr4442_native/sources/paint_fill_enable.aep` layer 16 (0 at 0.25 s, 100 at
1.5 s). `slider_percent_source_text_holds_each_native_slider_key` and
`source_text_expressions_outside_the_slider_percent_form_keep_authored_text`
(disabled, other syntax, wrong property or effect, default/custom-name collision,
custom-name authority, non-Slider effect, sibling hop and the keyed
`text_document_content_hold.aep`) failed before the change.
`percent_grammar_is_complete_and_binds_one_letter`,
`effect_name_consults_fnam_only_for_the_default_name_placeholder`,
`default_named_sliders_resolve_by_fnam_without_weakening_identity_checks`,
`held_percent_texts_require_whole_hold_values_from_0_to_100` and
`adjacent_hold_ranges_share_rounded_boundaries` pin the rules. On composition 32
of `text/import_text_additional_controls.aep`, whose Text layer 46 has a keyed
Grouping Alignment, with the three Hold keys of
`implemented_additions/native_transform_keys.aep` layer 91 (2, 10 and 19 at 0, 1
and 2 s) and an unchanged copy of layer 46 as a later sibling,
`slider_percent_expansion_over_the_animation_allowance_keeps_authored_text_and_siblings`
(test-only allowance) and
`slider_percent_expansion_past_the_identifier_space_keeps_authored_text_and_siblings`
failed before the transactional admission and now match the authored import
exactly. `slider_percent_expansion_above_former_identifier_ceilings_is_admitted_whole`
keeps all three segments and their tracks with identities past the removed
10,000-object and 1,000,000-identifier ceilings. A fresh local
import of a private production project (not redistributed) turns one cached
`30%` layer into six Hold segments, 0/15/55/65/90/100% at 0/0.1/0.366667/0.533333/
0.7/0.8 s, in both of its linked occurrences. No Adobe-authored expression
oracle, Adobe render, RGB/alpha comparison or FX → AEP export proof exists.
Export of the held segments is covered in *Held Text, occurrence clocks and font
identity* below.

## Direct Angle offsets driving Rotation — bounded import correction

Import recognizes only the complete binding form
`r = thisComp.layer("controller").effect("control")("Angle"); transform.rotation +/- r;`
(with an optional `var` declaration and validated parameter names). A unique
Angle Control with no enabled expression is combined with a static authored
rotation base. The result is independent editable Rotation values/keys, including
signed temporal speeds and unchanged influences/interpolation; **no JS is run or
emitted**. Controller editing linkage is lost and diagnosed.

Both source layers must have equal positive stretch. Different start times are
handled by translating controller-local key times into the owner's local clock,
so existing occurrence/parent clocks preserve composition-time events. Different
stretch, reversed time, ambiguous names, animated rotation bases and other
expression syntax retain diagnosed fallback. This is not a general expression
runtime. Static, owner, parent, matte and camera-normalized Transform paths share
the same resolver; the captured-expression path skips successfully resolved links.
Export is unchanged and outside this requested import-only fix.

CPU evidence: five `control_links::rotation::tests` plus six existing Slider-link
tests pass. They cover grammar rejection, static values, signed easing data,
owner/parent/camera consistency, start-time rebasing, ambiguous layers and
unsupported clocks/bases. These synthetic/mutated fixtures are supplementary.
A hash-pinned local Intro assertion fails on the prior archive (zero of six
Rotation tracks) and passes on fresh import: Box_05–08 and Box_Behind_01–02 now
have 2500→3542 ms keys, 0→±45 degrees, including the Behind layers' distinct start
times. The source/control is comp 3 / Scaler layer 36 / Rotater_Cntrl; the source
hash is the local Intro hash recorded below. At exactly 2.5 seconds the controller
is still zero, so not every opening geometry difference is explained by this fix.
Per explicit user instruction, verification stops at CPU tests and editable
structure: **no new preview/export/Adobe test or visual pass**. Licensed inputs
remain local. Minimal public native proof, alpha/audio and export proof remain
unverified; image-sequence work is deferred.

## Source Text font identity — bounded import and export correction

AE stores each Source Text font as a PostScript name in the document's COS font
set. All 771 font-set entries of the 196 Text layers in the 76 public
AE-authored fixtures have the form
`<< /0 << /99 /CoolTypeFont /0 << /0 (name) /2 n [/5 (version)] >> >> >>`, where
`/2` is 1 for the TrueType and 0 for the CFF faces present and `/5` is the
font-file version; each set also lists `Myriad-Roman`, `Helvetica` and
`AdobeInvisFont`.

| Direction | Mapping | Diagnosed fallback | Evidence |
| --- | --- | --- | --- |
| FX → AE, font-set entry | Each font is written as `<< /0 << /99 /CoolTypeFont /0 << /0 <name> >> >> >>`, the native dictionary, type tag and name. Previously the entry was `<< /0 << /0 <name> >> >>`, one level shallower than the name the reader and native files use, so a reimport lost the font. | FX has no face format or font-file version: `/2` and `/5` are not written rather than guessed, and the companion entries and root `/98` are not added. Whether AE accepts such an entry is **unverified**. | S: `font_entries_use_the_native_cool_type_dictionary_without_guessed_fields`, and `fresh_font_entry_uses_the_native_cool_type_dictionary_and_reimports_its_name` on pinned `pr4442_native/sources/text_document_font_style.aep` (its native `Arial-BoldMT` entry pinned in the test), failed before; the fresh export now reimports as `Arial`/`BoldMT` instead of the `sans`/`serif` fallback. |
| AE → FX, dash-less name | A PostScript name with no style suffix (`ArialMT`, `TimesNewRomanPSMT`, `SegoeUIEmoji`) imports as that exact family with an empty style, the FX convention for an exact PostScript identity that the Premiere converter already uses. Names with a style suffix still split at the last `-` (`AvenirNext-DemiBold` → `AvenirNext`/`DemiBold`). | Previously the style defaulted to `Regular`, which export cannot tell from an authored style and rewrote as a guessed `ArialMT-Regular`. Renderers resolve the exact name first (scene font key `ArialMT` instead of `ArialMT/Regular`); `TimesNewRomanPSMT` matches its bundled face by PostScript name, while `ArialMT` is not bundled under either name. No render comparison was run for this change. | S: `dashless_postscript_font_round_trips_exactly_with_an_empty_style` on pinned `pr4442_native/sources/text_document_point.aep` (native entry `ArialMT` pinned in the test) failed before and now imports, writes and reimports exactly `ArialMT` with an empty style. The `ArialMT` style assertions of the ignored `native_text_animator_keyed_controls_import_editable_tracks` (passes) and `intrinsic_text_document_controls_import_typed_editable_values` (fails earlier, on an unrelated colour-precision assertion) now expect the empty style. |
| FX → AE, empty style | An empty static, constant or keyed `fontStyle` exports the family verbatim as the PostScript name, without the candidate diagnostic. An empty family or a NUL still fails. | Previously an empty style failed and the Text layer was omitted. | S: `empty_style_marks_an_exact_postscript_name` failed before. |

## Source Text All Caps — bounded import and export correction

Field 12 of a COS character-style run is the caps mode. The independently
AE-authored All Caps sources `text/import_text_additional_controls.aep`
(SHA-256 `65a5929c222b2d44727adc40f18438adaa5e339a52568dab308f4e58c06b44e1`,
composition 1 `ALL_CAPS`) and `pr4442_native/sources/text_document_allcaps.aep`
(SHA-256 `431090eb1dc597836645ee3a066d055c61ba09153b3cec3f99e82a5e7c22a33c`,
composition 1) store `2`; normal text in the first source (for example
composition 77) stores `0`. Import previously recognized only `1`, and export
wrote `1`.

| Direction | Mapping | Diagnosed fallback | Evidence |
| --- | --- | --- | --- |
| AE → FX | `0` → `allCaps: false`, `2` → `allCaps: true`; the stored string is kept, not rewritten in uppercase. An absent field keeps the default, as sibling style fields do. | Any other code (including `1`, which this converter's earlier exports wrote for All Caps) or a non-integer value has no native evidence: normal caps are used with a field-12 `[AE-PROPERTIES]` diagnostic. Only the first character-style run is read (existing run diagnostic). | S: `native_all_caps_source_text_imports_all_caps_and_keeps_its_string` failed before and now passes on both sources (each previously imported `allCaps: false`) and the composition-77 normal control; `source_text_caps_field_distinguishes_normal_all_caps_and_unrecognized_modes`. R, descriptive only (no quality threshold or fidelity pass): case `aep-text-import-text-additional-controls-c1` compared a fresh import of composition 1, rendered at 30 fps, with its independent Adobe reference (long-term Asset `RkzSLPPLtRLQLMzjahAJ_vid`, SHA-256 `74d4b1f4944bc76b2707ef24800fa6fc8d1e8ab98e831b41f2f2b8338896c2f5`) over all 90 unique full-resolution 1920×1080 canonical RGB24 frames of `[0,3s)`: mean `0.9948002007783759`, minimum `0.9940262074416207` at 1/30 s, against `0.9756150882484486` / `0.9753178914281523` at 0 s before the fix. Paired decoded frames show uppercase text in the reference and the fresh import, and the stored mixed case before the fix. The render used an explicit local Arial whose match to the reference font is unverified; alpha is unverified; the scorer ran no CPU tests, and the registry still links this case to the ignored composite test (`UNRUN`). |
| FX → AE | `allCaps` writes `2`, normal text `0`, in each fresh Source Text document. | None new. | S: `all_caps_writes_the_native_all_caps_code` and `fresh_all_caps_text_exports_the_native_caps_code_and_reimports_all_caps` check the exact written bytes and failed before. Adobe reopen/render of the export is **unverified**. |

## Held Text, occurrence clocks and font identity — bounded corrections

Imported AE text layers become a layer Group holding a `Source content clock`
Group holding one Text per held Source Text value (keyed documents or a lowered
Slider percent expression). Export previously omitted every such layer with more
than one held value: a Null parent was chosen only for a chain of single
children ending in one Text, and a precomposition needs child render bounds that
FX Text does not have (`Text/font glyph bounds are not known from the FX text
box`). Each row is structural evidence only; no generated project has been
opened or rendered by Adobe.

| Direction | Mapping | Diagnosed fallback | Evidence |
| --- | --- | --- | --- |
| FX → AE, text-only Null parents | A Group whose single child is a text-only branch (Text, or a nonempty Group of such branches) becomes the existing exact Null parent, as a multichild Group already did. Each nested Group is classified on its own, so an identity clock Group holding the held segments becomes a Null and each segment a native Text layer with its own interval. | Unchanged: a Null does not carry blend mode, opacity, motion blur, masks, effects, mattes or a nonidentity clock, so such owners still need a precomposition and Text there remains omitted with the glyph-bounds diagnostic. | S: `identity_wrapper_of_held_text_segments_exports_each_segment_under_null_parents` (explicit FX input) and `slider_percent_segments_export_as_native_text_under_null_parents` (fresh import of the derived Slider percent storage above) failed before (subtree omitted) and now write two native Text layers at [0,1.5) and [1.5,2) s with their texts, parented to Nulls for the clock and the AE layer. `text_only_wrapper_keeps_the_null_parent_guards` pins the blend, opacity and motion-blur guards. |
| FX → AE, two-key occurrence clocks | A visual Group whose TimeRemap mapping has two Linear keys exactly on its visible input interval and `inputOffsetMs == 0`, the form in which import stores an offset, trimmed or stretched precomposition occurrence, exports as the existing exact affine occurrence record (reduced rational start, in, out and stretch), as audio occurrences already did. Formerly it went to native Time Remap, which needs keys outside the interval, and the subtree was omitted (`Group source clock cannot be represented exactly`). | A nonzero input offset and other keyframe clocks keep the bounded native Time Remap path or its diagnosed omission. A keyed Group-owned Transform channel (3D and skew included) under such a clock is rejected: FX evaluates those keys on the remapped content clock, which occurrence keys cannot express. The clock is only as exact as the imported milliseconds: AE's 3/2 stretch of `timing_stretch.aep` returns as 1900/1267, 0.33 ms late at the source end. | S: `native_offset_trim_and_stretch_occurrences_export_their_affine_clocks` (pinned `pr4442_native/sources/timing_precomp_source_range.aep`, `timing_trim.aep`, `timing_stretch.aep`) and `two_key_linear_group_clock_is_an_affine_record_while_its_transform_is_static` failed before; exported start/in/out lie within 0.5 ms of Adobe's records. Fresh exports of `timing_time_remap_linear`, `timing_time_remap_hold`, `timing_time_remap_bezier`, `timing_time_remap_negative_keys`, `timing_reverse_v2` and `timing_inactive` stay byte-identical. |
| FX → AE, text-only required precomposition | When Text must precompose, typically under such an occurrence clock, and the subtree is plain 2D Text in plain Groups, the precomposition sets native collapse transformations on a root-canvas source (warning: `Text has no FX glyph bounds, …`). The Text keeps its parent-space geometry, the clock stays on the occurrence record, and every nested Group is classified on its own, so held segments stay under Nulls. The Text owner's clock is validated before collapse (only a full-span identity clock, or a supported occurrence clock moved onto the record), so a canonical linear identity mapping, including a mapping domain wider than its visible window, or identity keys over the whole span keeps the same native identity record. Vector collapse alone requires a canonical linear identity mapping with zero input offset; identity-shaped TimeRemap keys and normalized offset occurrences retain its clock rejections. | Mixed Text and vector content, owner effects, occurrence masks, matte consumers, and Text or nested Groups with effects, masks, mattes, non-Normal blend, motion blur or 3D keep the glyph-bounds omission (a masked occurrence reports `Collapsed Text source requires a 2D occurrence without masks or matte consumers`). No glyph bounds are guessed and nothing is rasterized. Hidden-only content under a required precomposition has no visual bounds and keeps the `Precomposition has no finite visual child render bounds` omission. Adobe rendering of collapsed Text, with or without a source clock, is **unverified**; this relies on the writer's existing collapse and clock record fields, not a native probe. | S: `clocked_text_occurrences_export_as_collapsed_precompositions_with_exact_clocks` (explicit FX input: occurrences 0.5 s late and 0.5 s early) failed before (clock subtree omitted) and now writes collapsed precompositions with exact start/in/out/stretch, a root-canvas source and both held segments; `clocked_text_collapse_rejects_mixed_masked_blended_blurred_or_3d_content` pins the omissions. `clocked_text_collapse_keeps_each_supported_occurrence_clock_exact` (offset, negative start, two-sided trim, slow and fast keys, half and double rate, varied canvases) and `nested_clocked_text_occurrences_each_keep_their_own_exact_clock` pin hand-derived start/in/out/stretch records. `hidden_text_owner_with_a_wider_identity_mapping_collapses_like_canonical_identity` and `hidden_text_owner_with_explicit_identity_keys_collapses_like_canonical_identity` retain the prior owner-collapse correction at the root and inside a clocked source under canonical windowed clocks. `collapsed_vector_owner_keeps_requiring_canonical_identity` pins both vector clock rejections, and `clocked_text_exclusions_keep_a_supported_sibling_collapsed` pins effect and mask omissions beside a kept sibling. |
| AE → FX, Hold boundaries | A held value starts at the whole millisecond at or before its key, where the previous value ends; a product within float error of a whole millisecond is that millisecond. FX time is whole milliseconds, so a key between two of them (tick 11264 of 30720 per second, 30 fps frame 11) cannot be stored exactly. It was rounded up to 367 ms, which hid the new value at the key's own frame wherever time is sampled exactly: a 30 fps export showed the previous value at frame 11 in After Effects' layer timing. Tesseract's nearest-millisecond sampling showed the right value either way. | The value can appear up to 1 ms before its key; only a sample less than 1 ms before the key differs, and a key on a frame of any rate up to 1000 fps has no earlier frame that close. Keyed Source Text documents, and the authored text kept when a Slider expression is not lowered, use the same rule. | S: `hold_key_between_milliseconds_is_shown_from_its_own_frame` (the derived Slider percent storage above with that exact key: fresh import and 30 fps export, sampled at frames 10, 11 and 12), `hold_key_between_milliseconds_is_shown_from_its_own_frame_under_an_occurrence_clock` (the same text placed 0.1 s early: parent frames 7 to 9) and `keyed_source_text_between_milliseconds_starts_at_the_millisecond_before_its_key` (retimed `pr4442_native/sources/text_document_content_hold.aep`, with and without an unlowered expression) failed before; `adjacent_hold_ranges_share_the_millisecond_at_or_before_each_key` and `keys_before_source_zero_collapse_and_later_keys_start_at_their_millisecond` pin the rule. The private production import moves only its 15%/55% boundary from 367 to 366 ms. Adobe rendering of the boundary is **unverified**. |

## Unsupported static Position expressions — better fallback, not expression support

For an enabled, unresolved Position expression with a valid **non-animated**
stored value, import now retains that editable pre-expression position instead
of replacing it with the composition center. This applies to combined Position
and separated X/Y/Z controls through the existing occurrence/parent/matte static
Transform path. Captured expression tracks can still override this base. Animated
expression properties and non-Position expressions keep their existing fallback;
no expression code is executed or emitted, and no new cross-layer bindings exist.
The diagnostic explicitly states that the expression was not evaluated: motion,
offsets and mirrored placement controlled by that expression remain missing.
Export is unchanged and outside this import-only fallback repair.

`unsupported_static_position_expression_preserves_authored_base` failed before
and passes afterward for both combined and separated Position, retaining a
rotation sibling. Existing native-animation and six Slider-link regressions also
pass. Synthetic/mutated fixture coverage is supplementary, not new Adobe proof.
Local Intro comp 3 / Box_01 layer 20 and Box_02 layer 32 preserve native base X
≈799.875 and −800.1246 instead of 1920. The fresh 2.5-second preview recovers major
previously misplaced geometry: full-resolution RGB MAE against the existing
Adobe video falls from 90.019 to 22.901 (0–255 units). This is **one diagnostic
frame, not a fidelity pass**: extra rectangles, missing texture and unresolved
expression behavior remain visible. Native source SHA is the same local Intro
source recorded below. No new Adobe operation, public native fixture, Asset
publication, full-film comparison or alpha/audio/export proof was performed.

## Packed Easy Levels Histogram — import-only correction

`ADBE Easy Levels2` can store its render controls in the explicit Histogram
`aRbs/aRbp`, while ordinary numeric UI slots retain defaults or the selected
channel's cache. The importer now reads the observed **static version-1,
108-byte, big-endian five-channel layout** and lowers the master record into
existing editable FX Levels controls (normalized levels ×255, gamma unchanged).
The authored record takes precedence over UI caches and descriptor defaults.

Only finite normalized endpoints, increasing input ranges and positive gamma
are accepted. Unknown layouts, duplicate records, malformed metadata, animated
or expression-bearing Histograms omit that effect with a contextual diagnostic;
convertible sibling effects survive. Nonidentity records beyond the master are
explicitly omitted with their values diagnosed. Per-channel/alpha Levels and
clipping switches remain unsupported; this is not complete Levels fidelity.
Absent explicit Histograms retain the existing numeric/default path. FX → AEP
export remains unchanged (Pro Levels Individual Controls), outside this local
import-only repair.

Supplementary regression coverage: six `effects::native::levels_tests` cover
master/default/cache precedence, preserved fractions despite UI type hints,
malformed/unknown/range rejection, animation/expression/duplicate rejection,
per-channel diagnostics and the unchanged absent-Histogram path. The master test
failed before the repair. These synthetic storage tests do not establish
independent Adobe authoring or rendering proof for every master parameter.

Local educational Intro source SHA-256
`28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d`,
comp 705 / layer 720, independently contains master input black 48 and white 234
(with gamma 1 and output 0–255). A hash-pinned local assertion fails on the prior
import's 0/255 defaults and passes on a fresh import; the Adjustment and both
surrounding Tint effects remain editable and ordered. The licensed native source
and videos are not redistributed. Three fresh full-resolution PNG previews at
0, 1.5 and 15 seconds were compared with the existing independent Adobe video.
At the 0-second corner, RGB changes from prior video `(72,72,72)` to PNG
`(38,35,36)`, versus Adobe `(35,33,34)`; at 15 seconds, `(215,215,210)` becomes
`(233,233,228)`, versus Adobe `(231,231,226)`. These diagnostic samples show the
contrast correction but compare PNG with compressed video: **not pixel equality,
a registered feature score or whole-film proof**. Geometry/texture differences
remain. No new Adobe operation, native minimal fixture, Asset publication,
alpha/audio proof or export proof was performed.

## Pure Slider links driving Scale — import-only correction

A bounded importer normalization now recognizes a direct two-/three-component
Scale expression whose same-layer Slider references (including recursive pure
aliases) resolve to the same scalar curve. It copies native values and keys into
independent editable Scale tracks, preserving key times and temporal easing.
Slider percentages **and temporal speeds** are converted to native Scale fractions
before the existing FX percentage mapping. No expression code is executed or
emitted. The raw native-property reader remains unchanged.

Original shared-controller edit linkage is lost and diagnosed. Arithmetic,
time-dependent expressions, escaped names, ambiguous controls and cycles retain
the existing explicit expression fallback. The later independent-axes/one-hop
sibling increment above supersedes this checkpoint's original equal-curve and
same-layer-only restriction; other cross-layer expressions remain unsupported. This is not an AE expression
runtime. Owner, transform-parent, matte and camera-normalized animation paths use
the same lowering; captured samples do not add duplicate tracks for lowered links.
FX → AEP export is unchanged and outside this source-specific repair.

Supplementary CPU evidence: six `structure_document::control_links::tests` pass,
including a regression that failed before lowering, parser/ambiguity/cycle rejects,
value/speed unit preservation and owner/parent/camera path consistency. Storage
fixtures are synthetic or mutated native data, **not independent Adobe authoring
proof**. Two existing `mixkit_evaluated_*` tests remain ignored without their
external pinned expression captures; they were not counted as passing.

Local educational Intro evidence: SH01 comp 3 / HeroCircle layer 17 has Slider
keys 0 s → 0%, 2 s → 13.32547551601051%. The previous import had no Scale tracks.
Fresh import restores both axes on the owner, matte copy and parent copies,
including native Bezier data; an executable local assertion fails on the old
archive and passes on the new one. A fresh 3840×1600, 24 fps, 858-frame export and
synchronized comparison fully decode. Samples at 0, 0.5, 1, 1.5 and 2 seconds
show the previously frozen circle growing, but background tone and later
geometry/matte/missing-content differences remain substantial. This is not a
whole-film fidelity pass. Existing Adobe reference was reused; no new Adobe
operation, minimal native authoring, export inspection or registered feature
score was performed. Licensed source/media remain local, not public fixtures.
The unchanged two-footage ProRes normalization and unverified alpha/audio
limitations from the large-graph checkpoint still apply.

## Same-effect scalar aliases — bounded import correction

An ordinary Effect Parade control whose enabled expression is exactly one
`effect(name)(parameter)` reference to another control of **its own** effect
occurrence is lowered to that control's already-decoded values/keys, including
the existing integer-slider rounding and nearest-millisecond key convention.
The observed case is Mosaic Vertical Blocks = `effect("Mosaic")("Horizontal Blocks")`.
The effect name must uniquely match AE's current instance name: an explicit
rename wins, and an instance that keeps AE's `-_0_/-` placeholder is named by its
plugin display name (`fnam`). The parameter may be named by display or match
name. Chains of pure same-effect aliases are followed with cycle detection.
Captured Adobe expression samples keep precedence. The result is an independent
editable copy; live linkage is not retained and is diagnosed.

Ambiguous or unreadable effect names, references to another effect occurrence,
missing/ambiguous parameters, cycles, non-scalar or differing value kinds and
referenced controls with other expressions are diagnosed and keep the existing
stale-cache fallback. Arithmetic, `.valueAtTime()`, `thisLayer.` prefixes, index
references and all other expressions are not candidates and are unchanged. No
expression code is executed or emitted. Export writes the current editable
values/keys; the original expression is not restored.

Evidence (S, supplementary CPU only): `structure_document::control_links::effect_alias::tests`
(synthetic storage-level identity, rename, cycle and non-candidate cases) and
`structure_document::tests::mosaic_effect_alias`. The latter are public-**derived**
regressions: pinned Adobe-authored `effects_coverage/native_animated_controls.aep`
composition 326 and `native_static_controls.aep` composition 339 with
converter-test expressions and renames added. They are not Adobe-authored or
Adobe-rendered proof of the edited behavior. The derived animated case failed
before the change (Vertical retained its stale initial value and omitted its keys)
and passes after it; disabled-expression and fallback controls pass unchanged.

## Fresh root identity and package staging

**Export foundation; see the hybrid ledger for the occurrence coordinator.** The owned AE staging API
returns an unpublished AEP, relative media inventory, original diagnostics and
native root metadata for a package coordinator. It does not partition FX or
publish a linked Premiere package; existing native-only defaults are unchanged.
The fixed writer root (item1) has one independently observed Dynamic Link GUID
case: AE26.5x89 opened a generated, nonempty Rect AEP with one enabled ShapeLayer,
1920×1080,30fps,2s. It later reopened the current bytes, which differ from that file
only in their 30fps property clocks, with the same root facts. This is
root/inventory evidence, not control-value or pixel proof. The re-observed bytes are
pinned by the supplementary
`adapter::export::staging::tests::staged_root_matches_the_nonempty_native_observed_rect_bytes`
test. No arbitrary ID-to-GUID rule is claimed.

A separate fresh Text export failed AE opening with `Error reading the text
layer. Skipping the text layer.` Cause is undiagnosed; this work neither repairs
nor hides that failure. The Rect result is not evidence for Text compatibility.
Neither probe was rendered or uploaded: RGB/alpha/audio are **unmeasured**,
long-term Asset/fresh-download proof is **missing**, and generated Premiere link
acceptance is **unrun**. Import mappings are unchanged and not newly proved.
See [exact source/output hashes and native observations](../crates/aftereffects_file/tests/fixtures/hybrid/README.md)
and the [hybrid limitation ledger](hybrid-adobe-export.md).

## File-bound Dynamic Link selection and linked pictures

**Import identity foundation; ordinary Premiere import resolves links.** A prepared source
binds one absolute AEP path, SHA-256 and bounded parsed structure. A borrowed
selection resolves network-order GUID bytes only for header profiles with native
identity evidence and the nonzero item-ID/zero-suffix layout: AE26.5x89 macOS
(revision97/subtype10, producer `0x0f928659`, public H-IDENTITY-01) and AE26.3x87
(revision97/subtype7, producer `0x0f918657`), whose GUID ↔ item evidence is a
private package retained outside Git. The unchanged public `ae26_one_comp.aep`
only shows that the latter producer's header is accepted. Names/ordinals/default
roots are never substitutes. Unknown profiles/layouts and absent/non-composition
IDs fail explicitly; malformed/duplicate identities retain parser rejection.
Ordinary numeric-ID import has no new profile restriction.

`import_picture` imports a selection as the **picture of one Premiere Dynamic
Link placement**, for ordinary Premiere conversion: the composition's root Group
under a caller-given host parent, every generated layer/item/effect identity (and
the keyframe ids that embed them) from a caller-given first id, and footage under
a caller-given per-AEP asset namespace. It uses the same conversion and media
preflight as document import, with three destination rules: the preview
background is not materialized (the link keeps the composition's alpha), the root
layers take their composition's motion-blur switch as nested ones do, and all
audio is muted (audio layers hidden, video sound disabled, volume keys dropped)
because Premiere plays a link's sound only through audio track items. The
background rule is stated in the composition's `AE-COMPOSITION-SETTINGS` note;
Premiere reports every import note of a composition with its link and omits
linked-audio items, whose import is unimplemented. The Premiere host clips each
picture to its composition canvas with a guide rect and Add mask, as AE renders a
composition. No render or Adobe comparison of a converted linked picture has run.

`LinkedMedia` keeps each linked asset once and, for a repeated picture, only the
normalized temporary file that backs the kept asset. It also records every local
file that the pictures were read from, as SHA-256: footage packaged as it is,
and the PSD or AI source behind normalized or lowered media, from the bytes
preflight decoded. A relinked source also records its missing authored path.
The host checks these before publishing: `verify_sources` re-reads each file and
requires the authored path to stay missing, and `verify_packaged` compares each
packaged footage digest with the recorded one. This catches same-length footage
edits, which the archive writer's own size check does not. These are re-read
checks, not a filesystem lock. `adapter::linked_import::tests` covers footage,
PSD, AI and relinked-path changes, and the one retained normalized file.

**Runtime timing of an embedded picture.** The FX runtime evaluates keyframes
under a time-remapped Group (such as a layer's source clock) on a chain that
starts at the first Group with `playback`, from the document clock, without
the start offsets of the Groups above it; its render walk applies them. The
Premiere host therefore seeds that chain at its root-level clip group with an
identity-rate `playback`. Under a Premiere stage group or nest that starts
after the document start it cannot, and reports the linked clip's
time-remapped animation as running early by that start (placement, visibility
and media times are kept). This is a runtime limitation of the embedding, not
of the AE import; no evaluator change was made.

**Moved-project footage.** When an absolute authored footage path is missing,
import tries the one location AE relinks a moved project to: the alias's
`ascendcount_base` ancestor of the AEP joined with the last `ascendcount_target`
components of the authored path (this repository's AE writer relies on the same
reading). An existing absolute file always wins; counts that do not fit the AEP
location or path, and parent components, are diagnosed without any search; media
missing at both locations is omitted naming the relative location. Relative
authored paths keep resolving against the AEP. This applies to document import
too. `adapter::media::tests` covers relinking, absolute precedence, unfit counts
and genuinely missing media.

Archive selection imports (`import_to_tesseract`) verify source bytes before
conversion and after archive staging, before publication; a linked picture leaves
that check to its host, and Premiere rechecks the AEP hash with its other media
before publishing. Normalized PSD temporaries remain owned until archive writing
completes (`LinkedMedia` holds them for a host). There is no expression-sample
sidecar mapping in these APIs and no lock/atomic snapshot of all external media.
Existing feature approximations and diagnostics remain unchanged; identity
resolution does not upgrade their fidelity.

H-IDENTITY-01 independently authors SAME-name red/blue comps1/16, links their actual
GUIDs from Premiere and renders both sources plus the linked sequence at30fps.
All three long-term Assets were freshly downloaded and SHA-verified. The fresh
Check/Write assertion
`adapter::linked_import::tests::native_same_name_guids_import_distinct_editable_red_and_blue_compositions`
checks exact native payloads, distinct editable Rect fills, canvas/duration,
transforms, no flattened media and diagnostic parity. Native source sequence
frames0–29 are red and30–59 blue. Ordinary Premiere conversion of that native
package (`premiere_file`
`tests::linked_compositions::native_links_import_editable_same_name_compositions_by_exact_guid`)
places the editable red picture at 0–1s and the blue one at 1–2s from source
0.5s; that is structural evidence. **Fresh-converter RGB, alpha and audio fidelity
remain unmeasured**; static colors do not visually prove source-clock offsets.
FX→AEP implementation/proof is unchanged and no generated hybrid acceptance is
claimed. See [source, target, reference and limitation evidence](../crates/aftereffects_file/tests/fixtures/hybrid/identity/README.md).

## Mattes, still lifetimes, frame fades and still Geometry2 — bounded import correction

These four corrections serve Premiere-linked compositions and apply to every AEP
import. Export is unchanged and outside this scope; export of the new editable
structure is unverified.

- **Modern unselected matte.** A modern 164-byte layer record names its matte
  layer. Matte layer ID 0 now selects no matte, whatever matte mode the record
  keeps. Only a legacy 160-byte record, which has no ID field, uses the layer
  above. Positive IDs keep their missing/self-reference checks. Camera
  normalization uses the same rule. Before, a modern zero invented a matte from
  the layer above and hid content. All 16 matted layers in the pinned native
  fixtures store explicit nonzero IDs, even for the layer above.
- **Still lifetime.** An AV layer whose source is a still image has no sampled
  source clock. Its content now keeps the native parent lifetime
  (start + in × stretch to start + out × stretch, clipped at zero) without
  playback. Before, the affine source clock started content at the layer start
  time and lost any part before it. Video, audio, image sequences, solids,
  precompositions, text and shapes keep their existing clocks. Owner Transform,
  effect and mask keys keep the parent-identity layer clock.
- **Frame fade.** The `Fade In+Out - frames` controller (`ADBE CM
  FadeInOutFrames`) with the complete preset expression on Solid Composite
  Source Opacity lowers to two linear owner Opacity keys: 0 at the layer
  inPoint, the static owner opacity `framesToTime(frames)` later at the
  composition rate. The Composite multiplies the image of the effects before it,
  so owner Opacity is equivalent after mapped FX effects such as Drop Shadow.
  Required profile: one Effect Parade root, one controller bound by name,
  fade-out 0, Source Opacity 100, explicit background Opacity 0, Normal
  blending, empty compositing options, a 2D non-adjustment layer without
  preserved transparency, a forward clock, a static owner opacity, no rendered
  Layer Styles and no enabled effect after the Composite except Geometry2. Local declarations must keep the expression's
  control names; their declared defaults are not pinned because explicit values
  are required. Other cases keep both native effects as diagnosed omissions.
  Later edits of the frame controls do not change the keys. No expression runs.
  Each occurrence has one fade owner. The owner lowering runs after the layer
  contents. When the [caption pill importer](#bounded-caption-pill-source-opacity-fade--import)
  has committed the same preset as its paint-Group ramp, the owner adds no keys
  and no decline warning. Otherwise the owner profile above applies or its
  omission stays: on ordinary Shape layers, on caption geometry fallbacks, and
  when the caption fade is declined or rolled back.
- **Still Geometry2.** Transform (`ADBE Geometry2`) on a footage still now
  becomes an editable Group between the owner and its source content, on the
  source image plane before the owner Transform. Native static or keyed Anchor
  Point/Position values in source pixels become stage values or keys on the
  owner clock with native eases. An Anchor Point without an explicit value uses
  the 50% default; a local declaration must store that default in the plugin
  API percent form. Skew, shutter angle, sampling, point expressions, several
  Transform effects, owner FX effects, Layer Styles or masks, 3D owners and
  preserved transparency stay diagnosed omissions. The Shape post-layer mapping
  is unchanged. Raster sampling and motion-blur shutter remain approximate. The
  shared point-default decoder still reads the first `pard` value word as a
  fraction of the source size; this correction does not depend on it.

Evidence (synthetic structural CPU coverage only):
`structure_document::tests::still_layers_and_frame_fades` authors Effect Parade
and layer records from the documented control layouts, with small arbitrary
values, and grafts them into public host fixtures. It covers a modern zero matte
ID against the legacy implicit matte, the still lifetime, static and keyed
Geometry2 stages and their omissions, frame-fade owner keys and their omissions,
an ordinary Shape owner, duplicate parades and rendered Layer Styles.
`complete_source_opacity_formula_does_not_admit_modified_bindings` covers the
preset formula match. A mapped effect before the Composite and a committed
caption ramp have no synthetic regression in this checkpoint. Independent Adobe
proof is unavailable: no Adobe operation, RGB/alpha comparison, Asset
publication or render of converted output was made.

## PSD footage import

**User-approved import-only increment.** PDF-compatible AI and FX → AEP are
explicitly deferred; this is not general PSD support or completed fidelity proof.
[Native fixtures and complete two-target registry](../crates/aftereffects_file/tests/fixtures/psd_import/README.md)
record source/media hashes, AE readback, executable tests and local 30fps references.

| Feature × direction | Original → replacement, reason and impact | Implementation / evidence |
|---|---|---|
| Import: merged PSD footage | PSD v1 RGB8 stored composite → PNG-backed editable `ImageLayer`. AE occurrence transforms/masks/effects/timing use existing mappings; Photoshop internal editability is lost. Does not recompose PSD layers. | Implemented for raw/PackBits data with RGB and optional explicitly marked merged alpha. Source `psd_sources_v2.aep`, comp 2; `adobe_psd_merged_import_packages_png_and_preserves_editable_image` passed. Independent AE author/readback/render ran; fresh FX RGB comparison **unmeasured**, alpha **unverified**. |
| Import: individual PSD-layer footage | Native persistent ID **and** record index select raw raster planes → separate PNG assets, cropped or full-canvas according to source dimensions. Never use layer names or substitute the whole composite; existing AE placement remains editable. | Implemented for bounded in-canvas rectangular layers, normal blend, full layer opacity, no clipping/masks/effects/groups/unknown rendering tags. Source comp 16; `adobe_psd_selected_layers_import_distinct_cropped_pngs_and_native_placement` passed for two distinct cropped layers. Full-canvas and RLE cases have supplementary CPU assertions, not independent Adobe proof. |
| Import: color management | Stored RGB samples → PNG samples without embedded-profile conversion. This may change color in FX; normalization always emits an `AE-PROPERTIES` diagnostic. | Approximation, not color-fidelity support. Native fixture has no embedded profile; tagged-profile behavior is supplemental CPU evidence only. |
| Import: unsupported/malformed PSD | PSB, 16/32-bit, non-RGB, ZIP, extra/ambiguous composite channels, missing merged preview, unsupported layer semantics/geometry, malformed lengths or stale/ambiguous selectors → contextual omission, no unbound image asset or whole-image fallback. Unrelated convertible siblings survive. | No fixed source-byte, cumulative-read, pixel or layer/channel-record count quotas. PSD v1's 30,000 pixels/axis format constraint, checked lengths and selected-channel semantics remain. I/O/PNG-publication failures remain fatal. Missing/short native PSD selector records stay item-local decode failures. Large padded-source, repeated-read, record-inventory, corruption/identity and cleanup tests are supplementary CPU evidence, not new Adobe/render fidelity proof. |
| Export: PNG/PSD | No new mapping or Photoshop reconstruction; existing native image-export profile remains OpenEXR-only. | **Deferred by user**, unimplemented/unproved here. |
| Import/export: PDF-compatible AI | No changes in this increment. | **Deferred by user**, unimplemented/unproved here. |

**Proof blockers:** independent native AEP and two Adobe-rendered 30fps MP4s exist
locally; long-term Asset publication/fresh remote hash verification are **pending
publication authorization**. Source PSD is specification-built, not Photoshop-
authored. CPU assertions passed for the named native import cases, but fresh FX
render comparison remains **unmeasured** and independent alpha proof **unverified**.
No generic PSD fidelity pass, complete feature-test delivery or bidirectional
support is claimed. Original prototype hashes and its inconsistent-composite
limitation are retained separately; V2 was authored as a new pinned revision.

## Effect animation records — bounded export repair

**Import is unchanged.** Fresh Bulge static/animated comparisons remain low
(minimum RGB **0.636054012 / 0.629035026**); editable assertions passing does not
resolve those differences.

**Export:** animated Effect Parade records now follow native parameter kinds,
including distinct fixed/float/integer/checkbox-popup descriptors and native
Point/Color storage flags. Static serialization and editable key values/times
are retained; no new approximation, generated JS, baking or renderer/schema
change is introduced. Six source-based regression tests fail before and pass
after; checkbox/popup flag assertions cover their separate native representation.
Fresh Radial Blur animated export improves from minimum **0.856163292 to 1.0**,
with all 60 decoded RGB24 frames exactly equal to its independent Adobe reference.
Static export remains **1.0**, with identical AEP bytes. Both exact output AEPs
were separately Adobe-opened and script-read: editable controls, keys and Linear
interpolation match the explicit FX inputs (Point sampling error ≤0.000124459 px).
Reference sizes/hashes were verified; the retained receipts permit either fresh
downloads or verified cache entries. Alpha/audio remain unverified.

The approved remaining 53-case export panel adds **52 fresh Adobe render measurements / 3,120 unique frames**: 33 minima are
1.0, 50 exceed the 0.95 triage cutoff, and Grain static/animated remain low at
**0.845302267 / 0.845528906** (cause unestablished; no replacement semantics or
threshold relaxation introduced). Hue/Saturation animated export is **unmeasured**:
its expected-oracle mismatch blocked Adobe execution, not an Adobe load rejection.
The reference covers only animated colorize leaves, not all master controls.
Drop Shadow static still fails its editable-effect assertion because existing
canonicalization substitutes a Layer Style (retaining Spread, losing plugin-only
Shadow Only); its 0.978025882 RGB score is not a semantic pass.

This is **not all-low-score completion**. None of those 53 exports received fresh
Adobe control readback; all 53 runner rows remain FAILURE. Other imports and
non-selected exports were unrun in this follow-up. Vignette animation remains
omitted with diagnostics. Scoped export CPU contracts retain two failures
(`drop_shadow_static`, `vignette_animated`), independently reproduced without the
patch. The unified runner still reports failure for the measured Radial exports
because its historical control-evidence hashes differ; current exact-hash
readback is separate, not a rewritten historical receipt.
The preceding Radial Blur results and 53-case panel figures are bounded historical
measurements, not current exact-hash control proof for the 53 exports.

## Converter review hardening — supplementary CPU evidence

These repairs preserve their checkpoint's feature scope and limitations; they did
not themselves add native Shape/Mask Path animation, an expression runtime,
generated JS, or new FX capabilities. The later bounded Path-key mapping is
recorded below. Tests below use generated projects, explicit FX inputs, or
structural mutations of pinned native fixtures; they are **not new independent
Adobe-native feature cases**. For these converter-review repairs (distinct from
the effect-animation export repair above), no new Adobe open/control inspection,
native-render comparison, RGB, alpha or audio proof was obtained. Existing
case results and unrun/unmeasured statuses remain unchanged.

| Direction / area | Repair and remaining replacement or limitation | Targeted structural evidence |
| --- | --- | --- |
| Import: structural ownership | Transfer retained native child chunks instead of recursively cloning their payloads. Parsed content and unknown records remain intact. | `read_layer_transfers_nested_unknown_payload_without_cloning` checks allocation identity; this is not a measured memory benchmark. |
| Import: Effect Parade | A malformed **present** explicit control table now omits only that occurrence with a contextual diagnostic instead of silently substituting defaults. Sparse canonical definitions remain supported. Failed effect construction no longer consumes an occurrence ID, so valid later content retains its allowance. | `malformed_explicit_control_table_omits_only_its_effect_occurrence`; `rejected_effect_does_not_consume_occurrence_identity`. |
| Import: camera / matte relationships | Keep original native layer indices when removing a generated camera; resolve matte links into emitted indices. Do not normalize away a camera serving as an implicit legacy matte provider. This preserves structure, not camera-as-matte rendering fidelity. | `generated_camera_before_explicit_matte_keeps_original_index_mapping`, its between-participants preservation case, and `generated_camera_used_as_legacy_matte_provider_is_not_normalized_away`. |
| Import: discarded animation allowance | Charge retained Adjustment owner opacity rather than discarded Transform geometry; omit unused affine source remaps, reclaim discarded audio/Adjustment entry reservations, and account for rebase size changes without releasing retained inline remaps. Non-opacity Adjustment guide motion remains an explicitly diagnosed initial-geometry approximation. Sampled expressions obey the same retention rule. | `pruned_adjustment_transform_tracks_do_not_consume_reachable_animation_budget`, its reversed-order case, `releasing_discarded_entry_keeps_inline_remap_reservation`, `sampled_adjustment_position_does_not_consume_ordinary_sibling_budget`, and `animated_adjustment_guide_is_not_claimed_exact_after_track_pruning`. |
| Export: native writer robustness / lookup | Reject short footage Anchor/Scale vectors with a typed error instead of panicking. Index full source identities and fonts while retaining signed-zero equivalence, first-seen ordering and distinct source metadata. | `source_geometry_rejects_short_transform_tracks_with_typed_error`; planner/source and font-table preservation tests. Lookup complexity improves by construction; no timing benchmark or Adobe acceptance is claimed. |
| Export: referenced source variants | Exclude an already-consumed source selector from wrapper Transform lowering; retain the referenced wrapper/matte link. Apply precomposition origin translation exactly once, including unkeyed Anchor axes. | `review_referenced_source_variants_exclude_consumed_selector_from_wrapper_tracks`; `review_referenced_source_variant_anchor_origin_is_applied_once`. Native values are checked in source-dimension units, not assumed FX pixels. |
| Export: disabled animator values | Normalize disabled keyframes to their runtime-visible `disabledValue` across layout, hierarchy clocks, source variants, masks, Source Text/Text Animator and paint-presence controls. Do not substitute stale typed bases or enabled keys. Existing graph validation and unsupported/missing-value rejection remain in force. | Targeted disabled-value regressions in `export_document` and its mask, text, paint, hierarchy and source-variant tests. Normalization discards inactive key history; it does not restore disabled native animation controls. |
| Export: existing Layer Style mappings | Correct unit-to-native-percentage divisors and combine Color Overlay opacity with stop alpha, including zero alpha. Fold effective disabled Drop Shadow blur/spread values before coupling. Animated blur with nonzero spread still retains the diagnosed base-value approximation; no new Layer Style scope or fidelity is claimed. | `animated_layer_style_percentages_use_native_percentage_units`, `constant_color_overlay_opacity_combines_stop_alpha_and_stays_finite`, and disabled coupled-shadow regressions. Internal export/readback and round trips are supplementary only. |
| Export runner: animated Hue/Saturation oracle binding | Bind the generated three-control native oracle only to the independently keyable numeric Colorize leaves. Retain the full seven-control request in report identity and retain CPU assertions for the four non-keyable Master/toggle bases plus their animation-omission diagnostics. This corrects a pre-Adobe harness block; it does not make the independent source a Master-animation oracle. | `test_hue_animated_binds_the_differentiated_native_oracle` failed before the binding repair and passes afterward. No Adobe render, readback or score was run for this repair. |
| Export: transactional diagnostics | Roll back normalization diagnostics when the containing native subtree is omitted; retain its omission diagnostic and convertible siblings. | `review_failed_layer_transaction_discards_unpublished_normalization_diagnostics`. |

### Grain untouched-instance state on export owners — export repair

This is an **FX → AEP export-only** repair; Grain import remains the existing five
editable core controls (`amount`/Intensity, Size, Softness, Aspect Ratio and Seed)
and still omits the plugin's preview, channel, application, matching and masking
controls. Independently authored static composition `235` in source SHA-256
`7c65ebe724fc8403399979cc1354adca3abd737b6ad7a76ba8eafc5b61ccf95b` and animated
composition `222` in source SHA-256
`4e38ded65e44f6a1adfe22713ddb36557afd312959a612ee601b75849d3f73ce` store untouched
Add Grain instance state as raw zeroes even where AE's scripting API and parameter
descriptors report nonzero UI defaults. The prior exporter copied those UI defaults
into the instance after moving the effect from a 320×180 native owner to a cropped
120×80 source precomposition. In Adobe that selected the wrong preview/output state
and changed preview-region geometry instead of preserving the source's Preview mode,
owner-relative region and enabled guide box.

Export now retains mapped core values/keys, normalizes every unrepresented numeric
Add Grain leaf to the plugin's untouched raw state, and keeps Show Box enabled as in
the pinned native sources. Exact non-ignored regression
`export_document::tests::effects::grain_export_normalizes_unmapped_plugin_state_on_source_owner_canvas`
freshly imports both pinned sources, confirms that the fresh owner keeps the 320×180
source canvas (the final-root output viewport no longer crops it to 120×80), and
compares every common native control after fresh export. Its earlier cropped-owner
form failed before the repair at Red Intensity (`source=[0]`, `fresh=[1]`) and passed
afterward. This own-reader CPU proof is supplementary: no source bytes are replayed,
no JS or frame baking is used, and omitted controls remain uneditable.

Historical independent 30fps RGB references are unchanged: static long-term Asset
`KgyJFdWEhBPkqpii40jo_vid` (SHA-256
`9a14a4d5534209131bb7c0c4058cc4d9951b39f04623a7e5e115c68e02068115`) measured
minimum **0.8453022673743517** at 40/30s; animated Asset
`ga1PC9CApThLYtV8oPVv_vid` (SHA-256
`511587a57671878bc639271fa85bef4decbec1cb81ed1170311d3c99ade250b1`) measured
minimum **0.8455289062707554** at 4/30s. Those measurements predate this repair. No
fresh Adobe render, generated-project control readback, RGB comparison, alpha/audio
measurement or manual UI inspection was authorized here, so improved visual fidelity
and editable Adobe control proof remain **unmeasured/unverified**.

Import and export repairs are implemented separately; neither establishes the
other direction's proof. Independent Adobe acceptance and rendering of the
repaired exports remain **unverified**. Full-suite validation is not implied by
the targeted CPU regressions. The post-rebase review panel executed **65 tests**
(**32 new**, 33 neighboring), all passed with none ignored; the support-ledger
checker also passed. Full-suite/check/clippy and Adobe validation were not run.
The review PR records the exact panel.

## Bulge Pin All Edges import limitation — diagnosed, not retuned

This checkpoint is **AEP → editable FX only**. The independently authored
Shape-owner cases retain the native Bulge center, radii, height, Pin All Edges
boolean, two-key animation, owner and siblings. Import now emits a contextual
`AE-PROPERTIES` warning whenever the source Pin All Edges control is, or can
become, enabled. The warning records the fixed capability mismatch: current FX
pinning clamps out-of-content samples into its content rectangle, which can
repeat an occupied boundary beyond the native owner; AE's pinned Shape output
does not do that. The converter does not invert, disable, duplicate or otherwise
retune the authored control merely to improve RGB scores.

The checked-in [Effects evidence inventory](after-effects-evidence/effects-coverage-results.json)
pins both exact sources and references. Static case
`aep-effects-coverage-native-static-controls-c14` (source SHA
`7c65ebe724fc8403399979cc1354adca3abd737b6ad7a76ba8eafc5b61ccf95b`,
composition 14; long-term Asset `sHaQU1uVLIBHZLtuVx7p_vid`, reference SHA
`a64120485c6d14c6fd13a457450dc200c2f0adbe651aa6c84f8b8c27dcb8804b`)
measured 60 RGB24 frames at mean `0.6360589268`, minimum `0.6360540120`.
Animated case `aep-effects-coverage-native-animated-controls-c1` (source SHA
`4e38ded65e44f6a1adfe22713ddb36557afd312959a612ee601b75849d3f73ce`,
composition 1; Asset `j2gXkaxCZimVQkEaW6kd_vid`, reference SHA
`07b8d89ac38f784a4f6a1e7f93c2e52a99d00383f29778c1ed79f334abd3c7cb`)
measured mean `0.8033242714`, minimum `0.6290350262` at `29/30s`.
Its continuous geometry controls do not jump at 1s, while the native Pin All
Edges Hold key switches from `1` to `0`; all 30 frames before the switch score
`0.629035..0.639437`, and every frame from 1s through `59/30s` scores
`0.9689153507`. Together with the current FX clamp implementation and the
occupied-blue-boundary extension in the recorded actual frames, that isolates
pinning edge semantics as the large discontinuity. Remaining post-switch
Bulge-kernel, omitted taper/antialiasing, RGB-only and weak-source-discrimination
gaps are still limitations, not passes.

Normal regression
`effects::tests::native_controls::native_bulge_pinning_edge_semantics_are_diagnosed_without_retuning_controls`
freshly imports both pinned native targets and requires the warning while
reasserting the exact editable effect values, Shape-owner survival and no
`JsScript`. Existing feature-proof symbols
`effects::tests::native_controls::native_bulge_static` and
`effects::tests::native_controls::native_bulge_animated` retain their exact
static/keyed assertions. The regression failed before the warning and passed
after it. No new Adobe session, render, upload or score was run for this
diagnostic-only repair; the measurements above remain descriptive prior
evidence, and alpha/audio/manual UI inspection remain unverified.

**FX → AEP is unchanged.** This import diagnosis neither implements nor proves
export behavior. Existing export structure/readback/RGB evidence stays separate;
a result on another branch cannot establish this import direction. Matching AE
Pin All Edges rendering while retaining the authored editable boolean requires a
current FX capability change outside this converter-only scope.

## Ripple amplitude strength limit — import approximation

This checkpoint is **AEP → editable FX import only**; FX → AEP export is
unchanged. It approximates a capability mismatch and is not a fidelity pass.

| Feature × direction | Original → replacement, reason and impact | Implementation / evidence |
|---|---|---|
| Import: Ripple Wave Height (`ADBE Ripple-0006`) | **Original:** native Ripple displaces only within its Radius (`-0001`), which has no FX control. FX Ripple displaces its whole destination plane, sampling `center + direction × g(r)` with `g(r) = r + amplitude × sin(frequency × r − phase)`. Above `amplitude × frequency = 1`, `g` decreases on part of every ray, so rings fold over into mirrored slivers and tears. **Replacement:** the ordinary plane-width lowering runs first, including AE-evaluated expression fitting, whose key selection, times and easing are kept. Then the retained static amplitude and every emitted amplitude key are multiplied by one factor so the largest of them meets the strength limit `1.25 / frequency`. The frequency is the emitted static FX value: `2π × width / Wave Width`, or the FX default. The `1.25`, 25% beyond the `1 / frequency` fold-over threshold, is a requested modest visual strength. It is a visual approximation, not calibrated against native output or derived from native units, and it does not prevent fold-over. The retained value counts even while an emitted track overrides it, because it is also the static fallback when lowering fails; a large authored value therefore also attenuates smaller evaluated keys. A Ripple whose values are all within the limit is unchanged, and zero values stay zero. Key times and easing, center, phase, frequency, enabled state and sibling effects are retained. Each rescaled track is recharged exactly against the generated-animation allowance; a track the allowance cannot hold is omitted and the limited static value retained. A contextual `AE-PROPERTIES` warning records the factor. **Impact:** strong native ripples render much weaker in FX, and a round-trip export authors the reduced Wave Height. At the retained value and at every emitted key, `|amplitude| × frequency ≤ 1.25`; linear and hold segments stay within that limit. Because `1.25` exceeds `1`, `g` can still decrease on part of every ray where a value is near the limit, so rings can fold over slightly there. Overshooting cubic easing between keys (an authored Bezier ease or a fitted expression curve) and the amplitude-sized disk around the center are not bounded. Radius, Wave Speed and Type of Conversion remain omitted. Rings remain ellipses with the plane's aspect ratio, because FX measures distance in normalized UV. The pinned catalog and coverage sources use Radius 0, and their independent Adobe references show no visible ripple; FX still ripples. | `structure_document::effects::limit_ripple_amplitude`, called from `structure_document::effects::import_with_context` after the ordinary lowering. Import-path regressions in `structure_document::effects::tests` host a writer-generated explicit Ripple on the pinned catalog composition 310 owner, because the Adobe occurrence stores Wave Height sparsely; they are supplementary CPU coverage, not Adobe proof. `evaluated_ripple_amplitude_keeps_its_ordinary_fit_before_one_uniform_reduction` (0/200.5/400px evaluated at 0/1/2ms) keeps the ordinary middle key; an unlanded first candidate, which scaled before fitting, emitted 2 keys instead of 3. `evaluated_ripple_amplitude_is_limited_at_its_extrapolated_emitted_keys` (0/20px captured at 0.25/1.25ms, lowered to −5…35px) limits the emitted 35px key; an unlanded first candidate, under a `1 / frequency` limit, emitted `amplitude × frequency = 1.75`. `retained_ripple_base_counts_toward_the_reduction_and_is_the_failed_lowering_fallback` covers a 400px base beside 2px samples and a budget-failed lowering. `keyed_ripple_amplitude_keeps_nonzero_speed_bezier_easing_and_exact_charge` covers a nonzero-speed native Bezier ease, sibling Wave Warp units, exact recharge and its omission fallback. `ripple_reduction_depends_on_wave_height_over_width_not_the_plane` covers four planes, zero and in-limit heights, a 3.5px height at 20px width (`amplitude × frequency ≈ 1.10`), beyond the fold-over threshold, that the strength limit keeps, and unchanged center/phase/frequency. Public-source regression `effects::tests::native_controls::native_ripple_amplitude_is_limited_only_beyond_the_fx_strength_limit`: static and animated catalog composition 310 (20px height at 20px width) is limited; coverage compositions 482/469 (3px and 3→6px at 40px) are unchanged. The catalog expectations in `adobe_catalog_imports_concrete_editable_controls_without_scripts` and `adobe_animated_import_retains_editable_effect_keys_and_identity` changed from `height / 320` to the limit `1.25 × 20 / (2π × 320)`. No Adobe session, native-render comparison, calibration or RGB measurement was run, including for the `1.25` choice. Private-project preview stills are diagnostic only; visual fidelity is **unmeasured**. |
| Export: Ripple amplitude | Unchanged: `amplitude × width` → Wave Height and `2π × width / frequency` → Wave Width. | Existing export structure evidence only. |

## PDF-compatible AI footage → editable Shapes — implementation checkpoint

This checkpoint is **AEP → editable FX import only**. A reached local `.ai` still
whose bytes are PDF-compatible is decoded by a converter-local, strict single-page
PDF profile and lowered directly to existing editable `Shape` layers. Supported
content includes compound `m/l/c/v/y/h/re` paths, nonzero/even-odd solid
DeviceRGB/DeviceGray fills, solid strokes with width/cap/join/miter/dashes,
paint order, graphics-state transforms, page box origin/UserUnit/right-angle
rotation, and Form XObjects whose validated implicit BBox does not clip their paint.
An editable, unpainted Shape guide clips paints at the PDF page boundary. PDF
paint order is reversed into FX's topmost-first sibling order; static geometry
and its clip use the full source clock rather than duplicating occurrence trim.
The original AI bytes are not packaged or required to render the Shapes. No PNG,
raw-AI asset, generated `JsScript`, schema/model/evaluator/editor/renderer change,
or per-frame bake is used.

The replacement is deliberately narrower than general PDF/Illustrator rendering.
Only one unambiguous page with MediaBox-equivalent CropBox and no optional-content
layer configuration is accepted; encrypted, PostScript-only, object/xref-stream
and multi-page sources are diagnosed and omitted. A PDF/AEP footprint mismatch is
omitted instead of guessing page/layer/crop selection. Matching dimensions do not
prove the native selector: same-sized layer selections remain unverified, and
accepted sources explicitly diagnose the whole-artwork interpretation.
Authored clipping, CMYK/ICC/pattern/gradient/alpha state, text, image XObjects and
unknown render-state semantics omit the affected paint or poisoned graphics-state
scope, while restored and independent convertible siblings remain eligible.
DefaultRGB/DefaultGray resource overrides and page transparency Groups reject the
source; Forms with transparency Groups/optional visibility or such color overrides
are omitted. Text clipping poisons its scope rather than exposing unmasked artwork.
Form BBox containment is conservatively checked in Form-local coordinates, including
stroke expansion; crossing/uncertain paints are omitted individually, preserving
bounded siblings (a control-hull bound can omit curves whose actual pixels fit).
Device
hairlines are omitted rather than misrepresented as zero-width FX strokes. Stroke
geometry uses the CTM active at paint time even when a path crosses q/Q or
multiple construction CTMs; only a singular paint-time CTM forces stroke omission
while retaining convertible fill.

The restricted loader profile is not memory or CPU isolation. Fixed source-byte,
per-stream/aggregate decode, retained-cache, object/operator/path/shape/dash,
graphics-state, page-inheritance depth and warning quotas are removed. The
recursive Form interpreter still requires a 16-level stack safeguard and a
100,000-operation expansion budget against repeated acyclic Form amplification;
exhaustion is a fatal conversion interruption, not a successful content omission.
These are implementation/work safeguards, not PDF format limits. Form and
page-inheritance cycles, malformed input and unsupported semantics are still rejected. Eager object/xref-stream expansion remains disabled by the parser's
zero-byte loader setting, including escaped PDF names, because that native profile
is unsupported, rather than using a bypassable substring scan. Content arrays are
interpreted as one logical stream. Large files may use all available host resources;
no process sandbox or hard total-allocation/CPU bound is claimed. Capacity
regressions are supplementary CPU evidence, not Adobe or renderer fidelity proof.

Structural evidence uses three **specification-built PDF 1.4 `.ai` samples**, not
Illustrator-authored files:
`spec_case_1.ai` (`2adf359be7da341e586ff67a1335748b84d310116c04bee39eb10a8e73f29c38`),
`spec_case_2.ai` (`1e2dc6daff0d427d5e488357c422ae000946dfeb3b68d130177a616be4d44448`), and
`spec_case_3.ai` (`af90fc16ee8ed66edf96f448eb737fa29efcff608a74d4cc21d687679d5215ed`).
`vector_media::tests::specification_built_cases_decode_editable_paints_and_transforms`
and
`structure_document::media::vector::tests::specification_built_sources_lower_to_ordered_editable_shapes_without_assets_or_js`
assert paths, winding, paints, cap/join controls, transforms, IDs/order and absence
of assets/JS. Parser-state/Form/color and adapter routing regressions are also CPU
only. Native AEP selector proof is unavailable (a bounded scan of the existing
native fixture corpus found no `.ai` references); no selector is invented.
Illustrator-native source proof, independent Adobe AEP composition/render,
Asset publication, RGB/alpha measurement and manual UI inspection are
**unrun/unmeasured**. Export/restoration is deferred under the approved
import-only scope and is not implemented or claimed.

## Sampled expressions, Fill and sparse Shadow — current import-only checkpoint

This checkpoint is **AEP → editable FX only**. It does not add FX → AEP export,
Layer Styles, AI assets, an AE expression runtime, generated `JsScript`, per-frame
output, or FX model/evaluator/renderer changes. Expression values are evaluated
by an opt-in Adobe integration helper and supplied to the Rust importer in an
exact-source-bound sidecar; normal import remains Adobe-free. The
[crate README](../crates/aftereffects_file/README.md#optional-adobe-expression-sample-import)
shows the CLI workflow.

| Import area | Current replacement and limits | Evidence status |
|---|---|---|
| Enabled numeric expressions | Direct Transform and ordinary Effect Parade numeric leaves in the selected composition's reachable graph are requested at 1 ms intervals. A temporary native `time` expression measures each actual AE evaluation timestamp; the scratch composition, layer, Effect and null footage are removed before any source expression is evaluated. Native observations are linearly resampled onto an enclosing integer-ms fitter domain (at most one extra millisecond per endpoint, using bounded endpoint extrapolation), then fitted with unchanged `fit_scalar_curve`, 0.001 tolerance and 128-key cap. Every actual native timestamp is revalidated. Values and times must be finite, bounded, strictly increasing and close to their request. Unsupported destinations, capture errors or rejected fits retain the ordinary static/default import with contextual diagnostics. | At the earlier v1 checkpoint, a fresh import of the unchanged original Mixkit 562 source converted composition `20432`; that multi-composition sidecar now requires v2 recapture before reuse. In that run, composition `20219` / layer `21040` Scale became 86 editable scalar keys (43 per axis), with all captured milliseconds within 0.001 FX unit. The pinned sampled-Position v2 case records 2,001 native timestamps and verifies sparse X/Y tracks against independent authored readback. No JS or dense per-frame output was authored. |
| Capture scope and resources | Sidecar v2 binds either an explicit selected root composition or all compositions. The helper follows only the selected composition's affine reachable graph and rejects reachable Time Remap edges rather than guessing their clock. Import rejects a selected-root mismatch; legacy v1 remains parseable only for unambiguous single-composition sources. Bounds are 60s/property at 1 ms, 250,000 total vector samples, 4,096 records and 128MiB JSON; Rust also validates source identity and bounded strings/records. Script-file transport is opt-in for this helper; other Adobe helpers keep their existing default. | The full original capture completed in AE 26.5x89 with 25 candidate records, 206,982 value vectors and zero capture-error records. It explicitly excluded seven expressions below Shape/Mask subtrees (not necessarily Path-valued expressions); excluded counts were Layer Styles 0, Shape/Mask subtrees 7, nested effect parameters 0 and other 0. Therefore this is not a claim that every project expression was captured. A conversion-old-project modal required manual acknowledgement. A helper timeout does not cancel the native AE invocation, so a timed-out run must not be retried concurrently or blindly. |
| `ADBE Fill` | Import approximates Fill with a Tint/Tritone whose black and white endpoints both use the Fill RGB; native opacity `0..1` maps to effect amount `0..100`. This produces a flat tint while retaining source alpha, but mask, feather, invert, alpha/kernel behavior and the original Fill-control linkage are omitted and diagnosed. | The pinned original-source regression verifies all six editable RGB channels against captured native colors. New independent case `aep-effects-fill-isolated-c1` also verifies blue-owner retention and 65% amount at native float32 precision; its selected ignored CaseBatch assertion passed. `adobe-test` measured all 60 unique frames against the independently Adobe-rendered 30fps reference: minimum RGB similarity **0.9899770117468304**, descriptive only (no quality threshold or fidelity pass). Source composition 1 / SHA `22696788c14ba53d400609e15d923c5e60b58d86a2d066fbfc5e7a2c42a1aed9`; long-term Asset `GfAJR26dbjq79BPXEl13_vid`, SHA `b5d695af2bff39328d4255bb1c7c9a3f02fdbfd1d8a8fced9542746060f10da0`, freshly downloaded/hash-verified. Exact test: `adobe_test_support::fill_isolated::adobe_fill_imports_tint_endpoints_and_percentage_amount`. Native controls were script-read, not manually UI-inspected; alpha remains unverified. Export remains the existing Tint path, not Fill restoration. |
| Sparse `ADBE Drop Shadow` definitions | Import combines an instance's local `parT` definitions with canonical built-in definitions and explicit current-instance control names. Values from the current instance always win; values are never copied between effect instances. | A targeted source regression pins the prepared Main composition's layers `20261` and `21018`, whose local definition counts are 8 and 0, and retains each instance's own color, opacity, angle, distance and blur. This is structural import evidence, not composite/alpha fidelity proof. |

### Native artifacts and visual boundary

The pinned sampled-Position source remains unchanged at 89,839 bytes, SHA-256
`ff5d9d59e13079dffa0eae8153bcfbf3ec01c57227267a0a256fc7c638c8abbd`.
Its separately named v2 sidecar is 112,294 bytes, SHA-256
`4000185e2e5d11cc3fc532a7ec2df8cf7ab18c5b795489de693d56db214a794f`,
bound to selected root composition 1. Exact ignored test
`adobe_test_support::sampled_position_expression::adobe_sampled_position_expression_imports_sparse_editable_xy_tracks`
passed through the feature-proof target with a successful CaseBatch record. At 24fps its first native clock values are
0, `25/24576`, and `49/24576` seconds. A bounded 24/30/60fps probe showed the
clock is frame-rate dependent, so production creates each scratch clock with the
candidate composition's native frame rate; no observed ratio is hardcoded.
The selected `adobe-test` then completed fresh import/render and comparison over
all 60 unique frames against long-term Asset `cCOwYIA6NEsrFtj7qgRN_vid`
(reference SHA `9f03bafe11bbb07b901e21b92a4809d0590f503f5c25bd3e2847d4677f4b4a32`,
freshly downloaded/hash-verified). Minimum RGB similarity was
**0.992860212886913**, descriptive only: no quality threshold or fidelity pass.
Alpha and manual Adobe UI inspection remain unverified. This is import-only
proof; no expression runtime or FX → AEP export proof is claimed.

The original Mixkit source remained unchanged at 1,102,366 bytes, SHA-256
`fdca629f14c0b05d4dec459e7bec6a291b1b21b67e7a1341a3ba022b0f37c8d9`.
Its expression sidecar is 9,091,751 bytes, SHA-256
`16d03bb64086e73105b903210779b93a5248b4b380d70b8362a31a583d394b7f`.
The fresh 10s, 1080p, 30fps Tesseract render is 1,201,333 bytes, SHA-256
`308e164e47f20ab4aa80a64ce6267e5657a593fcb9988b2040a41db851c43c6b`.
The comparison used an independent AME render of a **prepared copy**—media
relink/resave only, source SHA-256
`cb8139e4a82f17dbaf121a1a7313e5a5990c96e243f5a62a51d570515eba7a84`—
at 29.97fps, 300 frames / 10.01s, 3,134,191 bytes, SHA-256
`923c53c1df907f61bae1f165d07cb62d34120052dfb5112d27c20ba686c6aa25`.
The original and prepared source identities are intentionally not conflated.

A timestamp-aligned side-by-side was generated and opened during the review.
A bounded review at 2.3s found the main circle
position/scale and background matched, while many decorations were omitted and
wavy-line geometry differed; the missing AI logo was explicitly excluded. This
is useful historical visual progress, **not** full-feature fidelity: there is no
formal 30fps native Asset, quantitative RGB score, alpha comparison or native
frame-render proof for the capture itself. Source, media and sample artifacts are
not committed because their license/publication status is unresolved. Separately
authored Adobe cases are pending integration; no case IDs or execution status are
claimed here.

Executed external-source structural regressions (fixtures remain local):
- `structure_document::animation::expressions::tests::mixkit_evaluated_expression_import_regression`
  and `mixkit_evaluated_fill_import_regression`: **2 passed** with the exact original
  AEP and sidecar hashes above. Both perform fresh conversion of the native properties;
  the Scale test asserts sparse editable tracks and the Fill test asserts each RGB target.
- `effects::native::tests::mixkit_native_sparse_shadow_regression`: **failed with
  the sparse-definition repair disabled, passed after restoration**, on the exact
  prepared-copy hash above. The synthetic duplicate-instance test additionally uses
  different values to guard against cross-instance leakage.

## Feature summary

<!-- Existing fixture/control-appendix links land on current limits, not removed historical journals. -->
<a id="layer-styles--current-implementation-first-checkpoint"></a>
<a id="huesaturation-master-values--partial-export-repair"></a>
<a id="effect-parade--direction-current-converter-checkpoint-in-progress"></a>
<a id="nested-export-preservation"></a>
<a id="audio-layer-and-property-animation-completion-checkpoint"></a>
<a id="adjustment-layer-bidirectional-checkpoint"></a>

| Feature | AE → FX: editable mapping | FX → AE: fresh native mapping | Static / animated / combination restriction, replacement and impact | Structural / Adobe / RGB / alpha / audio evidence |
| --- | --- | --- | --- | --- |
| Project, root composition, timeline | All native composition item IDs/names, including nested compositions, can be listed from bounded metadata with `tsrct-conv inspect`. An explicit item ID selects the reachable graph; omission is accepted only when exactly one composition exists. | New AE26-oriented root, new IDs, current FX size/duration; `--fps` defaults to 24 and cannot infer source FPS. | Multiple compositions now fail with actionable `--composition` / `tsrct-conv inspect` guidance rather than choosing an inferred root; zero size becomes 1px; folders, metadata, custom view state and unused items lost. | S import/export; two historical **empty** writer projects opened in Adobe, not proof of general content. RGB/alpha/audio unverified. |
| Layers, clocks, parenting, precomps | Source-local versus parent clocks, bounded remap/stretches, bounded parent/matte chains and precomp instances become editable groups/links. Still images keep their native parent lifetime without a source clock. | Finite compatible clocks, parent references, supported groups/source variants and precomps; identity wrappers normalized. | Repeated instances become independent copies. Unknown bounds, cycles, unsupported remap/keys, parent opacity/visibility and hierarchy combinations diagnosed/omitted; no arbitrary shared identity restoration. | S bounded; source-relative Anchor sidecar repair has synthetic S and user-observed nonblack export, **not** independent sidecar fidelity.  |
| 2D/3D transforms, camera, blur | Numeric position/anchor/scale/rotation/opacity, compatible keys, stored 3D values, bounded motion blur/frame blend. | Supported native keys/3D sidecars, canonical generated camera, selected shutter/frame-blend settings. | No new general camera/light/material system; quaternion easing, unsupported coupled keys/projection, Pixel Motion and camera combinations differ. | S selected; historical frame-blend RGB mismatch remains measured, not passed.  |
| Adjustment layers | Direct editable stack scope/order, nested scopes, selected gates, effects, opacity keys and static masks/matte. | Fresh solid-backed Adjustment with native flag, supported guides, effects, clocks and gates. | Parent guide hierarchy flattened; animated/grandparent/3D gates and matte-provider combinations diagnosed. Existing FX wet/dry alpha and feather behavior are not AE-equivalent. | Ten independent native targets: import S 10, measured RGB minima 0.731985–0.988786; export S 10, Adobe open/render 10, native controls 9 matched / 1 partial, measured RGB minima 0.941530–1.0. **Not fidelity passes**. Alpha/audio unverified. [Exact cases](../crates/aftereffects_file/tests/fixtures/adjustment/evidence.json). |
| Solids, paints, native vector groups | Solids, ordered paint ownership, bounded group transforms, fill/stroke, static outlines and eligible parametric Rect/Ellipse/Star/Polygon. | Solids; eligible single-/bounded two-paint Shapes, separate opacity and selected vector Groups, bounded dash/paint keys. | Shared/cross-scope producers may become independent static copies; unsupported combinations and unknown bounds omit affected paint/subtree with warning. Fractional Star points floor after clamp; non-Hold animated point counts excluded. | S bounded; low-level solid Adobe probes only; broad independent vector/control/render proof incomplete. [Shape cases](../crates/aftereffects_file/tests/fixtures/shapes/README.md). |
| Boolean Merge Paths | Existing FX BooleanOperation maps bounded native geometry and owned paints. | Bounded Merge modes, transforms and **Rect Size/center/Roundness, Ellipse and PolyStar parameter keys**, including nested operands. | Wrong-kind geometry, unmapped paint/modifier keys, incompatible clocks or Path keys omit Boolean subtree, retain independent siblings. **Not transform-only** for eligible parametric operands. | Export seven focused CPU tests passed after repair; independent animated-Boolean native oracle, Adobe opening/readback and RGB **unrun/unmeasured**.  |
| Shape and Mask Path keys | Bounded native keys → typed editable Path tracks, owner clocks, Linear/Hold/zero-speed Bezier. | Fresh parallel timing/contour records for Shape and compatible same-parent Mask guides. | Unequal non-Hold topology, unsupported ease/expressions/compound fusion and incompatible mask clocks diagnosed; no JS or sampled contours. | CPU/native control assertions pass; bounded Adobe Shape/Mask exports and Mask import RGB/alpha pass; Shape FX raster coverage **fails**. [Panel evidence](../crates/aftereffects_file/tests/fixtures/path-keys-proof/README.md). [Exact limits](#native-shape-and-mask-path-keys). |
| Stroke, gradients, fills | Static fill rules/caps/joins, bounded Hold-only Join keys, strokes/dashes, linear/radial gradients and stops mapped where representable. | Eligible paint controls/keys and static gradient streams; reflected gradient normalized to mirrored linear endpoints/stops. | Variable-width taper/wave, animated gradient stops/axes, midpoint bias, nonstandard gradients, odd/invalid dash patterns and incompatible paint combos omitted/approximated; original linkage lost. | S selected; typed gradient descriptor CPU repair and **user-reported opening**, but no independent gradient score/control readback.  |
| Text, masks and mattes | COS point/box text, bounded animator tracks, static masks, selected mattes; modern matte ID 0 means no matte; unsupported masks diagnosed. | Fresh point/box text, selected animators, static masks and supported matte relations. | Fonts/layout/baselines, linked parental feather, expression-dependent text and complex selector/path combos may differ; Path keys have the bounded mapping above. | S selected; masked Adjustment cases have bounded Adobe readback/RGB, **not general** text/matte or alpha proof. |
| Media/video, source replacement | Package approved local assets; occurrence-local supported media replacement and layer clock/crop/fit where representable. Restricted single-page PDF-compatible `.ai` stills lower to editable solid-path Shapes without packaging the AI. | Eligible packaged WAVE/known QuickTime/EXR, finite source variants and supported clocks. AI/Illustrator restoration is not implemented in this import-only increment. | Missing assets, unknown codecs, optical-flow differences, unsupported live switch/dependencies and resource bounds diagnosed; no transcoding guarantee. AI clipping, non-RGB paint, gradients/text/images/alpha/OCG/selectors and broader PDF forms are diagnosed omissions; no raster/raw-AI/JS fallback. | Ordinary media S selected. AI S uses three specification-built PDFs and bounded CPU tests only; no native AEP selector, Illustrator source, Adobe render/control, RGB or alpha proof. [AI checkpoint](#pdf-compatible-ai-footage--editable-shapes--implementation-checkpoint); [media fixture inventory](../crates/aftereffects_file/tests/fixtures/media/README.md). |
| Audio layers/gain | Separate audio from AV visuals, stereo dB → scalar gain; unequal channels choose quieter gain, mute exact zero. | WAVE/established QuickTime, equal stereo dB from gain, bounded gain keys, mute/trim/playback and finite Hold switches. | Stereo separation lost; gain↔dB continuous Bezier is approximate, zero floors at −192dB; no unsupported formats or implicit speed change. | S cases; 28 pinned native E2E references and one smoke **failed duration contract**; general Adobe export acceptance and audible fidelity **unrun/unmeasured**. [Audio suite](../crates/aftereffects_file/tests/fixtures/audio_e2e/README.md). |
| Effect Parade | Mapped named plugins/controls become editable EffectRecord values and compatible scalar keys. The bounded `Fade In+Out - frames` preset becomes owner Opacity keys; Geometry2 on a still becomes a source-plane Group. | Fresh plugin records/current FX values and compatible numeric keys, not original plugin bytes. | Unknown plugins/controls default or omit, kernel differences, coupled Color/Point cubic keys, popups, duplicates, expressions, expansion/clipping and owner/clock combinations diagnosed. | Historical 55-case panel: import S **52 pass/3 fail**, export S **55 pass**, Adobe controls **50 pass/2 fail/3 rejected**; RGB measured 55 imports/52 exports, **not quality passes**, alpha unknown. Separate Vignette animation omission; [control-level appendix](formats/after-effects-effects.md) and [results](after-effects-test-results.md#current-effects-panel). |
| CC Vignette | Native Amount/Angle-of-View controls and compatible keys map to editable FX amount/radius; native Center/Pin Highlights nondefaults diagnosed. | Static base Amount ×100 and radius ×60; **animated export omitted with a diagnostic**, base retained. | Kernels differ; FX feather has no native control. Centered/zero defaults replace native Center/Pin Highlights. No per-frame bake or hidden source restoration. | Historical import I-S and measured RGB-I, fresh static E-S; no Adobe export readback/RGB-E for Vignette. [Control appendix](formats/after-effects-effects.md#vignette--fx-vignette--cs-vignette-cc-vignette); [animated omission test](../crates/aftereffects_file/src/export_document/tests/effects_edge_coverage.rs). |
| Hue/Saturation Master subset | Signed Master controls have a source-based **static** assertion; older Colorize-on/animated expectations remain red. | Integer static Master default and rendered state repaired; animated Master/Colorize toggle omitted with base retained, fractional value rounded, out-of-range default retained. | AE reports four leaves non-keyable; other channel ranges/unselected combinations unproved. | Selected Master-only export 60-frame RGB min/mean **1.0**, Adobe UI/readback proof **unrun**; import RGB **unmeasured**. [Repair/evidence](after-effects-evidence/hue-master-export-repair.md). Older failures remain in checkpoint, not relabeled. |
| Layer Styles (native style phase, **not** Effect Parade) | Drop/Inner Shadow, Outer/Inner Glow, Bevel & Emboss, Satin, Color/Gradient Overlay, Stroke → nine existing FX LayerEffect forms. | Edited styles → nine native style identities, bounded compatible scalar keys. | Native Blend/kernel/gradient geometry, noise/contour and ordering approximated; Color Overlay uses constant GradientOverlay, duplicates/phase normalized. Color/vector/offset/stop animation retains base with diagnostic. Hidden Pattern Overlay omitted (no FX style). | Nine independently authored **static** native cases; source assertions and edited export readback tracked separately. Reference publication/final targeted execution and RGB/alpha **not established**. [Per-style replacements and proof](after-effects-evidence/layer-styles.md). |
| Scalar JavaScript preparation (**export only**) | Never emits JS. | Supported **layer-local scalar** `layerTimeJsCode` is evaluated/fitted to editable keys before Check/Write; original archive unchanged. | Adaptive fit tolerance and bounded owner interval approximate, not exact; legacy/mixed clocks, dependencies, layer references, non-scalar and failed scripts keep diagnostic/omission, but source length alone no longer rejects a script in ordinary AE, selected AE scopes, or Premiere export. Trusted scripts only: VM caps are not wall-clock/heap isolation. Evaluation threads reserve an empirical 32 KiB of native stack per source byte (minimum 16 MiB), without the former 32 KiB source / 1 GiB stack clamp. Stack-size arithmetic overflow and thread-start failure abort preparation. This is **not proven native-parser safety**: recursive parsing and run-time generated code can still abort the process. Positive oversized-source regressions are structural evidence, not Adobe fidelity proof. No expression runtime. | Targeted 72 CPU tests passed at checkpoint; 2,215 scripts → 29,820 keys bulk result is structural, later Adobe/direct-FX comparison **visibly mismatched**. Isolated motion fidelity unmeasured. Historical budgets/proof archived in the prior revision. |
| Other unsupported semantics | Markers/trackers, auto-orient, preserve-transparency/collapse, unknown sources, lights/materials and expressions not independently implemented. | Unknown effects/FX shaders, editor state and unsupported semantics omitted with context; no original-record replay. | Converter preserves independent siblings where safe; unsafe/malformed framing, integrity, dangling reference and I/O failures can abort instead of producing misleading output. | S diagnostics where present; independent visual/alpha/audio proof absent. |

## Native Shape and Mask Path keys

The prior converter exclusion is superseded by explicit approval for **both
import and export**, using the existing typed FX Path capability. No FX schema,
editor/API, evaluator or frontend changes are part of this increment. Static and
keyed compound Shape Paths are split into separate native Path properties under
the original shared paint scope. Changing contour counts require Hold segments;
there is no invented particle correspondence or morph. A later
explicitly approved rendering bug fix restores antialiasing for native ProRes
exports only; Auto/H.264/HEVC and playback policies remain unchanged.

| Feature × direction | Implementation / replacement | Evidence and limits |
| --- | --- | --- |
| Shape Path import | Native `om-s/omks` geometry paired with timing records; cubic controls, signed millisecond keys, closedness and compatible topology retained. Source-local keys remain under the imported owner clock. | `native_path_key_panel_keeps_linear_hold_and_bezier_authored_keys`: existing native compositions 1/17/32, two authored keys at 500/2000 ms. `pinned_native_paths_import_keys_and_match_independent_adobe_values`: upstream hashed source/projection pairs, one Shape plus three Mask paths. |
| Mask Path import | Source-normalized vertices/handles scaled to source pixels; Path track targets the editable guide; native start/stretch maps into its parent clock. Atomic animation-budget failure retains the initial outline. | Same pinned projection test checks native vertices/controls/closedness and first-vertex per-frame interpolation within 0.002 pixels. Additional native two-key mask, owner-clock and budget tests run CPU-only. |
| Shape Path export | Current edited typed keys → fresh native 64-byte timing records and authored `shap` values; never source-subtree replay. One effective constant/disabled key writes a static contour. A stable-count compound path splits into ordered independent native tracks with original key times/easing and shared paint. | `path_keys_stable_multicontour_exports_separate_editable_tracks` failed before and passed after the split; existing single-contour structural tests pass. No independent Adobe acceptance or rendered-fidelity proof for the new split. |
| Mask Path export | Compatible same-parent unmodified Shape guide copied to native mask; relative affine and source normalization applied to **every key**. Static text-on-path policy is unchanged. | `path_keys_export_animated_mask_with_every_key_in_owner_coordinates` inspects fresh records and supplementary reader readback. `path_keys_mask_clock_mismatch_is_diagnosed_without_losing_owner` pins the omission boundary. |

Limits and their impacts:

- Linear, Hold and zero-speed Bezier sides are mapped. The pinned mixed
  Linear/Bezier Path projection establishes a one-sixth diagonal linear handle;
  other nonzero-speed or arbitrary diagonal cubic controls are rejected rather
  than assigned an invented Path speed scale. Mixed-ease segments other than the
  pinned two-second duration are rejected in both directions; different native
  versions remain unproven.
- Non-Hold keys require matching command topology; unequal vertex counts or
  open/closed morphs retain the initial import outline / omit the affected
  export content with diagnostics. FX's approximate unequal-topology morph is
  not substituted for AE semantics. Hold may change topology.
- Native export supports bounded coordinates/keys and ordered compound Shape
  Paths. Changing contour count requires Hold; absent slots use an open
  one-vertex native Path. All-empty intervals also hide the shared paint group
  through editable native opacity keys: one-vertex geometry alone can leave
  a round-cap dot in fresh exports. Stroked partial disappearance is diagnosed
  and omitted until per-contour visibility preserves shared paint semantics.
  Fill-only held geometry is retained without claimed persistent particle
  identities. Non-Hold count changes remain omitted with context.
  FX per-point mirror/corner controls, unsupported expressions and RotoBezier
  remain outside the established mapping; independent siblings remain.
- Import transforms supported forward/reverse clocks; reversed Hold boundaries
  cannot be represented exactly and retain the initial outline. Submillisecond
  key collisions are rejected. Whole-track geometry and animation allowances
  prevent partially published tracks.
- Mask export requires a shared owner/guide start, a guide covering the owner's
  duration, and proven static relative transforms. Group playback, Image/Video source clocks, animated transforms and
  cross-parent guides are diagnosed omissions, not silently assumed equivalent.
  Guide links become independent copied keys, not live shared editing identity.
- Nested Shape bounds use control hulls and temporal overshoot enclosure rather
  than frame sampling. Existing unsupported Boolean Path-key programs remain
  diagnosed omissions.

**Proof status (bounded AE 26.5x89 panel):** four new independently authored
sources, native 24fps / output 30fps, five editable keys, and eight immutable
long-term MP4/MOV Assets were produced; fresh download hashes were verified.
Fresh import structure and independent Adobe reopening of both edited exports
pass key times, geometry, topology and temporal-control assertions. Both native
exports pass all registered RGB/RGBA/alpha gates; the Mask import and edited-FX
render also pass. **Shape FX raster coverage remains a measured failure**, so
strict visual proof remains incomplete. The user explicitly accepted delivery
of the editable bidirectional implementation with these residual Shape raster
differences documented; no proof-complete or pixel-identical claim is made.

The initial scorer omitted its preregistered alpha-convention normalization;
those raw RGB results are diagnostic, not comparable fidelity gates. The current
comparison uses an explicit premultiplied-black derived FX copy, leaving native
references unchanged. The remaining Shape discrepancy includes the renderer's
shared legacy +0.5 px stroke-width compensation; changing that shared behavior is
not included in the narrow ProRes antialiasing fix. Further stroke-policy
changes are deferred under the user-accepted milestone; this does not relabel
the failing comparisons as passes. Root-content
`--fx-solo main:1` excludes the diagnosed opaque document-preview background;
this is not full-project alpha proof. See the [case/Asset/hash/measurement
record](../crates/aftereffects_file/tests/fixtures/path-keys-proof/README.md).
The artifact manifest now checks every scored MOV/control artifact and native
source hash before comparison. Historical import/readback/render attribution
remains **procedurally observed**, not cryptographically recorded at execution;
the post-run hashes do not retroactively authenticate that causal link.
Historical ignored grouped tests remain **UNRUN** unless separately recorded.

Source/provenance: [Path fixture inventory](../crates/aftereffects_file/tests/fixtures/path-animation/README.md).
CPU regression commands: `make test-aftereffects-file filter=path` and
`make test-aftereffects-file filter=mask` from `opensource/conv`.

## ShapePath JS baking and held contour disappearance

Export preparation now uses `fx_keyframe_bake::value_curve` and its shared Boa
runtime for layer-local ShapePath scripts. No FX model, schema, evaluator,
editor or renderer changes, generated JS, footage flattening or source-AEP replay.

| Feature × direction | Original semantics → implementation / limitation | Executed evidence |
| --- | --- | --- |
| ShapePath JS → AEP | Sample the owner's integer-millisecond clock; remove equal held samples and jointly reduce continuous coordinate runs to Linear keys within 0.01 geometry-space units. Hold keeps discontinuities and topology changes. Dense working history is reduced in 256-value windows; output remains bounded by the native u16 key field. Window boundaries can retain additional keys. This is a sampled-clock approximation, not a screen-space/submillisecond/final-pixel guarantee. | Scalar/Path adapter tests, including steps, variable contours, linear reduction, stateful-script rejection and the explicit editable disappearing-triangle input. S02's 59 Path scripts became 780 compound editable keys; fresh Adobe export rendered all 55 frames at 1920×1080/30fps, without a black interval. |
| Held compound Path → AEP | Split ordered contours; missing slots use native one-vertex paths and all-empty intervals carry separate native group-opacity keys. Partial disappearance with strokes remains diagnosed/omitted because empty round-cap slots can draw dots. All affected transitions must be Hold, so slots do not interpolate between unrelated particles. Physical particle identities are not recoverable from unlabeled JS contour arrays. | Pinned Adobe 26.5x89 source, comp 1; four independently read native keys at 0/500/1000/1500 ms with 3/1/3/1 vertices. Its 60-frame native reference hides the shape in [0.5,1) and [1.5,2), including round-cap stroke. Fresh-export geometry records match the native one-vertex records, but initial export without visibility keys left a 56-pixel dot at 0.5s; broad blackdetect alone missed it. The regression now requires explicit editable opacity keys. Final Adobe readback verifies 100/0/100/0 opacity and four Path keys; critical absent frames have no channel above codec-black value 2. Visible fill/stroke appearance still differs (diagnostic aggregate RGB PSNR 19.5688 dB), so this is not full raster fidelity. |
| Native held disappearance → FX | Existing importer remains unchanged; fresh import retains editable Path keys and their one-vertex geometry. | `native_held_disappearance_imports_editable_path_controls` passes. Imported-FX pixel comparison is unmeasured; no new broad import-fidelity claim. |

The independent source, exact editable input, reference Asset/hash and case
execution details are in
[`hold-disappearance-v1-evidence.json`](../crates/aftereffects_file/tests/fixtures/path-keys-proof/hold-disappearance-v1-evidence.json).
The reference Asset was freshly downloaded and SHA-verified; publication is not
itself a conversion pass. Alpha remains unmeasured.

S02 is **partial user-visible recovery, not full-film completion**. Its original
FX and Adobe-export clips both contain 55 frames. Full-resolution RGB planar
FFmpeg comparison measured aggregate PSNR **16.0535 dB**, worst **10.33 dB** at
local **0.1s** (frame 3); this is a mismatch/diagnostic, not a fidelity gate pass.
Custom WGSL effects are still omitted and nested effect/style expansion can
clip; the original glitch/glow appearance differs. The original private source,
S02 media and comparison artifacts remain ignored local files. Other full-film
scenes, alpha and audio are not established by this case. A full-film conversion
attempt reached its external 20-minute timeout before publication; no new full
AEP or full-film render was established. Whole-project JS evaluation cost remains
an implementation/performance blocker.

Unsupported dependencies/layer references, playback remapping, legacy script
clocks, malformed/non-finite paths and unsupported per-point controls remain
contextual failures, not silent static success. Fresh out-of-order probes plus
ascending playback reject obvious stateful scripts; arbitrary JS remains trusted
input under external process limits, not a sandbox or proof of purity.

## Diagnostic code inventory (source semantics → replacement and impact)

Stable codes below are emitted by [`diagnostic.rs`](../crates/aftereffects_file/src/diagnostic.rs),
except where a row is retained explicitly as historical provenance. Contextual messages
identify source composition/layer and feature-specific fallbacks; this inventory is
**not** a guarantee that every optional property is detected.
The feature rows above supply individual restrictions. This table is retained
here so the offline `test-aep-support-ledger.py` guard checks all emitted codes.

| Code | Original / trigger | Current replacement or omission; reason, impact and evidence |
| --- | --- | --- |
| `AE-VERSION` | Other producer/version | Decode only known records with uncertainty; no cross-version Adobe guarantee (S). |
| `AE-SELECTION` | Historical: multiple possible source compositions | Retained as a stable historical limitation identifier for prior evidence. Current import does not emit it: more than one composition without `--composition` is a typed ambiguity error, and `tsrct-conv inspect` lists all IDs/names (including nested compositions) without media probing or conversion. |
| `AE-PROJECT-METADATA` | Project-level settings/organization | Reachable selection only; folders, proxies, unused items, color/export/editor state not reconstructed (S). |
| `AE-COMPOSITION-SETTINGS` | Native per-comp settings | Canvas/duration and bounded shutter settings; Group FPS/aspect/background/viewport incomplete, zero dimensions use 1 px (S). |
| `AE-GROUP-BOUNDS` | Fixed AE canvas | Child-bounded Group; composition edges, alpha clipping, 3D anchor/projection can differ (S, fidelity unmeasured). |
| `AE-INDEPENDENT-COPIES` | Reused native source | Fresh per-occurrence editable identities; shared editing linkage lost (S). |
| `AE-PLACEHOLDER` | Missing/unsupported item | Named non-rendering Group, retain siblings; pixels lost, never a fidelity pass (S). |
| `AE-PROPERTIES` | Unsupported/defaulted property, expression or effect | Bounded mapped values retained; unknown control/animation omitted/defaulted, mapped Layer Styles separately retained; no expression evaluation (S selected). |
| `AE-PARENTING` | Native transform-parent chain | Bounded transform wrappers; missing/cyclic/deep parents and inherited opacity/visibility/lifetime not equivalent (S). |
| `AE-TRACK-MATTE` | Native matte provider/relation | Supported references/helpers; unsupported/missing/cyclic providers diagnosed, alpha unverified (S selected). |
| `AE-BLEND-MODE` | Native transfer mode | Known explicit mapping or diagnosed fallback; pixel math may differ (S). |
| `AE-LAYER-SWITCHES` | Native rendering switches | Eye/audio, solo/guide and selected blur/blend; collapse, auto-orient, sampling/transparency limited (S). |
| `AE-LAYER-METADATA` | Names/labels/editor state | Source identity may be described; labels/quality/lock/shy/comments not controls; long export names truncated (S). |
| `AE-TIMING` | Invalid/fractional/reverse clocks | Bounded ms/native ticks and visibility; invalid duration→1s, invalid range→parent span, invalid mapping→1×; negative prefix clipped; reverse-boundary mismatch retained (S). |
| `AE-MISSING-REFERENCE` | Missing source/parent/matte | Omit link or non-rendering placeholder, preserve independent siblings (S). |
| `AE-CYCLE` | Recursive source graph | Stop cyclic branch, keep noncyclic occurrences (S). |
| `AE-EXPANSION-LIMIT` | Excessive nesting depth | Depth-bounded partial graph; omitted branch changes content. No fixed generated-occurrence count cutoff or process memory-isolation guarantee (S). |
| `AE-UNKNOWN-ITEM` | Unrecognized source kind | Parsed kind retained low-level, placeholder in editable FX rather than guessed content (S). |
| `[AE-EXPORT]` | Unsupported FX→native mapping | Fresh mapped content only; affected leaf/layer/subtree omitted with contextual warning, independent siblings retained; malformed global identity/I/O fail (S selected; Adobe fidelity unmeasured). |
## RGB Invert and repeat-edge blur bounds — import-only checkpoint

Static `ADBE Invert` with Channel RGB (native popup 1) becomes existing editable
Levels with input black/white 0/255 and gamma 1. Blend With Original `b` percent
sets output black to `255 * (1 - b/100)` and output white to `255 * b/100`;
alpha remains unchanged. Other channels, keyed/expression controls, malformed
values, and percentages outside 0..100 omit this effect with a diagnostic while
retaining its owner and convertible siblings. Export continues to use the
existing native Levels writer; this does not add a native Invert writer.

Gaussian Blur with Repeat Edge Pixels now carries the known host bounds through
existing `layerSize`. Adjustment layers use their composition canvas because
they filter the sibling stack; other unsized effect Groups use source-local
bounds. This avoids accidentally clamping a 4K adjustment into an HD output
rectangle. Unchecked repeat edges retain the prior payload.

The small [native control excerpts](../crates/aftereffects_file/tests/fixtures/effects/cosmic-controls.provenance.json)
preserve the original sparse plugin defaults. Focused CPU tests cover editable
inversion, rejection of unsupported modes, and distinct adjustment/source bounds.
The real-project rendering diagnostics show the reported title/crop improvement;
they are not general AE algorithm or strict parity certification. Static CC Toner
is covered by the separate bounded import approximation below. Fractal Noise and
CC Split remain omitted.

## Cached text baseline and bounded caption pills — import-only checkpoint

Boxed text with a valid cached first baseline explicitly uses existing FX Top
alignment after placement consumes that baseline. This prevents legacy explicit
leading from shifting the box a second time. Point text and missing/malformed
baseline caches keep their previous fallback; the cached baseline does not
recompute when the font is edited.

One complete stock caption program is recognized: preceding text layer glyph
width plus static named Width Padding/Width Override controls, static Height and
Roundness controls, a 0.6-second native `ease` reveal beginning at `inPoint`, and
the matching negative-half-width group anchor. The bounded importer preserves
editable Rect size/anchor tracks, source-local clocks, corner radius, and two
independent Fill/Stroke owners. It rejects altered programs, ambiguous or dynamic
bindings, enabled Source Text expressions/keys, changing horizontal text geometry,
and mandatory-track failures. It introduces no expression runtime or frame bake.

Width uses the source's fixed cached glyph bounds and emits that approximation
explicitly. Text/font edits and missing-font substitution do not re-evaluate
`sourceRectAtTime`. In the Bonsa diagnostic, the original Exposure cache plus
padding is about 835 px while the native missing-font preview pill is about 773 px;
those preliminary measurements are not a calibration or an exact-fit claim.
The source font is preserved. The locally licensed Bonsa controls are exercised
through explicitly ignored, opt-in tests; the native boxed-text fixture separately
pins the baseline correction. New minimal native caption/render parity remains
unproven.

Separate Fill/Stroke ownership preserves authored color, opacity and paint blend
modes. Native gamma/compositing parity remains unverified.

## Static CC Toner ramps — import-only approximation

Native Tritone (Tones 2) and Pentone (Tones 3) lower at their original stack
position into two existing editable effects: a full-strength grayscale
TintTritone followed by RGB ColorCurves with identity Master. Source colors remain
normalized floats; native sparse defaults come from the pinned local `parT`
control excerpt, never from zero-filling missing values. Tritone knots use
0/0.5/1; Pentone adds 0.25/0.75. Existing FX luma weights 0.299/0.587/0.114 and
piecewise-linear, equally spaced interpolation are explicit approximations, not
established Cycore semantics.

Partial Blend with Original uses Adjustment wet opacity only for a normal
Adjustment whose only enabled nonidentity native effect is Toner, with static 100% layer
opacity and no masks/matte/authored or malformed Layer Styles. The native 0.4 value yields a 60% wet gate around the
whole grayscale/ramp pair. A Color Balance (HLS) stage is proved identity only
when all three canonical Hue/Lightness/Saturation controls resolve to static
finite zero. Exact pure cross-composition effect-property aliases resolve unique
composition/layer/effect names and the same canonical parameter identity; missing
aliased owners, ambiguous resolution, animation or unresolved source controls never
qualify. Canonical absent
HLS defaults are zero; explicit malformed or nonzero controls take precedence.
This does not map HLS to Hue/Saturation, evaluate arbitrary expressions or retain
controller edit linkage. Disabled siblings preserve their state and cannot
reset an earlier wet override. Other partial owners, unsupported modes,
malformed/duplicate values, and animation/enabled expressions on consumed
controls omit Toner with a contextual warning while retaining the owner and
other convertible effects. The existing dry-plus-wet alpha warning remains:
opaque RGB interpolation is represented; translucent alpha equivalence is not
established.

[Native control-only fixtures](../crates/aftereffects_file/tests/fixtures/effects/cosmic-toner-controls.provenance.json)
and focused CPU tests pin colors, sparse defaults, ordered editable ramps,
rejection controls, disabled siblings and actual Adjustment opacity. Source-only
readback and local Bonsa preview comparisons are diagnostic evidence. Independent
minimal Adobe render/alpha proof and native CC Toner export reconstruction remain
unproven. Fractal Noise remains outside this checkpoint.

## Flat-profile CC Split 2 — bounded import approximation

One enabled CC Split 2 on an unparented, planar, full-opacity Normal Adjustment
with centered identity Transform and a known composition-sized square-pixel solid source
can split a closed visual sibling stack into independent editable copies. The
bounded profile requires static left-to-right horizontal endpoints, a flat unit
256-sample custom profile in the observed native framing, and nonnegative static
identical side amounts or identical monotone zero-to-positive scalar-key schedules.
Unequal sides are declined because their native side assignment is unproven. Expressions, animated
endpoints, additional pixel effects, crossing parent/matte/dependency references,
physical media and failed import budgets retain the original siblings with a
contextual omission diagnostic. Rejected copies do not consume identities or
animation or shape serialization reservations.

Each side uses an editable hard half-plane mask followed by vertical Group
Position. Candidate displacement is source height times the authored amount/100,
with the admitted equal amount moving the upper half upward and the lower half
downward. Source key times, easing and start/stretch use the ordinary numeric
clock conversion. Separate Hold opacity gates retain the original unmasked stack
before motion support and outside the effect lifetime; the split copies receive
no additional time remapping. Above siblings stay in place and the original
Adjustment retains its after-Split Layer Styles. Existing Posterize Time holding
also encloses the new split graphs and keys.

This is an explicit approximation: extending a finite source segment across a
full half-plane, custom-profile interpolation flags, the native opaque allocation
word, raster/antialias kernel and the exact Cycore displacement law remain
unverified. CPU checks establish editable structure, independent identities,
source clocks and rejection/rollback. Existing Bonsa native movie comparisons
are diagnostic and do not establish a strict feature-fidelity pass. Native CC
Split 2 export reconstruction remains unsupported.

## Full-span Adjustment Posterize Time — import-only checkpoint

A full-composition, zero-start, unit-clock Normal Adjustment with static 100%
opacity and a sole enabled Posterize Time effect can hold a closed visual stack
below it. Static rates and strictly ordered Hold keys in the existing 1–60 fps
range become interval gate Groups containing full-duration static-rate Groups
anchored at source zero. Gates have explicit identity time mapping and remain
outside the held clock; the native switch is rounded upward to the first integer
millisecond on or after it. A native one-third-second switch therefore uses
334 ms: 333 ms still uses the earlier grid. Arbitrary submillisecond continuous
boundaries and general Adjustment timing remain unrepresented.

The first branch retains existing identities, content and keyframes. Later
branches reconvert the original native sibling indices through the ordinary
layer, matte, Set Matte and Preserve Underlying Transparency paths, assigning
fresh identities and references. Copies are independently editable; edits are
not shared between intervals. Above siblings remain at their live clock and
composition canvas masks stay outside the held stack. No effect-enabled or
opacity animation, scripts or per-frame geometry are generated.

Only closed visual scopes are admitted. Physical Video/Audio and unknown layer
kinds in the held stack, crossing parent/matte/mask/text-path/effect/graph
references, scripts or dependent graph entries, masks/styles on the Adjustment,
other enabled effects, opacity animation, remap, partial spans, non-Hold rates,
invalid controls, direct sibling matte helpers allocated after native occurrence
ownership, and failed depth/identity/animation/shape allowances retain the
original siblings and an explicit omission. Above physical media remain allowed.
Failed reconversion restores the importer contexts and accounting atomically.

The native control excerpt and focused CPU tests establish editable ownership,
ceiling boundaries and rejection behavior. The private source-backed Cosmic
check verifies two 954-layer scopes and 202 owned animation entries per branch,
including layer, effect and FX-item target namespaces; the earlier 74 count
omitted 128 FX-item entries. Original entries remain unchanged. This is structural
and clock evidence, not independent Adobe pixel fidelity or native export
reconstruction proof.

## Bounded self-inverse mask stage — import approximation

An enabled opaque solid with an identity 2D occurrence Transform, unit source
clock and one supported editable Add mask can use a bounded self-inverse
Set Matte profile. The native provider must reference the same source layer at
selector `-2`, with static inversion enabled and the pinned sparse Alpha and
Composite With Original defaults. Conflicting full parameter tables, duplicate
or unknown controls, nondefault effect compositing options, expressions on
selection/mask controls, foreign providers, parent/matte clocks, unsupported
masks and failed depth/identity admission keep the original owner.

Two identity helper Groups separate the source mask from the shadow effects
and preserve the observed stage order:
original Add-masked source, two existing Drop Shadows, outer inverse copy of
the same editable path, then existing Glow and optional Inner Shadow style.
Separate source-mask and shadow Groups are necessary because FX places a
group's own masks after its leading decoration effects. Existing layer, guide,
mask and effect identities, source playback and animation tracks are retained
within the transaction; the inverse mask and two Groups get fresh IDs. The same
strict profile applies to ordinary occurrences and All Effects track-matte
provider samples; Source and Adjustment samples retain their original behavior. No
tracks, expressions, source media, FX schema or renderer behavior are added.

The exact native layer/property excerpt and companion complete native Alpha
parameter table back the regression. The disposable frame6318 prototype
removes the opaque lower-half fill, but bar width/color and background remain
different from Adobe. Selector `-2` interpreted as Masks-stage is a bounded
approximation rather than a general independently proved selector law. Existing
shadow/Glow/style approximations, omitted Linear Wipes and the generic Set Matte
omission diagnostics remain explicit. No full native fidelity or export
reconstruction is claimed.

## Bounded foreign mask-path inverse stage — import approximation

An exact one-hop `thisComp.layer(name).mask(name).maskPath` alias can be copied
independently into an opaque solid's editable mask guide when the uniquely named
same-composition provider matches the native Set Matte reference. Both masks
must be ordinary hard Add masks, with equal unit start clocks, identical solid
bounds/color, static identity 2D geometry and no parent, remap, matte or chained
path expression. The native provider path and all supported keys/easing are
imported directly, without sampling the provider's composite or evaluating
JavaScript. The destination's activation and source playback remain independent.

The admitted effect order is two Drop Shadows, inverted provider Set Matte at
selector `-2`, Glow, then static 180-degree Geometry2 rotation, followed by the
existing optional InnerShadow style. Separate identity Groups preserve mask,
shadow and inverse-mask order; a post-Glow Group rotates around the source center
before the outer style. Absent Geometry2 point overrides require unambiguous
complete same-project legacy declarations: kind-6 Point values and percentage
default slots must both describe 50/50. Center derives from the decoded source
bounds. Modern 0.5 declarations and explicit normalized point overrides are not
reinterpreted, and the generic point decoder is unchanged.

Ordinary and All Effects matte-provider occurrences use the same bounded path;
Source and Adjustment samples retain their existing behavior. Candidate guide,
Groups, identities, graph additions and animation accounting commit together.
Ambiguous/missing aliases, foreign-reference mismatch, unsupported controls or
defaults, private/chained paths, unequal clocks/bounds and budget/depth/identity
failures retain the original cached owner. The self-only profile and generic
Set Matte guards remain unchanged.

The native-derived fixture retains both layer envelopes and their companion
legacy Geometry2 declarations. Local disposable comparisons remove the false
gray lower-half fill and expose the full title, while title color, outlines,
bar thickness, unsupported Wipes/Split and typography remain different from
Adobe. Independent editable path copies are not linked to subsequent provider
edits. Selector `-2` and effect-buffer raster behavior remain explicitly
approximate; this does not claim general expression or native export support.


## Bounded caption pill source-opacity fade — import

The complete recognized caption geometry profile can consume the ordered four
static Slider controls, enabled Fade In+Out frame controller and final Solid
Composite Source Opacity expression. With base Source Opacity 100%, fade-out
zero and explicit background Opacity zero, the expression becomes one clamped
linear 0→100% ramp on the existing isolated paint Group. Source-local endpoints
are derived from inPoint plus the authored frame count divided by composition
FPS; six frames at the stored native 29.9700012207 rate round to 11600→11801 ms,
not an imposed 200 ms interval. The five existing geometry tracks, separate fill
50%/stroke 100%, text and all surrounding clocks remain intact.

Recognition requires the complete expression and exact controller/name binding,
unique static controls, an isolated static 100% Normal paint Group, unit stretch,
no masks/mattes/remap, and no rendering effects or styles outside this closed
profile. Fresh disabled empty style scaffolds are neutral. Empty local parameter
tables use the source-proven canonical sparse defaults; supplied local tables,
parameter labels, popup order and explicit blend controls must agree. This does
not validate same-project global EfdG declarations or generally implement Solid
Composite, arbitrary expressions or the Fade controller as a second opacity
operation. Rejection preserves diagnosed geometry; mandatory opacity-track
failure rolls back the complete caption transaction and accounting. Only a
committed fade stops the generic [frame-fade owner lowering](#mattes-still-lifetimes-frame-fades-and-still-geometry2--bounded-import-correction)
on the same occurrence.

The existing licensed native-source regression covers all ten template
compositions, including nine selected Premiere captions. In-memory controls
exercise formula/binding/background/blend/style/mask/compositing and budget
rejection. Source-only matte samples retain the five geometry tracks without
the occurrence-effect fade; ordinary and All Effects samples include it.
The existing Premiere movie provides diagnostics; independent feature fidelity
remains unverified. Cached
font width, font fallback and blend-space differences remain documented limits.

## Uniform point-text glyph scale — import approximation

A finite positive uniform full-run point-text glyph scale can use the existing
inner Text transform, preserving the outer occurrence position/anchor/clock.
Box/path/mixed or affected animator geometry keeps the existing omission.
Tracking and raster stroke scale geometrically, so native per-glyph fidelity
remains approximate. Identity scales leave the original representation unchanged.


## Equal authored foreign masks and fixed-direction shadow distance — import follow-up

The bounded inverse-matte profile also admits a uniquely referenced sibling with
an equal independently authored hard Add mask: native path geometry, decoded
path times/easing and metadata must agree. Opaque native key-record bookkeeping
pointers are not interpreted as geometry. Equal solid bounds/color, source start
clocks, identity occurrence transforms and existing depth/identity guards remain.
This path retains the destination's authored editable guide and track; no alias
or graph is generated. Optional opposed Linear Wipes before a final static
180-degree legacy centered Geometry2 remain diagnosed omissions. The final
Transform sits after mask, shadows, inverse mask and Glow, before outer styles.
A self-provider may use the same guarded optional final Transform. The shared
legacy validator preserves complete same-project Point default declarations and
rejects modern/conflicting defaults or explicit point overrides.

`native_equivalent_foreign_mask_post_wipe_transform_preserves_stages_and_source_center`
uses exact native layer 949/provider 948 envelopes from source SHA
`6d632ac99c9e081746d51b83a651cb311dc063f0541bbe43ba1a241bd8870fdd`, composition 907,
and the same-source legacy Geometry2 declarations. It failed before lowering
(rotation0 instead of180) and now checks editable stage order, centered rotation,
source playback and both mask identities. Supplementary renamed-ID/name and
1280x720-source controls prove reference/bounds-driven admission. Path, order,
default, depth and allocation conflicts retain the original owner.

A static finite Drop Shadow direction makes animated native Distance an affine
projection into the existing editable `offset` Vector2 target. The same scalar
component drives both axes with sin(direction),−cos(direction); native source
clock, key values and temporal easing remain intact. Animated Direction,
expressions, malformed/nonfinite values or unsupported tracks keep the initial
offset with diagnostics. `native_fixed_direction_shadow_distance_becomes_editable_vector2_keys`
failed with no Distance track before the fix, then pins both native three-key
shadow offsets. The generic45-degree control checks both axis values and easing;
adversarial controls reject nonlinear/coupled cases. Shadow Color animation,
shadow/Glow raster appearance, omitted Wipes, native selector−2 alpha semantics
and export reconstruction remain unverified or unsupported. These are structural
import regressions; independent Adobe visual/alpha fidelity is not established.


## Static Fast Box Blur and shared color easing — import approximations

Static `ADBE Box Blur2` Both-dimensions controls are approximated with the
existing editable GaussianBlur, retaining source effect order, enable state,
Repeat Edge Pixels and source-plane dimensions. Sequential integral box radii
have variance `iterations * radius * (radius + 1) / 3`; the existing blurriness
convention uses four sigma. This is a source-derived variance approximation,
not a calibrated native kernel. Fractional radii, one-axis dimensions, animated
controls, expressions, conflicting native declarations and nonempty compositing
options retain diagnosed omissions. The exact four native layer envelopes pin
radii 50/11/60/122 and two iterations; renamed source and invalid-control tests
cover generic admission. Native edge alpha and renderer kernel fidelity remain
unverified.

Native color keys use one shared temporal speed in RGB-vector units 0..255.
For unchanged alpha, the import uses positive RGB Euclidean segment distance
in those units to compute a common progress ease, independently of component
sign or target scaling. The exact native Tint leaf regression failed with
`y1=-61.834213830208924`, then recovers `.14/.86` incoming/outgoing progress
handles in both clock directions. Nonuniform RGB controls and changed-alpha
rejection cover generic behavior. Alpha-varying Bezier speed normalization is
not established and keeps a diagnosed static target; Linear/Hold keys need no
speed normalization. This does not change the engine or native export easing.

The uniform point-text scale approximation admits finite vertical-only
Position3D animator keys when vertical glyph scale is unchanged and all spatial
tangents are zero, plus scalar Opacity/Stroke Width animation. The exact native
Text layer envelopes preserve0.91 horizontal scale, position/stroke keys and
font family; renamed text and changed-scale controls confirm control-based
admission. Horizontal/depth motion, unknown geometry controls and expressions
remain guarded omissions. Raster stroke and tracking scaling stay approximate.

For the bounded centered 180-degree post-effect inverse-matte Transform,
existing screen-space Drop Shadow offsets and their editable Vector2 keys are
negated as part of source-transform lowering. Key identities, clocks, easing,
graph edges and exact serialized animation-budget accounting are retained.
The native regression failed with offset `[0,-2]` instead of `[0,2]` before
compensation. Shadow softness/kernel units and other post-effect transforms
are not inferred from this half-turn mapping.


## Black composite / Lightness alpha / black unmatte — import approximation

A bounded consecutive Solid Composite (opaque black, 100% source/solid,Normal),
Shift Channels (alpha from Lightness,own RGB), Remove Color Matting (black,
Clipping off) pipeline becomes editable source/prefix effects above an opaque
black provider, and a white source-sized Rect gated by that provider's Luma.
Prefix Tint remains on the source before the black backing. Later effects,
styles, occurrence transform/opacity and source playback remain outside or
unchanged. Effect ordinal metadata preserves the split when preceding effects
expand to multiple editable records. Admission now requires grayscale Rect or
ordinary Text paint, or a full-strength grayscale Tint with equal native channel
tracks; arbitrary media, colored paint and unknown RGB effects are declined.
Grayscale paint does not prove intrinsic color-font rasterization. Native
Lightness and black-unmatting edge alpha remain explicitly approximate. This
is a complete guarded pipeline, not general support for these three effects.

Exact native layer 986/1027 regressions failed with no alpha gate, then retain
native text/content, moved Tint effect identity and667 ms key, and the complete
animation graph/budget from a rejected-Shift control. Conflicting source color,
channel, clipping, disabled/animated/expression stages, compositing controls, existing
matte,helper depth and identity exhaustion leave owner/allocator unchanged.
Sparse defaults are source-proven; supplied popup declarations use slot62
native defaults rather than slot56 current values. Owner masks/playback and
other source topologies remain outside this bounded profile.

Effect Drop Shadow Softness is approximated as Softness/2 in existing FX sigma
units. The shared affine field preserves keyed control units and inverse export.
An independent native Softness 4 edge measured 3.257 px 10–90 width, versus existing
FX sigma 4 at 8 px and sigma 2 at 2.985 px. The native edge center remains about 1 px
later, so this is a kernel-unit approximation rather than pixel equivalence.
A pinned native static regression failed 4 versus 2 before the mapping; independent
native scalar keys cover affine animation/inverse units. Inverse export restores source Softness with the shared factor of 2; this
preserves native control units without a new Adobe export parity claim.
World-space placement scaling is handled separately by the bounded linked
placement correction below.


## Bounded Fractal Noise — import approximations

A static uniform Basic/Spline Normal `ADBE Fractal Noise` stage on a proven opaque
solid plane with no native masks maps to the existing editable `TurbulentNoise`
effect. The converter validates the Fractal-specific ABI, including Evolution
0023, Opacity 0029 and Blend 0030, independently of Turbulent Noise. Native popup
defaults come from the declaration default slot, not its current cached value.
The pinned declaration has a cached Blend value 5 and a Normal default 2.
Source brightness, contrast, uniform scale, complexity, sub-influence, invert,
rotation, offset, opacity and static evolution are preserved as controls. The
renderer uses a different kernel, frame-relative feature scale, evolution and
HDR handling, so this is an appearance approximation with a diagnostic.

A separate Basic zero-contrast, zero-brightness Multiply stage on the same
proven opaque plane maps to editable Exposure. Its multiplier is
`1 - opacity/100 + (opacity/100)*0.5`; coordinates and evolution cannot alter
the constant in the related noise model. Adobe does not publish the exact
native contrast formula, so this inference is explicitly diagnosed as an
approximation. Only alpha-preserving Fractal/Toner/Tint/Exposure prefixes are
admitted. Other blending modes, animated controls,
nondefault cycle/seed/sub-transform settings and unsupported sources remain
omissions. Expression-driven coordinates are ignored only in the constant
zero-contrast branch, never evaluated or fitted.

Static Basic/Spline Multiply/Screen controls can instead form an independent
opaque generator over a known finite 2D square-pixel solid canvas with no native
masks. This separate admission does not certify prefix opacity or relax the
in-place Normal gate. Source-stage Groups retain prefix/suffix ownership,
opacity and occurrence visibility, including hidden/inactive content. Equal
nonuniform width/height is equivalent to uniform scale for this approximation;
unequal axes remain unsupported by this existing TurbulentNoise path. Prior/later unsupported Geometry2 stages keep
their omission diagnostics, so this does not establish spatial-effect parity.
Generator effects retain their source identity; disabling one leaves opaque
white Multiply or black Screen carrier paint. This is RGB-neutral over opaque
input, but changes transparent prefix alpha; bypass the enclosing generator
Group for a complete bypass. Source-disabled effects add no generator stage.
Shape-byte and identity reservations are
transactional. Native and generic tests cover stage order, disabled visibility,
shared native ordinals from Toner expansion and rejected-output rollback.

The exact native layer 910 and layer 887 envelopes plus the complete same-source
Fractal declaration table are pinned in
`tests/fixtures/effects/native-fractal-noise-controls.rifx` with byte/hash
provenance. Tests cover source-derived values and effect order, renamed source
content, popup default/cache disagreement, opacity-weighted constants, dynamic
controls, nonuniform scales, changed declarations, masks and alpha-changing
prefixes, and rejected-import identity/animation-budget preservation. This
checkpoint adds import approximations only; it establishes no native export
parity or pixel-equivalence claim.


## Transparent Vegas contour import approximation

An enabled self-input `APC Vegas` Image Contours stage with Intensity, Transparent
blend, Pre-Blur 0 and Render All Contours can use existing editable LumaKey,
SimpleChoker and Alpha mattes. A finite threshold transition between adjacent
integer code values retains source whites at threshold 255 instead of erasing
them with a degenerate cutoff. A shared source provider preserves original
editable content, prefix effects and animation identities. Two Alpha-matted
proxies provide centered dilation/erosion at `±width/2`; their inverse-alpha
gate produces a full outline in the native stroke color. Width 0 is explicitly
transparent; width above 20 px exceeds the existing morphology radius and is
declined. Suffix effects/styles and the occurrence transform stay outside.

Admission proves grayscale visible Rect/Group paint, skips structurally consumed
matte providers and permits only known grayscale-preserving effects. Arbitrary
media/shaders and intrinsic colored Text glyphs remain outside this profile;
white alpha-output gates can supply the title silhouette. Native segments,
animated rotation/sweep, tolerance, hardness and opacity gradients are omitted
with a diagnostic. Native unpremultiplied Intensity and edge alpha remain
approximate. The exact native envelope and declaration table are pinned in
`native-vegas-contour-controls.rifx`; tests cover source width/color/threshold,
shared-provider topology, unchanged source tracks and identity, zero width,
colored or animated paint, depth, ABI conflict, ID and byte-budget rollback.
This is an import approximation, with no new export or neon-animation claim.

The black-composite/Lightness-alpha profile also preserves combined text paints
inside matte providers by splitting them into editable fill-only and stroke-only
Text siblings in native paint order. Text, font and geometry remain unchanged;
animators, selectors, anchor options, font variations and their existing keys
receive independent copied IDs. Key bytes are admitted before construction;
late sibling failures roll back owner, ID cursor, graph and reservations.
Nondefault text opacity, dependencies, disabled/scripted tracks, local effects,
masks/path or wiggly selectors are declined.

## Linked placement pixel units and straight spatial easing

A positive static uniform, unrotated and unparented Premiere Motion placement
compensates existing raw screen-pixel DropShadow offset/blur/spread and signed
SimpleChoker radius, including independent native typed keys/constants. The
compensation visits only branches with identity, unanimated internal scale;
static internal rotation is allowed because uniform scaling commutes with the
already-imported post-effect half turn. DropShadow signs and all timing/ease
metadata remain intact. Radius beyond the existing `±10` Choker clamp is
declined. GaussianBlur is excluded because the existing renderer already
inherits world scale. Dynamic/nonuniform/rotated/parented placements and unsafe
script/dependency graphs retain diagnosed original pixel values.

Pinned native four-door controls prove all eight shadows and their offset keys
halve under source placement 50 percent while retaining both half-turn signs.
Generic signed morphology, unsafe placement, clamp and script controls exercise
the same rule. These are source-unit corrections, without a native-kernel or
complete exit-transition appearance claim. Straight two/three-dimensional
spatial segments with one positive shared speed and zero tangents similarly
use positive Euclidean-distance progress. The exact mirrored native Position3D
keys now share the same Bezier handles; per-axis and curved spatial paths keep
the previous mapping.

The mandatory Group playback API migration also repairs converter-owned
production/test adapters. Canonical Audio linear and affine source clocks retain
the historical export behavior under explicit playback windows. These are
compatibility repairs to existing contracts, without new export feature or
independent Adobe persistence/fidelity proof.


## Hard-edge Linear Wipe Solid / post-Anchor profile — import approximation

One or two enabled hard-edge Linear Wipes on an isolated finite 2D square-pixel
raster Solid become editable Shape half-plane masks in source coordinates. The
first mask adds and the second intersects, preserving sequential wipe order.
Completion uses the native scalar animation and immediate-parent clock; the
travel vector is `[sin(angle), cos(angle)]`, with 90 degrees traveling left to
right, 0 percent complete retaining the canvas and 100 percent removing it.
Projected finite-canvas extent supplies an analytical angled normalization;
Adobe edge antialiasing and exact angled completion normalization are unverified.
No pixel equivalence is claimed.

An optional final Geometry2 stage admits only a normalized static or bounded
axis-aligned Anchor curve with centered default Position and all other controls
at pinned native defaults. Anchor coordinates scale by the decoded source size
and move the already-masked source, before the original owner transform/styles.
Direct same-layer aliases must uniquely refer to a preceding Wipe's complete
native parameter suffix, with the supported opposed-angle `+180` grammar. They
become independent values/keys; arbitrary expressions retain their diagnostics.
Feather, angle animation, curved/diagonal Anchor motion, conflicting declarations,
unknown/duplicate controls, masks, mattes, collapse, 3D and mixed effects decline
atomically, including late animation/output allowance or identity failure.

The unchanged native Linear Wipe/Anchor fixture records source SHA, envelope
byte ranges and hashes in `native-linear-wipe-anchor-controls.provenance.json`.
The source regression failed with no mask stage before the fix. Six focused
tests retain native Completion/Anchor values and clocks, prove cardinal endpoint
polarity and alternate angled extents, resolve opposed aliases, preserve reversed
nonzero clocks, and check rollback. Renamed occurrences and alternate source
values supply generic controls. This is import-only structural proof, not new
Adobe playback, export parity or native angled-raster proof.


The existing Glow effect retains native threshold/radius/intensity controls,
but current FX raster compositing is premultiplied-over rather than the native
Add / Behind controls. Native threshold extraction, additive alpha and blur
kernel remain unverified. No Glow graph replacement was added in this scope;
source controls, existing approximation and omission diagnostics remain.
Mixed shadow / Set Matte / Glow / Wipe stacks do not enter the isolated Solid
Wipe profile, so their previously diagnosed Wipe omissions remain.
