# Audio conversion: structural regression and proof inventory

Support/limitations: [single ledger](../../../../docs/after-effects-support.md#audio-layer-and-property-animation-completion-checkpoint).
This inventory supplements existing pinned cases; it is not a new independently
Adobe-authored feature revision or a completed native-render proof chain.

## Native source targets

| Case | Native source / SHA-256 / target | Concrete assertion / status |
|---|---|---|
| `audio-native-switch-on` | `media/audioEnabled.aep`; `b6b887df9e2b055f4a2aac128b24f510ff116012e9a0e7cd6929c8dfb467f3d2`; comp 1, layer 14, footage 13 | Editable Audio source ID, 5943 ms intrinsic/source duration, unity gain; fresh export must retain a source-backed audio occurrence instead of losing its audio-only hierarchy |
| `audio-native-switch-off` | Same immutable source; comp 15, layer 27, footage 13 | Editable zero gain and source identity retained independently of the visual switch |
| `aep-media-import-audio-media-controls-c63` (supplementary regression) | `media/import_audio_media_controls.aep`; `36920d6bdc07dbb6292cc305327ace44efbacacb182dc18da3c979439f5473a8`; comp 63 `AUDIO_GAIN_KEYED` | Fresh native import produces AudioVolume keys on the Audio ID; compare every time/value to decoded native source dB and the documented scalar projection; fresh export retains editable keys and an explicit continuous-curve approximation diagnostic |

The first source comes from the pinned py-aep revision recorded in
[media/provenance.json](media/provenance.json); the upstream independent JSON
sidecar is retained. The keyed source's historical authoring/reference identities
are in `aep_authoring_provenance.json`, `aep_feature_cases.json` and
`aep_video_references.json`. Tests preserve native source bytes/FPS. Original
`wav.wav` / `audio-stereo.wav` media and fresh native audio render/readback proof
are not supplied by these assertions. Existing fixture reuse is supplementary,
not new feature-level independent Adobe evidence.

Historical comp-63 RGB reference: Asset `ymghDJX5CgZobjHTHe7o_vid`, SHA-256
`4324a42bb2ddb9670a27bc4df9969c505dd13a77d5fef2fbf74a0d76eb9431b2`.
It was **not freshly downloaded, hash-verified or audio-scored for this change**.
No transfer/upload or Adobe operation ran; no new Asset IDs were allocated.

## Executable coverage

Unless otherwise noted, symbols are under
`export_document::tests::audio` in `src/export_document/tests/audio.rs`.

| Exact test symbol | Direction / exercised contract |
|---|---|
| `audio_native_import_retains_source_duration_and_independent_switches` | Import; unchanged native switch-on/off targets, source/range/gain assertions |
| `audio_native_import_fresh_export_keeps_audio_only_hierarchy` | Native import → fresh export; regression for omitted audio-only Groups and fractional-frame source lifetime. Internal roundtrip only |
| `audio_native_keyed_levels_import_and_fresh_export_retain_editable_gain` | Both; pinned keyed source, typed AudioVolume target and every source key's time/gain, emitted key count and approximation warning |
| `audio_export_explicit_remap_keeps_trim_and_stretch_but_rejects_gain_keys` | Export; explicit FX 1–3 s TimeRemap occurrence, source 0.5–1.5 s: static gain keeps the 2× native stretch. Two Hold gains at the mapping's 1000 ms fixed point are still diagnosed and omitted; supported sibling retained |
| `audio_export_rejects_explicit_remap_source_clock_gain_keys_and_retains_its_sibling` | Export; same remap with Hold gains 0/750 ms on its source clock (runtime switch at parent 1.5 s). Before the guard, export accepted it with the switch at 1.75 s. Now diagnosed and omitted; ordinary Linear sibling keeps the same keys at 1.0/1.75 s |
| `audio_export_continuous_gain_keys_keep_editable_curves_with_approximation` | Export; Linear and Bezier gain, two keys only, native Bezier records. 33 parameter samples per example, ≤0.04 absolute linear-gain error **for these two CPU examples only**; not a general tolerance or audible/native-render proof |
| `audio_export_hidden_layer_keeps_editable_source_with_audio_disabled` | Export; hidden Audio retains source and gain, audio/video switches disabled |
| `audio_export_hidden_group_mutes_descendant_without_losing_gain_keys` | Export; hidden container mutes its audio descendant, not merely the native eye switch |
| `audio_export_static_zero_is_exact_mute_but_gain_keys_can_unmute` | Export; static zero uses audio-off; animated gain can override a zero base |
| `audio_export_past_eof_preserves_audible_prefix_without_stretching` | Export; retain playable source interval at 1×, diagnose omitted editable silent tail rather than dropping the audio layer |
| `audio_export_movie_audio_does_not_enable_its_video_channel` | Export; Audio backed by QuickTime does not expose the source's video |
| `audio_export_clocked_group_keeps_audio_without_visual_bounds` | Export; partial-span audio-only Group survives as a source-backed precomposition |
| `audio_export_source_switch_preserves_trim_and_gain_keys` | Export; finite Hold source changes, both paths and source clocks, gain keys on all reminted occurrences and updated explicit startTime metadata; offset-only audio does not acquire inferred stretch |
| `audio_export_playback_remap_retains_keys_and_diagnoses_conflicting_gain_clock` | Export; four guard-spanning playback keys; incompatible occurrence-owned gain diagnosed, supported sibling retained |
| `audio_export_unsupported_gain_hull_retains_supported_sibling` | Export; negative gain-control hull diagnosed, supported audio sibling survives |
| `export_document::audio::tests::audio_gain_floor_and_continuous_control_hulls_remain_finite_and_ordered` | Export numeric helper; 49 endpoint pairs spanning zero, sub-floor, attenuation and amplification |
| `export_document::audio::tests::audio_gain_rejects_invalid_control_hulls_and_above_floor_excursions` | Export rejection; negative/nonfinite and equal-floor endpoint excursion guards |
| `adapter::export::tests::audio::audio_package_check_write_and_reimport_preserve_edited_gain_and_wave_bytes` | Export/package/import; synthetic four-second 8 kHz PCM WAVE, Check read-only, Write diagnostic parity, exact published bytes, fresh package import after deleting original inputs. **Not original native fixture audio** |

## Execution and missing proof

- Initial six scoped CPU cases: **2 passed / 4 failed** on the pre-fix code;
  failures: native-import hierarchy export, clocked audio Group export, hidden
  audio export, and audio-only native visual switch.
- Intermediate focused `cargo test ... audio_` selection via Makefile CARGO_ENV:
  **22 passed**, including existing import/switch/descriptor tests. Subsequent
  final results are recorded below after execution.
- Review regression for explicit `startTime` + source switching: **failed before
  the metadata fix, passed afterward** using the same targeted test.
- Final `make test-aftereffects-file`: **469 passed, 0 failed** (17 new
  regression tests); doc tests: 0. Existing support-ledger checks passed.
- `make fmt`: **passed** (workspace formatting check).
- Makefile-environment `cargo clippy --locked -p aftereffects_file --all-targets
  -- -D warnings`: **passed**. Validation took approximately 25 seconds with
  the local build cache; no external model eval or Adobe operations.
- New native feature-authoring/readback revision: **not performed**.
- Adobe open/edit inspection of fresh exports: **unrun**.
- Independent full-duration 30fps native render + immutable long-term Asset +
  fresh download/hash verification: **not performed for this change**.
- Waveform/sample timing, stereo/channel, pitch and audible-output comparison:
  **unmeasured**. RGB comparison would not satisfy this requirement.
- Whole opt-in feature-proof corpus and GPU/rendering suites: **unrun**.

Import and export implementation are separately exercised above; neither direction
has complete independent Adobe/audio fidelity proof. Existing registry `UNRUN` or
`unmeasured` statuses for unrelated cases are deliberately not promoted by these
supplementary regressions. No CI or required checks were changed.
