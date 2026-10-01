# AEP test execution checkpoint

Except for the explicitly labeled font-provisioning follow-up below, this is a
historical record, not a fresh run against the current branch or release. The
linked JSON receipts are retained unchanged from the original measurement
checkpoint; their use of “current” or “fresh” refers to that run.

This is **test coverage and measured failures**, not a completed converter or a
fidelity pass. The [single support/limitation ledger](after-effects-support.md)
remains authoritative. [Machine-readable final results](after-effects-evidence/effects-coverage-results.json)
bind cases to native source/composition identity, immutable Asset hashes, explicit
FX/export hashes, exact test symbols, critical samples and measured outcomes.
Intermediate exports, movies, readback dumps and logs are not fixtures or Git data.

## Targeted CC Vignette bidirectional follow-up

A bounded follow-up fixes the omitted `CS Vignette` mapping in both directions
without changing FX runtime/schema. The pinned Adobe26.5x89 catalog sources are
`catalog.aep` composition240
(`7519496eb44f5ebc0eff5c2ad78476afdd738303cfb18449ac4f3596e82070c2`) and
`animated_catalog.aep` composition240
(`71ba020d675ecab5fc3236730b2b6a90a49bab2cb25f779f1e68ccf47c4134a4`),
with unchanged long-term references `PEowe80w7SF0lbGjNSfK_vid`
(`35948982035df7935985c6165b18b81f59572e02cc31355abdf9986164fad3b8`)
and `Ez9KMV8ArdA2IrFk2DjX_vid`
(`7c55a7bcf875e110bb286900f98fe9d6007971cfb3355506b25eb20e68e5a59c`). Source assertions failed first because
the whole effect was omitted, then passed with editable Amount/radius values,
0/1s keys, stable identities, retained owner and explicit Center/Pin Highlights
loss diagnostics. The fixed mapping remains approximate: Amount percent maps to
FX amount, Angle of View maps linearly to radius, and FX feather stays0.35.

A fresh isolated two-case run used converter
`77eba0ff0856004bb509d68edb18e170a97217f75de87d120e4e88f8021a1c4c`, renderer
`7ae0743d8d3e75ecbfd93e4fe25e0414df7387e1c1f66262acd941e30689a66c`
and comparator
`a15a8fa61f9816c28b9090ba21838844f368f232fabc6a188046793a71e17803`, comparing all60 unique 320×180 RGB24 frames per
case at30fps. Static mean/minimum changed from `0.906804/0.906795` to
`0.916242/0.915804`; animated changed from `0.882187/0.873343` to
`0.892980/0.880689`. This is a measured improvement, **not** a fidelity pass:
the centered FX kernel cannot represent animated native Center or Pin Highlights,
RGB does not prove alpha, and no quality threshold is defined. FX → AEP export
now has explicit edited-FX CPU structure coverage: static/linear Amount `80→120`
and Angle `39→54` are read from fresh native records alongside centered Center
`[60,40]` and zero Pin Highlights. Authored FX feather is diagnosed and omitted.
At the original checkpoint this was own-writer/own-reader evidence only; the
following later targeted follow-up adds independent Adobe measurements without
retroactively changing that checkpoint.

### Isolated CC Vignette Adobe proof — limited milestone

Adobe After Effects **26.5x89** independently authored a gray full-frame,
single-effect `.aep` at `effects/vignette_isolated.aep` (source SHA
`eb89097ed3a96220efa2f839363f62499478d637bc0f4a2fe5bcb9c58a286756`,
comp **1**, 320×180, 24fps, 2s). Native controls were read back in Adobe:
Amount `60→120`, Angle `35→55` at 0/1s, Center `[160,90]`, Pin Highlights `0`.
Adobe's independent full-duration **30fps/60-frame MP4** is immutable long-term
Asset `eXXIEDzEVdbzBl1VRiJb_vid`, SHA
`11c6ec25dd6ccbaefe93a543677482f0c01f96ab93c5398ea5dc11f7fd2f5ecd`;
fresh Asset download matched the local SHA. Contact-sheet frames 0/15/30/59
show the gray corner darkening from RGB 181 to 97 by frame 30 (center 207).
Fresh import test `adobe_isolated_vignette_import_preserves_editable_amount_and_radius_keys`
**ran and passed**, asserting editable FX Amount `0.6→1.2`, radius
`35/60→55/60`, exact key times, stable effect ID, and no `JsScript`.

For **matched FX → AEP export scoring**, two further *independent Adobe-authored*
single-feature sources in `effects_coverage/` contain a 120×80 blue shape
matching the explicit FX export input. `vignette_static_only.aep` (SHA
`e6ec8ab7c1c725770ca91459ce81a3c603722e72a210bee2f163eb9757202d5a`,
comp 1) uses two **equal** native keys (Amount 80/80, Angle 39/39) to retain
Adobe's editable parameter definitions while producing visually static media;
its 30fps Asset `OxUGp7lpH1nIEGQGCP7i_vid` is SHA
`caa3e54573801e4e7792a9eae94bab76a89ff23db420c7eb5501824c69ba797e`.
`vignette_animated_only.aep` (SHA
`d51c8f3433a2b470db75cf42fb556fc1fe01e52143fdd6fd2e3aba22fe019c04`,
comp 1) keys Amount 80→120, Angle 39→54; its 30fps Asset
`YD2YeUpoGRF9uK463o5K_vid` is SHA
`8a909508cc1e52812b3e2482cf377235c4b4e267f0d823d87b68f94647b77291`.
Both new videos are 320×180/2s/60 decoded frames, inspected at 0/15/30/59
and **freshly re-downloaded with matching hashes**; the animated native
center changes `[45,90,137]→[37,74,111]` at frame 30. Exact source hash,
comp ID and Asset identity are in `aep_video_references.json`. Fresh-import
assertions `adobe_isolated_vignette_static_coverage_imports_editable_controls`
and `adobe_isolated_vignette_animated_coverage_imports_editable_keys`
**ran and passed**. The historical import-only registry retains its required
`UNRUN`/`unmeasured` fields; the local test execution is reported here.

`make adobe-test case_ids='fx-export-vignette-static
fx-export-vignette-animated'` from the enclosing repository root selected only
these two export cases. On the new matching independent Adobe references, the
scorer measured **static minimum
1.000000000** and **animated minimum 0.945333074** at 1s (animated mean
0.954086868). All 60 frames were sampled at native 320×180 RGB24; the
static rendering matched the reference, while the generated animated AEP's
60 frames were identical at the first-frame image. On a separate generated
full-frame test, Adobe UI read `valueAtTime(0/.5/1)` as Amount `80/100/120`
and Angle `39/46.5/54`, and Adobe's isolated 1s render differed from 0s;
the full sequence nevertheless froze. This is an **export temporal-render
failure**, not a fidelity pass. Adobe separately opened and read the generated
static and animated coverage AEPs: control values/keys matched their explicit
FX inputs. The AEP bytes inspected in the UI (SHA
`f4e83da13807e8850b6eb3f0eeb611cceef5b8115fa258471fb7ab2817759c9f` /
`5d6c9d0b201639ab90694fae9502b1b954b48a1d0ff029fbcd732c37b8a8db6f`)
are **not** the same bytes as the subsequent scored AEPs (SHA
`d713deca5254cd85146ab02000f07d3ee2a593e88d4330133d3ad933c7f9cac8` /
`13ba5c648552ccffd197701c8a718ea43c4c6915847995cb94d584e43550fb54`);
the UI readback is not exact-hash evidence for the score run. Scored AEP native acceptance and 60-frame Adobe render did run.

After independent Adobe evidence exposed the frozen animated render, the
limited export-safety amendment **keeps static Vignette** but diagnoses and
omits each animated Amount/radius target, retaining its authored static base
and other effects' animation. The normal regression
`animated_vignette_export_diagnoses_static_fallback_without_losing_siblings`
ran and passed; all **604 normal After Effects crate tests passed**. The
pre-existing opt-in `effects_native_coverage::vignette_animated` still **fails**
with `missing native animation`; it was not removed, ignored or rewritten to
match the fallback. It remains an explicit unimplemented-animation contract.

The two-case `adobe-test` was rerun **after** the amendment. Both current
AEPs were accepted and rendered by Adobe for 30fps/60 frames; matched RGB
minimums remained static **1.000000000** and animated **0.945333074** at 1s.
The animated output is now a **diagnosed static replacement**, not a silent
frozen-key export or a visual fidelity pass. `adobe-test` exited **2** and both
case statuses remained **FAILURE**: static native-control evidence is absent;
animated additionally fails its retained native-animation assertion. The
unrelated opt-in CPU suite also exited 2 (`suite_failures=1`). The old
checked-in 63-case control inventory is preserved; the two new cases are
explicitly pending exact-hash Adobe control evidence, not fabricated passes.
No threshold was lowered, no missing proof was labeled measured and RGB H.264
does not establish alpha. Full animated FX→AEP support is future work beyond
the user-approved limited static/export-diagnostic milestone.

## Results by direction

| Evidence | Actual result |
|---|---|
| Historical source/composition registration | 600/600 linked; 597 required + 3 supporting. These are import contracts, not 600 individual executed passes. Some historical aggregate tests stop at their first failed assertion. |
| Native references | All 600 registered references verified. This wave adds 55 independent Adobe 30fps MP4s, long-term Assets, freshly downloaded and SHA-verified. |
| Normal CPU suite | 500 passed, 0 failed, 0 ignored |
| Opt-in native feature suite | 784 tests: **702 passed, 82 failed, 0 ignored** |
| Current Effects import CPU | 55 distinct native contracts: **52 passed, 3 failed** |
| Current Effects export CPU | 55 explicit edited-FX cases passed native editable-structure assertions; separate mapping inventory and 10 edge/owner tests passed |
| Independent Adobe export control readback | **50/55 passed**; Hue/Saturation static/animated controls failed; 3 geometry-motion exports were rejected |
| Fresh-import RGB comparisons | **55/55 measured**, no quality threshold/pass claimed |
| Native export vs independent Adobe reference RGB | **52 measured, 3 Adobe-rejected/unscored** |
| Alpha / audio / manual Adobe UI inspection | **Unverified / unverified / not performed** |

“600/600 linked” means each target has concrete test references, not the helper's
`links_checked` evidence state. That state remains2/597 required cases because
other entries explicitly retain pending historical execution/proof declarations.
Published assets and live measurements are recorded separately, not erased by
those declarations.

The 82 opt-in failures comprise the unchanged 74 preexisting failures, 5 newly
connected historical contracts and 3 new Effects import failures. Failure symbols
are retained in the result JSON; no assertions were weakened and no failures were
converted to ignores. The fixture registry deliberately retains historical
external-runner `UNRUN`/visual `unmeasured`; this live result record is separate.
`aep-feature-proof.py check --require structural` therefore still fails honestly.

## Current Effects panel

The new panel exercises **29 mapped FX types**, 29 nondefault static cases and
26 compatible animated cases. Drop Shadow, Shift Channels and Posterize Time are
static effect-control cases; temporal effects also receive intrinsic geometry
motion. Typed values and expected native controls are authored explicitly in
`effects_coverage/cases.json`, independently of the production mapping tables.
`imports.json` independently states the resulting editable FX controls, times and
values; its 320×180 native Shape coordinate plane differs deliberately from the
120×80 authored FX content plane. The eight older cubic/Hold/split-knot cases now
actually consume their committed `.fx.json` and `.expected.json` files.

Three independent Adobe-native sources are pinned: Brightness/Contrast (2
compositions), static controls (28), animated controls (25). The latter two were
split by Adobe's `reduceProject` from an unpublished candidate that exceeded the
reader's then-existing chunk-count safety limit. At that checkpoint the limit
was **not** raised (the later removal is recorded in the support ledger), native source
FPS stayed 24, and every published reference was freshly rendered from the final
split source at 30fps. The discarded candidate and its trial movies stay outside
Git. Source bytes and previously published reference Assets were not overwritten.

### Failures worth fixing next

- **Import:** Brightness/Contrast static native brightness20 imports as0;
  both Hue/Saturation cases lose authored master hue15 (observed0).
- **Export native controls:** Hue/Saturation ignores master H/S/L values and
  loses requested master animation. Four native scripting leaves cannot be keyed;
  the independently authored animated source retains their initial values and
  animates only the three keyable colorize leaves. It is **not an equivalent
  native oracle for full master-control animation**.
- **Export Adobe acceptance:** `pixelMotionBlur-static`,
  `pixelMotionBlur-animated`, and `posterizeTime-static`, each with intrinsic
  rectangle-size motion, produce Adobe missing-data/no-composition errors.
  Their CPU structure tests passing does not establish native acceptance.
- **Export rendered semantics:** the Brightness/Contrast pilot shows the shape
  shifted by −60/−40 and frozen rendered effect animation despite matching native
  key/value/interpolation readback. The render comparison retains that failure.
- Content-bound normalized coordinates, native defaults and omitted controls
  produce real visual differences. Grain's native preview-region lines and Wave
  Warp's unmapped intrinsic speed are recorded, not removed from the oracle.

## RGB measurement policy and limitations

Each comparison uses all **60 unique full-resolution 320×180 RGB frames** over
`[0,2s)`, canonical RGB24 and no downsampling. The comparator additionally emits
an inclusive `t=2s` probe: it repeats the last decoded frame, is validated as such,
and is **excluded** from unique-frame means/minima. Critical samples are 0,
1/30, 1/2, 29/30, 1, 31/30 and 59/30 seconds.

- Import per-case means: **0.636059–0.999997**; worst sampled minimum **0.629035**.
- Export per-case means: **0.562430–0.961011**; worst sampled minimum **0.552474**.
- These are descriptive measurements, **not fidelity passes**. Structural
  failures remain failures even when a largely black frame scores highly.
- Uniform content and the selected control positions insufficiently distinguish
  some Bulge, Gaussian Blur, Ramp, Ripple and Sharpen controls. Mosaic's interior
  and individual Levels controls are also not isolated by these samples. Their
  native control contracts and measured RGB differences exist, but **complete
  feature-discriminating visual proof does not**. Exact notes remain per case.
- The four-frame publication contact sheets do not establish every temporal
  control. Full-frame scoring is separate. RGB MP4 never establishes alpha.
- The panel does not cover every owner/control/interpolation combination or
  implement unsupported controls. Native nested-precomp effects, cubic Point/
  Color, Drop Shadow polar animation and the other ledger limitations remain.

## Historical reproduction procedure

The commands below document how to reproduce the historical evidence; the
font-provisioning follow-up did not rerun the historical Adobe-export comparisons
or establish those export results for a newer build. Run standalone commands from
the conv workspace (`opensource/conv` in the enclosing repository). Scoring
targets live in the enclosing repository's root `Makefile` and require an FX
renderer, authorized Asset access and separate Adobe tooling. Use
`make test` for the checked-in Rust tests, not for Adobe comparisons.

```sh
make test-aftereffects-file
make test-aftereffects-feature-proof                 # expected known failures
make test-aftereffects-feature-proof filter=effects::tests::native_controls
make aep-score-offline-test                          # no Adobe/network/GPU
make aep-score-list
make -C ../.. aep-score-run \
  case_ids="aep-effects-coverage-brightness-contrast-controls-c14" \
  scratch_dir=tmp/aep-score
```

`aep-score-run` builds matching local tools, imports the selected pinned
composition afresh, verifies/stages its immutable reference, renders at30fps and
records atomic per-case reports. It never implicitly sweeps all600 cases.
Timeouts, cancellation, identity failures, native conversion/render failures and
low numeric scores are distinct. Local hash-matched MP4s can be supplied with
`reference="case-id=/absolute/reference.mp4"`; otherwise the long-term Asset is
freshly verified/cached. Reports and media belong under ignored `tmp/`.

## Explicit local fonts for import scoring

The renderer deliberately rejects missing fonts rather than silently substituting
another family. Supply licensed local font files explicitly when the imported
archive does not contain them:

```sh
make -C ../.. aep-score-run case_ids="<registered-text-case-id>" \
  font_args='--font "/path/to/Arial.ttf" --font "/path/to/Helvetica.ttc"'
```

The Python runner also accepts repeated `run --font FILE` arguments. It snapshots
and hashes each file into case-owned scratch, then uses the existing
`tsrct project import-font` command on the freshly imported archive **before**
rendering. It does not change the pinned AEP/reference, rewrite text controls,
search host fonts, or relax strict font resolution. With no `--font`, behavior
is unchanged. Font packaging errors are `font_import_failed`, distinct from AEP
import and render failures; command output and per-font hashes are retained.

Font hashes identify the supplied local bytes, **not** the font versions used by
Adobe to create the reference: those identities remain unverified unless separate
native evidence pins them. Successful provisioning is not a visual fidelity pass.
Fonts are embedded in the scratch `.tsrct`; use only where licensing permits,
and never commit or publish these fonts/archives without redistribution rights.
For parallel investigations, copy executables to an isolated location and pass
explicit `--tsrct`, `--tsrct-conv`, and `--validation` paths to the Python
runner so another build cannot replace a binary during the run.

### Local font-provisioning checkpoint

The follow-up import RGB run retried all **119** missing-font cases (115
`ArialMT/Regular`, 4 `Helvetica/Regular`) with explicit local fonts. All 119
completed fresh import, font packaging, rendering and comparison: **10,710
unique frames**, mean similarity range **0.961895–0.998892**, worst minimum
**0.960076**. Every case's `project.json` was byte-identical to its earlier
unprovisioned import; only archive font assets/metadata were added. These are
measured RGB results, not new editable-feature or export proofs.

The local font SHA-256 identities were:

- Arial.ttf: `525979822591a3447cfc49d943d6f7683508e25543407871c0ed8fed05fd2bd9`
- Helvetica.ttc: `25eceb458d4baf628ee0b6a135a9f8ac5ec7b2826646720834bd2bf00dfc825a`

All 119 used isolated copies of these executables:

- tsrct: `7ae0743d8d3e75ecbfd93e4fe25e0414df7387e1c1f66262acd941e30689a66c`
- tesseract-conv: `3f52b1847c665403b3471a645f0ee39018a7efd98d4e9bc9067ba0887359a683`
- validation_cli: `a15a8fa61f9816c28b9090ba21838844f368f232fabc6a188046793a71e17803`

Relative to the enclosing repository checkout, local raw evidence is retained under
`tmp/aep-font-audit/runs/`; the reconciled
case IDs, scores and result paths are in `tmp/aep-font-audit/final/`. Combining
these 119 with the earlier 481 leaves **600 scored, 0 execution failures**, with
51 mean≤95%, 10 minimum-only≤95%, and 539 above95% on both metrics. The earlier
481 preserve two renderer-build cohorts; this aggregate is **not** a single-
binary 600-case fidelity gate. No source/reference, FX runtime, export support,
CPU assertion status, alpha or audio proof changed.

### Follow-up automated validation

After integrating the converter-tooling relocation, the complete scoring target
passed: **35 runner tests and 15 readback tests**. The missing pytest dependency
was installed in an ignored, isolated virtual environment (pytest9.1.1), not
worked around by skipping readback. The broader `make test-offline` passed
**268 Python tests, 82 subtests, and 22 JavaScript tests**; these overlap the
scoring target and must not be added as disjoint coverage counts.

The standalone converter's `make test` passed **1,541 tests**, with no failures
or ignored tests. `make adobe-test-cpu` executed all 863 opt-in AE tests, including
export assertions: **783 passed, 80 failed, 0 ignored**. The failing modules are
`adapter` (2), `effects` (3), `export_document` (27), and `structure_document` (48).
The exact same 80 failure symbols were observed before and after the upstream
integration. This PR does not change the AE crate's source/tests/fixtures, whose
tree matches the integrated main revision; these failures were not hidden or
weakened. They remain incomplete converter work, not environmental skips.

Root `make check clippy fmt` and converter `make check clippy fmt
check-aep-support-ledger` passed. The relocated runner also completed a fresh
one-case-per-font RGB smoke with the same pinned executables. Export CPU tests
running does **not** mean Adobe opened or rendered freshly exported AEPs; that
independent export verification was not performed by this harness-only change.

## Historical export reproduction procedure

Generate the historical panel's explicit export inputs/oracles/AEPs/FX archives:

```sh
AEP_EFFECTS_COVERAGE_DIR="$PWD/tmp/aep-effects-panel" \
  make test-aftereffects-feature-proof filter=effects_native_coverage
```

For After Effects readback, `scripts/aep-effects-native.jsx` accepts a host
configuration JSON (see the script's environment-variable contract) with
`mode: "inspect"`, absolute `inputs` and
fresh `receipt` paths, and an explicit `cases` array. It requires a newly owned blank
project or an exact known clean `expected_project`; it never overwrites sources.
Only the owned host may be closed. Use fresh config/result paths. A native render
uses the selected export's `<case>` composition (for example,
`gaussianBlur-static`) and Adobe's
`Use this frame rate: 30; Quality: Best; Resolution: Full` render settings, native
frames0–47, preserving the2-second duration. Do not use `aerender -reuse` on an
unknown host. Render into scratch, inspect warnings, then verify60frames at30fps;
exit0 alone is not acceptance. The three rejected cases remain failed.

Validate returned control receipts (explicit cases, no directory sweep).
Only a completely finished, error-free `inspect` receipt from the pinned
Adobe26.5x89/build89 producer is accepted. Partial captures, authoring receipts,
and late project-state failures cannot pass. The final52-case accepted-output
inspection was rerun as one clean completed batch with unchanged input hashes:


```sh
python3 scripts/aep-effects-readback-check.py \
  --inputs tmp/aep-effects-panel --receipt tmp/adobe-readback.json \
  --case gaussianBlur-static --output tmp/native-control-result.json
```

For native-export RGB comparison, use the corresponding immutable Asset from the
case manifest and the newly Adobe-rendered export (not a renderer-generated
baseline):

```sh
target/debug/validation_cli video --left native-reference.mp4 \
  --right fresh-adobe-export.mp4 --sample-interval-secs 0.03333333333333333 \
  --max-dimension 320 --canonical-rgb24 --json
```

The raw inclusive result is not itself a60-frame summary; the checked
`aep_test.evaluate_comparison` routine validates timestamps/indices/critical
samples and removes the duplicate terminal probe. No failed Adobe render receives
a numeric zero score. No new CI lane or merge-enforcement claim is introduced.
