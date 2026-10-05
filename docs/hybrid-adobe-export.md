# Editable Premiere / After Effects hybrid conversion

## Implementation status

**Ordinary Premiere CLI conversion is editable in both directions.**
`project.tsrct --to premiere -o output` automatically generates and packages needed
editable AEP scopes; `output/project.prproj --to tesseract -o imported` resolves
those links into editable FX through the ordinary Premiere import (next section).
No manual selection or hybrid flag is required.
Standalone format-library behavior remains available. This is not an Adobe
fidelity claim.

Export uses one retained Premiere conversion, its typed loss locations and
source boundaries, the existing AE picture-only writer, native picture
replacement, and the existing package publisher. It does not maintain a second
capability inventory, source-wire serializer, encoded-control observer or media
read-monitoring layer. Ordinary converter diagnostics remain visible; absence of
diagnostics is not proof that every source semantic was preserved. If AE omits
an entire requested picture scope, a typed omission result keeps the existing
native conversion with both formats' warnings. When a typed omitted source
layer belongs to a root that had native picture, the two results are compared
on the picture they lose in that scope: the painting leaf layers (shown layers
without children, sound excluded) under every subtree the native export omits
plus the owner of each native partial picture loss, against the leaves under
the layers After Effects omits. The linked scope replaces the native picture
when it loses fewer, or as many while losing nothing the native export kept,
reported as `HYBRID-LINKED-PREFERRED` with both counts and the AE omissions
under the scope's diagnostics; otherwise the native scope is kept with
`HYBRID-NATIVE-RETAINED`. The counts differ in kind: the native count holds
omitted leaves plus the owners of partial picture losses (a dropped effect or
key on a kept layer counts its layer once), while the AE count holds omitted
leaves only, because AE reports its approximations and dropped controls as
untyped warnings. Neither count weighs area, duration or pixels, so the rule
is a layer count, not a fidelity measure, and roots with no native picture can
still gain best-effort AE content. No empty AEP replaces native
picture; malformed-input and I/O failures still abort publication.

### Genuinely empty source roots

A source document with zero authored layers can export its original root as a
zero-layer editable AEP, linked for its full duration in Premiere. The link
represents the source composition, not a fabricated matte, footage or placeholder
layer. This route requires an exact supported linked rate and an unchanged
representable duration, no authored animation, unknown document/composition
fields (including retained dimension fields) or painted document background.
Both the archive source and supplied view must satisfy these limits.
Nonempty sources whose layers were
omitted still fail the existing no-content guard; native-only empty Premiere
publication and import admission are unchanged.

The existing independently authored `ae26_one_comp.aep` and Adobe-opened/resaved
empty writer controls establish bounded AE26 empty-composition structure and
nonzero duration. The existing generated-root GUID and Dynamic Link wrapper
contracts are reused. `empty_root_` CPU tests assert the emitted zero-layer AEP,
source name/canvas/duration, full-length linked source and timeline ranges,
Check/Write publication, rejected clocks/background and nonempty omitted content.
These are structural tests, not new Adobe acceptance, empty-root link rendering,
alpha/background equivalence or edit-propagation proof. No new native render
or source oracle accompanies this integration.

Selected AE scopes can [prepare unsupported whole video sources](media-preparation.md#hybrid-export-destination-preparation)
without changing editable clocks or weakening complete-scope fallback. Native
Dynamic Link `ModificationState` is encoded as the 16 bytes of its fresh UUID;
`ImporterPrefs` retains the UTF-16 composition GUID. A Premiere-authored record
pins these distinct encodings. The generated package was observed online and
rendering in interactive Premiere Pro 2026. Full relocation, edit propagation,
RGB/alpha/audio comparisons remain unverified; managed offline reports do not
by themselves establish an exporter defect.

Import is the ordinary Premiere import, which resolves each linked composition
through `aftereffects_file` (`ResolvedAfterEffectsComposition::import_picture`):
it prepares each actual AEP file once and resolves its exact composition GUID.
Each occurrence gets separately allocated editable layer/effect/item IDs at
conversion time, and footage is packaged under one namespace per resolved AEP.
`Premiere::import_with_linked_compositions` with `import_editable_picture` remains
a caller-supplied route whose content takes the same placement. No intermediate
Tesseract serialization/reparse, generic JSON ID remapping, image flattening or
hidden original-AEP replay is used.

## Linked scope rates

Export admits exact 24, 25, 30, 50 and 60 fps for linked AEP scopes. These rates
are exact in AE's 16.16 rate field and Premiere's frame clock. Sequence cadence,
source footage rate, key times and duration remain separate; selecting a rate
does not reinterpret footage or resample editable keys.

The fractional 24000/1001, 30000/1001 and 60000/1001 sequence rates are not exact
AE 16.16 rates. Export retains the native Premiere result and its limitations,
with `HYBRID-NATIVE-RETAINED`, rather than aborting or changing cadence.

The native `hybrid/rates/native.aep` fixture contains independently saved and
reopened 50/60 fps compositions, matching frame-numbered footage, a trimmed
picture and Hold opacity keys. `native_high_rate_scopes_keep_composition_media_and_key_clocks`
checks fresh import and linked export at both rates. Native AE save/reopen and
30 fps renders retain the exported rates, durations and selected frame numbers.
RGB output matches the clean-base direct-AEP control, but not the strict
independent-reference limits. See the [support ledger](after-effects-support.md#exact-5060-fps-linked-scopes)
for this visual residual and the unverified package relocation, Premiere
Dynamic Link, alpha and audible sound limits.

## Lumetri-derived edited controls — bounded linked export

Existing Exposure, saturation-only HueSaturation animation and TemperatureTint
exporters consume current FX edits through admitted linked AEP scopes, including
an identity picture Group host. They are not direct Premiere effect writers.
Exact 24/25/30/50/60 fps scope admission does not cover fractional sequence rates
or AE-scope rejection/omission. Native-only retention with diagnosed omissions
of these controls (and related Vignette) is established for an ordinary clip
host. The supplementary identity Group's earlier fractional native-only export
was rejected by the former final gap check rather than publishing missing
picture. Current export admits uncovered gaps, but Lumetri-specific Group
fractional/AE-fallback behaviour remains unverified. Contrast retains its
existing native mapping.
The [control ledger](../apps/tesseract-conv/README.md) records the selected
replacements, clipping/colour-space and feather approximations.

`human_lumetri_nested_edited_linked_export_keeps_controls_defaults_and_keys`
checks one Group owner, ordered native controls, neutral Master/channel gamma
defaults, current Contrast and independently keyed controls. Flat source-derived
tests own exact scalar projections. The fractional/fallback regression asserts
visible omission diagnostics rather than calling those domains supported.
These are supplementary generated-native structure checks, not independently
Adobe-authored nested placement, reopening, RGB or alpha proof. Import is unchanged.

## Pixel Motion Blur through linked pictures

Pixel Motion Blur already uses the ordinary linked-AEP export route: Premiere's
unsupported-effect loss selects its picture owner, and the AE writer emits editable
`ADBE OFMotionBlur` controls from the current FX payload. It does not write a
Premiere-native Pixel Motion Blur filter or replay an original AEP. Linked import
uses the existing AE mapping. Manual Shutter Angle, Shutter Samples and Vector
Detail remain editable; this is not equivalence of the two optical-flow algorithms.

`native_pixel_motion_blur_import_and_edited_linked_export_keep_controls` selects
composition 391 from the pinned Adobe-native `native_static_controls.aep` fixture
(SHA-256 `7c65ebe724fc8403399979cc1354adca3abd737b6ad7a76ba8eafc5b61ccf95b`).
It asserts the imported manual 120°/8-sample/20-detail controls, exports both those
values and explicit 240°/16-sample/40-detail edits through Premiere Check/Write,
and reimports the linked package to assert them. This is source-backed import
and supplementary structural export coverage of the existing route, not new
Adobe acceptance. Automatic shutter mode, animated controls, temporal sampling,
owner/clock fidelity, alpha and independent native-render comparison remain
unverified by this test. See the [AE limitations](after-effects-support.md).

## Linked-composition import increment — picture only (user-approved import scope)

**Import:** ordinary `.prproj` → `.tsrct` conversion now imports each video clip
of a linked composition as editable FX. The AEP resolves as a package file,
is read once per resolved path, is bound to its hashed bytes, and its composition
is selected by the native GUID in that exact file; names never select. The clip
follows the existing video-clip rules (Motion/Opacity and keys, masks, stage
groups, effects, matte, Enable, speed/Time Remapping). Its picture is a group
on the same clip clock as a video layer, so the clip's own Motion, Opacity and
effect keys keep their clip times. When the clip shows the composition from a
source In, at another speed or time-remapped, a `Premiere linked source` group
under it maps the clip clock to the composition clock; its children are the
composition's editable root Group from the ordinary AE importer and a
`Linked composition canvas` guide, whose Add mask on that root clips its
content to the composition's canvas, as After Effects renders a composition
(the clip's Motion, masks and effects apply to the clipped picture above). Where the
clip group's parent clock is the document clock, the group also carries an
identity-rate `playback` (its start to 0, its end to its duration), which the
render walk reads as its plain start offset. It is needed because the FX
runtime evaluates keyframes under time remapping on a chain that starts at the
first Group with `playback`, from the document clock, without the start offsets
of the Groups above it; its render walk does apply them. Without that seed, the
composition's remapped animation ran early by the clip start in a root
evaluator probe. Under a stage group or nest that starts after the document
start no seed can help: the clip group has no `playback`, and a linked clip
whose content is time-remapped (its source group or the composition's own
source clocks) is **reported as an approximation**: its After Effects
animation under time remapping runs early by that start, while placement,
visibility and media times are kept. A linked matte moved under such a stage
is reported the same way. Native keys are
on the source clock, which that clip clock matches only at unit forward speed:
on a retimed linked clip, effect keys are omitted with a reason and their static
values kept, as the existing rule already does for Motion and Opacity keys.
Every placement, including repeated and nested ones, imports its own copy with
fresh layer/item/effect/keyframe identities. Composition footage is packaged per
resolved AEP (`premiere-aep-<n>-item-<id>`), so equal item IDs of different AEPs
never share assets. A repeated picture keeps only the normalized temporary file
that backs its packaged asset, until archive writing. The AEP joins the existing
pre-publication media hash checks in Check and Write, and so does every local
file that the pictures were read from: footage bytes, PSD/AI sources behind
normalized or lowered media, and a relinked source's authored path, which must
stay missing. Packaged footage must also have the bytes that conversion read.
These are re-read checks, not a filesystem lock.

Premiere plays a link's sound only through audio track items, so the imported
picture is muted. Audio items import separate editable sound groups with their
own placement, trim and gain. The two projections share packaged source assets,
not placement identities. Sound-only groups export as ordinary editable native
audio clips; the diagnostic states that grouping and the original Dynamic Link
relationship are not retained. No black video nest or second sound copy is added.
Static gain composes with AE gain; simultaneous gain curves use editable sampled
Linear/Hold keys with a curvature diagnostic. Non-affine gain clocks retain AE
automation with the item's static gain. Export approximates nonunit/non-affine
sound playback at normal speed from the selected start/first source key.
Missing AEPs,
producers without identity evidence (see below), absent GUID targets and canvas
mismatches omit the affected clips with reasons; a composition shorter than the
source range that a clip shows, or one that forms no editable picture, omits that
clip. Siblings convert; a changed, unreadable or unpackageable file stops the
conversion. A composition's enabled motion blur sets the document's one shutter,
and a clip that requests other settings is approximated. Frame blending
on a linked clip is reported and dropped. Moved AE projects now find footage at
AE's native alias relative location when its absolute path is missing (shared
AE import behavior; see the AE support ledger).

The accepted Dynamic Link header profiles are AE26.5x89 (public H-IDENTITY-01)
and AE26.3x87, whose GUID ↔ item evidence is a private customer package retained
outside Git (Premiere 26.3.0 ImporterPrefs GUIDs, an independently produced AE
validation receipt and the AEP's own items agree on three compositions). The
public `ae26_one_comp.aep` test only shows that this producer's unchanged native
header is accepted; it is supplementary, not GUID identity proof. A third exact
pair, format96/subtype6 `[0,96,0,6]` / producer `0x0f8a0656`, is admitted from
Co-Editor's independently saved Premiere GUIDs and native AEP items:
[reduced native-derived fixture provenance](../crates/aftereffects_file/tests/fixtures/hybrid/format96/provenance.json).
Five occurrences select IDs2165,41,1,92,121 by file/GUID, not display names.
The AE selection regression and public Premiere Check/Write regression
`native_format96_links_keep_all_five_editable_pictures_with_missing_footage`
retain editable Text/Rect siblings when genuinely missing linked footage is
omitted by the separate generic policy correction. The reduced fixture keeps
opening video53/layer54 and its native video metadata to exercise host admission.
Missing content remains diagnosed, never flattened or replaced with colour bars;
invalid present linked media remains fatal. The fixture omits unrelated/native
media content and is not Adobe-opening or
render-fidelity proof. No general AE version acceptance is claimed. Export is
unchanged by this import-only profile admission.

**Caller-supplied content:** `Premiere::import_with_linked_compositions` takes
each placement's content from a `LinkedCompositionResolver` instead (for example
`ResolvedAfterEffectsComposition::import_editable_picture`, the same AE
conversion as the built-in import). That content takes the same placement, clock,
canvas, blend, stage and omission rules; an After Effects profile, identity or
content failure that the built-in import would omit omits that clip, and any
other importer failure stops the import. Return the source import's typed notes
in `LinkedComposition.diagnostics`; the conversion report preserves their
composition/layer context, including Time Remap expression omissions and cycle
approximations. Callers with no source notes supply an empty vector. This does
not add expression or loop support. The AEP is rechecked with the other
media, but the lifetime and freshness of the caller's assets are the caller's:
the linked-footage checks above apply to the built-in import only.

**Export:** hybrid AEP scopes remain picture-only. Sound is exported through
native audio clips, not a second audio-enabled AEP scope. This preserves edited
sound without duplicating it through the linked picture.

**Acceptance:** the existing native nest-audio fixture path verifies the shared
placement, trim, gain and single-sound-owner behavior. Linked-AE source tests
add independent placements, asset sharing and edited native audio export. A
human-authored audio-bearing Dynamic Link project is not an acceptance gate;
direct Dynamic Link native reopening and audible comparison remain unverified.

**Evidence:** CPU structural tests, plus one root runtime test. The native H-IDENTITY-01 package converts
with distinct same-name red/blue editable Rects at their native placements and
source offsets (`tests::linked_compositions::native_links_import_editable_same_name_compositions_by_exact_guid`).
Supplementary typed and derived cases cover repeated and nested placements,
equal item IDs of two AEPs, per-AEP footage assets, muted linked pictures with
independent audio items, unresolvable links, video-clip placement and key parity
(trimmed, retimed and nested), canvas clipping, a clip past its composition's
end, caller-supplied content (placement, blend, omission and failure rules, and a
document identical to the built-in import's for H-IDENTITY-01), and
pre-publication AEP, footage and PSD source drift (`tests::linked_compositions`);
the production CLI route converts H-IDENTITY-01 with these clocks and canvas
clips (`hybrid::tests::premiere_cli_import_places_native_linked_compositions_on_their_clip_clocks`),
plus the AE-side identity, muting,
motion-blur, relinking, media-freshness and normalized-file rules
(`adapter::linked_import`, `adapter::media`). The root evaluator test
`premiere_keyframes::a_linked_composition_keys_its_clip_on_the_clip_clock_and_plays_from_its_source_in`
(`crates/fx_composition`) evaluates derived Dynamic Links to a native AE
composition, trimmed and untrimmed at a nonzero start: the clip's Rotation,
Opacity and mask guide match the same clip as a video at its start, middle and
end, and the path keyed under the composition's own source clock matches the
standalone composition at the source time. Staged and nested linked clips are
covered only by their approximation diagnostic, not by a timing assertion. **No converted render, alpha, audio or Adobe comparison has
run**; native RGB/alpha/audio fidelity, edit propagation and relocation beyond
the relinking rule remain unmeasured.


## Feature × direction and limitations

| Feature | FX → Premiere/AEP | Premiere/AEP → FX | Evidence / limitation |
|---|---|---|---|
| Ordinary native picture/audio | Existing converter and diagnostics; no extra AEP for native-only input | Existing clip/track/audio importer | Native defaults remain unchanged. No new fidelity claim. |
| Editable linked picture | Actual lowerer losses select root picture scopes; scopes use `media/ae-NNNN/compositions.aep` | Fresh editable conversion of each file-qualified GUID | Picture scopes must be contiguous among picture roots and independently replaceable. Intervening standalone Audio is allowed: its zero-picture slot is replayed and AE audio switches stay disabled. Unrelated interleaved pictures still fail explicitly. |
| Dependencies and ordering | Typed parents, mattes, mask guides, shader inputs and animation references close the source scope; backdrop-dependent roots and descendant blends inside Groups conservatively include their backdrop | Existing native Motion/Opacity/effects/matte staging wraps the imported AE content | Converter-specific unsupported effects and hierarchy approximations remain diagnosed. Arbitrary nested-container selection is not implemented; selection promotes a loss to its root scope. |
| Clocks | AEP scopes support exact 24/25/30/50/60 fps; fractional sequence rates retain native output with diagnostics; existing packing checks exact native source/timeline ranges | The clip group keeps the native clip clock and keys (with an identity-rate seed on the document clock); a `Premiere linked source` group carries the native trim/speed/remap; AE children keep their source clocks | A clip showing past its composition's end, or on another canvas, is omitted with a diagnostic; conflicting motion-blur settings are approximated. Time-remapped AE animation under a later-starting stage/nest runs early (approximated). Existing millisecond FX timing limitations remain. |
| Audio | AEP native audio switches are disabled; independent/embedded native sound stays in Premiere | AEP picture expansion is muted; native audio imports independently | Linked AEP audio itself is unsupported and diagnosed. Replacing a nested native-audio owner remains rejected by packing. CPU structure is not an audible comparison. |
| Identity and assets | Actual generated root identity and portable AEP-local paths | One parse per AEP file; disjoint generated IDs per occurrence; footage under one namespace per resolved AEP (a caller-supplied importer names its own); picture clipped to its composition canvas | GUID support is restricted to the independently observed AE26.5x89 macOS and AE26.3x87 profiles. Unknown profiles and absent targets omit the clip with a diagnostic; cycles and expansion limits are diagnosed within the picture. |
| Check/Write and publication | Same conversion and package assembly; Check does not publish | Same import and archive validation; Check does not publish; the AEP, linked footage and PSD/AI sources are rechecked before Check returns and before publication (caller-supplied assets excepted) | Fresh output only, source/path/SHA checks and owned rollback. Recoverable rollback is not crash atomicity or filesystem locking. |

AE import/export capabilities and every format-level approximation remain in the
[AE support ledger](after-effects-support.md). Premiere retains its existing
source-profile, canvas, codec, frame-rate, retiming and audio restrictions. This
work does not modify FX schema, evaluation, editor or renderer semantics.

## Large-project export follow-up

Native lowering no longer constructs an invalid nest when a picture-only group's
range extends past its exported children. A neutral Normal group without its own
effects, masks, matte, Opacity changes or Motion changes now retains its supported
children: the native nest ends at the actual inner picture extent, and the
remaining group interval is transparent. Child source ranges, local key times
and sequence duration remain unchanged. The authored FX group range is retained
for AE; a typed Placement approximation still selects the existing root-scope
fallback. Other longer groups are omitted with a reason. Groups containing
retained nested sound still fail explicitly instead of losing that sound.
Child animation and motion-blur evidence is committed only when its containing
nest survives; omitted child scripts are not counted as written. Native
input/writer duration validation is unchanged.
The shorter nest does not independently extend the top-level timeline through
the authored tail.

Native-only and hybrid export now accept uncovered timeline gaps, including
partial overlays and disabled picture replacements, without requiring an explicit
black canvas. No warning, automatic background or source rewrite replaces the
removed coverage gate. Top-level gaps use Premiere's native black background;
nested gaps can remain transparent over lower content. Timing, sequence duration,
layer order, writer validation and staging/publication checks remain unchanged.
This is an admission-policy change, not source/destination pixel or alpha equality.
Compositing backdrop closure excludes standalone Audio roots, which remain native audio; explicit
property/layer dependencies on audio are not silently ignored. Audio may occur
inside a selected root interval without changing picture selection. The existing
picture-only AE writer may also stage that audio asset, but its AE audio switches
are disabled; Premiere retains the original native sound occurrence.

The routing collector and owner lookup retain inputs beyond their former 1,024
entries. Packing source boundaries use the actual `u32` token range rather than
1,024 slots. Human diagnostics remain bounded. This does not remove the other
packing, hierarchy, dependency or format limits, or establish bounded memory.

Supplementary regression symbols:
- `a_plain_group_longer_than_its_children_keeps_their_clock_and_transparent_tail`
- `an_extended_plain_group_from_the_native_fixture_keeps_its_child`
- `a_long_group_with_own_compositing_or_animation_is_still_omitted`
- `hybrid_picture_replacement_exports_without_a_black_canvas`
- `uncovered_leading_internal_and_trailing_gaps_export_without_changing_content`
- `hybrid_picture_backdrop_does_not_absorb_standalone_audio`
- `hybrid_owner_lookup_keeps_layers_beyond_1024`
- `more_than_1024_source_boundaries_remain_replaceable`
- `routing_events_are_not_truncated_at_the_former_count_limit`
- `a_long_plain_group_keeps_child_script_keys_but_an_omitted_effect_group_does_not`
- `hybrid_long_plain_group_retains_native_footage_when_ae_omits_it`
- `hybrid_long_plain_group_still_rejects_nested_sound`
- `hybrid_long_plain_group_exports_an_uncovered_tail_without_a_canvas`

The neutral-group retention regression failed before the fix: the shortened
child's whole nest disappeared. The unchanged pinned native source
`feature_nested_sequence_strict.prproj` (SHA-256
`cad9e0090d74574bf2aaf6ba069a61e8e1a97548b998d75c33d5a2c085fec3ed`,
Outer `dab91e14-ca76-47e7-93fc-99bf6bcc94be`) is freshly parsed and imported;
the export test explicitly edits its first group's FX window to 1–5 seconds.
Its child stays at inner 0–3 seconds, source 1–4 seconds, and the emitted
native nest spans 1–4 seconds. This derived edit failed on the old lowering
and passes with retention. Additional CPU cases preserve child script keys,
typed loss routing, CLI Check/Write parity and byte-identical MP4 media when
AE rejects that MP4. Nine own-control/compositing exclusions and nested sound
retain their rejection or omission; uncovered tail gaps are now accepted. These are editable
structure and packaging checks; import implementation is unchanged and no new
Adobe acceptance, alpha, audio or rendered-fidelity proof is claimed.

The long-group, gap-ordering and audio-backdrop cases failed before their fixes.
A fresh local 15-second, 1920×1080, 30fps export containing 1,657 layers now
publishes a Premiere project, two editable AEP scopes and unchanged WAV media.
This is conversion execution, **not a fidelity pass**: native/AE diagnostics
include omitted custom WGSL effects and approximated script curves. No source
archive or output video is committed, no independent oracle is replaced, and
this export-only repair makes no new import-support claim.

Adobe Media Encoder 2026 accepted the generated **Premiere project** (not a
standalone AEP submission) and encoded its linked AEP picture into a 15-second,
1920×1080, 30fps MP4: 450 video frames plus 48kHz stereo AAC. Fifteen decoded
samples at one-second cadence showed scene content rather than offline slates
or all-black frames. This establishes bounded AME/Dynamic Link execution, not
independent visual equality, alpha correctness or audible audio fidelity.
Premiere's editing UI showed `no sequences`; usable Project-panel/timeline
opening remains **unverified**, and is not claimed from AME success. Source-side
Tesseract comparison and independent Adobe-native feature proof are incomplete.
The 67 scoped CPU cases, workspace all-target check and formatting, Premiere
all-target clippy and CLI all-target clippy (`--no-deps`, warnings denied) passed.
The AME probe preceded the final written-animation accounting correction; it is
not exact-final-commit Adobe acceptance evidence.

## Authored black Shape canvas — export gap validation (historical)

The following observations describe the former coverage guard and its bounded
Shape recognition. The coverage guard and its guard-only Shape analysis have
since been removed. Authored Shapes still export as editable graphics; gaps no
longer need opacity proof. The recorded conversions and failed native outcomes
below remain historical evidence, not newly passing native results.

A private 77.867-second launch source already has an opaque 1920×1080 black
Shape spanning 0–77866ms, followed by a standalone Audio layer. Final native
export nevertheless rejected it as an uncovered sequence: ordinary graphics
are not assumed opaque, and only a special legacy Rect supplied explicit canvas
coverage. Adding a background or treating every graphic as opaque would conceal
the bug or weaken the guard.

That implementation proved coverage from a **retained final native graphic**: one
enabled, static, identity-transformed, closed four-corner full-frame black Shape,
with opaque Normal blending, no stroke/shadow, no clip/object animation or Vector
Motion, and no transition on its track. Its interval must cover the entire gap.
Nonmatching graphics retain the previous rejection. The original editable Shape
and sound remain in the project; no layer is inserted, stripped or retimed.
Coverage is checked after picture replacement, so removing the graphic removes
its coverage too. The shared timeline gap scanner and all import behavior are
unchanged. This bounded recognition is not a general opacity/geometry analyzer.

Supplementary CPU regressions:
- `authored_black_shape_with_audio_remains_editable_and_covers_the_sequence`
  failed with the original gap error before the fix, then passed. It checks both
  audio/root orders, unchanged source bytes, the retained editable Shape's black
  fill and full-frame coordinates, 2336-frame endpoint, and unchanged sound
  placement/source range through generated-project readback.
- `only_proven_static_full_frame_black_graphics_cover_gaps` rejects 23 altered
  native-output cases, including opacity/animation/blend/transition, geometry,
  transforms, disabled picture, wrong canvas and incomplete temporal coverage.
- `removed_authored_black_shape_cannot_cover_a_disabled_replacement` ensures
  a removed source Shape cannot certify a disabled replacement; no final output
  is published on the error.
- At that checkpoint, `final_native_and_incomplete_overlay_still_reject_uncovered_gaps`
  passed with the final gap gate intact. It is now replaced by a positive uncovered-gap
  export regression.

A local reduced archive containing only the original black Shape and exact
original Audio bytes reproduced the error in 0.50s. The fixed optimized release
converted that same archive to `.prproj` in 1.20s (26.0MB maximum RSS), with its
source hash unchanged. Existing unknown-field/motion-blur warnings remain; this
is best-effort conversion, not a new fidelity claim. The regression uses a short
public audio fixture instead of the private mix. No private archive/audio is
committed and no native reference or threshold is changed.

**Export implementation:** the full 216MB launch conversion was rerun with the
fixed signed release (`93ec61408` source). It published `project.prproj` in
930.70s wall / 863.11s CPU, with the original source hash unchanged. Maximum RSS
was 34.72GB and peak footprint 71.72GB: memory remains unresolved. Crucially,
there were **14,021 warnings and no linked AEPs** in the final package. AE scopes
were rejected (including unknown text/font bounds and unsupported image native
profiles), so the coordinator retained its best-effort native Premiere result.
This proves the gap error is fixed and publication completes, **not** complete
content preservation. The preceding 973s/69.48GB run failed at the gap guard.
**Import:** no change and no new proof claim. Adobe opening/control inspection, independent
native-render RGB/alpha/audio comparisons are **unrun/unmeasured**. Source-pattern
regressions and our own writer/readback are supplementary evidence, not a pinned
Adobe-native feature oracle or an end-to-end fidelity pass.

## Critical-review corrections: content retention and backdrop boundaries

Three coordinator regressions are fixed with supplementary CPU evidence:

- A Screen/Glow root could pull a supported native MP4 into an AEP that omitted
  that MP4, then delete the native footage on partial AE success. Typed omitted
  layer IDs now prevent that replacement. The strengthened
  `hybrid_picture_backdrop_does_not_absorb_standalone_audio` asserts both video
  and WAV bytes survive, checks `--check`/write parity, and verifies one native
  audio placement with the Audio root at the beginning, middle and end.
- Descendant blends in Normal Groups now conservatively close over lower
  pictures. `hybrid_group_child_blend_includes_the_editable_backdrop_in_ae`
  verifies one linked picture and reimports the editable background from it;
  six dependency tests cover nested Groups, dynamic isolation and Audio.
- Standalone Audio inside a picture interval no longer rejects conversion.
  `hybrid_complete_picture_scope_can_cross_standalone_audio` also checks the
  successful-AEP path, with one native audio placement. Existing AE staging
  tests verify disabled root/nested AE audio switches; audible parity is unmeasured.

The missing-MP4, middle-Audio and descendant-closure assertions failed before
these corrections. Final targeted validation passed **66 CPU cases**: 25 CLI
hybrid, 12 packing, 16 prepared Premiere, 11 AE staging and two AE omission/guide
regressions. Workspace all-target check and formatting, and AE/Premiere/CLI
all-target clippy (`--no-deps`, warnings denied), passed. No full suite ran.

All four local minimal CLI probes now export. The MP4-backed cases retain native
footage instead of selecting an AEP that loses it; their unsupported effects
remain diagnosed, not newly supported. A fresh export of the unchanged
SpamShowreel source still produced a 15-second, 1920×1080/30fps project, two AEPs
and the byte-identical WAV. No Adobe operation was rerun after these corrections.
These are source-retention, dependency and editable-structure checks, **not**
independent Adobe-native fixture proof or measured render/alpha/audio fidelity.
Import implementation is unchanged.

## Native forward-ramp picture fallback

Native preparation now retains an unmasked, bounded strictly forward linear
TimeRemap ramp as an explicitly reported constant-speed approximation, using
the original curve's selected source endpoints after its signed input offset.
The typed TimeRemap loss still selects the **original** document for AE. An
admitted AE scope replaces the approximation with editable original ramp keys;
AE rejection/omission keeps the native picture and its reported feature losses.
Source bytes, duration/media admission and standalone sound are retained.
Motion/Opacity/effect keys on the approximation are omitted with static values
kept even at average speed 1; embedded retimed sound remains omitted.

Mask/track-matte owners and sources, nonlinear easing, holds, reversing ramps,
extrapolation and fractional-millisecond selected endpoints remain outside this
fallback. Enabled custom shaders, source matte/key/choker effects and unknown
effect payloads on the video also exclude newly approximated ramps, since omitting them can
expose foreground RGB. Disabled effects and exact two-key playback retain the
existing rules. In particular, Worlds RGB-plus-alpha dependency scopes are **not**
recovered by this native approximation, and no alpha codec admission is relaxed.
Import implementation is unchanged. The new CPU lowerer/CLI regressions and
isolated shader-free Worlds B layer 40010 packaging are supplementary structural
evidence. Original Worlds A layer 40010 carries a custom shader and remains
outside this fallback. Independent Adobe acceptance, render fidelity, alpha, sound and edit
propagation have not been measured for this approximation.

## CPU regression evidence

Earlier CLI activation checkpoint: **12/12 bounded CPU cases passed** together
in the final run. Workspace formatting, Premiere/CLI all-target clippy with
warnings denied (`--no-deps`), dependency portability and task-owned public
inventory checks passed. AE all-target clippy still reports its existing warning
baseline; comparison with the saved pre-change baseline found no added or removed
warning signatures. The full suites were not run. Historical receipts for deleted
observer code are not counted.

The panel includes:

- `hybrid_check_write_and_reimport_preserve_native_sound_and_editable_picture`
- `hybrid_native_only_uses_the_ordinary_converter`
- `hybrid_middle_scopes_with_equal_guids_import_their_own_edited_files`
- `hybrid_unsupported_clock_and_interleaved_dependencies_leave_no_output`
- `hybrid_linked_source_in_and_repeated_occurrences_keep_clocks_and_unique_ids`
- `hybrid_linked_changed_canvas_is_an_explicit_error`
- `hybrid_native_compositions_use_caller_reserved_ids_without_reparsing`
- Existing package collision, source-drift, ownership/rollback and writer-SHA tests.

CLI activation adds **6 process-level CPU cases**, using the non-test binary:
`premiere_hybrid_cli_creates_linked_aep_and_reimports_editable_content`,
`premiere_hybrid_cli_rejects_unsupported_clock_without_publication`, and existing
native-route, FPS, omission and script-key export cases. The AEP-generation case
failed before routing was enabled (the normal command did not publish an AEP),
then passed. The omission case also reproduced a native-content regression before
adding the typed empty-scope fallback. These checks cover packaged file/GUID
resolution and editable reimport, not Adobe Dynamic Link execution.

Merging this export with the linked-picture import replaced three import
contracts with the reviewed placement rules (see the import increment above):
`hybrid_linked_source_in_and_repeated_occurrences_keep_clocks_and_unique_ids`
became `caller_supplied_compositions_take_the_linked_clip_placement` (the source
clock on the `Premiere linked source` group, the seed on the clip group, canvas
clip, blend and unique IDs); `hybrid_linked_changed_canvas_is_an_explicit_error`
became `a_caller_supplied_composition_that_cannot_be_placed_omits_only_its_clip`
(a canvas mismatch omits the clip and keeps its siblings; any other importer
failure still stops the import); and
`premiere_hybrid_cli_creates_linked_aep_and_reimports_editable_content` now
expects a missing linked AEP to omit its picture with a diagnostic while the
native sound converts, rather than fail the import.

The edit integration case rewrites one AEP through our converter, then performs a
fresh import. It tests that the changed file is consumed, not Adobe UI editing or
live Dynamic Link refresh. These CPU cases do not establish RGB, alpha or audio
fidelity and do not replace independently Adobe-authored feature tests.

## Independent Adobe evidence — unchanged and incomplete

The pinned [H-IDENTITY-01 source and references](../crates/aftereffects_file/tests/fixtures/hybrid/identity/README.md)
contain independently authored SAME-name compositions1/16, their actual Premiere
GUID payloads, and immutable long-term30fps reference assets with verified source
hashes. They discriminate red/blue native targets and establish the restricted
file/GUID interpretation. They do **not** prove this generated package's fidelity.

The earlier [nonempty Rect probe](../crates/aftereffects_file/tests/fixtures/hybrid/README.md)
established one generated root's native GUID and limited Adobe acceptance.
Separate Text output failed native opening. Historical reopen/name propagation
is not current-output acceptance or live refresh evidence.

The large-project follow-up above adds one generated-package AME execution,
not independent native-render comparison or verified Premiere editing-UI
acceptance. Alpha, audible audio fidelity, edit propagation and relocation remain
**unmeasured**. No reference upload or native oracle replacement was performed.
Missing independent proof remains a delivery limitation, not a passing feature test.

The implementation checkpoint also recorded an unrelated AE generated-Rect SHA
expectation mismatch. This is historical test status, not a new validation run;
no oracle or threshold was changed.

## Maintenance

Change the actual format converter and its behavioral regression tests when a
feature changes. Change the coordinator only for cross-format scope, placement,
clock or asset handling. Do not add parallel control inventories, proof receipts,
source-schema mirrors, serializer audits or custom I/O witnesses.


### Linked channel-matte current edits

The genuine Premiere-saved `premiere_channel_matte` fixture exercises linked AEP
Green Set Matte through the ordinary importer, current Blue channel edits and
matte bypass, fresh Premiere/AEP packaging and reimport after graph-label changes.
The original trimmed video bytes and independent video/audio clocks are asserted.
Set Matte is inside the linked source, not a newly supported Premiere-native clip
control. See the [mapping and limitation ledger](after-effects-support.md#premiere-linked-greenblue-channel-mattes)
for native identities, executable contracts and the distinction between offline
native-control readback and unmeasured Adobe/RGB/alpha evidence.
