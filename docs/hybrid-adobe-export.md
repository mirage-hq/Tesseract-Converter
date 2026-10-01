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
native conversion with both formats' warnings. Partial AEP success also keeps
the native scope when a typed omitted source layer belongs to a root that had
native picture. Effect-only approximations do not count as layer omission, and
roots with no native picture can still gain best-effort AE content. This guard
conservatively retains a whole root when one descendant is omitted; it is not a
per-property fidelity comparison. No empty AEP replaces native
picture; malformed-input and I/O failures still abort publication.

Import is the ordinary Premiere import, which resolves each linked composition
through `aftereffects_file` (`ResolvedAfterEffectsComposition::import_picture`):
it prepares each actual AEP file once and resolves its exact composition GUID.
Each occurrence gets separately allocated editable layer/effect/item IDs at
conversion time, and footage is packaged under one namespace per resolved AEP.
`Premiere::import_with_linked_compositions` with `import_editable_picture` remains
a caller-supplied route whose content takes the same placement. No intermediate
Tesseract serialization/reparse, generic JSON ID remapping, image flattening or
hidden original-AEP replay is used.

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
picture is muted. **Audio items of linked media are omitted with a contextual
reason: linked-audio occurrence import is not implemented.** Missing AEPs,
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
header is accepted; it is supplementary, not GUID identity proof. No general AE
version acceptance is claimed.

**Caller-supplied content:** `Premiere::import_with_linked_compositions` takes
each placement's content from a `LinkedCompositionResolver` instead (for example
`ResolvedAfterEffectsComposition::import_editable_picture`, the same AE
conversion as the built-in import). That content takes the same placement, clock,
canvas, blend, stage and omission rules; an After Effects profile, identity or
content failure that the built-in import would omit omits that clip, and any
other importer failure stops the import. The AEP is rechecked with the other
media, but the lifetime and freshness of the caller's assets are the caller's:
the linked-footage checks above apply to the built-in import only.

**Export:** the hybrid export above is unchanged by this import, which reads its
linked AEP scopes back as editable pictures.

**Checkpoint (user-approved):** this increment delivers linked pictures as an
editable project and flags existing AE import approximations and defects in
each composition's diagnostics. Repairing shared AE content or effect fidelity
and importing linked audio are outside this checkpoint.

**Evidence:** CPU structural tests, plus one root runtime test. The native H-IDENTITY-01 package converts
with distinct same-name red/blue editable Rects at their native placements and
source offsets (`tests::linked_compositions::native_links_import_editable_same_name_compositions_by_exact_guid`).
Supplementary typed and derived cases cover repeated and nested placements,
equal item IDs of two AEPs, per-AEP footage assets, muted linked audio with an
omitted audio item, unresolvable links, video-clip placement and key parity
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
| Clocks | AEP scopes require integral24/25/30fps; existing packing checks exact native source/timeline ranges | The clip group keeps the native clip clock and keys (with an identity-rate seed on the document clock); a `Premiere linked source` group carries the native trim/speed/remap; AE children keep their source clocks | A clip showing past its composition's end, or on another canvas, is omitted with a diagnostic; conflicting motion-blur settings are approximated. Time-remapped AE animation under a later-starting stage/nest runs early (approximated). Existing millisecond FX timing limitations remain. |
| Audio | AEP native audio switches are disabled; independent/embedded native sound stays in Premiere | AEP picture expansion is muted; native audio imports independently | Linked AEP audio itself is unsupported and diagnosed. Replacing a nested native-audio owner remains rejected by packing. CPU structure is not an audible comparison. |
| Identity and assets | Actual generated root identity and portable AEP-local paths | One parse per AEP file; disjoint generated IDs per occurrence; footage under one namespace per resolved AEP (a caller-supplied importer names its own); picture clipped to its composition canvas | GUID support is restricted to the independently observed AE26.5x89 macOS and AE26.3x87 profiles. Unknown profiles and absent targets omit the clip with a diagnostic; cycles and expansion limits are diagnosed within the picture. |
| Check/Write and publication | Same conversion and package assembly; Check does not publish | Same import and archive validation; Check does not publish; the AEP, linked footage and PSD/AI sources are rechecked before Check returns and before publication (caller-supplied assets excepted) | Fresh output only, source/path/SHA checks and owned rollback. Recoverable rollback is not crash atomicity or filesystem locking. |

AE import/export capabilities and every format-level approximation remain in the
[AE support ledger](after-effects-support.md). Premiere retains its existing
source-profile, canvas, codec, frame-rate, retiming and audio restrictions. This
work does not modify FX schema, evaluation, editor or renderer semantics.

## Large-project export follow-up

Native lowering no longer constructs an invalid nest when a picture-only group's
range extends past its exported children. It reports the group omission so the
existing root-scope AE fallback can run; supported siblings remain native.
Groups containing retained nested sound still fail explicitly instead of losing
that sound. Child animation and motion-blur evidence is committed only when its
containing nest survives; omitted child scripts are not counted as written.
Native input/writer duration validation is unchanged.

Gap coverage now runs on the assembled picture before final native writing,
including native-only output. An incomplete overlay still fails and cleans its
private staging; no final coverage check was removed. Compositing backdrop
closure excludes standalone Audio roots, which remain native audio; explicit
property/layer dependencies on audio are not silently ignored. Audio may occur
inside a selected root interval without changing picture selection. The existing
picture-only AE writer may also stage that audio asset, but its AE audio switches
are disabled; Premiere retains the original native sound occurrence.

The routing collector and owner lookup retain inputs beyond their former 1,024
entries. Packing source boundaries use the actual `u32` token range rather than
1,024 slots. Human diagnostics remain bounded. This does not remove the other
packing, hierarchy, dependency or format limits, or establish bounded memory.

Supplementary regression symbols:
- `a_group_longer_than_its_children_is_omitted_without_losing_siblings`
- `hybrid_replaces_picture_before_validating_native_gaps`
- `final_native_and_incomplete_overlay_still_reject_uncovered_gaps`
- `hybrid_picture_backdrop_does_not_absorb_standalone_audio`
- `hybrid_owner_lookup_keeps_layers_beyond_1024`
- `more_than_1024_source_boundaries_remain_replaceable`
- `routing_events_are_not_truncated_at_the_former_count_limit`
- `omitted_long_group_does_not_claim_its_child_script_keys_were_written`

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
