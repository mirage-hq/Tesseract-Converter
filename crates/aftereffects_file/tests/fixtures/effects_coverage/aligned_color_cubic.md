# Aligned shared cubic Effect COLOR (control-only checkpoint)

Native source `aligned_color_cubic_native.aep`, SHA256
`cd84c6fc05ab2c5f960c0660fcc367d18ad013089cc987596e29c185c60e1c2e`,
composition 1 (`W09 aligned COLOR cubic native oracle`), 32×32 square pixels,
24fps, 1.25 seconds. Managed AE26.5x89 job
`a74a9362e51946ed96e6f8639d69193a` independently authored Tint on a gray solid.
The saved source is the **edited** red endpoint .9. Its author JSX and provenance
retain separate original .8 and edited .9 readback phases; neither oracle was
replaced with converter output. No external media, fonts or private source assets.

Feature × direction:
- Import: `effects::tests::native_controls::aligned_color_cubic_independent_native_import_preserves_edited_rgb`
  freshly parses/imports the native source and asserts three editable RGB targets,
  original 0/1000ms knots, edited endpoint and normalized shared cubic controls.
  Import implementation is unchanged; queue1583 passed (no ignores).
- Export: `export_document::tests::effects_native_coverage::aligned_color_cubic_full_export_retains_keys_and_endpoint_edit`
  uses the two explicit `aligned_color_cubic_{original,edited}.fx.json` inputs,
  320×180, 24fps, 2 seconds. No source AEP is used to construct these FX inputs.
  Queue1558 failed because COLOR keys were missing; queue1583 passed after the fix.
  Own-reader assertions are supplementary.
- Fresh native acceptance: build1591 at commit0b1cb905f freshly converted both
  input archives. Managed job `1c9d606a100a41498c1371602a06b6a8` opened the two
  generated exports and the unchanged independent native source, inspecting native
  COLOR getters without rewriting keys. `aligned_color_cubic_acceptance.json`
  pins input FX/archive/generated AEP hashes, build receipt and native observations.
  Its JSX uses only private managed snapshots. Original/edited red curves, shared
  native 255-scaled RGB vector speeds/influence25%, two native keys at0/1sec and
  untouched green/blue/default alpha0/white/amount pass. Twenty-one samples at
  0,.125,.25,.5,.75,.875,1 sec match independently computed cubic(.25,.1,.75,.9),
  worst RGB error2.3841860041784457e-8 versus5e-7 tolerance. The edited export
  matches the independent edited-native control. Unused first incoming/last
  outgoing interpolation are not a segment assertion. Cleanup/fresh READY verified.

Executable offline evidence check (no Adobe launch):

```sh
python3 crates/aftereffects_file/tests/fixtures/effects_coverage/aligned_color_cubic_acceptance_assert.py
```

Negative guards retain authored static COLOR plus diagnostics for unequal knots,
unequal active curves, mixed active interpolation, changing alpha, invalid native
handles/speeds/ticks. No sampling/bake/flatten/JS, generic Color/scalar change or
runtime/schema expansion. Actual49 has no Tint COLOR tracks; this minimal public
profile is not claimed as lost actual49 content.

## Independent reviewer follow-up

`aligned_color_cubic_reviewer_proof.json` records a native key/ease edit on the
byte-identical accepted generated original AEP: red endpoint .8→.9, updated shared
255-scaled vector ease, preserved white/amount/alpha and exact same-host save/reopen.
Keyed EffectColor does not reproduce the static-control hidden-parent failure;
this does not prove the full plugin catalog or fresh-host reopening.

The unchanged independent native source was rendered in Adobe at **30fps output**,
full 32×32 canvas and full native 1.25-second duration. Native output contains
38 frames/1.266667 seconds; the 24fps source bytes/rate are unchanged, with no
FFmpeg retiming or trimming. Full RGB decode and samples before/at/between/after
the 1-second key show the intended nonblank COLOR progression. Reference SHA256
`35809a40e6d0e32fdffd5d1e16985138e1d8a4275a761e9be3f86301dc36c550`.
The video stays in ignored managed storage, never Git.

Reviewer COLOR queue1714 passed37 tests/5 existing ignored. Workspace queue1715
still fails12 baseline tests (1670 passed/411 ignored), not a suite pass.

**Incomplete proof:** long-term Asset publication/fresh hash download and
full-frame converter/reference RGB comparison remain incomplete. Scoped workspace
uploader executables were unavailable; the legacy local-reference-copy helper is
not Asset publication and was not used. Alpha/audio, Tint luma/kernel and
full-project fidelity remain unmeasured. This is native editable-control acceptance
and an inspected independent reference, not a visual pass or a completed formal
feature-proof chain.
