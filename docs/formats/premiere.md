# Premiere Pro (`.prproj`)

[All formats](README.md) · [CLI reference](../../apps/tesseract-conv/README.md) ·
[implementation notes](../../crates/premiere_file/README.md) ·
[accuracy ledger](../../apps/tesseract-conv/README.md#conversion-accuracy-and-limitations) ·
[fixture and Adobe-evidence inventory](../../tests/README.md)

**Native Premiere ↔ `.tsrct`: partial, with possible data loss.** Import reads a
selected or provably top-level sequence into an editable document. Export authors
a new Premiere project from the current edited document; it does not replay
hidden source XML.

**Explicit media preparation (import only):** used unsupported video is now a
fatal Check/Write admission error, not a successful clip omission. Target-scoped
inspection includes nested/disabled source references and supported Dynamic Link
footage before FX omissions. These new checks are audio/video-only. `transcode`
converts one media file to one output file with library/external backends, without
reading a project or assembling a map; import consumes an explicit source-bound
`--media-map` and reruns normal admission. Missing/still/effect policies remain
separate. Premiere's current admission does not accept the AE ProRes4444-alpha
preparation route: it is blocked rather than losing alpha. See
[media preparation](../media-preparation.md) for preservation restrictions and
backend setup. The regression evidence is structural/admission-only; no new
independent Adobe, RGB, alpha or audio fidelity proof is claimed. Export is
unchanged and has no new proof from this workflow.

[Projects](#project-selection-structure-and-publication) ·
[Timeline and playback](#timeline-playback-visibility-and-nesting) ·
[Media](#media-assets-stills-and-generators) ·
[Motion and effects](#motion-compositing-masks-and-effects) ·
[Text and captions](#type-tool-text-fonts-and-captions) · [Audio](#audio)

## How to read these tables

The status words have the definitions in the [format index](README.md#reading-support-status).
They apply only under the conditions in the same row.

- **Native → `.tsrct`** means Premiere import; **`.tsrct` → native** means Premiere export.
- **Structural** means implementation/unit/integration assertions or converter
  read-back. It is not proof that Premiere accepts or renders an export.
- **Adobe** means the linked inventory records an independent Adobe/AME render,
  Premiere read-back, or both. Evidence is case-specific. `structural_only`
  remains unverified visual equality even when an Adobe video exists.
- **Diagnosed** means an omission is returned and printed on stderr by the CLI.
  It does **not** mean every loss is warned about. Rows marked **unwarned** identify
  deliberate precision/layout/metadata losses for which the implementation does
  not emit a user-facing omission.
- **Omit property/effect** keeps the layer with that feature absent or static;
  **omit occurrence/layer** removes that item while convertible siblings remain;
  **reject** stops that timeline or export as stated. Invalid structure, unsafe
  references, publication failures, or no convertible selected timeline can be fatal.

Test symbols below are searchable Rust test function names. The principal test
owners are the [public conversion tests](../../crates/premiere_file/tests/conversion/),
[native-format tests](../../crates/premiere_file/src/format/tests/), and
[mapper tests](../../crates/premiere_file/src/convert/tests/).

## Project, selection, structure, and publication

Source: [library responsibilities and limits](../../crates/premiere_file/README.md),
[CLI timeline/media handling](../../apps/tesseract-conv/README.md#timeline-and-media-handling).

| Property or operation | Native → `.tsrct` | `.tsrct` → native | Exact conditions and retained semantics | Unsupported/loss handling | Evidence |
| --- | --- | --- | --- | --- | --- |
| Conversion unit | Partial | Partial | One native sequence becomes one editable FX document; export consumes one standalone `.tsrct`. | Full web projects and application-wide editing state are outside this boundary. | Structural: `web_project_file_is_not_an_editable_document_archive`. Adobe: not applicable. |
| Top-level sequence discovery | Supported | — | Every sequence proven not to be nested is converted independently. | Broken links that make nesting uncertain require `--sequence`; no guessing. | Structural: `broken_occurrence_link_requires_selection_and_preserves_other_clips`. Adobe: none. |
| Explicit sequence selection | Supported | — | `--sequence <GUID>` selects one top-level or nested sequence and can retain valid clips in a cyclic timeline. | Unknown/malformed selection rejects; cyclic placements themselves are omitted. | Structural: `explicit_selection_converts_a_nested_sequence`, `explicit_selection_preserves_valid_clip_in_a_cyclic_timeline`. Adobe: selected-root and nested-child cases are [scored](../../tests/README.md#nine-focused-feature-gates). |
| Multiple independent roots | Supported | Partial | Import emits one archive per convertible root; media identity is project-wide. Export authors one root plus generated inner sequences. | A root that cannot convert is diagnosed and omitted; all selected roots failing is fatal. | Structural: `premiere_to_tesseract_builds_every_root_with_duplicate_names_and_independent_original_media`, `unsupported_second_timeline_keeps_first_project`. |
| Legacy save shapes (CS6 to CC 2015) | Supported | — | `TrackItem` `Type`/`MediaType`/`TrackIndex`/`TrackRefCount`, `VideoClip` `PosterFrame`/`FrameBlend`/explicit off states (`ScaleToFramePolicy 0`, `FrameHold 0`), inert interlace options and unset media content boundaries are read; `FrameBlend true` maps to Simple frame blending and legacy `ScaleToFrameSize true` to the modern policy. The writer never writes these fields. | A non-inert interlace option or a real content boundary rejects the clip with its field name. | Structural: `legacy_track_item_fields_are_read_and_never_written`, `legacy_clip_settings_map_or_reject_like_their_modern_fields`, `legacy_clip_fields_are_read_and_the_poster_frame_is_never_written`. Adobe: none; corpus shapes only. |
| Check mode | Supported | Supported | `--check` performs conversion, media validation, and encoding without creating output. | It does not prove later writes or rendering will succeed. | Structural: `checks_validate_real_media_without_writes_and_share_execution_failures`. |
| Publication | Partial | Partial | Source/media identities are rechecked before publication; output requires hard links. | Not crash-atomic; interruption can leave partial output. Successful process exit is the completion signal. | Structural: [publication tests](../../crates/premiere_file/tests/conversion/publication.rs), including `later_missing_media_keeps_first_project_without_staging_directory`. Adobe: none. |
| Project paths and aliases | Partial | Partial | Live aliases must agree; a live absolute alias can recover stale relative hints. Media saved on Windows (every alias drive-absolute, `C:\…`, which the host cannot open) resolves only through its package-local `RelativePath`, read with `\` separators; its aliases are never opened. | Conflicting live aliases reject. On a POSIX host, Windows-saved media rejects without a package-local hint or with a hint that names a drive, and drive-relative, UNC, device and mixed Windows/POSIX aliases reject; a Windows host keeps its unchanged native path handling. | Structural: `native_stream_rate_and_absolute_aliases_must_match_the_source`, `windows_saved_media_paths_resolve_only_through_package_local_hints`, `windows_saved_media_converts_only_from_its_verified_package_copy`, `windows_saved_paths_keep_the_timelines_of_pinned_fixtures`, relocation tests. |

## Timeline, playback, visibility, and nesting

Source: [timing rules](../../crates/premiere_file/README.md#timing),
[nested-sequence rules](../../crates/premiere_file/README.md#nested-sequences), and
[playback mapper](../../crates/premiere_file/src/convert/premiere_to_tesseract.rs).

| Property or operation | Native → `.tsrct` | `.tsrct` → native | Values, keyframes, and precision | Unsupported/loss handling | Evidence |
| --- | --- | --- | --- | --- | --- |
| Sequence canvas | Supported | Supported | Any positive size at the origin (1920×1080, portrait, square, ultrawide or odd), square-pixel SDR. Premiere saves square pixels as `PixelAspectRatio` `1,1` or as any equal pair, such as `1920,1920`. The document keeps the sequence size; export writes the document size. Each placement carries its sequence's frame; media keep their natural size (Anchor Point × source, Position × canvas). | A malformed, zero, overflowing or off-origin frame, a malformed `PixelAspectRatio` or non-square pixels (an unequal pair such as HDV `1920,1440`, named in the diagnostic) omit the timeline or placement; a placement frame other than its sequence's, a Color Matte or adjustment layer of another size and a nest of another canvas omit the placement. Captions on another canvas keep their stored size with one approximation warning. | Structural: `custom_canvases_keep_their_size_and_place_the_source_by_its_own_frame`, `derived_vertical_canvas_converts_at_its_own_size_in_both_directions`, `custom_canvas_premiere_build_writes_the_document_size_and_reimports_it`. Adobe: the independent 1080×1920 `premiere_isolated_vertical_canvas_26_5` import passes its score gate. The root export reopened and its render matches the independent native render across all 60 frames. The user accepted only the shared frame-zero colour-cell error (4.0 against the unchanged 3.0 bound). Other non-HD sizes and custom-canvas nests retain structural coverage only; AEP-package native proof remains pending. |
| Sequence frame rate | Partial | Partial | Import: 23.976, 24, 25, 29.97, 30, 50, 59.94, 60. Export: 23.976, 24, 25, 29.97, 30, 59.94 (`--fps`, default 30). | Import omits an unlisted-rate timeline. Export rejects 50/60 because native display codes are unpinned. Imported sequence rate is **unwarned lost** and not automatically restored. | Structural: `rounding_bounds_and_shared_boundaries_hold_at_every_frame_rate`; Adobe: AME rate probes, not pinned score gates, per [export rate](../../crates/premiere_file/README.md#export-rate). |
| Timeline boundaries on import | Supported | — | Absolute start/end round to nearest millisecond, ties forward; each moves ≤0.5 ms and adjacent cuts share one rounded boundary. | Precision loss is **unwarned**. | Structural: `absolute_boundary_rounding_preserves_adjacency`. Adobe: mixed-rate gate; 24 fps cut has documented one-source-frame lag. |
| Timeline boundaries on export | — | Partial | Video start/end snap to nearest export frame, ties forward; each moves ≤ half a frame. Audio boundaries remain exact milliseconds. | A layer collapsing to zero frames rejects. Precision loss is **unwarned**. | Structural: `absolute_boundaries_snap_but_source_in_stays_off_the_sequence_grid`, `snapping_rejects_layers_that_collapse_to_zero_frames`. |
| Stale unit-speed `OutPoint` | Supported | — | A forward unit-speed clip plays from In for its duration; a saved Out up to one sequence frame off that end reads as the played end. | Larger differences, empty ranges, other speeds, reverse and remaps keep the saved Out and the source-span rejection. | Structural (synthetic): `unit_speed_clip_plays_from_in_for_its_duration_past_a_one_frame_stale_out`. Adobe: no fixture or render in this repository. |
| Cuts, gaps, move/add/delete | Supported | Supported | Current editable layer ranges and ordering are used; repeated placements keep independent ranges. | No hidden original edit decision list is restored. | Structural: `current_edited_document_builds_premiere_cuts_gaps_and_deleted_state`; Adobe: cut/trim/gap gates. |
| Constant positive speed | Supported | Supported | Positive finite `PlaybackSpeed`; import produces bounded two-key linear editable playback. Export derives speed from the selected key-value span/frame-snapped active span; that nonempty interval may lie inside a larger `sourceRange`. | Invalid/nonpositive native value omits occurrence. Export requires two linear keys spanning the active input window after `inputOffsetMs`, inactive extrapolation and values within the authored selection; any other playback shape follows the nonconstant FX playback row. | Structural: `constant_speed_and_reverse_import_as_editable_time_remap`, `exports_slow_fast_and_reverse_constant_playback`, `bounded_constant_playback_respects_signed_input_offsets`, `bounded_constant_playback_rejects_unsupported_curves_and_windows`, `bounded_constant_playback_keeps_media_duration_checks`. Adobe: focused half-speed cases in inventory; bounded-selection repair has no new native score here. |
| Constant reverse | Supported | Supported | `PlayBackwards=true` plus positive speed maps to reversed two-key playback; native reverse source bounds are converted through intrinsic duration. | Invalid flag omits occurrence; reverse export selects the descending key-value interval within `sourceRange` under the same bounded constant-playback checks. | Structural: `focused_native_reverse_fixture_imports_as_editable_time_remap`, `exports_slow_fast_and_reverse_constant_playback`. Adobe: reverse case remains `structural_only`; bounded-selection repair has no new reverse native score. |
| Constant-rate inner media clip | Partial | Existing playback mapping | Nested trim preserves the authored source-span/timeline-span ratio and editable constant playback, including reverse stored bounds. | Inner source-time remapping remains diagnosed and omitted; variable curves and holds are not trimmed by this path. | Structural: `constant_rate_inner_clips_keep_rate_aware_source_trims`, `native_opening_slow_clip_trims_to_the_nested_window`, `nest_window_trims_its_slow_inner_clip_in_proportion` (synthetic). Native-derived provenance: `cap2-native-import.md`. |
| Native variable Time Remapping | Partial | Unsupported | Import supports an untrimmed full-media curve as editable playback with Linear/Hold/Bezier easing and millisecond key times. | Trimmed native ramps omit occurrence/timeline with diagnosis. Export does not write native variable TimeRemapping. | Structural: `adobe_native_variable_speed_ramp_preserves_source_clock_curve`, `trimmed_variable_speed_ramp_is_omitted_with_diagnostic`. Adobe: variable-speed fixture evidence is case-specific in inventory. |
| Explicit native Frame Hold | Partial | Unsupported | `FrameHold=4` with explicit nonnegative source-tick `FrameHoldStart` imports as two editable playback keys selecting one source instant, rounded to milliseconds. | Held instant must precede the media end. Unknown/incomplete holds, speed/TimeRemapping combinations, Motion animation and unsupported master/graphic/adjustment clocks reject. Native export omits this nonconstant playback with diagnosis. | Structural: `native_explicit_frame_hold_imports_as_editable_constant_playback` and rejection regressions; [native source provenance](../../crates/premiere_file/tests/fixtures/cap2-native-import.md). Independent broader-sequence Adobe review is diagnostic only. |
| Nonconstant FX playback / source time remap | — | Unsupported | Only bounded two-key linear forward/reverse playback is native. | Other playback, and a `source.timeRemap` whose source span differs from its active span, omit the occurrence (diagnosed). FX does not render `source.timeRemap`; one over an equal span exports at unit speed with a report. | Structural: `playback_that_native_speed_cannot_carry_omits_its_clip`. Adobe: none. |
| Frame blending | Supported | Supported | Native `TimeInterpolationType`: `0`/absent = sampling, `1` = Simple, `2` = Optical Flow; FX boolean true exports Simple. | Unknown native code is diagnosed and falls back to sampling; no fatal error. | Structural: `adobe_time_interpolation_maps_to_editable_frame_blending`, `frame_blending_modes_export_without_loss_diagnostics`. Adobe: half-speed frame-blending case in inventory. |
| Clip Enable | Supported | Supported | `ClipTrackItem/IsMuted=true` ↔ hidden layer; source, trims, transforms, and disabled-only duration remain editable. | Invalid native bool omits occurrence. | Structural: `disabled_clip_and_muted_track_import_hidden_and_export_disabled`; Adobe: disabled import/export gates prove rendering, while pixels alone do not prove retention. |
| Track video output (“eye”) | Partial | Unsupported | A muted native track flattens to `isHidden` on every occurrence. Export writes enabled tracks and per-clip disable. | Track grouping/toggle identity is **unwarned lost**; invalid bool rejects the track. | Structural: `disabled_clip_and_muted_track_import_hidden_and_export_disabled`. Adobe: derived disabled fixture. |
| Plain nested placement | Partial | Partial | Import expands a same-canvas, enabled, effect-free nest at normal speed (outer rate, frame-aligned window) into an independent clipped group; its static or keyed Motion (without a Track Matte Key) becomes the group's, clipped to the nested frame. Export writes a plain named group as a new inner sequence. | Sharing is **unwarned lost**: repeated native nests become unlinked copies and identical groups are not re-shared. Inner black canvas is not copied. | Structural: `xml_edited_nests_import_as_independent_groups_over_the_outer_clip`, `nests_round_trip_through_premiere_as_one_sequence_per_group`. Adobe: derived nested gate; native UI authorship unverified. |
| Retimed nested placement | Partial | Unsupported | A forward constant-speed nest whose saved In/Out its PlaybackSpeed confirms (inner frames per outer frame; the inner rate may differ) becomes a group whose `playback` maps the placement linearly onto that inner window, each end rounded once to a millisecond. Inner placements that share time with the window keep their inner-clock places and trims. A retimed nest inside a normal-speed nest keeps its rate. | Frame Blending/Optical Flow of the nest and its Motion keys are diagnosed omissions (static values kept); inner effects keep their own rules (an inner Warp Stabilizer stays omitted). No sound. Reverse, time remap, an unconfirmed window or one whose rounded endpoints coincide omits the nest. Export omits the group; remapped-group export is not added. | Structural (synthetic): `mixed_rate_retimed_nest_maps_its_placement_onto_its_inner_window`, `a_retimed_nest_maps_its_window_onto_children_on_the_inner_clock`. Adobe: none. |
| Non-plain nest/group | Unsupported | Unsupported | Nest must have default Opacity, no Motion beside a Track Matte Key, no effects, reverse or time remap, a speed that its window confirms, the same canvas, the outer rate unless retimed, and valid bounds. Group must be visible, named, identity, and free of animation/effects/masks/matte/blend/background/time-remap/motion blur. | Import omits nest; export omits group and does not inspect media beneath it. A group longer than children rejects. | Structural: `a_nest_whose_master_clip_plays_media_is_omitted`, `media_under_an_omitted_group_is_not_inspected`. |
| Inner still/text/matte | Unsupported | Unsupported | Inner video children use ordinary video mappings. | Per-item diagnosed omission; convertible video siblings remain. | Structural: nested mapper tests summarized in the [accuracy ledger](../../apps/tesseract-conv/README.md#conversion-accuracy-and-limitations). Adobe: none. |
| Cross Dissolve / clip transitions | Partial | Unsupported | Topology and native range are read; a Version 6 record's omitted StartPercent, EndPercent, SwitchSources and Reverse read as their defaults. A default Cross Dissolve (Legacy) imports only as the incoming-only head of a retained static Normal Color Matte on the document clock that no Track Matte Key uses: from the cut at the matte start to its end inside the matte, two editable Linear Opacity keys go from 0 to the matte's static Opacity, which they replace, and the value then holds. | The linear ramp is a declared approximation: the Legacy curve, frame phase and compositing space are unmeasured. Every other host or form (tail, two-sided, multiple, partial, nondefault, retimed, keyed, blended, staged, nested, Track Matte Key source) keeps a diagnosed feature omission; no fake opacity graph. Writer rejects native transition records; export omits the faded matte as a keyed Shape. | Structural (synthetic): `a_head_cross_dissolve_fades_its_matte_in_to_the_static_opacity_once`, `matte_dissolves_outside_the_incoming_only_static_head_are_reported_not_keyed`, `a_track_matte_key_source_gets_no_head_dissolve_whether_its_keyed_clip_converts`, `detected_cross_dissolve_is_reported_without_faking_an_editable_graph`. Adobe: none. |

## Media, assets, stills, and generators

Source: [supported video formats](../../apps/tesseract-conv/README.md#supported-video-formats),
[media implementation](../../crates/premiere_file/README.md#timing), and
[public media tests](../../crates/premiere_file/tests/conversion/video_formats.rs).

| Property or operation | Native → `.tsrct` | `.tsrct` → native | Exact supported values/conditions | Unsupported/loss handling | Evidence |
| --- | --- | --- | --- | --- | --- |
| Video geometry/color | Supported | Supported | Progressive, square-pixel, limited-range 8-bit 4:2:0 SDR; BT.709 or allowed unspecified declarations; source dimensions match native/document facts. | Full range, HDR/non-BT.709, interlace, clean aperture, rotation, or mismatched dimensions omit import occurrence or reject export. | Structural: `container_color_profiles_are_validated_in_both_directions`, `source_rect_cannot_disagree_with_packaged_video_frame`. |
| Omitted video `sourceRect` | — | Supported / unverified | Export resolves the natural frame from the inspected active media's dimensions, including replacement footage. Contain, Cover, Stretch and legacy None have unit media scale on that natural frame; the authored transform remains unchanged. | Explicit non-origin, fractional or mismatched frames retain their existing checks; Custom fit retains its existing diagnostic. Media under a natively omitted clip stays uninspected. Frame-dependent script fitting still runs before this resolution and keeps its existing limitations. | Structural: `omitted_video_source_rect_uses_inspected_media_dimensions`, `natural_video_frame_uses_active_media_and_preserves_the_original_view`, `nonlinear_video_without_source_rect_is_omitted_without_inspecting_its_media`. No new Adobe render claim. |
| Video intrinsic duration in milliseconds | — | Partial | Authored `sourceIntrinsicDuration` must equal the exact inspected source endpoint projected either by floor or by nearest half-up rounding. The native media keeps the exact source clock. | A floor value that differs from nearest is a contextual metadata approximation; it does not select an AE picture scope. Other mismatches reject. Source-range and final-frame coverage checks remain authoritative. | Structural: `intrinsic_duration_accepts_only_floor_or_nearest_of_the_exact_source_clock`; the 121-frame 24fps endpoint accepts 5041/5042ms and rejects 5040/5043ms; an exact 5000ms endpoint rejects 4999/5001ms. No new Adobe render claim. |
| H.264 | Supported | Supported | `avc1` in MP4 or MOV, accepted parameter/profile constraints; bytes unchanged. | Other H.264 sample-entry forms reject. | Structural: `adobe_derived_video_formats_import_each_codec_as_an_editable_layer`, byte checks. Adobe: H.264/HEVC gate. |
| HEVC Main | Partial | Partial / unverified | `hvc1`, Main 8-bit 4:2:0, complete parameter arrays, one layer, bounded parsed SPS/VUI forms; bytes unchanged. | Unsupported SPS/VUI branches reject. Exported native HEVC record acceptance is unverified. | Structural: `hevc_sequence_parameters_decide_range_color_aspect_scan_and_size`, `authored_video_formats_export_with_each_codec_type_and_unchanged_bytes`. Adobe: AME reads derived HEVC input, not generated HEVC export. |
| Other video codecs | Unsupported | Unsupported | Only `avc1`/`hvc1` accepted. | Named rejection for ProRes, VP9, AV1, MPEG-4 Part 2, in-band `avc3`/`hev1`, APV, DNx, MJPEG, and others. | Structural: `unsupported_codecs_are_named_in_both_directions`. |
| Containers/tracks | Partial | Partial | `.mp4`/`.mov`, one video; picture-only selected import preserves unused audio/data streams. Consumed sound retains strict unambiguous audio admission. | Fragmented files, captions, unsupported codecs or consumed ambiguous sound reject. `.m4v` rejects in both directions. | Structural: `selected_picture_retains_unused_audio_streams_but_consumed_audio_stays_strict`, media metadata/edit-list tests. |
| Media frame rates | Partial | Supported exact rates | Exact listed CFR, bounded quantized/irregular import and positive raw native source clocks. Selected unit source intervals may use the first affine edit before the final physical sample. | Native count mismatch, invalid sample tables, retiming or later-edit crossing reject the bounded exception. Export remains exact listed CFR. | Structural: `selected_irregular_endpoint_requires_exact_native_count_and_safe_interior`, `selected_partial_edit_rejects_crossing_later_segments_and_ambiguous_origin`; Adobe: mixed 25-in-24 gate; source-bound decode evidence is diagnostic. |
| Media bytes and identity | Supported | Supported | No transcoding. Repeated placements of one media record/asset share bytes; distinct records remain distinct even for identical files. | Conflicting aliases or changed bytes reject. | Structural: `repeated_media_survives_premiere_tesseract_roundtrip_with_one_packaged_asset`, `distinct_same_byte_files_keep_independent_identity_after_package_relocation`. |
| Canvas JPEG still | Supported | Supported / unverified | `IsStill`, exactly canvas-sized, supported baseline JPEG, orientation 1/absent; original bytes. | Unsupported encoding/orientation/size omits import occurrence or rejects export. Generated native still reopen is unverified. | Structural: `derived_still_fixture_imports_editable_image_layers_and_packages_original_images`. Adobe: still-image gate uses a derived source. |
| Canvas PNG still | Supported | Supported / unverified | Single-frame PNG accepted by renderer decoder; straight alpha retained; native `AlphaType` must agree with file. | Animated PNG, contradictory alpha, unsupported type/size omit/reject. Some alpha-record forms are inferred. | Structural: `still_files_that_contradict_their_native_record_omit_only_their_occurrence`. Adobe: transparent-PNG span in still gate. |
| Other still formats/sizes | Unsupported | Unsupported | No centered-fit conversion for non-canvas images. | TIFF/PSD/GIF/WebP/APNG, image sequences, unsupported JPEGs, >11 h placements omit/reject. | Structural: still/image-media rejection tests. Adobe: none. |
| Still Motion/Crop/Opacity/effects | Unsupported | Unsupported | Default static placement only. | Nondefault Motion/Crop/Opacity or Motion keys omit native occurrence; export reports unsupported image properties and writes defaults or omits/rejects as the image rule specifies. Active effects are diagnosed and absent. | Structural: still mapper tests. Adobe: none. |
| Still playback/retime | Partial | Partial | Native nonunit/reverse/time-remap does not change a still's constant picture. | Import keeps placement and diagnoses discarded clock; Linear Wipe omits occurrence. Exported image animation is not retained. | Structural: still interaction tests. Adobe: none. |
| Embedded ICC on still | Partial | Partial | Profile bytes remain embedded unchanged. | Feature omission is diagnosed for each placement/layer; color-management parity is unbounded. | Structural: `embedded_icc_profile_is_reported_in_both_directions_and_the_still_kept`. Adobe: none. |
| Color Matte | Supported | Partial / unverified | Native `COLR` canvas matte ↔ full-frame static solid rectangle; a static Opacity imports as the rectangle's opacity, which a bounded head Cross Dissolve (Legacy) keys (transitions row); color rounds to 8-bit on export. | Other generator media remain unsupported; a matte stream of another size than its sequence omits its placement. A faded (keyed) matte rectangle is no Color Matte on export: the Shape rules omit it. Generated matte records have not been reopened in Premiere; hash/state fields are inferred. | Structural: `isolated_color_mattes_round_trip_as_editable_solid_fills`. Adobe: derived Color Matte gate confirms red/blue channel order. |
| Linked After Effects composition (Dynamic Link) | Partial | Partial | A video clip of a linked composition imports its editable After Effects content, selected by exact AEP file and native GUID (never by name), as a group on the clip clock under the ordinary clip rules for Motion, Opacity, blend, keys, masks, effects, matte and clocks; only the content plays from the source In, speed or remap, through a source group, and it is clipped to its composition canvas by a guide and mask. Each placement, repeated or nested, owns a fresh copy. Composition footage is packaged per AEP. Two AE header profiles with native identity evidence are accepted. A caller-supplied importer (`Premiere::import_with_linked_compositions`) can supply the content; it takes the same placement. The CLI's Premiere export writes linked AEP picture scopes ([hybrid ledger](../hybrid-adobe-export.md)). | A missing AEP, another AE producer, an absent GUID target or a canvas that differs from the link record omits that media's clips with the reason; a composition shorter than the source range a clip shows omits that clip; siblings convert. The composition's own import approximations are reported per link; frame blending is diagnosed and dropped; effect keys on a retimed linked clip are diagnosed and dropped, keeping static values; under a stage group or nest that starts after the document start, time-remapped linked content is diagnosed as an approximation (its animation under time remapping runs early at runtime). Export limits are in the hybrid ledger. | Structural: `native_links_import_editable_same_name_compositions_by_exact_guid`, `a_clip_past_its_composition_s_end_is_omitted_and_its_siblings_convert`, `caller_supplied_compositions_take_the_linked_clip_placement`, `a_caller_supplied_composition_that_cannot_be_placed_omits_only_its_clip`, `a_caller_supplied_import_editable_picture_matches_the_built_in_import`, CLI `premiere_cli_import_places_native_linked_compositions_on_their_clip_clocks`, `repeated_and_nested_placements_each_own_identities_and_clocks`, `a_linked_clip_is_placed_and_keyed_on_the_video_clip_clock`, `a_retimed_linked_clip_plays_its_composition_at_the_clip_speed_with_static_keys`, `a_linked_clip_in_a_nest_is_keyed_on_its_clip_clock`, `a_linked_matte_keeps_its_clock_seed_only_under_a_stage_on_the_document_clock`, `a_staged_linked_clip_after_the_document_start_reports_its_remapped_animation`, `unresolvable_links_are_reported_and_their_siblings_convert`. Runtime: root `premiere_keyframes::a_linked_composition_keys_its_clip_on_the_clip_clock_and_plays_from_its_source_in` (derived links, root level only). Adobe: native source identity renders only; converted render unmeasured. |
| Implicit black canvas | Partial | Partial | Import adds editable black rectangle because uncovered gaps otherwise do not render black. Export recognizes bottommost black origin-pivot rectangle as canvas. | This is converter-authored structure, not preservation of a native layer. Canvas must cover actual gaps. | Structural: `black_canvas_is_semantically_validated_and_required_for_gaps`. Adobe: gap gates. |
| Other solid rectangles | — | Partial | Static opaque full-frame solid, origin or centered pivot, no stroke/gradient/transform/effects/masks/matte; one native matte per color. | Diagnosed layer omission for any unsupported paint/geometry/property. | Structural: color-matte omission tests. Adobe reopen unverified. |

## Motion, compositing, masks, and effects

Source: [effect-stack implementation](../../crates/premiere_file/README.md#effect-stacks),
[unsupported edits](../../crates/premiere_file/README.md#unsupported-edits), and the
[property-level accuracy ledger](../../apps/tesseract-conv/README.md#conversion-accuracy-and-limitations).

| Property or operation | Native → `.tsrct` | `.tsrct` → native | Static/keyframe support | Preserved, approximated, or lost semantics | Evidence |
| --- | --- | --- | --- | --- | --- |
| Motion Position | Supported | Partial | Static normalized position ↔ canvas pixels. Paired X/Y keys support Linear/Hold and measured point-path representation; spatial tangents retained where accepted. | Unpaired/mismatched export tracks or unsupported cubic forms are diagnosed and animation omitted, static value retained. Times round ≤0.5 ms on import. | Structural: `edited_fx_position_paths_export_on_the_source_clock`, `first_keys_after_the_source_in_export_and_reimport_unchanged`. Adobe: 26.5 motion fixture export gate. |
| Motion Anchor Point | Partial | Partial | Static, and Linear keys without spatial tangents: normalized source coordinates ↔ source pixels (`anchorPointX`/`anchorPointY` with their own key ids). Export needs both axes keyed at the same times and easing. | Hold, Bezier or spatial Anchor Point keys omit the clip on import; unpaired, mismatched or non-Linear tracks are diagnosed on export and the static value is retained. | Structural: `static_motion_imports_as_editable_source_and_canvas_coordinates`, `native_probe_anchor_point_and_scale_width_keys_import_as_editable_axis_tracks`, `edited_anchor_point_and_scale_width_keys_export_as_native_motion_keys`, `anchor_point_and_scale_width_tracks_export_on_the_source_frame`. Adobe: motion fixture (static); the independent `premiere_motion_anchor_scale_width_probe_20260930` import passes its score gate (picture only, 1920×1080 source and sequence); a generated export of an explicit edit passes a bounded Adobe export gate (picture only). |
| Motion uniform Scale | Supported | Partial | Static and paired identical X/Y tracks; Linear/Hold/Bezier. Native scalar duplicates to two editable axes. | Export writes uniform Scale only from identical values/times/easing over equal static axes; an X track alone is Scale Width (next row). Other mismatches are diagnosed and keys omitted; static may remain. Static Scale Width is ignored while Uniform Scale is on (**unwarned**, measured no picture loss); export writes Scale Width equal to Scale. | Structural: `adobe_trimmed_source_scale_keeps_two_editable_source_clock_axes`, `mismatched_or_unpaired_scale_axes_are_omitted_with_diagnostics`. Adobe: Linear/Hold gates; Bezier visual difference/readback limits in ledger. |
| Motion nonuniform Scale | Partial | Partial | Independent static axes; Linear Scale Width keys without Uniform Scale ↔ the FX `scaleX` track alone. Export writes an X track alone as Scale Width keys with Uniform Scale off, even over equal static axes. | Scale Height keys without Uniform Scale, Scale Width keys under it and non-Linear Scale Width keys are diagnosed (clip omitted on import; animation omitted, static retained on export); no flattening to one animated scalar. | Structural: `adobe_derived_static_motion_keeps_independent_position_anchor_and_axis_scales` and the Anchor Point row's tests. Adobe: static-motion case; the probe's import passes its score gate (picture only); the same bounded export gate covers Scale Width. |
| Motion Rotation | Supported | Supported | Static; Linear/Hold/Bezier scalar keys on source clock. | Native key time rounds ≤0.5 ms; secondary interpolation field, unused final outgoing mode, and native metadata are **unwarned lost**. Unholdable curves are diagnosed and animation omitted. | Structural: `imported_rotation_is_editable_and_retains_signed_off_trim_keys`, `edited_fx_rotation_keys_export_on_the_source_clock`. Adobe: Linear/Hold/trimmed gates. |
| Intrinsic Opacity | Supported | Supported | Static 0–100 and Linear/Hold/Bezier keys; only Normal native blend pair `(18,0)`. | Other blend pairs omit native clip or export as diagnosed unsupported blend. Tesseract encoded-value blend differs from AME linear-light blend; no automatic compensation. | Structural: `adobe_normal_pair_18_0_preserves_static_opacity_without_omission`, `edited_fx_opacity_keys_export_on_the_source_clock`. Adobe: JRB-2081 reopen/render gate. |
| Blend modes | Unsupported except Normal | Unsupported except Normal | Normal only. | Import unsupported pair omits occurrence. Export diagnoses and uses Normal for unsupported FX blend. Visual error unbounded. | Structural: `lightening_22_10_blend_pair_omits_only_its_occurrence`, `only_normal_blend_mode_exports_without_fallback`. Adobe: `(22,10)` diagnostic shows it is not Normal. |
| Scalar key timing/easing | Partial | Partial | Native outgoing mode maps to next FX key's incoming interval. Linear `0`, Hold `4`, Bezier `5` where the property is measured. | Millisecond rounding and ordinary native metadata loss are **unwarned**. Distinct keys colliding after rounding diagnose and omit that property animation. Unsupported numeric modes fail closed per property. | Structural: `premiere_frame_ticks_round_to_nearest_signed_millisecond`, `native_key_times_colliding_after_millisecond_rounding_warn_and_keep_clip`. |
| Bezier into a Hold-start key | Partial | Partial | Premiere ignores that scalar key's stored in-handle; accepted export curves must arrive with zero-length handle, be straight, Linear, or Hold. | Other curve is diagnosed: Motion/Opacity/graphic property keeps static value; effect omitted; Linear Wipe export stops. Point keys retain stored handle but are unprobed. | Structural: `a_bezier_curve_into_a_key_that_starts_a_hold_is_reported_and_the_static_value_exports`. Adobe readback, not general render proof. |
| Motion Crop | Partial | Partial | Static left/top/right/bottom becomes canonical editable mask guide; export recognizes that exact guide. Static `StartKeyframe` owns authored Crop values; stale `CurrentValue` caches are ignored. | Edge feather is diagnosed approximation; Crop + animated Motion drops Crop but keeps Motion; Crop + Linear Wipe drops both mask representations. Noncanonical export masks are diagnosed and not exported. | Structural: `static_crop_uses_source_dimensions_and_follows_motion_across_aspect_ratios`, `crop_feather_approximations_are_reported_without_rejecting_negative_native_values`, `native_static_crop_uses_authored_start_instead_of_stale_current_value`; [native source provenance](../../crates/premiere_file/tests/fixtures/cap2-native-import.md). Adobe: media-fit/crop structural case; combination render unverified. |
| Cardinal Linear Wipe | Supported | Supported | Angles 0/90/180/270, completion, feather, source handles; completion keys use scalar rules. | Unsupported active angle/wipe omits occurrence. Native transition topology is not recreated. | Structural: `adobe_linear_wipe_preserves_editable_overlap_mask_and_source_handles`. Adobe: scored Linear Wipe gate. |
| Effect stack order/bypass | Supported for mapped effects | Supported for mapped effects | Standard effects preserve current order; Premiere `Bypass` ↔ FX `enabled`. | Native component IDs/versions/UI state are **unwarned lost**. Unsupported effects are separately diagnosed. Ordering relative to arbitrary intrinsic chains is not proven. | Structural: `adobe_derived_stack_imports_order_and_bypass_and_reports_the_unknown_effect`, `edited_stack_order_and_bypass_survive_export_and_reimport`. Adobe export of general stacks unverified. |
| Static Gaussian Blur Legacy | Partial | Partial | `AE.ADBE Gaussian Blur 2`; Blurriness 0–30000 exact in data, Repeat Edge Pixels, both dimensions only. | FX renderer caps visible blur at 300 and kernel differs from Premiere: unbounded visual difference, **no conversion warning for renderer/kernel difference**. `layerSize` has no native counterpart and is diagnosed on export. | Structural: blur mapper tests. Adobe: static blur case is diagnostic mismatch, not a gate. |
| Keyed Gaussian Blurriness | Supported | Supported | Linear/Hold/Bezier `effectProperty`; source-clock keys, including off-trim; first key supplies pre-key value; export writes first key as `StartKeyframe`. | Invalid flag/range/key form diagnoses and omits blur, not clip. Times round ≤0.5 ms. Renderer cap/kernel limitation remains **unwarned**. | Structural: `adobe_keyed_blurs_import_as_editable_blurriness_tracks`, `edited_keyed_blurs_survive_export_and_reimport`. Adobe: Premiere reopen + AME edited export evidence. |
| Other blur variants | Unsupported | Unsupported | Film Impact `AE.Impact_Blur_FX` outside its uniform 26.5.1 and 26.2 subsets (crate README), single-axis dimensions, keyed Repeat Edge Pixels/dimensions, unexpected layouts/children are unmapped. | Diagnosed effect omission; clip and other effects remain. | Structural: format/effect rejection tests. Adobe: none. |
| Static Corner Pin | Supported | Supported | Four normalized source-frame corners, including off-frame; projective warp. | Degenerate/nonconvex quad diagnoses and omits effect. Renderer edge antialiasing differs **without a conversion warning**. | Structural: `adobe_corner_pins_import_as_editable_corner_tracks`. Adobe: fixture/export accepted; visual case remains `structural_only` below score floors. |
| Keyed Corner Pin | Partial | Partial | Straight spatial paths; Linear/Hold and bounded Bezier temporal easing; x/y tracks pair by time/easing. One-axis export supplies static other coordinate. | Curved paths, unpairable tracks, or quad not provably convex are diagnosed and effect omitted; conservative rule can omit a valid animation. | Structural: `edited_corner_pins_survive_export_and_reimport`. Adobe: Premiere reads 16 edited keys and AME renders edit. |
| Geometry2 centered uniform zoom | Partial | As Corner Pin | Positive uniform Scale Height with original keys/easing, centered Anchor/Position, full Opacity, zero Skew/Skew Axis/Rotation/Shutter Angle, composition shutter selected and bilinear Sampling; renders over an adjustment composite through Corner Pin. | Other forms are diagnosed effect omissions. Native effect identity changes to Corner Pin on export. | Structural: `native_geometry2_adjustment_zoom_keeps_editable_corner_keys`, `native_geometry2_rejects_nonpositive_scale_keys_and_keeps_geometry_marker_policy`. Native-derived provenance: `cap2-native-import.md`; focused reference frame comparison is external diagnostic evidence. |
| Crop/Wipe with standard effects | Partial | Partial | Crop or supported wipe geometry survives. | Other standard effects are diagnosed and omitted because arbitrary mask/effect order is not representable. | Structural: reader order regressions. Adobe render order unverified. |
| Active keying/Radial Wipe/masked effect | Unsupported | Unsupported | Coverage/transparency-changing effects are not flattened to opaque clips. | Active occurrence is diagnosed and omitted whole. Bypassed effect loses settings but clip remains. Name matching has documented localization gaps. | Structural: real-record reader tests described in ledger. Adobe: none. |
| Unknown active standard effect | Unsupported | Unsupported | No generic effect translation. | Diagnosed effect omission; clip and mapped siblings remain. Visual error unbounded. | Structural: Tint fixture/test. Adobe: Tint diagnostic mismatch. |
| Unknown bypassed effect | Unsupported | Unsupported | Premiere picture is unchanged while bypassed. | Diagnosed effect omission; settings/re-enable affordance lost. | Structural: bypass tests. Adobe: none. |
| Other FX effects/shaders | — | Unsupported | Only Gaussian Blur and Corner Pin mappings above. | Each unmapped effect or unmapped animated parameter is diagnosed and omitted; animation is not silently flattened. | Structural: hand-written mapper tests in `convert::effects::tests`. |

## Type-tool text, fonts, and captions

Source: [text/font implementation](../../crates/premiere_file/README.md#fonts),
[text ledger rows](../../apps/tesseract-conv/README.md#conversion-accuracy-and-limitations), and
[text/caption public tests](../../crates/premiere_file/tests/conversion/).

| Property or operation | Native → `.tsrct` | `.tsrct` → native | Exact supported values/keyframes | Unsupported/loss handling | Evidence |
| --- | --- | --- | --- | --- | --- |
| Single text object | Partial | Partial | Premiere 26 Type-tool graphic with a uniform text document ↔ root FX text or a graphic group. | Inline mixed styles, pre-26 encoding or malformed payload omit the graphic/layer. Complete-line styles have the bounded mapping below. | Structural: `adobe_native_point_text_stays_editable_without_media`, `single_style_text_survives_premiere_export_and_reimport`. Adobe: scored static point-text gate. |
| Wording | Supported | Supported | CR and CRLF paragraph breaks normalize to LF, including a CRLF split across style runs; empty and trailing lines and Unicode are retained. Uniform Source Text keys map to held field tracks on the generator clock. | Keyed mixed-style Source Text and unsupported keyed fields omit the graphic with diagnosis. | Structural: native reader and text payload tests. Adobe: static point-text gate; keyed semantics are structural. |
| Font identity | Partial | Partial | Import keeps PostScript name as family with empty style. Export uses stored PostScript name or packaged registry match by family/style, typographic, or selection name. | Import diagnoses each font once as unpackaged. Export omits text if identity is unusable/unpackaged; variable instance without PostScript name is not guessed. | Structural: `every_imported_font_is_reported_once_as_unpackaged`, `packaged_faces_name_text_by_family_typographic_or_selection_name`. Adobe: pinned Arial point-text. |
| Size/fill | Supported | Partial | Opaque fill and size; export colors round to 8-bit, numeric fields to f32. | Quantization is **unwarned**. Disabled fill or translucent paint omits layer/cue as documented. | Structural: payload/export tests. Adobe: point text proves one white fill/size only. |
| All caps/tracking | Supported | Supported | Static values per uniform document or complete styled line; supported uniform Source Text fields can be held keys. | Caption cues require defaults and omit otherwise. Small caps unsupported. | Structural: text payload, native whole-line and caption attribute tests. Adobe: arbitrary values unverified. |
| Leading | Partial | Partial | Native automatic or explicit leading imports as explicit FX spacing `1.2 × size + native leading`; Source Text size/leading keys keep spacing held with the document. Mixed complete-line blocks accept native automatic leading only and advance by the incoming line size. | f32 narrowing is **unwarned**; spacing below the supported 0.8 em bound omits text. Mixed-style explicit leading is diagnosed. No universal visual bound. | Structural: native fragments, empty-line/local-transform and held-key export/readback tests. Independent native frame comparisons are diagnostic, not a new scored gate. |
| Justification | Partial | Partial | Left/center/right/one justified mode. | Other full-justify modes omit graphic/layer. | Structural only. |
| Point text | Supported | Supported | Top/center/bottom document alignment around the native origin; local anchor adjustment preserves source rotation/scale, with empty lines counted and single-line controls unchanged. Static equivalent baselines export top-aligned; coherent held alignment tracks restore native keyed alignment. | An arbitrary or non-held keyed pivot cannot be silently discarded and omits the text with diagnosis. Other listed unsupported features retain their existing handling. | Structural: `native_point_titles_keep_the_centered_baselines_and_explicit_auto_leading`, local-transform and held-key export/readback regressions. Adobe: scored `premiere_isolated_text_point`; [native fragments](../../crates/premiere_file/tests/fixtures/point_text_lines/README.md) bind additional diagnostic frame evidence only. |
| Complete-line mixed styles | Partial | Partial | Static point text with complete-line style runs, automatic leading and left/center/right alignment imports as editable uniform Text children under one common transform/opacity owner. Export writes independent native Text objects and keeps line edits/styles. | Inline mixed styles, mixed-style box text, full justification and keyed mixed topology omit the graphic with diagnosis. Existing common-shadow guards require all lines filled and the whole owner unscaled/unrotated. Background remains a diagnosed feature omission. | Structural: unchanged native opening-title fragments, editable child mutation and export/write/readback, split-CRLF and shadow guard regressions. Independent Adobe frame comparison is diagnostic; general rich-text and font fallback parity are not certified. |
| Box text | Partial | Partial | Bounded box and top/center/bottom vertical alignment. | Authored first baseline omits export layer; import evidence uses generated payloads rendered by Premiere, not a pinned Premiere-authored box. First-line/line-slot differences are **unwarned** and unbounded generally. | Structural text tests; Adobe visual equality unverified. |
| Outside stroke | Partial | Partial | Native outside width `w` ↔ centered FX stroke `2w` under fill. | Corner joins may differ **without warning**; outline-only/stroke-over-fill/translucent cases omit. | Structural; Premiere measurement only, no general side-by-side pass. |
| Static text transform | Supported | Supported | Position, anchor, uniform scale, rotation, opacity; static Vector Motion composes into text. | Nonuniform scale and unsupported graphic parameters omit occurrence/property as specified. Numeric narrowing is **unwarned**. | Structural: graphic mapper tests. Adobe: point-text transform only. |
| Text-object keys | Partial | Partial | Position, paired Scale, Rotation, Opacity; Linear/Hold. Bezier is measured for Text Scale/Opacity, not Position/Text Rotation. Off-trim keys retained on generator clock. | Bezier Position/Text Rotation or bent in-handle after Linear/Hold omits graphic/property with diagnosis. Key times round ≤0.5 ms. | Structural: `adobe_graphic_transform_keys_stay_editable_in_both_directions`. Adobe: Premiere readback/render recorded, case remains `structural_only`. |
| Keyed Vector Motion | Partial | Partial | One-text FX group ↔ native Vector Motion; Position/Scale/Rotation keys; Bezier measured for Scale/Rotation, not Position. | Unsupported group fields omit graphic; group name/description are **unwarned lost**. | Structural: same graphic-transform test. Adobe: edited export reopen/render. |
| Graphic clip Opacity | Partial | Partial | Native intrinsic clip Opacity ↔ one-text group's opacity; Normal `(18,0)`, Linear/Hold/Bezier on generator clock. Text's own opacity remains separate. | Other blend pair or invalid chain omits graphic. Combined Opacity+Vector-Motion chain order is inferred. | Structural: `adobe_graphic_clip_opacity_keys_stay_editable_in_both_directions`. Adobe: saved fixture and edited export readback/render; not pinned score gate. |
| Static shadow | Partial | Partial | One enabled normal-blend DropShadow on filled, unscaled, unrotated text. Angle/distance/blur/size/opacity mapping uses AME calibration. | Color/numeric narrowing is **unwarned**. Nonblack parity and some defaults are inferred; renderer blur/spread differs without conversion warning. | Structural: `adobe_scaffold_text_shadow_and_stroke_stay_editable_in_both_directions`. Adobe: seven calibrations; fixture remains diagnostic `structural_only`. |
| Unsupported shadow combinations | Unsupported feature | Unsupported feature | Scaled/rotated/unfilled text; Scale/Rotation keys; rotating/scaling Vector Motion; multiple, disabled, animated, nonnormal shadow; invalid ranges. | Diagnosed **shadow-only omission**; text stays. | Structural: `convert::text_shadow::tests`. Adobe: none. |
| Other text effects | Unsupported | Unsupported | No inner shadow, satin, bevel, glow, gradient overlay, or readable generic stroke effect mapping. | Diagnosed per-effect omission; text remains where otherwise valid. | Structural text-effect omission tests. |
| Masks/track matte on text | Unsupported | Unsupported | No native graphic representation written. | Export diagnoses and omits text occurrence, keeping siblings; never exports visibly unmasked text. | Structural: root/group mask and matte tests in `convert::graphic::tests`. Adobe: none. |
| Background/responsive/path/animators | Unsupported | Unsupported | Text background, responsive design, path/anchor options, text animators, font variations, underline/strike/baseline shift are outside subset. | Import/export omits graphic/layer or named feature according to validation; no silent static replay. | Structural: graphic validation tests. Adobe: none. |
| Caption cue import | Partial | — | Single-style wording, PostScript font, size, fill, stroke, shadow; one layer per cue, preserving overlaps/gaps and track order. | Unsupported cue omitted; caption content never fails otherwise convertible sequence. On a canvas other than 1920×1080 cues keep their stored size, which Premiere scales with the frame by an unmeasured rule; the sequence reports one approximation. | Structural: `pinned_captions_stay_timed_editable_text_through_edit_and_graphic_export`, `pinned_caption_fixture_reads_three_cues_on_two_tracks`. |
| Caption default placement | Partial | — | Centered bottom-aligned box; last-line baseline at 95% frame height, earlier lines at 1.2× size; 80% frame-width box. | Wrap width and higher-track paint order are unverified; renderer baseline offset remains, **without conversion warning**. | Adobe calibration and diagnostic caption comparison in inventory; case is `structural_only`. |
| Caption styling/structure outside subset | Unsupported | — | No background, disabled fill, all caps, tracking/leading, alternate alignment/justification, text box, differing cue/template style, several blocks, effects, retime/speed. | Diagnosed cue omission. Repeated/dangling/overlapping malformed cues and malformed/duplicate tracks are omitted at cue/track scope. | Structural: `caption_styling_beyond_font_size_fill_stroke_and_shadow_is_omitted_by_attribute`, `unsupported_caption_structure_is_omitted_without_failing_the_sequence`. Adobe: none. |
| Hidden caption track/cue | Partial | — | Hidden track or disabled cue imports as hidden text layer. | Reading is inferred; track-level identity is lost. | Structural: `hidden_caption_tracks_and_disabled_cues_import_as_hidden_text_layers`. Adobe: none. |
| Caption export | — | Partial | Caption-derived text exports through Type-tool box graphics; hidden layer exports Clip Enable off. | Native caption track, language, cue/template identity are **unwarned lost**; reimport sees graphics. | Structural: `native_caption_cues_survive_tesseract_and_back_as_graphics`, public caption test. Adobe reopen unverified. |

## Audio

Source: [audio rules](../../crates/premiere_file/README.md#audio),
[audio accuracy rows](../../apps/tesseract-conv/README.md#conversion-accuracy-and-limitations), and
[public audio tests](../../crates/premiere_file/tests/conversion/audio.rs).

| Property or operation | Native → `.tsrct` | `.tsrct` → native | Exact supported values/keyframes | Preserved, approximated, or lost semantics | Evidence |
| --- | --- | --- | --- | --- | --- |
| Audio media | Partial | Partial | Mono/stereo WAV, MP3, M4A/AAC, or at most one AAC stream in supported MP4/MOV; inspected layout/rate/duration must match. | Other layouts/channel remaps omit placement; malformed media rejects per direction. | Structural: `adobe_audio_clips_keep_placement_trim_level_and_channels`, audio-media tests. Adobe: audio comparisons are not video score gates. |
| Linked A/V | Supported | Partial | Import creates silent picture plus one sound layer on same asset, avoiding double playback. Export writes separate unlinked picture/sound placements. | Native linkage is **unwarned lost**. | Structural: `adobe_linked_av_source_plays_its_sound_once`. Adobe: linked render had identical AAC and frames in recorded case. |
| Linked composition sound | Unsupported | Unsupported | Premiere plays a linked composition's sound only through its audio items, so its imported picture is muted and nothing plays it twice. | Each audio item of linked media is a diagnosed occurrence omission (linked-audio occurrence import is not implemented). | Structural: `a_linked_composition_s_sound_item_is_omitted_and_its_picture_imports_muted`, `equal_footage_ids_of_two_aeps_package_distinct_muted_assets_and_linked_sound_is_omitted`. Adobe: none. |
| Static source/clip/track/master level and mute | Partial | Partial | Import multiplies all static stages into one linear layer gain. Export writes clip Volume, leaving track fader unity. | Stage ownership is **unwarned lost**. Unsupported automation at source/track/master is diagnosed and dropped. | Structural: `adobe_clip_volume_levels_import_as_layer_volumes`. Adobe: static segments measured against AME. |
| Clip Gain | Supported | Supported | Native `AudioClip/Gain` multiplies imported gain. Export uses clip Volume through +15 dB and Clip Gain for remaining boost. | Native stage arrangement is not restored; resulting gain is retained. | Structural: audio mapper/public tests. Adobe: +20 dB export rendered within 0.001 dB in ledger evidence. |
| Clip Volume layouts | Supported | Supported | Modern `[Mute, Level]` unity `0.177827939391`; legacy `[Bypass, Level]` unity `0.5`; missing Level value means layout maximum. | Unsupported/bypassed/invalid Volume is diagnosed and omitted while sound remains. | Structural: `legacy_clip_volume_imports_with_its_own_scale`, `adobe_clip_volume_levels_import_as_layer_volumes`. Adobe: modern and legacy AME measurements. |
| Clip Volume Linear keys | Partial | Partial | Source-clock Level keys become editable `AudioVolume` keys. Native fader-position curve is fitted to cubic easing; key values exact. Recognized imported fit can export back as one native Linear segment. | Import curve approximation is deliberate and **does not produce a user-facing warning**; stated error bounds are in ledger. | Structural: `keyed_clip_volume_becomes_editable_volume_keys`, `adobe_clip_volume_keys_stay_editable_through_export_and_reimport`. Adobe: AME audio comparisons, not scored. |
| Clip Volume Hold keys | Supported | Supported | Hold stays Hold; keys before In/after Out retained. | Mixer samples at 100 Hz, so observed step may land within 5 ms; this playback limitation has no conversion warning. | Structural: same keyed-volume tests. Adobe: measured Hold about 4.9 ms early in historical comparison. |
| Other FX volume curves | Partial | Partial | Export subdivides to native Linear pieces on 1 ms grid until sampled error ≤0.25 dB above −60 dB or piece is 1 ms; max 4096 keys. | Curve subdivision is a **diagnosed approximation** once per track only when a 1 ms piece remains; ordinary bounded subdivision is not described as a loss warning. >4096/nonfinite/negative omits animation, keeping static sound. | Structural: `volume_keys_export_as_clip_volume_keys_on_the_source_clock`, `volume_keys_beyond_the_reader_limit_are_reported_without_losing_siblings`. Adobe: revised exports measured in ledger; not a score gate. |
| Channel Volume | Partial | Partial | Unity on every channel accepted. | Nonunity/keyed Channel Volume is diagnosed and dropped; clip plays without it. | Structural fail-closed audio tests. Adobe: one diagnostic measured the resulting level difference. |
| Pan/solo/insert effects | Unsupported | Unsupported | No editable mapping. | Diagnosed and dropped while otherwise valid sound remains. | Structural audio reader/mapper tests. Adobe: none. |
| Mono pan law | Partial | Partial | Premiere places mono on a centered stereo track at 1/√2 (−3.0103 dB). Import folds that factor into the effective static and Level-key gain of direct mono media with matching, unremapped channels, one static centered stereo Balance and the default stereo master/inlet route. Export divides effective mono gain by the factor before the Level/Clip Gain split and writes that route, so FX mono unity writes native +3.0103 dB. A nested sequence's stereo mix is not normalized again. | Any other route (unknown, automated or noncenter pan; other track output, master or inlet routing) is diagnosed (`mono centered-stereo gain not normalized`) and imports without the factor; Premiere's level on that route is unmeasured. Pan itself is still unsupported. Export fit recognition omits the factor, so an imported mono fade from or to silence can become bounded Linear pieces (for example four keys instead of two). | Structural: `mono_mix_normalization_requires_the_measured_static_center_and_route`, `mono_mix_requires_a_track_uid_in_the_stereo_inlet`, `mono_static_and_keyed_effective_gain_write_reciprocal_native_levels`, `a_nested_mono_source_is_normalized_once_before_the_stereo_nest_gain`, `adobe_clip_volume_levels_import_as_layer_volumes`. Adobe: generated project measured 0.70703 (−3.011 dB); pinned level case confirms. Static gain is measured both ways: a Tesseract render of the level case's import puts its mono fits −0.013/−0.012 dB from AME (+3.0 dB without the factor), stereo fits unchanged; AME renders a generated FX mono unity export at −0.0009 dB, and Premiere opened, saved and reopened it. General audio fidelity still fails (sample count and offset); mono key interpolation, other routes and playback are not validated. |
| Audio timing | Supported | Supported | Import boundaries/source-in round ≤0.5 ms; export keeps boundaries exact milliseconds; no final-frame hold. MP4-family edit segment rounds to a whole sample. | Picture may move up to half a sequence frame relative to its own sound on export; timing precision loss is **unwarned**. | Structural: audio placement tests; Adobe: pinned 1 s offset/0.25 s trim and sample comparisons. |
| Retimed/reversed sound | Unsupported | Unsupported | Audio must play unit-speed forward. | Import omits sound placement. Export diagnoses and drops embedded sound of retimed/reversed video while picture can export. | Structural: `retimed_audible_video_exports_its_picture_without_sound`, native audio retime tests. Adobe: none. |
| Hidden video's embedded sound | Unsupported | Unsupported | Hidden picture itself remains exportable disabled. | Export diagnoses and omits its sound because hidden-layer audio semantics are unverified. | Structural: `hidden_audible_video_exports_its_disabled_picture_without_sound`. Adobe: none. |
| Audio inside groups/nests | Partial | Partial | Import: a stereo nest audio item plays the inner sounds that its In/Out shows, from In for End − Start at unit speed, at each sound's Volume times the item's static stages. They join the nest's group when the item is the only one at that nest's sequence and ranges, with the nest's Enable and no Level keys; otherwise they are sounds of the sequence that holds the item. An item's Linear/Hold Level keys move from its In/Out clock to each sound's. Export: a group's sounds go into its nested sequence, with one default-Volume audio item and a `Link`, as Premiere 26.5.1 saves them. | Held Levels and Hold products are exact; Linear segments keep the fitted fader curve of Clip Volume Linear keys. Diagnosed omission of an item longer than Out − In, retimed, reversed, remapped, mono, or with an unreadable or Mute-keyed Volume or an overflowing gain, and of one sound alone whose Level and the item's both change (one along a Linear segment) or whose keys or gain overflow. The nest grouping of a sound that plays alone is **unwarned lost**. Export omits a group with sound but no picture and reports embedded video sound. | Structural: `adobe_nest_sound_imports_through_its_audio_item_and_exports_linked`, `an_audio_item_that_no_group_carries_plays_its_sound_alone`, `adobe_nest_audio_item_level_keys_play_its_sound_until_its_end`, `a_group_sound_exports_into_its_nest_with_its_volume_keys`. Adobe: `premiere_isolated_images_nests_26_5` records a static nest sound through its item and the reopened linked export ([inventory](../../tests/README.md)); [`premiere_isolated_nest_audio_outer_keys_26_5`](../../crates/premiere_file/tests/fixtures/nest_audio_outer_keys_proof/README.md) is Premiere's save of a Level-keyed item whose AME render gives source facts only (`structural_only`; converter render not compared). A keyed inner sound under a keyed item, several items and hidden or disabled pictures are XML edits only ([ledger](../../apps/tesseract-conv/README.md#conversion-accuracy-and-limitations)). |

## Evidence boundary and failure summary

The machine-readable fixture inventory distinguishes `video_reference` from
`structural_only`; see its [video-reference contract](../../tests/README.md#video-reference-contract)
and [structural regression inventory](../../tests/README.md#structural-regression-inventory).
The included reference/scoring helpers require authorized Asset access and
external Adobe/renderer tools; their Make integration targets require the
enclosing repository checkout. Standalone `make test` neither downloads nor scores
remote references. A passing Rust test, converter self-read, or
`.prproj` → `.tsrct` → `.prproj` round trip is structural evidence only.

A successful conversion can therefore contain:

1. **Diagnosed omissions** (stderr): unsupported occurrences, layers, properties,
   effects, or approximations explicitly recorded by the operation.
2. **Documented but unwarned losses**: precision rounding, native metadata and
   ownership flattening, sequence-rate restoration, some layout normalization,
   and known renderer/application differences called out row-by-row above.
3. **Case-specific verified behavior**: only the exact Adobe fixture/property and
   direction named by the evidence; it does not generalize to arbitrary feature
   combinations, fonts, codecs, alpha, audio, or application UI state.

Keep the native source, review stderr, inspect the editable result, and separately
validate generated native projects in Premiere/AME when that direction matters.

## Measured default Film Impact Pop geometry

Import recognizes the pinned static 43-control `AE.AE_Impact_Pop` profile and
retains incoming-only 30/32-frame entrances as existing editable ScaleX/Y and
PositionX/Y tracks. Sixteen measured normalized knots preserve the overshoot,
undershoot and settled size. Position follows the source center as scale changes,
so an off-image anchor does not cause the title to travel. Static anchor,
rotation, opacity, source media bytes/ranges and playback clocks stay intact.

This geometry approximation covers flat Image/Video hosts with static 2D Motion,
Normal blend, unit playback, no masks/crop/Transform stages and the document
clock. Unfamiliar controls, keyed/private profile payloads, conflicting Motion,
other picture durations, two-sided picture topology and multiple picture Pops
are diagnosed omissions.
A nonoverlapping tail Dissolve may own the same clip's opacity independently.
Native early exposure/motion blur, Fade 25 behavior and a general spring formula
are unproved and explicitly remain approximate; no composition shutter or
opacity law is invented. The invisible initial knot collapses animated geometry
without replacing the static scale.

The native profile excerpt is
`crates/premiere_file/tests/fixtures/film-impact-pop-profile.xml`, from Bonsa
source SHA `ace57eb53250a0c41b6aeff89474753fc5c68fe4c41157e4daf89a703b7bbe55`
transition 1006/component 1607. CPU tests relocate only its parent-clock range,
prove fixed-center geometry and source-clock preservation, and retain existing
Dissolve opacity. Independent native title/video comparisons remain diagnostic
until the corresponding strict feature gate is met.

The same profile also imports a two-sided transition between static point-text
graphics, including one complete-line styled block with editable Text children.
Each half uses its own duration: incoming phase is `30 × (t − cut) / (end − cut)`;
outgoing phase is `30 × (cut − t) / (cut − start)`. The outgoing owner keeps its
half-open range ending at the cut; the incoming owner begins collapsed at that
cut. Disjoint head and tail windows merge into one track per property, with
scale one outside the windows. The actual graphic root owns these tracks,
including when static Vector Motion or clip opacity creates an outer group.
Static native transforms, child editability, opacity keys and generator/owner
clocks are preserved. Half durations must be positive whole sequence frames
with distinct knots after millisecond rounding.

Graphic Pop uses the authored text-block origin composed through the common
transforms. The native default pivot appears to use font ink bounds, which the
source does not provide; authored origin is an explicit approximation (about
3 pixels of vertical difference at the visible peak in the diagnostic reference).
No font-metrics subsystem, ascent constant or engine shutter is inferred from
the plugin controls. Non-Normal blend, enclosing nest clocks, shapes, multiple
independent text objects, box text, backgrounds, keyed text/Vector Motion,
retained shadows, overlapping windows and conflicting geometry owners reject
with contextual diagnostics. A retained shadow's calibrated static-geometry
guard remains authoritative; already omitted shadows are not resurrected.
Both halves and all owner tracks validate before graph publication.

The unchanged [native graphic fragments](../../crates/premiere_file/tests/fixtures/graphic_pop/README.md)
retain placements 421–424 and transitions 425–427 from source SHA
`e80cca0275ee14286e6de71cff289b1624e62e39eebed4e345d83b9e3f85001f`,
sequence `84f4bbb6-d3c8-44e4-87b1-9de28c0d7a37`. Its three transitions have
6/6/7-frame halves. CPU regressions prove common ownership, cut/local-clock
behavior, static Vector Motion, disjoint windows and rejection atomicity. The
existing independent native reference and fitted yellow-title bounds are
diagnostic evidence, not a scored visual pass or fresh Adobe playback/reopen
certification. Import remains a fitted geometry approximation; this extension
does not implement an editable Film Impact plugin exporter.

## Irregular presentation timing — import-only approximation

Nonconstant decode durations that do not match a supported quantized grid, or
nonuniform presentation starts on a selected unit picture, retain an explicit
Irregular physical clock. Sample byte bounds, positive contiguous decode timing,
exact STTS/CTTS coverage, declared physical duration and unique sorted PTS remain
mandatory. The shared parser/player reads CTTS offsets as signed i32, including
legacy files marked v0. Existing exact-CFR/full-edit behavior is retained. New
irregular legacy v0 admission additionally requires selected unit interior ranges,
nonnegative first PTS equal to first-edit origin, unique signed PTS normalized
inside physical decode duration, and an explicit legacy-clock diagnostic.
Malformed versions/counts, duplicate or out-of-bounds PTS still reject; whole-file
irregular legacy v0 remains unsupported.

An unlisted native source frame duration must remain positive and satisfy
`sample_count * native_frame_ticks == intrinsic_duration`. Whole-source admission
keeps the full-source edit and rounded endpoint rules. Selected unit picture
intervals have a bounded alternative: after clipping through unit nests, every
consumed interval must fit inside the first positive rate 1 edit whose media origin
equals first physical PTS, and end before the final physical presentation sample.
Later edits, uncertain final-frame use, reverse/nonunit playback and Time
Remapping reject that alternative. Existing accepted CFR and fractional movie
tails retain their full-source behavior. No nominal FPS or adjusted endpoint is
invented. Original bytes, source ranges and actual PTS remain unchanged, with one
contextual approximation diagnostic. Export stays exact listed Constant-only.

Selected inspection/import grants that context only when raw native inventory
proves physical picture/nest placement coverage in every parsed sequence copy,
retained sound placements and resolved references. Omitted uses or unmatched
flattened nested sound keep strict whole-source admission. Premiere holds every
keyed Motion and Opacity property at its first key before that key, also when
the source In comes earlier and the static StartKeyframe differs. The FX
animator holds the first key the same way, so import keeps the real source keys
and adds no trim key, and export writes them back unchanged.

Selected picture-only use can retain unused audio and nondrawing data streams;
consumed sound retains strict layout/codec/timing admission. Unmirrored quarter
turns require native/container agreement and import for any use, whole source or
selected intervals. Validation uses encoded dimensions; editable
Motion/sourceRect/mask geometry uses displayed dimensions, and the unchanged
decoder supplies rotation. Skew, mirroring and unknown translation reject. Export
of a rotated source is unsupported. Full-range H264/HEVC is bounded to
supported 8-bit 4:2:0 basic SDR with an explicit full-range bitstream signal and
coherent VUI/nclx flags; HDR, 10-bit full range, reserved flags and existing
unsupported codec/color forms still reject.

Offline controls assert preserved archive bytes and editable Video clocks, safe
first-edit trims, unchanged CFR tails/positive CTTS origins, native count and
endpoint bounds, malformed tables/duplicate PTS, audio consumption, range ambiguity,
and orientation agreement. Independent source-bound unchanged-renderer probes
support the admitted range/orientation cases; these are practical decode evidence,
not strict native timing or bidirectional fidelity proof. The licensed Gail
missing-timestamp discriminator remains a separate pending native evidence item.

### Measured neutral Film Impact Stroke profiles

Import retains three exact static white, square, neutral `AE.Impact_Stroke_FX`
profiles on opaque physical video: Size 6 with Prescale 99 or 100, and Size 66
with Prescale 99. The reader checks all 31 native controls, including the observed
Pre Transform UI expansion shape. The measured 66/99 profile also admits a
saved Prescale slider cache of 95 with static Start 99; independent native
geometry at the original Alix/Bella frame follows 99. Neighboring cache values,
other profiles, keys and private payloads still reject. Changed, animated or private controls retain
an effect omission; this is an import-only geometry feature.

The editable outer picture group owns the original clip Motion and Opacity.
Its video child preserves the original asset, muted audio, source range and
intrinsic duration, with authored centered Prescale. Size 6 uses an existing
outside Stroke at six source pixels times the static uniform Motion scale.
Size 66/99 uses a white original-source-bounds rectangle behind the video: this
is a declared approximation of the measured border, not a general Size equation
or an alpha-outline implementation. Group children are in top-to-bottom order,
so the picture precedes the backplate.

Pop geometry and nonoverlapping tail Dissolve opacity continue to target the
explicit outer picture owner. Child active ranges use the clip's local clock;
no playback remap or source-clock scaling is introduced. Nonuniform or keyed
Motion, nesting, masks, crops, other standard effects, frame blending, linked
compositions, still/alpha sources and retimed clips remain outside the subset.
Native antialiasing, outline rasterization and general plug-in semantics remain
unverified; unsupported behavior keeps its diagnostic.
