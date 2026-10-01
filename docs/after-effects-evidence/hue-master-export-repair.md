# Hue/Saturation Master: measured static export repair

Converter-only, independent of Brightness/Contrast #4490 and Vignette #4476.
No Layer Styles, schema, evaluator, renderer, generated `JsScript`, baking, or
original-AEP replay. [Support/limitation ledger](../after-effects-support.md#huesaturation-master-values--partial-export-repair).
[Machine-readable before/after results](hue-master-export-results.json).

**The one pinned static Master-only export now passes independent Adobe RGB
comparison: minimum/mean 1.0 over 60 frames, with exact decoded RGB24 equality
both across the canvas and in the colored-patch ROI.** This is not broad
conversion completion: UI values, fresh-import RGB, fractional/mixed controls,
alpha and the full `adobe-test` suite remain unverified, regardless of PR review status.

| Direction | Implementation / executed proof | Missing proof or omission |
|---|---|---|
| Import | Existing decoder unchanged; new pinned source freshly imports editable hue 50/saturation -60/lightness 20/Colorize false, no script | Import RGB unmeasured; two historical Colorize-on expectation tests still fail |
| Export static | Typed Channel Range `aRbp` state plus visible `pard` values; fresh explicit FX export accepted/rendered by Adobe, exact 60-frame native-reference RGB equality | Reopened UI values and fresh output scripted-control readback unrun; only this integer Master-only case measured |
| Export animated | Master/toggle tracks omitted with contextual diagnostics, authored bases retained; numeric Colorize sibling keys retained in CPU tests | Master/toggle animation unsupported; fresh numeric Colorize Adobe verification not rerun |
| Export fractional | Diagnosed nearest-integer Master approximation (ties away from zero), consistent in UI defaults and rendered state | Independent fractional Adobe proof absent; not fractional fidelity |

## Independent native source and reference

- Case `aep-effects-coverage-hue-master-static-adobe-c1`.
- Source `crates/aftereffects_file/tests/fixtures/effects_coverage/hue_master_static_adobe.aep`.
- SHA-256 `e720cfaa8bcebf2fd4ede1ad4116d4e267c3bc37e3a148d5f6c555c2b559be38`, 80,907 bytes.
- Independently authored in Adobe After Effects **26.5x89**, not by our writer.
  Accompanying `hue_master_static_adobe.jsx` preserves the executed authoring
  body, with formatting and destination-path externalization only.
- Composition **ID 1**, `hueMasterStatic`, 320×180, 2 seconds, **24fps unchanged**.
  One centered 120×80 solid, authored RGB (48,100,151). Colorize off;
  Master Hue=50, Saturation=-60, Lightness=20.
- Adobe accepted all four assignments without errors and immediately returned
  those values. Controls 0004–0007 reported `canVaryOverTime=false`; 0008–0010
  reported true. After reopening this Adobe-native source, scripting returned
  zero Master values despite retained native state and rendered color.
  Scripted readback is not a reliable substitute for rendered or UI evidence here.
- Independent `aerender`, Best/Full, **30fps override**, H.264 Match Render
  Settings/40Mbps; source frame indices **0–47 inclusive** give 60 output frames.
- Long-term Asset **`eEeno9ksJPILcDemDHsH_vid`**, MP4 SHA-256
  `c3d34c11c524c0335fbfb69e2b90ce83302af217b9393d2e813828792a0c0a53`.
  13,477 bytes, 60 frames, 30fps, 2 seconds, 320×180, yuv420p/SMPTE170M, no audio.
- Original render-log SHA-256
  `48dc3fdca7cf2b5f80c34b6640684a8a197896bad6dcfd8c6bf56c5b2be5d93e`.
- Full decode validated; immutable publication freshly downloaded and hash/size
  verified. A separate fresh download was staged for this comparison and checked
  again before final scoring. No MP4 committed; source/reference not replaced.
- Native contact sheet frames 0/30/59 inspected: identical lavender patch on
  black, no error/offline slate. Critical output frames registered: 0/15/30/59.
  Reference center RGB (123,113,146), matching the final export at every sample.

## Rejected first hypothesis and corrected serialization

Commit `207c8102a` wrote three signed 16:16 `parT/pard` values, matching visible
native defaults, but omitted the Channel Range arbitrary state. Its fresh AEP
`4cd113b885ae2ac926c3aede9297cc0fcdbcbc975734ab87315a368f30c71c1c` opened and
rendered without Adobe diagnostics. Whole-frame minimum/mean **0.9877521248**
exceeded 95%, yet center RGB **(47,98,150)** remained essentially the original
solid color rather than reference **(123,113,146)**. **That feature result failed.**
The mostly black background hid the error; the threshold was not weakened and
this score was not accepted as a fidelity pass.

The native source also contains:

- a default `aRbp` directly in the Channel Range `parT` run;
- a Channel Range `tdbs` descriptor followed by sibling `LIST aRbs / aRbp` in
  the owner's `tdgp` run;
- 45 signed big-endian integer words: three Master H/S/L values, then six
  default color ranges, each with four hue boundaries and zero H/S/L offsets.

The corrected writer constructs these typed values from **edited FX controls**
and explicit default range semantics. It does not copy source chunks or cache
original project bytes. Global definitions get zero Master defaults; owner
values remain instance-local. UI `pard` values are retained alongside the rendered
state. Fractional Master requests are rounded with a diagnostic because the
observed rendered-state representation is integer; numeric Colorize is unchanged.

A scratch diagnostic AEP established exact RGB equality before implementation;
that was **not** substituted for the final proof. The final proof below uses a
fresh AEP from the real converter after the corrected implementation.

## Final fresh export and comparison

- Explicit, separately specified input `hue_master_static.fx.json`, SHA-256
  `58f9576a5f51993143ad1ee9d99900802869334c697ce642e7651475f5ccc240`.
  This is not an import/export round trip.
- Fresh output AEP SHA-256
  **`c40ee1b6bcea5fcf0f6c6db6e2260d1bf7ddad7a09baa2239b11046e1ab2dcfb`**.
- Adobe rendered **320×180, 30fps, 60 frames, 2 seconds**, exit 0, no warning/error
  diagnostics, AEP hash unchanged during rendering.
- Output MP4 SHA-256
  `1610134560f0b739e59854630af803a26837da7934303347d25b6ac20d4f7473`.
- Render-log SHA-256
  `4a78d4b125ecc7635c94388b9fa3225e20356526a4f6289a16395a69603b08ac`.
- `validation_cli video --canonical-rgb24 --sample-interval-secs 0.03333333333333333
  --max-dimension 320 --max-samples 61 --json`, evaluated on the 60 unique frames
  in **[0,2s)** (the inclusive endpoint is reported separately by the helper):
  **minimum 1.0, mean 1.0**, satisfying the unchanged minimum **>0.95** gate.
- Separately decoded every frame to RGB24 with FFmpeg: **10,368,000 bytes exactly
  identical**, SHA-256 `92329ad2c621e7cfaac4de8931d376cb9e73a0db14d2d02aa29152a38f84b2a1`.
- Patch ROI **x=100, y=50, width=120, height=80**, across all 60 frames:
  **byte-identical**, SHA-256 `c2535c8ab604c31858f439c070025c16c50d061e90d6949c43944fbe28c6cf95`.
  This eliminates background dilution; the patch is not blank or unchanged.
- Native/default control pair remains unrun; the failed converter output is a
  negative regression sample, not a substituted independently authored default oracle.

The initial ownership block was later explicitly cleared by the user, including
permission for the subsequently started GUI host. Each render checked for
unapproved hosts and used a separate `aerender` **without `-reuse`**. Existing
GUI projects were not opened, modified, closed or terminated.

## Local regression contracts and review

Executed after the correction:

- `make -C opensource/conv test-aftereffects-feature-proof filter=hue_master`:
  **6 passed**. Exact symbols:
  - `writer::effects::tests::hue_master_values_are_encoded_in_the_native_parameter_table`:
    now also compares complete Channel Range descriptor/state against the
    independent Adobe source. This stronger assertion **failed before the
    correction** (missing Channel Range), then passed. It also asserts default
    state for a second owner and unchanged canonical global definitions.
  - `writer::effects::tests::hue_master_fixed_preserves_signed_fractions_and_rejects_overflow`:
    numeric ABI boundaries (not fractional rendering proof).
  - `effects::tests::native_controls::adobe_authored_hue_master_static_imports_editable_values`.
  - `export_document::tests::effects_native_coverage::explicit_hue_master_static_exports_editable_aep`.
  - `export_document::tests::effects_native_coverage::hue_master_out_of_range_retains_convertible_siblings`.
  - `export_document::tests::effects_native_coverage::hue_master_fractional_values_have_diagnosed_integer_state`.
- `make -C opensource/conv test-aftereffects-feature-proof filter=export_document::tests::effects_native_coverage::hue_saturation`:
  **2 passed** (`hue_saturation_static`, `hue_saturation_animated`). Checks retained
  bases, contextual omission diagnostics and numeric Colorize sibling keys.
  Historical requested-control oracles remain unchanged, not reclassified as passes.
- `make -C opensource/conv check clippy fmt check-aep-support-ledger`: **passed**.
- Registry/manifest/source-hash/test-anchor validation and `git diff --check`: passed.

The JSON feature registry permits **import only**. Export is separately tracked
by `export_case("hueMasterStatic")`, its explicit FX input, direction table and
measured result above. Reference publication is never called an export result.
Independent focused reviews cover the serialization/animation boundaries;
source-only structural evidence is not labeled Adobe UI verification.

## Animated expected-oracle binding correction (CPU-only)

A later bounded export run reached fresh Rust success for
`fx-export-hueSaturation-animated` but stopped **before Adobe** with
`ExportAdapterError: hueSaturation-animated: expected oracle differs from cases.json`.
This was a harness contract mismatch, not an Adobe load failure or evidence of an
exporter regression. The generated artifact intentionally contains only controls
0008–0010: the three numeric Colorize leaves keyed by the independent animated
source. The case registry still requests all seven animated FX controls, while the
fresh Rust case separately asserts retained bases and contextual omission
diagnostics for non-keyable Master H/S/L and the Colorize toggle.

The adapter now binds that exact three-control artifact to its reviewed native
scope and records the complete seven-control request separately in report identity.
It rejects any other requested-control shape or artifact subset. Targeted offline
regression `test_hue_animated_binds_the_differentiated_native_oracle` failed before
the repair and passed afterward. No fixture/oracle was rewritten, no assertion or
threshold was weakened, and no Adobe render, control readback, scoring or upload was
run. The independent source remains proof only for its three keyable Colorize
leaves, not for Master/toggle animation.

## Remaining limitations

Two unchanged opt-in import tests `native_hue_saturation_static` and
`native_hue_saturation_animated` still fail expected Master Hue15 vs native0 on
older Colorize-on sources. Their importer, source fixtures and expectations were
not altered to make them green. Master/toggle animation, fractional fidelity,
individual ranges, mixed Master/Colorize, alpha/audio and reopened UI inspection
remain unsupported/unverified as described in the ledger. No fresh-import RGB,
full `adobe-test`, 55-case aggregate or GPU suite was run. Other PRs' evidence is
not borrowed; #4490 was open and #4476 merged at the prior status check, and this
branch is not stacked on either. Full bidirectional completion is not claimed.
