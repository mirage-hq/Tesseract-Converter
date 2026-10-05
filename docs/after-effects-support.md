# After Effects ↔ editable FX: current support and limitation ledger

## Oversized native source working-plane approximation

**FX → AEP only; import unchanged.** Previously, an otherwise valid Group whose
finite all-time geometry exceeded native composition dimensions was omitted with
its whole subtree. Every AEP export (ordinary, picture-only and selected-root)
now retries exactly that overflow with a bounded source working plane after
ordinary certified/collapsed export fails. There is no option or CLI flag.
Representable and exact collapsed sources retain their original output bytes.
No native dimension cap is raised.

Original semantics include offscreen content sampled by blur/motion blur. The
replacement keeps editable native Group/child/effect/animation payloads but uses
the finite output-derived planar inverse demand as the source canvas, with the
existing symmetric 3D camera and source-origin rebasing. When the ordinary path
has validated a native source clock, the retry broadcasts its spatial union over
that same checked source duration; it does not replace offset source time with
zero-based occurrence time or guess a duration. Original native occurrence
endpoints, source intervals and affine/remap values are retained. Known supported input
reach remains additive; unproved Gaussian/Directional/Radial blur reach may be
omitted, never replaced by a guessed radius. Every rescued owner emits a contextual
`AE-EXPORT` warning: offscreen blur, motion-blur and source-sampling boundary
contributions may differ. This is an approximation, not a pixel-equivalence claim.

For a successfully rescued Group only, Radial Blur Center and its independent
X/Y keys are lowered in the existing logical FX composition point domain
(`UV × logical composition dimensions`) and translated by the actual native
source origin, like retained content. Cropped canvas dimensions do not retarget
that point. Existing lowering at the actual output frame rate retains the merged
native Point key times/interpolation. This is a control-coordinate correction,
not additional blur-kernel permission; ordinary sources, other effect controls
and native Zoom/FX sampling-kernel limitations are unchanged.

Targeted CPU regressions
`viewport_approximation_review_checked_offset_clock_retains_late_nested_source`
and `viewport_approximation_review_radial_center_preserves_logical_points_and_keys`
exercise the real staging route: offset 1–3s / nested late 2–3s source content and
native clock endpoints, plus asymmetric rotated/translated content with noncentral
static and independently keyed centers. Raw native point values plus the actual
source origin must equal logical FX points. This is own-reader structural
coverage, not native opening/readback or RGB/alpha proof.

Nonunit/unproved clocks, projective or noninvertible owners, owner masks/mattes/
fills, foreign consumers, unresolved animation dependencies, unsupported input
profiles and Full inherited demand do not gain a fallback. Original geometry,
near-plane and animator validation runs before the overflow retry. Unknown Text
bounds and unrelated errors are not rescued. No shader reconstruction, source
mutation, frame flattening, schema or renderer change is included.

Evidence statuses are separate:

- **Public CPU structure:** regressions under
  `adapter::export::staging::tests::viewport_approximation` exercise standard
  ordinary, picture-only and selected-root retention (failing before the fix),
  unsafe-source rejection, representable/exact-collapse outputs pinned to their
  pre-fix SHA-256, native child/camera retention, checked source time and
  logical Radial Center base/keys. They exercise real archive preparation and
  own-reader native structure, not independent Adobe proof.
- **Private source-derived CLI structure:** a converter built at `f0d4fe01f`
  (same lowering, then behind a flag) completed CLI Write for a two-scene
  source-derived archive in 78.59s. All 13 requested Group/child presence assertions passed;
  own-reader inventory found 252 reachable layers, 34 compositions, three
  approximation diagnostics, no native-canvas overflow and no unresolved
  sources. This is not a full-original run or an independent native fixture;
  private source/media and raw evidence remain outside this public ledger. A
  full-original Write did not finish within a 30-minute limit and produced no
  output.
- **Native acceptance/fidelity: blocked, unmeasured.** An intermediate
  `a938619ec` headless-adobe inspection started but timed out after 600s.
  Owned cleanup succeeded, but READY remained false. No Adobe acceptance,
  editable-control readback or native render/RGB/alpha proof was obtained.
  The Adobe step is stopped; no automatic replay is authorized. Required
  independent Adobe-authored source/reference feature proof remains missing;
  own-reader structural success does not satisfy that gate.

The export fix is implemented with structural evidence only; import is unchanged.
The 47s scope remains skipped; 65s shader content remains unsupported.
Both-direction fidelity and whole-task proof completeness are not established.

## Deterministic Phase 2 — exact bounded Shape Scale/key/helpers

**AEP → FX:** bounded named nonrecursive numeric helpers, immutable native
`numKeys`/`key`/`nearestKey` records and recoverable range probing now feed existing
editable Shape-group ScaleX/Y destinations. Native/FX Scale stays in percent units;
existing observation fitting, budgets and transactional failures are retained.
Unused Shape captures still receive contextual diagnostics. No JsScript or FX,
renderer, persisted schema, dense fallback or duplicate fitter is introduced.

Helper inputs/returns carrying live AE API receivers (including aliases and
conditional references), and arrays carrying those receivers, are rejected at
admission rather than losing receiver/truthiness validation. Native Key records
and numeric/vector helper values remain admitted; ordinary numeric API reads
inside helpers remain available. This bounded profile does not infer helper API
receiver types. Rejected expressions retain authored fallback with contextual
diagnostics, not a manufactured helper result. Dependency `numKeys` reads honor
fatal clock/interpolation errors even inside `try`/`catch`. Production evaluator
regressions cover these guards; no new independent Adobe/render proof is claimed.

Measured gain: **one** pinned Lemon Scale target (composition15368, owner795192752,
VectorGroup1). Fresh20,002-vector error1.7195134205394424e-12; separately serialized
original/matte-copy consumers in two10,001-vector windows:1,438 keys/error
9.550340841074645e-9 and4 keys/error0. Original observations and1e-8 native-unit
criterion are unchanged. Other captured Scale mappings remain unmeasured until
independently sampled; names/census counts are not coverage.
[Exact proof boundaries and test](ae-expression-evaluation.md#phase-2-exact-shape-scalekeyhelper-slice).

Deferrals: native Bezier dependency interpolation, builtin ease/smooth and other
Shape fields retain authored fallback with diagnostics. Case06's WigglePosition
frequency dependency is still rejected; Geometry2 precomp-owner/effect-point
lowering is a separate unfinished gate. No random implementation is added here.
**FX → AEP:** expression authoring/restoration unchanged and unproved; ordinary
numeric-key export does not establish native expression restoration. Rendered,
font and alpha fidelity remain unmeasured. This is the approved exact import-side
increment, not completed bidirectional/native-render feature proof.

## Expression input keys: eased, spatial and separated native properties — import increment

**AEP → FX:** the numeric expression evaluator, including the approximated
random APIs (`wiggle` etc.), now accepts the native keys that AE authors by
default:
- Bezier temporal ease (Easy Ease, speed/influence)
- spatial Auto Bezier Position paths
- separated Position (`xPosition`/`yPosition`/`zPosition`, composed for
  `transform.position`)
- 3D X/Y Rotation and Orientation

Formerly only nonspatial Linear/Hold keys and five Transform properties were
admitted, so wiggle on keyed or separated properties was denied or skipped.

There is no new interpolator. The pre-expression `value`, `valueAtTime` and
dependency reads use the keyed import's existing mapping:
- `easing_for_key` / `straight_spatial_easing_for_key` for per-component FX
  easing;
- `spatial_position::prepare` for spatial Position.

Evaluated samples are then lowered by the unchanged
`fx_keyframe_bake::curve_fit` path.

| Feature | Original semantics → replacement | Reason / impact / evidence |
|---|---|---|
| Bezier temporal ease as expression input | AE speed/influence ease → the same FX cubic easing ordinary keyed import authors, evaluated with the converter's 24-step cubic solver | Keeps expression input equal to what keyed import renders. Max error vs Adobe `valueAtTime(t,true)` over 61 frames: separated X 8.9e-6, X Rotation 2.7e-6, Slider 1.8e-7. |
| Spatial Position path as expression input | AE spatial Bezier path speed → keyed import's adaptive Linear refinement (0.25 source units) | Max measured error 0.205 px (cases c1/c16). Diagnosed per owner (`AE expression input keys approximated`). |
| Spatial tangents on non-Position properties | Tangents → ignored, per-component temporal ease kept | Same as keyed import. Diagnosed; no fixture exercises it. |
| Separated Position / 3D Rotation / Orientation | Dimension leaves and 3D Transform leaves → existing editable targets (PositionX/Y/Z, RotationX/Y, OrientationX/Y/Z) | Lowering is unchanged. Orientation interpolation follows keyed import, not AE's spherical interpolation (static Orientation only in the fixture). |

Evidence (independent native source authored through headless-adobe
`run_jsx`, AE 26.5x89):
- `expression_samples/wiggle_coverage.aep`, SHA-256
  `270686adc7d6de144f2a651f4e1e62a686b9620c8867046fdbcd3dc7660c185a`.
  - Compositions 1/16/30/44/58 = cases `aep-expression-samples-wiggle-coverage-c{1,16,30,44,58}`.
  - Adobe pre/post samples are in `wiggle_coverage.readback.json`.
- 30fps Adobe renders (60 frames each) are committed under `tests/references/aep`.
  Automated full decode shows motion in every case; it was not human-inspected.
- Tests in `expression_eval/wiggle_coverage_tests.rs`:
  - `wiggle_coverage_base_values_track_adobe_pre_expression_samples`
  - `wiggle_coverage_native_wiggle_lowers_to_editable_varying_tracks`

  Both are RED on unchanged main (`only finite ordered nonspatial Linear/Hold keys
  are admitted`; missing wiggle diagnostic) and GREEN after.
- Wiggle values use the converter's documented kernel, so wiggle cases are
  structural. Fresh import + Tesseract render vs the committed Adobe MP4s (`make aep-score-run`, full-resolution RGB, all 60 frames): c1 base mean 0.99704 / min 0.99054 (at 1.5s, the spatial refinement); wiggle cases are descriptive only because the kernels differ: c16 0.97215/0.96623, c30 0.98131/0.96956, c44 0.99486/0.98957, c58 0.99450/0.99114 (before the random-kernel repair below; see that section for current values). Alpha/audio unmeasured.

**FX → AEP:** expressions are not restored; the baked editable keys export
through the existing numeric-key writer. Baked keys use the existing writers unchanged.

Shape, Mask and Text Animator targets are covered by the next section. Not
covered: reverse-stretched layers.

## Expression evaluation — approved direction scope

The repository owner explicitly approved this narrowed scope for the expression
increments below: **import (AEP → FX) only**. Evaluated expressions are baked into
ordinary editable FX keyframe tracks and held texts. Those export through the
existing writers unchanged; no expression-specific export path or proof is part
of this work. Target-specific writer omissions are the writers' existing ledgered
limitations. Expressions are never restored as live AE expressions.

## Source Text, 3D layer space and Shape sourceRectAtTime — import increment

**AEP → FX:**
- **Source Text expressions** are evaluated as strings. Numbers, `toFixed` /
  `padStart` and other string/number methods, template literals,
  `String`/`Number`/`parseInt`, and `text.sourceText` reads are supported. Runs of
  equal strings become held editable text segments at source-local frame starts,
  through the existing held Source Text path.
- **3D `toComp`/`fromComp`/`toWorld`/`fromWorld`** use full Orientation·X·Y·Z
  rotation, depth, and AE's default composition camera (50 mm on 36 mm film,
  zoom = width × 50/36). Depth is reported as zoom − zoom/distance, as measured.
  Explicit camera layers fail with a diagnostic.
- **Shape-layer `sourceRectAtTime(t, includeExtents)`** is computed from
  Rect/Ellipse/Polystar geometry and group transforms; extents include half the
  stroke width. Text layers and free-form paths fail with a diagnostic.

Evidence: `expression_samples/expression_apis2.aep`, SHA-256
`b222051ea8b0aa583e7846383e7a1476f5c9fdd2cd5223f43adfda694befd205` (headless-adobe
`run_jsx`, AE 26.5x89), case `aep-expression-samples-expression-apis2-c1`, 30fps
reference committed.

| Feature | Evidence (all 61 frames vs Adobe) |
|---|---|
| Source Text strings | Exact match |
| 3D toComp / fromComp | toComp equal to Adobe within 1e-3 (exact max not printed); fromComp 1.9e-8 |
| sourceRectAtTime | Exact without extents; 5.7e-6 with extents |

Fresh import produces every Adobe string as held text
(`source_text_expressions_import_as_held_text_segments`).

Still unsupported (diagnosed):
- Text-layer `sourceRectAtTime` (needs font layout metrics the converter does not have)
- Camera-layer projection
- `textIndex`/`textTotal` Expression Selectors (no per-character expression target in FX)
- `createPath`/`points()` (would require per-frame Path baking)
- lookAt roll

## Extended AE expression API — import increment

**AEP → FX:** the converter evaluator now implements, as our own code, the following
AE expression APIs. Results are still baked into editable keys (no JsScript).

- **Interpolation**
  - `ease`, `easeIn`, `easeOut`: AE's cubic Hermite curves, measured from Adobe
    (easeIn(0.5)=0.375, easeOut(0.5)=0.625).
  - `smooth(width, samples, t)`: box average of the pre-expression value.
- **Derivatives**
  - Global `velocity` and `speed`.
  - Property `.velocity`, `.speed`, `.velocityAtTime(t)`, `.speedAtTime(t)` and `.smooth()`.
  - All use a ±10 µs central difference.
- **Layer space**
  - `toComp`, `fromComp`, `toWorld`, `fromWorld`: planar Transform chain including
    parents, with AE default anchors and positions.
  - 3D X/Y rotation and Orientation fail explicitly.
- **Lookups**
  - `content(name|index)` with nested `.content()`, `.transform`, and named members
    (`size`, `position`, `strokeWidth`, `opacity`, `start`/`end`/`offset`, …).
    AE default names (`Group 1`, `Ellipse Path 1`) are resolved.
  - `mask(name|index).maskOpacity/maskFeather/maskExpansion`.
  - Layer `width`, `height`, `name`, `active`, `parent`, `hasParent`.
  - Bare `anchorPoint`/`position`/`scale`/`rotation`/`opacity`.
- **Loops**: `loopInDuration` / `loopOutDuration`.
- **Helpers**: `degreesToRadians`, `radiansToDegrees`, `dot`, `cross`, `rgbToHsl`,
  `hslToRgb`, and `lookAt`.
- **Syntax**
  - `%` and compound `+= -= *= /=`.
  - `for` / `while` / `do…while` / `continue`, bounded by a per-evaluation runtime
    limit of 1,000,000 loop iterations (a runaway loop is an evaluation error).
- **Values**
  - AE Transform defaults omitted from the AEP (Position at composition center,
    source-center Anchor, Scale 100, …) are modelled.
  - Omitted result components are filled from the pre-expression value.
  - Unknown Contents/Mask members fail instead of becoming `undefined`.

| Feature | Original semantics → replacement | Reason / impact / evidence |
|---|---|---|
| lookAt | AE Orientation aiming the layer Z axis → same X/Y aim, roll 0 | Measured X/Y equal to Adobe within 4e-13; AE's third component (182.98 in the fixture) is not reproduced. |
| Reads of spatial Position paths (layer space, Contents Position, Position loops/velocity) | AE path speed → keyed import's 0.25-unit refinement | Measured: toComp 0.0147, Contents Position 0.0084, loopOutDuration 0.0065, Position speed/10 0.069. |
| Per-character / text / path APIs | `textIndex`, `textTotal`, Expression Selectors, `sourceRectAtTime`, `createPath`, Source Text strings | No editable per-character or path target exists in the current FX model. Rejected with a contextual "unknown AE identifier/unimplemented function" diagnostic; authored fallback retained. |
| 3D layer space | toComp etc. with X/Y rotation or Orientation | Fails explicitly instead of returning a planar guess. |

Evidence: an independent native source authored through headless-adobe `run_jsx`
(AE 26.5x89).
- `expression_samples/expression_apis.aep`, SHA-256
  `1ac1debce220b6dbc7f1bf582d2726562026d6ec7288e09f0468fc32f78aab34`, case
  `aep-expression-samples-expression-apis-c1`.
- Adobe readback is sampled after all layers exist.
- `expression_apis_match_adobe_evaluated_values` compares every probe at all 61 frames.
  Non-spatial APIs (ease family, smooth, loops/`%`/helpers, `mask()`, layer
  attributes, colour helpers, lookAt aim) match within 4e-13 to 3.6e-6.
- `api_tests.rs` covers analytic values, runaway loops and unknown members.
- The 30fps Adobe render is committed under `tests/references/aep`.

**FX → AEP:** baked editable keys export; expressions are not restored.

## Expression targets: Shape contents, Masks and Text Animators — import increment

**AEP → FX:** the converter expression evaluator (including approximated `wiggle`)
now reaches three more kinds of target. Each is lowered onto the same editable FX
target that ordinary keyed import already uses:
- Shape-content numeric leaves: Group Transform, Ellipse, Star, paints, Trim and
  scope controls through `add_leaf_numeric`;
- Mask Feather, Opacity and Expansion;
- numeric Text Animator properties (not selectors).

New converter-only identities `PropertyIdentity::Mask` and
`PropertyIdentity::TextAnimator` are added. Adobe sidecars reject them
(`ConverterOnlyIdentity`), and existing v2/v3 sidecars are unchanged.

| Feature | Original semantics → replacement | Reason / impact / evidence |
|---|---|---|
| Shape leaf identity | AE `propertyIndex` path → native storage ordinals, keeping the Adobe-captured Scale family indices | Converter-internal only; the fixture matches Adobe paths by their match-name chain. |
| Shape/Text clock | Composition-clock samples → the layer-local / Text-segment clock (`rebased_samples`) | Fixes a latent offset in the earlier Shape Scale lowering for layers with start ≠ 0 or stretch ≠ 1. Samples before local zero are dropped. Fixture layer starts at 0.5s. |
| Vector2 targets (Ellipse Size, Mask Feather, Text Position) | One shared-easing track → `fx_keyframe_bake::value_curve` Linear/Hold keys at integer-ms source observations (0.001 FX-unit reduction) | The scalar cubic fitter cannot share keys across components. Error vs Adobe evaluated values is bounded by half a millisecond of motion (Ellipse Size measured 0.0166). |
| Mask Opacity units | Native fraction ↔ expression percent | Scaled ×100 into the evaluator and ÷100 onto the FX mask target. |
| Text Animator expressions | AE evaluates once per layer → one editable animator track | Diagnosed (`evaluated once per layer, not per character`). Per-character context (`textIndex`, Expression Selectors) remains rejected. |
| Integer-flagged Shape leaves (e.g. Trim) | AE integer flag → interpolated like keyed import | The discrete Linear/Hold restriction now applies only to Effect controls. |
| Compound Rectangle, dynamic Path, caption box, native-rect controls | Specialized paths → existing behaviour (expression keys omitted with a diagnostic) | Unchanged. No fixture exercises them. |

Evidence (independent native source authored through headless-adobe `run_jsx`,
AE 26.5x89):
- `expression_samples/wiggle_targets.aep`, SHA-256
  `d67b8862fd9346013407ae311ccf84f552182d17ee3552dc212f79acf76c003c`; compositions
  1/14/29 = cases `aep-expression-samples-wiggle-targets-c{1,14,29}`.
- Adobe pre/post samples and native property paths are in `wiggle_targets.readback.json`.
- 30fps renders are committed under `tests/references/aep`.
- Tests in `expression_eval/wiggle_targets_tests.rs`:
  - Base values vs Adobe pre-expression samples (61 frames): spatial Vector Position
    0.0126, all others ≤ 3.2e-6.
  - Fresh-import lowering onto varying editable tracks. Deterministic `value` tracks
    vs Adobe evaluated: Ellipse Size 0.0166, Trim End 9.7e-7, Mask Feather 0.0075,
    Mask Expansion 8.9e-7, Text Opacity 3.1e-6.
  - Contextual wiggle diagnostics.
  - Wiggle perturbation span.
- RGB vs Adobe (`aep-score-run`, Arial supplied locally) is descriptive only,
  because wiggle kernels differ: c1 mean 0.95731 / min 0.91365, c14 0.83102 / 0.71141,
  c29 0.87713 / 0.83710. Mask feather rasterization and text layout were not
  separately isolated.

**Random kernel repair.** FNV-1a inputs that differed only in their last character
hashed a fixed multiple apart, so adjacent wiggle/noise lattice cells and per-frame
random streams were almost equal. Every `wiggle` was a slow drift (e.g. a 30-unit
Fill Opacity wiggle spanned 0.94 over 2s). A murmur3 finalizer now decorrelates
lattice values and stream seeds.
`wiggle_targets_perturbation_spans_its_amplitude_like_adobe` requires each
perturbation to span at least half its amplitude, as Adobe's do. Phase 1 RGB after
the repair: c1 0.99704/0.99054 (unchanged; no wiggle), c16 0.97389/0.96611,
c30 0.98308/0.96872, c44 0.99510/0.98981, c58 0.99498/0.98883.

**FX → AEP:** baked editable keys export through the existing writers;
expressions are not restored. They need no expression-specific export path.

## Random numeric expression approximation — stacked import increment

**AEP → FX:** the explicitly approved random exception admits seedRandom, random,
gaussRandom, wiggle and noise in the existing numeric Transform/Effect evaluator.
Each invoked API produces an owner/property `AE-PROPERTIES` **approximated**
diagnostic, including dependency usage. Native authored/captured sidecar precedence,
unsupported deterministic APIs, interpolation, dimensions and existing fitter
failure handling remain unchanged. Editable numeric keys replace live expressions;
no JsScript, new FX capability, per-character context or renderer change.

| Feature | Original semantics → replacement | Reason / impact / evidence |
|---|---|---|
| seedRandom | AE seeded/time-dependent stream → property-local FNV/xorshift stream, offset/timeless reset | Authorized approximation; sequence not Adobe-identical; seed/clock isolation unit assertions. |
| random | AE scalar/vector uniform random → checked scalar/vector bound interpolation on that stream | Changes stochastic values; repeatability/range tests. |
| gaussRandom | AE Gaussian distribution → custom Box-Muller, midpoint and range/6 sigma | Tail distribution differs; finite/vector/repeatability tests, no native distribution equality claim. |
| wiggle | AE fractal motion → seeded smoothstep lattice perturbation, native-time authored base, frequency/amplitude/octaves | Frequency is respected; kernel/motion differs from Adobe.1..16 octaves supported; other counts diagnosed unsupported. Hash-pinned Intro native source structural regression, not pixel equality. |
| noise | AE Perlin-like coherent noise → custom seeded1D/2D/3D value-lattice noise | Kernel differs; continuity/range/repeatability tests; four-dimensional inputs unsupported. |

[Full algorithm limitations](../crates/aftereffects_file/src/expression_eval/RANDOM-APIS.md).
Licensed Intro source SHA256
`f315ca13611716c924825ac5b16a8a8a5b4f1fdb95261a824f2ab7393a0209f4`,
Scene01 composition1/layer921/ADBE Rotate Z, was used by a removed licensed-source test; recorded results are historical
evidence only (no longer executable). Licensed programs/bytes are not published.
`random_intro_native_wiggle_rotation_becomes_editable_with_diagnostic` (removed; historical) checks a
fresh structural import, numeric motion, exact owner diagnostic, editable Rotation
track and no JsScript. RED CPU job2917 rejected wiggle before this increment.
GREEN CPU job2926 passed all ten targeted tests, including the hash-bound native
case and fresh converter-path diagnostics for every API. Job2927 passed converter
workspace check/all-target clippy/fmt; the scoped package suite had2033passed,
5failed,428ignored. All five failures reproduced on the exact unchanged #4966 base
c09488205b0f4a44246d5c1464aae74a7cafa9de in2932 (2024passed,5failed,428ignored):
export accumulated-evaluation/remapped-window tests, two vector2 easing tests and
Ripple amplitude fitting. This increment does not repair peer-owned fitter/lowering.
No Adobe call, new native fixture,30fps render, immutable Asset, visual score,
alpha/audio or native-equivalence pass is claimed. Root-render reachability and
whole-Intro fidelity remain unmeasured.

**FX → AEP:** random expression authoring/export unchanged and untested; outside
this explicitly import-only increment. Existing key export is not proof of native
expression restoration. This is partial random coverage, not completed AE fidelity
proof or support for AnimationComposer/text selectors/Shape dependencies.

The random exclusions in the historical deterministic milestone below are
superseded only by this explicitly diagnosed random increment.

## Random-expression no-change traces — two private native cases

These are import-side limitations, not new fidelity passes. Source bytes and
programs remain private; paired exports used the same fixed renderer and policy.

- **Case 05**, source SHA256
  `ee4f3a3c745106ae77d28d75e70e1ab1a2bcff3bbea16d4d2b244689f6dad720`,
  selected comp16: comp110/layer527 `Blur_Map` Scale wiggle is admitted and lowered
  into existing editable Scale tracks (FX layer32; 88 X keys, 268 Y keys).
  It is not rejected by text/per-character admission or overridden by authored
  keys. The native layer is hidden. Its actual consuming layer536 Camera Lens
  Blur depth-map parameter `ADBE Camera Lens Blur-0010` references layer527
  (`tdpi`), but Camera Lens Blur has no current native FX counterpart/mapping and
  is omitted with an existing contextual diagnostic. Replacement: keep the
  editable hidden map/Scale tracks and convertible siblings, omit the consumer.
  Impact: animated depth-map blur is absent; restoring Scale alone produces no
  visible change. Both full decoded RGB frame sequences are identical. Compound
  Blur and Displacement Map omissions are separate; the latter references
  layer540, not this concrete wiggle map. No renderer/schema extension is made.
- **Case 06**, source SHA256
  `e2eb353f50cfe421647552002dfddda6a85ac27dad8d809f3b2e619813e194f6`,
  comp1/layer75: Geometry2 Position uses existing editable Group Transform
  capabilities, but this precomposition occurrence is not supported by the
  current specialized Geometry2 mapper. Its expression is admitted and reaches
  dependency evaluation. The WigglePosition
  frequency dependency has six scalar native Bezier keys at
  0.833333333/0.9/1.033333333/2/2.066666667/2.2 seconds, values 0/1/0/0/1/0;
  amplitude is static30. The fresh evaluator admits only nonspatial Linear/Hold
  source keys, so dependency validation fails before wiggle executes. There is
  also an independent later gate: `geometry2::prepare` requires a planar Shape
  owner and rejects this precomposition (native kind0); its point reader accepts
  only bounded origin/Position aliases, not general evaluated point overrides.
  Replacement: report the evaluation denial, then omit Geometry2 with the
  existing contextual owner-kind diagnostic while retaining the owner/siblings.
  Impact: missing jitter. This is **unfinished converter dependency sampling and
  occurrence mapping**, not an absent FX primitive or a text-animator failure.
  A pinned-source CPU RED check (job2979) reproduces the dependency rejection;
  no GREEN repair or new visual proof exists. A repair needs independently
  reviewed source-key sampling and precomposition/evaluated-point mapping,
  coordinated with their owners; fixing only random admission cannot restore
  this occurrence. Lowering/fitter and FX runtime/schema remain unchanged.

Both paired full-resolution RGB measurements still fail0.99; equal outputs do
not prove restoration. FX→AEP implementation and native export proof are unchanged
and untested by these traces. No new Adobe capture, alpha/audio or immutable Asset
proof was run.

## Deterministic numeric expression evaluation — partial import-only milestone

**AEP → FX:** a closed Boa profile evaluates direct Transform/ordinary Effect
numeric programs after Essential overrides and fits composition-FPS observations
through existing editable scalar-key lowering. Unsupported programs, native
interpolation, ambiguous dependencies and cycles retain contextual fallback;
no JsScript, shader, source replay, renderer/schema change or duplicate fitter.
Controller linkage is replaced by independent numeric keys and is no longer live.
Shape/Ease-and-Wizz, random/seedRandom/wiggle and per-character selectors remain
unsupported in this milestone (random APIs and eased/spatial/separated inputs are
superseded by the later increments above). [Admission and detailed evidence](ae-expression-evaluation.md).

**Merged-main checkpoint:** main6819ffa passed isolated check2814; merge5f1113a4d
passed converter workspace check and77 expression tests (0failed,3ignored) in2815.
The ledger conflict is resolved while preserving both sections. The approved
additive `fit_scalar_curve_with_observations` now constrains cubic construction,
acceptance, adjacent1ms spans and key minimization at **original actual times**;
legacy fitter behavior remains unchanged. No duplicate solver, rounded oracle,
tolerance weakening, dense fallback or shared FX change was substituted.
Unrepresentable sampled-endpoint intervals are diagnosed rather than published.
Serialized Position keys match49 source-frame vectors (166 X+Y keys, max1.4211e-14)
and, independently,2001 freshly evaluated native-time vectors (1146 X+Y keys,
max1.1938e-12) under the fixed1e-8 native-unit criterion. Output is adaptive but
key-heavy. The49-frame track itself has0.296836713 error at the unsampled native
grid; the native-grid result is a **separate constrained track**, not default
import behavior. Continuous arbitrary-time equality is not claimed. See the
[detailed proof and limitations](ae-expression-evaluation.md). Current head
full-suite/native render/alpha/font validation remains unrun.

**Historical evidence:** CPU job2791: workspace check/clippy/fmt and75 passing
expression tests. Full offline suite2750 has one export-hierarchy failure,
independently reproduced on the clean base in2757. Hash-pinned native Bold01
import asserts ten editable Fill targets; cloned blue Essential override is
supplementary isolation evidence. Authorized typed Adobe capture queue2743 failed
its temporary-clock identity restoration check and returned verified READY.
No Bold01 sidecar or Bold01 Adobe equality, fresh render/alpha/exact-font pass is
claimed. Separately, fresh evaluation matches2,001 checked-in independent native
sampled-Position vectors (maximum1.1795e-12 error); this proves that numerical
subset separately from the newly proved finite-observation lowering above.
Workspace check/clippy/fmt and27 fitter tests passed in2879; its expression panel
had77 passes and one stale Bold01 diagnostic-text assertion, corrected without
weakening editable-color assertions. Final corrected expression counts are on
the PR. Draft partial Adobe feature proof only; see the detailed profile.

**FX → AEP:** unchanged. Original expressions/live control linkage are not
reconstructed; existing bounded native numeric-key export remains available.
The user approved this import-only milestone. Lemon's new numeric native capture
is verified (41 targets,0–20s,820082 vectors, receipt-equal sidecar properties);
Merged main3f96a125 validates version3 Shape identities but does not register
Shape-expression destinations. Fresh full-composition conversion of all four
bounded native batches is now executed (job2903:1 native test passed,0 failed;
820082 vectors,41 targets,82 window records). **3 ordinary Opacity targets**
(owners795192753–795192755) have one persisted scalar key each and **0.0 error**
at every original native observation in both windows. **38 Shape targets**,
including all **12 data-wedge Scale** owners15517–15528 and **6 background Scale**
across owners795192752–795192755, are explicitly diagnosed, not restored:
`captured native Shape expression target ... has no editable expression mapping;
authored fallback retained`. Full native paths/composition/layer identities remain
in the diagnostics. This is missing converter mapping, not renderer unsupported.
No Shape target is claimed exact. A portable synthetic-identity regression fails
without the explicit diagnostic; the ignored private native test validates all
41 pinned targets when explicitly invoked. Ease-and-Wizz/native Bezier admission
for fresh source evaluation remains unsupported; captured-value mapping is distinct
from fresh evaluation, rendered/alpha/font fidelity and feature export proof.

## Public-source review repairs — CPU evidence only, native fidelity unmeasured

These corrections keep the current FX model/runtime. Their CPU regressions are
supplementary structural evidence; no Adobe authoring, native render, readback,
Asset publication or RGB/alpha measurement ran for them.

### Import (AEP → FX)

- **Signed integer-slider defaults:** a sparse plugin `pard` of kind 1
  (`PF_Param_SLIDER`) now decodes its default as a signed 32-bit value, so `-1`
  is no longer imported as 4,294,967,295. Checkbox and popup defaults remain
  unsigned. `integer_slider_default_is_signed_while_checkbox_and_popup_are_unsigned`.
  Plugin parameter tables are indexed once instead of rescanned per name.
- **Text animator Z:** Anchor Point 3D, Position 3D and Scale 3D import as 2D XY
  editable controls. A non-neutral static or keyed Z (Anchor/Position ≠ 0,
  Scale ≠ 100) is now diagnosed (`... Z component is not representable in 2D
  editable text animators; Z omitted and XY retained`) instead of silently
  dropped. Approximation: per-character Z motion and depth are lost; XY is kept.
- **Huge finite Source Text font size:** the manual-leading fallback of
  `1.2 × font size` is clamped to the largest finite value, so a finite COS size
  near `f64::MAX` no longer panics the import.
  `huge_finite_font_size_does_not_overflow_the_manual_leading_fallback`.
- **PSD sources:** raw merged composites and editable layer channels are validated
  against their stored bytes before the output image is allocated.
  `hostile_merged_dimensions_without_pixels_fail_before_allocating_the_image`.

### Export (FX → AEP)

- **Animated Wiggly amount:** native Min Amount is now the negated Max Amount at
  every key, rather than a static negative of the initial amount, so fading the
  editable amount to 0 also stops native wiggle.
  `animated_wiggly_amount_keeps_native_min_and_max_symmetric` reads both native
  tracks back with our own reader (not independent Adobe evidence).
- **Static Offset Paths miter bounds:** static Path enclosure now multiplies a
  Miter-join Offset Paths amount by its miter limit, matching the animated
  analyzer, so acute offset corners are not clipped by an underestimated
  precomposition canvas. `static_mitered_offset_paths_bounds_match_the_animated_analyzer`.

## Full-span alpha stack transfer — import checkpoint

| Direction / source semantics | Existing editable replacement | Limits and proof |
|---|---|---|
| AE → FX, enabled ordinary full-composition Stencil Alpha (native17) / Silhouette Alpha (19) | Identity composition-clock Group masks accumulated lower siblings with Alpha / AlphaInverted TrackMatte. Original source is consumed as a matte, not source-colored paint; source IDs/keys, editable geometry and upper siblings survive. | No new FX/schema/renderer fields, JsScript, guessed geometry, source-name profile or media substitution. Structural CLI RED2774: three actual pinned-source tests failed on absent alpha stack wrappers. Shared Rust queue GREEN2797: three licensed CLI tests plus three supplementary CPU tests passed, zero failures/ignored; standalone fmt and workspace/all-target Clippy `-D warnings` passed. Independent native RGB/alpha comparison remains unmeasured. |
| AE → FX, non-rendering Null/helper kind or source already diagnosed as a placeholder | No new alpha matte or lower-stack restructuring; original non-rendering carrier retained with a contextual `AE-BLEND-MODE` reason. | Does not infer eligibility from opacity or empty children. Valid transparent/empty raster precompositions remain eligible. Supplementary regressions compare complete converted documents to Normal-mode controls for Null, model-kind and missing-source carriers; native alpha/render proof remains unmeasured. |
| AE → FX, partial-span, Adjustment, preserve-transparency-dependent, already restructured or over-depth alpha operator | Original best-effort Normal fallback with contextual `AE-BLEND-MODE` reason. | Partial spans need unmasked intervals; Adjustment/preserve-transparency sources require backdrop-dependent alpha. No permanent mask outside an admitted full-span lifetime or approximate source-copy/crossfade is claimed. |
| AE → FX, Stencil/Silhouette Luma (18/20), native four-color gradient / selected-mask Stroke plugin | Existing contextual blend fallback / effect omission remains. | Current FX LumaInverted is `(1-luma)*sourceAlpha`, not the full luminance complement outside source coverage. Existing two-point gradient and alpha-silhouette Stroke do not implement the exact native plugins. No shader/approximate substitute. |
| FX → AE | Existing Group/matte exporter unchanged; no native stencil-mode reconstruction added. | Fresh export/editability/native acceptance/render/alpha proof for this import lowering is unrun. Import structure is not export fidelity proof. |

Licensed native sources stay outside Git. CLI tests
`tests::alpha_stack::alpha_stack_cli_native_case01/02/03` (removed; historical)
are no longer executable; recorded results are historical evidence only.
The licensed-source tests verified SHA256s
`400b0b05e9e6ee7678ebb47621c3d721e9b974669eeb548283e80ea129fd88c4`,
`a0449301d389f9a8d8a4014d87aac3fb1946a0a0b552e9928bd768d47ce00754`,
`e4d0e1af4ba74a30e709d355023fad0c07aadc06a057464fb07058e13ea526fc`.
Each freshly imports root1891 through CLI parse/dispatch/publication and asserts
native1620/1624,1247/1251,500/503 mask only their lower stacks. Reference videos,
Adobe UI/alpha readback, Asset publication and post-fix scoring are not newly
executed by these CPU tests. Prior mismatch scores remain failures until rescored.

## Native Text Path mask identities — import mapping correction

**AEP → FX:** static `ADBE Text Path` storage with an explicit `tdli` now
resolves the native mask identity against `mkif` before selecting the existing
editable Text Path guide. The numeric `cdat` cache is not a reference fallback:
it can be zero while the native path is selected. Files without `tdli` retain
legacy numeric ordinal selection. Duplicate/malformed references, ambiguous or
absent mask identities and dynamic references are diagnosed; editable text and
convertible siblings remain. No FX/schema/renderer feature or approximation is
introduced. Existing guide geometry, layout controls and animation clocks remain
unchanged.

The private independently authored Lemon source SHA256
`4f91a7b24be96ed4924d68614aef8b40e6d1fad3d4274174eb06714a92488f84`,
composition15368, has twelve month owners15569–15580 with `tdli=2`, native mask
identity2, and cached numeric selection0. The ignored environment-pinned
`lemon_native_tdli_month_text_paths_bind_editable_guides` (removed; historical) asserts fresh editable
path binding for all twelve, reverse/perpendicular controls and JANUARY margin
1307.7. Licensed bytes stay private. Source-backed RED executed on main
`72caf3f3b`: one failed, zero ignored, at missing JANUARY path binding.
GREEN job2771 on buildable-main `0a44080ee` executed the same pinned test
successfully across all twelve labels, six resolver boundary tests, seven regular
text-path neighbors and two explicitly selected native-fixture import tests.
Those last two cover the existing independent path-control source, including six
composition targets. Converter fmt and workspace all-target clippy also passed.
Ignored export/native proof cases remain unrun. These are editable-structure
checks, not pixel-equality evidence.

The existing independent 30fps full-source diagnostic reference SHA256
`da2d636af790a6fd875a7fa96d9c32952eeaa32d5ba1807e482323fb9a4dc23f`
is not isolated feature fidelity or completed long-term Asset proof. Native font
substitution, background/data expressions and early title timing remain separate
limitations; no fresh native readback or RGB/alpha/audio comparison ran for this
mapping. **FX → AEP** implementation and proof are unchanged, including the
separate animated Text Path export checkpoint below. Bidirectional/native fidelity
completion is not claimed.

## TemperatureTint edited export approximation

FX → AEP now lowers the existing TemperatureTint effect into ordered, editable
individual-channel Exposure offsets, using the canonical AE26 parameter registry.
Temperature contributes R/B offsets±0.0012×temperature; tint contributes
R/B offsets−0.0006×tint and G offset0.0012×tint. Master controls stay neutral.
Separate native effects preserve independently timed scalar tracks and bypass
without combining clocks or inventing packed keys. The mapping follows the actual
FX shader, whose tint direction contradicts its descriptive text; the engine is
unchanged. Native colour space, premultiplication and intermediate clipping may
change RGB/alpha appearance. This is an approximation, not calibrated white balance.

AEP → FX does **not** reconstruct these per-channel Exposure records; existing
master-only import diagnostics remain. Premiere's Lumetri importer selects saved
white-balance scalars into editable TemperatureTint separately; see the CLI ledger.
`temperature_tint_export_preserves_independent_scalar_keys_and_bypass` and the
CLI's `human_lumetri_white_balance_edited_linked_export_retains_native_offset_keys`
assert generated editable controls/keys, not independent Adobe acceptance or
pixel equality. The native source's white balance is neutral; nonzero/keyed import
mutations are supplementary. Native nonzero proof and RGB/alpha comparison remain
unmeasured, with no new formal feature-proof promotion.

## Rust review repairs — structural evidence, native fidelity unmeasured

These corrections retain the current FX model/runtime and existing best-effort
boundaries. The listed CPU regressions **passed** in shared Rust queue job2655,
alongside the bounded `review_` panel (102 selected tests across six packages,
plus seven FFmpeg-library-mode repeats). They are not independent Adobe feature proof. No new native authoring/readback,
30fps reference, long-term Asset publication, or RGB/alpha/audio measurement ran.
A native layer shell with mutated controls or our own reader of an export is
supplementary evidence, not a new Adobe-authored feature oracle.

### Import corrections (AEP → FX)

- **Deep Source Text COS:** cloning, style equality and cached-layout searches now
  traverse admitted containers iteratively, like parsing/destruction. Existing
  interpretation, diagnostics and text approximations remain; no depth quota or
  new text mapping. `review_source_text_deep_containers_survive_production_walks`
  uses a pinned `text/text_ranges.aep` layer shell with supplemental 10,000-level
  COS on an abort-isolated 64KiB thread. `review_cos_clone_and_equality_preserve_value_semantics`
  and `review_layout_baseline_search_keeps_native_preorder_and_line_leaves`
  guard ordinary semantics. Independently native-authored deep payload proof is
  absent. Export implementation/proof is unchanged.
- **Static gradient duplicate knots:** merging retains stable same-position color
  and alpha events instead of erasing hard transitions. Simultaneous channels
  pair events by index and hold the shorter channel's final event. Midpoint bias
  and animated-gradient limitations remain. `review_merged_gradient_retains_color_and_alpha_discontinuities`,
  `review_coincident_endpoint_gradient_keeps_two_authored_stops` and
  `review_native_fixture_derived_duplicate_gradient_keeps_editable_events` cover
  editable knots and numerical ramps; the native gradient contributes colors to
  a supplemental mutation only. Simultaneous-event Adobe fidelity is unmeasured.
  Export implementation/proof is unchanged.
- **Equal-endpoint nonspatial Bezier excursions:** nonzero effective temporal
  handles cannot be represented by normalized FX easing. The coupled animation
  target set is omitted with contextual diagnostics, retaining static values and
  independent convertible siblings, rather than silently erasing motion. Direct
  easing-helper linear fallbacks also diagnose excursion loss. Linear/Hold,
  zero-handle and spatial-distance policies remain. No baking is substituted.
  `review_equal_endpoint_bezier_excursion_is_contextually_omitted` is supplemental
  numerical evidence in both clock directions, not native feature fidelity.
  Export implementation/proof is unchanged.
- **Keyed Fast Box Blur owner clock:** existing radius-to-Gaussian keys use
  `start + source_time × stretch` on the receiving parent-identity effect owner.
  Invalid clocks omit the keyed effect atomically without consuming identity or
  animation allowance; owner/siblings remain. Kernel, variance, interpolation
  and edge-alpha approximations are unchanged. `review_box_blur_radius_keys_keep_owner_clock_and_fail_atomically`
  uses the pinned static Box Blur donor and Shape shell with generated keys and
  rational-clock mutations, not independently authored animated proof. Export
  implementation/proof is unchanged.

### Export corrections (FX → AEP)

- **Solid writer envelope:** `write_solid_composition` now retains two native item
  envelope tails after each layer's `Ewst`, as the mixed writer already does.
  `review_solid_native_two_layer_envelope_boundaries_are_preserved` compares the
  complete inter-layer boundary with unchanged Adobe-authored
  `render/export_add_blend.aep` (SHA256
  `d04ffbca88577b14b1702e24a003c3a92aecf038a741a478df4e1fb8c9169aee`).
  `review_solid_no_layers_keeps_the_empty_writer_byte_identical` guards empty
  output. Native opening/editability/render of the repaired public solid writer
  remains unrun. Import implementation/proof is unchanged.
- **Temporal matte wrapper:** specialized temporal Image-source wrappers apply
  ordinary inbound-matte display suppression; reminted source children stay
  enabled and links/modes remain. `review_temporal_image_matte_hides_wrapper_only`
  checks Alpha/Luma output structure using explicit edited FX over pinned Transform
  scaffolding. Own-reader assertions are not independent Adobe acceptance.
  Import implementation/proof is unchanged.
- **Hidden-container Video audio:** inherited visibility gates every emitted
  footage audio switch after editable gain lowering, retaining hidden Video and
  its gain tracks; Audio-specific picture-switch handling remains unchanged.
  `review_hidden_group_mutes_video_without_losing_gain_keys` checks hidden/shown
  identity and opacity-container paths with supplied media metadata, not decoded
  media or independently measured sound. Import implementation/proof is unchanged.
- **Scalar script fan-out:** preparation evaluates the complete dependency-connected
  same-clock scalar script domain in original stable-ready graph order, retaining
  each node's source, seed provenance and dependency slots. A consumer captures
  its value at its graph position rather than after a reduced upstream closure.
  Unvalidated domain clocks/types/references retain scripts with diagnostics,
  not incorrect baked tracks. `review_script_domain_fanout_matches_full_graph_order`,
  `review_script_domain_preserves_sources_seeds_slots_and_call_accounting` and
  `review_script_domain_unvalidated_sibling_is_diagnosed` are supplemental VM
  numerical evidence. Existing 4× FPS sampling, tolerance, unsampled state and
  disconnected-global limitations remain; general runtime equivalence is not
  claimed. Import implementation/proof is unchanged.

Import and export implementation corrections above are separate from incomplete
independent native proof in **both directions**. No bidirectional feature or
conversion completion is claimed by these review regressions.

## Static Point2D to Position3D — user-approved zero-Z policy

**AEP → FX:** a complete direct `comp(name).layer(name).effect(name)("ADBE Point Control-0001")`
Position binding can copy a static native Point into an independent editable
`Position::ThreeD([x,y,0])`. **Z=0 is an explicit user-approved converter policy,
not an Adobe-documented or independently established native coercion rule.**
A contextual diagnostic states that policy and the loss of live controller linkage.

Admission requires a true native3D Position receiver with finite three-component
Continuous storage, an enabled direct expression, no keys or separated dimensions;
and one uniquely identified planar source-less Shape controller, native Point
kind/metadata and static finite two-component source. The new policy path may read
explicitly typed native Point storage when the instance omits its local value
declaration; it never infers defaults from that absence. Present malformed or
ambiguous declarations are rejected. The legacy same-comp Point reader still
requires its local declaration. Producer canvas units
are applied once. Explicit storage takes precedence over local declaration defaults;
malformed explicit storage never falls through to a default. Current-composition
occurrence overrides remain authoritative, and source chunks/items are not mutated.
Keyed/expression-chain Point sources, ambiguous/missing identities, malformed planes,
non3D/keyed/separated receivers and executable/indexed/arithmetic suffixes retain
the diagnosed original fallback. Other Transform/TextAnimator/coercion paths are
unchanged; no script, shader, camera, schema or renderer capability is added.

Original licensed Logo14 source SHA256
`f8f9c9db3a80ce1b5c3ab686db7efae505f318299ae1b4f403af8715c994695b`
and Logo15 `5042c5fb645f2712f64427fdc4197d60d73f469ba6571a4c294b32f8eaa0c40f`
bind root1098 and receiver1371/layer1881 above Brand500/layer2384.
The private environment-path regression
`brand_native_static_point_position3d_uses_user_approved_zero_z` (removed; historical) verifies source
hashes, fresh imports, XYZ `[1920,1799.6666870117188,0]`, preserved3D/Brand ownership,
policy diagnostics and immutable source state. Licensed source bytes/media are not
tracked. Prior failed native inspections do not establish Z semantics or fidelity.
Executed evidence: queue2476 compiled and failed the original Logo14 cached-XYZ
assertion (genuine RED); queue2526 and post-main-sync2540 executed all six policy
tests with both pinned sources (six passed, zero failed/ignored). Queue2569 ran
control-link neighbors:104 passed, zero failed,11 ignored (those11 remain unrun).
Queue2540 also passed converter-workspace fmt and clippy. Fixed converter receipt
2532 freshly converted both originals; receiver1218 retained3D and became
`[1920,1799.6666870117188,0]` in both actual CLI archives.

At root6s, fixed current-renderer receipt2551 exported both old1480 CLI documents
and both repaired2532 documents (jobs2565–2568). Bundled Montserrat Bold/Light bytes
were packaged into separate render copies without substituting authored font names
or changing transforms/selectors. Brand is visibly absent in both old samples and
present, fully on canvas, in both repaired samples. A supplementary luma<100 count
in ROI`[1470,1650,900,300]` changed from0 to53027 pixels in each case; repaired bounds
are`[1610,1720,2228,1877]` on3840×2160. The baseline converter1480 differs in upstream
code from2532: this visibility A/B supplements the source regression, not an
isolated whole-document equality claim. First font-unpackaged exports2559–2562
failed missing-font admission and are not GREEN. Private licensed media, hashes,
receipts and the contact sheet remain in the handoff evidence, not Git.
Per the later user instruction, the seven-file change was recreated directly on
last buildable main `76db7e7d3128547c567ccfaada294042ff6de450`, without newer main
fixes. Stable-base converter2594 and renderer2595 built;2600 passed all six policy
tests/both pinned sources, fmt and clippy. Fresh CLI2594 converted both originals;
exports2604/2606 at root6s independently show Brand visibly on canvas in both.
Newer main's unrelated compilation failures may still affect PR CI; no such repair
is included here. Current-stack visibility is established; independent Adobe
fidelity, native font version identity, full-duration frames, alpha and audio
remain unmeasured.

**FX → AEP:** existing editable3D Transform export is unchanged. Native expression
reconstruction, user-policy equivalence to Adobe evaluation, fresh edited export,
Adobe acceptance/readback/render, alpha/audio/font fidelity remain unproved. The
policy does not repair Brand's separate TextAnimator alias or nested own-camera
semantics. Roll back by reverting this converter-only mapping; retain pinned oracles.
## Static Group Drop Shadow and physical-font support enclosure

**FX → AEP:** a static zero-spread Drop Shadow stack no longer disables the
physical-font source projection. Its content support is united with the offset
shadow support expanded by native AE Size (`2 * FX blurRadius`, 130 for P037),
in source coordinates before the existing owner transform, including native 3D.
This is the user-approved effect-definition upper bound, not a fitted radius or
an exact visible-alpha prediction. Unsupported effects, effect-property animation,
nonzero spread, nonfinite controls, masks, skew and projected child Text retain
the existing guarded profiles. Consumer-demand cropping policy is unchanged.
No runtime/schema, font substitution, flattening or source asset changes were made.

**AEP → FX:** unchanged; this patch is an export enclosure/omission repair.
It uses W03's emitted-Text/whitespace bounds implementation, merged in #4932
at `48c2ef9ce5eb0cf55251922f3354372729558d0f` (following #4892).

**Native evidence:** independently authored AE26.5x89 controls (fixture SHA
`0160adcfd09c70f4f13b7eedbb539b57e855312fbb0c7b2b44513aef8f418273`),
author job `7f088e9d8dab4e90b49857db91f2d691`, RGBA render job
`d2c932b43e0a4e128952c7809d09a5f1`, 16 full-resolution 768x768/30fps frames.
At softness 130, nonzero alpha expanded 121px on the 128px rectangle and 122px
on the previously retained large control, both enclosed by 130px. Distance 28
shifted the shadow by 28px. Size 0/1/2/10/20/65 measured 0/1/1/9/19/61px.
These are bounded alpha observations, not general exact-kernel or alpha-fidelity proof.

The omission regression
`embedded_font_bounds_retain_a_mixed_motion_blurred_storyboard_card` failed with
unknown Text glyph bounds before the change (CPU job2494), then passed (job2509;
3D-owner variant and whitespace tests job2521). The support arithmetic test is
`native_shadow_defined_support_contains_measured_extent_and_content`.
Fresh original P037 exports retain Group810 and its native shadow after the fix.
Fresh native renders completed cleanup/READY: before `ba1c150339fd4778801016795dfa51dc`,
after `c8df0f112b11447f9ade08116850a6e5`. Full-resolution canonical RGB24
`validation_cli`, rgb-hybrid, 0.25s, 30 samples: mean 0.445683803 → 0.448142771;
minimum 0.230712899 → 0.230719817, worst 7.0s. All samples remain below 0.99.
Unchanged source/reference hashes and full canvas/duration/FPS were checked.

**Native numeric edit repair:** the historical hidden-property failure
(`c809631c1b8f4f3ebfdcde39b81a922f`) is repaired by matching independent native
Drop Shadow descriptors: unrestricted EffectFloat leaves and EffectScalar angle,
all with native storage1. Descriptor regression RED2574 → GREEN2578.
Fresh generated acceptance job `2dd4179cfae24d2aa6e7f8101d5db2ab` changed
Size130→150 and distance28→38, saved/reopened, and selected exact layer ID149.
The native 3D owner and all 60 editable source children survived; cleanup/READY
verified. Independent oracle job `9e2aab56d81c47acb198184724f0fe0e` supplies
only stripped descriptor metadata, not replayed project/value payloads.

Fresh #4932-base and repaired P037 renders (`388fb07469e4414f89f2e6e898707617`
and `288e8149e150473ebc1980cfdf6a3560`) completed cleanup/READY at full
1920×1080/30fps/native duration. Canonical RGB24 rgb-hybrid/0.25s validation
has 30 samples each: mean 0.448125854 → 0.448141668; minimum 0.230712899
on both, worst 7.0s. All 30 remain below 0.99. Converter gate2586 passed
1929 AE tests with 424 existing ignored, plus workspace check/clippy/fmt/tests.
Diagnostic font substitution was explicitly allowed; replacement appearance,
general shaping, camera equality, alpha/audio fidelity and formal long-term
Asset publication remain unproved. The case is not a strict visual pass.

## CustomShader effect omission — corrected export policy

**FX → AEP:** always drop the CustomShader effect, including identified, legacy
and disabled records. Never map, execute, recognize or approximate its WGSL.
Every dropped effect names the shader: `CustomShader "<name>" dropped; no native
mapping or approximation`. Keep the owning layer and all children, parenting,
transforms, clocks and supported effects exactly as ordinary unshaded lowering
would emit them. Existing unrelated unsupported-feature guards remain in force;
retention is not a fidelity claim. This supersedes #4944's blanket owner omission.

**Only owner exceptions:** an Adjustment carrying a shader is dropped (including
mixed native-effect stacks), or a childless plain white Rect/Shape shader canvas
is dropped. Source-data canvas classification is: Rect with enabled fill, no
stroke, and solid RGBA `[1,1,1,1]` (the explicit fill-paint override is authoritative);
Shape with exactly one solid RGBA `[1,1,1,1]` fill at opacity1 and no strokes.
Groups/roots, Text, Image/Video/precomps, colored/gradient or stroked shapes are
never excluded by this classifier. Groups with children are never shader canvases.
Each exception emits both the named effect diagnostic and
`owner omitted: CustomShader adjustment or plain white shader canvas`.
**AEP → FX:** unchanged; this is an export-only unsupported-effect policy correction.

**Structural evidence:** RED queue2938 (baseline lowering from `a9897bc92`)
failed all four requested regressions: shader Adjustment/white Rect named-drop
diagnostics, retained Group/root children, and retained nonwhite Shape. It also
failed mixed/disabled/legacy retention and the white Shape diagnostic control.
GREEN queue2951 passed17 shader tests plus the independent unmapped-effect
media-preparation negative control; fmt/check passed. The introduced Clippy
collapsible-if warnings were fixed; final queued validation is recorded in the PR.
Controls include byte-identical unshaded Group/Shape exports and ordinary movie
staging. Shader texture inputs no longer create native demands or block ordinary
media preparation. Synthetic controls are not independent Adobe feature proof.

The earlier full CPU run2940 had four inherited script-bake/Vector2 failures,
reproduced on base `eabcd021c` in2947. Its fifth failure was this change's stale
shader-based unmapped-effect assertion: replaced with a genuinely unsupported
Chromatic Aberration control, plus a shader-preparation regression (both GREEN).
No inherited test is suppressed or claimed passed.

**All49 source-policy inventory:** hash-verified unchanged archives contain545
shader owners; #4944's classifier excludes all545 versus24 under the corrected
rule (521 owners retained). The old excluded owners contain2495 unique descendant
occurrences; corrected exclusions contain0 descendants. Important: #4944's actual
code promotes children rather than unconditionally pruning every subtree, so2495
is the affected descendant scope, **not a claim that all2495 were dropped**.
Other hierarchy/bounds guards can separately omit promoted descendants. These
counts isolate this shader policy, not total native-converter omissions.
Ignored reproducible evidence: `tasks/accuracy-W09/shader-effect-only/all49-policy-inventory.json`.

Policy-excluded owners / unique descendants in their source scopes:

| Case | #4944 owner / descendant scope | Corrected owner / descendant scope |
|---|---:|---:|
| P001 | 1 / 0 | 1 / 0 |
| P002 | 3 / 0 | 0 / 0 |
| P003 | 0 / 0 | 0 / 0 |
| P004 | 0 / 0 | 0 / 0 |
| P005 | 2 / 0 | 2 / 0 |
| P006 | 15 / 66 | 0 / 0 |
| P007 | 0 / 0 | 0 / 0 |
| P008 | 0 / 0 | 0 / 0 |
| P009 | 7 / 4 | 1 / 0 |
| P010 | 0 / 0 | 0 / 0 |
| P011 | 1 / 0 | 0 / 0 |
| P012 | 0 / 0 | 0 / 0 |
| P013 | 2 / 0 | 0 / 0 |
| P014 | 1 / 0 | 0 / 0 |
| P015 | 0 / 0 | 0 / 0 |
| P016 | 112 / 0 | 0 / 0 |
| P017 | 0 / 0 | 0 / 0 |
| P018 | 0 / 0 | 0 / 0 |
| P019 | 1 / 0 | 1 / 0 |
| P020 | 0 / 0 | 0 / 0 |
| P021 | 1 / 0 | 0 / 0 |
| P022 | 1 / 0 | 0 / 0 |
| P023 | 2 / 0 | 1 / 0 |
| P024 | 0 / 0 | 0 / 0 |
| P025 | 0 / 0 | 0 / 0 |
| P026 | 0 / 0 | 0 / 0 |
| P027 | 0 / 0 | 0 / 0 |
| P028 | 1 / 0 | 1 / 0 |
| P029 | 0 / 0 | 0 / 0 |
| P030 | 0 / 0 | 0 / 0 |
| P031 | 0 / 0 | 0 / 0 |
| P032 | 4 / 39 | 0 / 0 |
| P033 | 25 / 153 | 0 / 0 |
| P034 | 0 / 0 | 0 / 0 |
| P035 | 0 / 0 | 0 / 0 |
| P036 | 1 / 0 | 1 / 0 |
| P037 | 17 / 82 | 0 / 0 |
| P038 | 65 / 0 | 3 / 0 |
| P039 | 81 / 467 | 0 / 0 |
| P040 | 0 / 0 | 0 / 0 |
| P041 | 0 / 0 | 0 / 0 |
| P042 | 0 / 0 | 0 / 0 |
| P043 | 0 / 0 | 0 / 0 |
| P044 | 0 / 0 | 0 / 0 |
| P045 | 0 / 0 | 0 / 0 |
| P046 | 169 / 470 | 9 / 0 |
| P047 | 17 / 1206 | 0 / 0 |
| P048 | 9 / 4 | 2 / 0 |
| P049 | 7 / 4 | 2 / 0 |
| **Total49** | **545 / 2495** | **24 / 0** |

**Visual proof:** per-PR native renders and `validation_cli` were explicitly
cancelled by the user; scoring is deferred to the post-merge full49 rescore.
The stopped P047 attempt never progressed past CPU conversion, so no new native
render, READY result, score, alpha/audio or editable native-control proof is
claimed. Import implementation/proof is unchanged. This correction is not a
strict visual pass and is not gated on per-case score improvement.


## Explicit available-font inventory — import-only Inter fallback

**AEP → FX:** optional `AfterEffectsImportOptions.available_fonts` / CLI
`--available-fonts <JSON-array-file>` declares the available PostScript faces.
Absent inventory retains the previous identities and serialization; an explicit
empty inventory means none are available. No system-font probe, private runtime
catalog, font-name-specific table or new alias is used.

A missing static/held Source Text font becomes an available same-style Inter
PostScript face when declared, otherwise `Inter-Regular`, with empty style to
preserve the exact PostScript namespace established by #4805. `AE-PROPERTIES`
warnings identify the original and replacement and explain changed glyph metrics,
shaping/layout and the requirement to stage/package the bytes. If Inter-Regular
is also absent, the warning explicitly says so. The original name is retained
in diagnostics, not in a new persisted field. An inventory is a declaration,
not proof that font binaries are actually present at export.

**Evidence:** `adapter::fonts::tests::available_fonts_native_import_replaces_unavailable_face_and_warns`
imports the unchanged Adobe-native `pr4442_native/sources/text_document_point.aep`
(93,997 bytes, SHA-256 `bc32cae3896c3aaf2e7a02f5283ce9ef31c9b0ef53f8c8a30a2b6c30b69eea34`)
through the ordinary adapter and checks the published editable archive/report.
With the declared inventory limited to Inter-Regular, pre-fix queue2234 failed:
actual font ArialMT versus expected Inter-Regular. Supplemental tests cover found
faces, held documents, generic style selection, no-inventory compatibility and
an unavailable Inter fallback. This is parser/editable-policy evidence, not a
new independent native-render font oracle. Inter RGB/alpha, Adobe acceptance,
font shaping/layout fidelity and full export are unmeasured.

**FX → AEP:** unchanged and unrun for this increment; an edited/substituted document
names Inter, not the original authored font. No original-font restoration claim.
FX schema, renderer and font aliases are unchanged.

## Twirl owner and frame planes — both-direction editable approximation

Native Twirl operates in source-local effect coordinates; FX Twirl evaluates
post-transform frame UV. For standalone root, unparented Shape/Text owners with a
static planar Transform and no owner mask/matte or Layer Styles, import retains the native
composition-sized input plane, makes the geometric owner Transform identity,
and appends an editable full-composition CornerPin **after** the source effect
stack. This transports the warped image, not just its center. Opacity, source
clock, supported scalar/Point keys and child geometry remain editable.

For eligible FX Groups carrying Twirl with a full-span identity **own** clock
and active window, export puts the current transformed
content and source clock **inside** a frame-sized native composition and puts
the ordered effects on its spatially identity occurrence. Authored static and
animated owner opacity stays on that outer occurrence, after the effect image;
the inner geometry has opacity100 and no copied owner-opacity track. The
`twirl_plane_carrier_applies_fractional_and_keyed_opacity_after_alpha_sensitive_effects`
regression covers fractional opacity, opacity/position/effect keys and a
Twirl/SimpleChoker/bypassed-Blur stack. Twirl and accompanying Group
CornerPin controls use the full frame, not the painted child bounds. No
native source bytes, generated scripts, or renderer/schema changes are used.
Externally referenced owners, masks/mattes, 3D and Layer Styles retain the prior
editable approximation with a contextual staging diagnostic. Direct child
containment references travel inside the carrier and do not block admission;
noncontainer parent consumers elsewhere in the document still do. Nonidentity ancestor
transforms are not compensated by this bounded export staging. A delayed or
remapped own Group clock uses the ordinary editable occurrence with an explicit
effect-clock approximation, not a full-duration carrier whose effects run early.
The nested source-content clock is distinct and remains supported inside an
eligible carrier.

Layer Style owners (including bypassed styles) retain the original Transform
and editable effects with an explicit staging decline: appending transport after
screen-space styles would rotate shadow offsets and scale widths. This avoids
claiming source-plane correction at the cost of an undocumented style-phase change.

Built-in linked-picture import does not add standalone late transport: flattened
Groups evaluate Twirl/CornerPin in the destination frame, not the source canvas.
Caller-supplied pictures with Twirl receive a destination-frame approximation
note too. Their current user-authored Transform, CornerPin, controls and keys are
not undone or rescaled. A different canvas/placement is not repaired by guessing
Point/radius gains; both linked routes retain nearest editable fallback.

The unchanged native `effects_coverage/native_static_controls.aep` composition
625/owner 637 and `native_animated_controls.aep` composition 612/owner 624
contain a 120×80 rectangle translated by 100,50 in a **320×180 native effect
plane**. Their centers 30,60→90,20 are transported to 130,110→190,70 by the
late CornerPin. Unsupported dynamic/3D/ancestor/media-plane import cases keep
their editable original Transform and controls with an explicit source-local
versus post-transform-frame deviation; animation is not frozen to admit them.

`native_twirl_translated_owner_stages_image_and_frame_on_import_and_edited_export`
pins both source hashes, asserts the full-frame image transport, source geometry,
control keys and order, and independently edits the FX Transform and center keys.
Fresh export assertions follow reachable composition edges from the root,
check the 320×180 effect input, identity carrier, single internal translation,
visibility windows, supported control keys and disabled Blur. The existing
`native_twirl_static` and `native_twirl_animated` source-control tests remain.
The standalone structural tests passed before the admission corrections. New
`twirl_plane_nonidentity_own_clock_keeps_ordinary_occurrence_and_window`,
`twirl_plane_rotated_nonuniform_style_owner_keeps_screen_style_phase_and_bypass`,
and `linked_twirl_plane_different_canvas_*` regressions cover the bounded
clock/style admissions and real built-in/supplied public consumption into a
640×360 destination from a 320×180 source. Their execution is pending for this
revision. Combined style/transform/bypass and edited-clock cases are supplementary,
not newly Adobe-authored sources. Independent Adobe native/render proof remains
pending; these tests establish no pixel equivalence.

Kernel, percentage-radius falloff, interpolation and clipped frame-edge/alpha
behavior remain approximations, not measured equivalence. The CornerPin clips
to its transported frame; native outside-frame sampling may differ. Prior
failed native-equivalence evidence is retained and is not relabeled a pass.

## Exact 50/60 fps linked scopes

**Import:** Existing native AEP import retains footage rates, trims and Hold
keys. The new fixtures exercise this path without changing footage
interpretation.

**Export:** Premiere hybrid scopes admit exact 50 and 60 fps in addition to
24/25/30. The AE composition rate and Premiere link clock agree; key times and
footage interpretation are not rewritten. For admitted constant-rate 50/60 fps
AVC MP4 sources, the native footage duration uses the validated presentation
sample count divided by its rate, as AE saves it. It no longer loses a fraction
of a 24576 Hz tick at source endpoints such as 3.2 seconds. Other source profiles
keep their existing admission and precision rules.

Fractional Premiere sequence rates cannot be exact AE 16.16 composition rates.
Their native Premiere output and per-feature limitations remain available with
a warning; no AEP scope changes the cadence. Source-backed tests cover 50/60 fps
composition/media clocks, trimmed footage, Hold opacity keys, muted picture,
Check/Write parity and reimport. Native AE save/reopen retains the exported
50/60 fps compositions, two-second root duration and 3.2-second footage.
Selected frame numbers and Hold boundaries agree in 30 fps AE renders.

Independent-source RGB comparisons remain below the 0.99 mean / 0.98 minimum
limits (about 0.962 / 0.945). All 60 decoded RGB frames at each rate equal the
clean-base direct-AEP control, so this visual residual predates linked-rate
admission; its exact cause is not established here. Native checks relink pinned
media in private copies and do not prove unattended package relocation,
Premiere Dynamic Link, alpha or audible sound fidelity.

**Import:** unchanged. The fixture tests exercise the existing editable AE and
linked-Premiere import paths; no new footage interpretation policy is implied.

## Linked-picture destination media and source clocks

**Export:** selected hybrid picture scopes can prepare otherwise unsupported whole
video sources through the existing media engine, then recheck native AE admission.
Exact H.264 and ProRes-alpha packet clocks remux to video-only MOV without edit
lists; eligible opaque sources use the existing H.264 encoder. Unverified alpha
precision, audio/timecode and unsupported timing, colour or topology reject the
complete selected scope, retaining its native Premiere fallback. An enabled
unmapped effect prevents video preparation for that scope; archive integrity and
ordinary media interpretation still run. I/O, cancellation and malformed backend
results remain fatal. Ordinary standalone export admission is unchanged.

Verified QuickTime sources retain exact 24576 Hz duration when representable.
Floor-millisecond intrinsic aliases additionally require remap keys/control hulls,
static samples and affine endpoints inside physical EOF. Held source clocks use
equal enabled Time Remap endpoint keys; ProRes 4444 keeps alpha. Image and legacy
media transfer matte ownership to the native wrapper; canonical Video keeps its
Alpha-only guard. A failed provider still removes dependent owners.

Short positive-affine Group occurrences can bound child animation over the visited
source interval without changing source duration or keys. Bounds include authored
and native tick-rounded curves. Group-owned transforms, Boolean subtrees and
unproved clocks, spatial paths, Scale, motion blur or temporal effects retain
all-time bounds. These guards do not certify arbitrary RGB/matte appearance.

Focused tests cover source clocks, native Time Remap bounds, fractional duration,
packet-preserving preparation, complete-scope fallback and editable linked
placement. An unchanged generated package was observed online and rendering in
interactive Premiere Pro 2026, with Link Media disabled. That is one-machine
acceptance, not full relocation, edit-propagation or RGB/alpha/audio fidelity proof.
Managed offline observations remain separate headless-path evidence. No new
comparison result is claimed.

**Import:** unchanged; linked pictures retain the existing editable conversion.
Direct linked-AE audio occurrences remain unsupported and diagnosed. Native
Premiere nest-audio conversion is a separate existing path.

## Remapped Video source-owned Opacity — bounded omission repair, fidelity blocked

**AEP → FX:** implementation unchanged. **FX → AEP:** a full-canvas, full-source,
identity-geometric 2D silent Video with native keyed playback and sampled Linear
source-owned scalar Opacity can retain its movie in a noncollapsed editable
source-clock precomposition. Inner direct footage owns the Opacity and optional
Mosaic controls on a unit source clock; the original outer occurrence retains
Time Remap/Bezier/Hold, visibility, identity, order and its proved containing
Group relationship. Explicit non-containment transform parents remain rejected.
Controls are not moved to occurrence time, inverted, flattened, pre-rendered or
replayed. The general native Time Remap/animated Transform guard is unchanged.

Admission rejects trimmed/source-switched media, dynamic/nonidentity geometry,
3D, audio, non-Linear Opacity, frame blending/motion blur, non-Normal blend,
masks/mattes/styles, effects other than Mosaic, and unproved outgoing owner
references. Existing source-duration and native control-hull checks remain;
remaps reaching beyond Adobe's frame-normalized source container are rejected,
not shifted. Full file bytes and original playback values are preserved. Native
continuous footage sampling and fitted script/effect values remain approximate.
CustomShader is not supported or translated by this repair.

The public `source-clock-opacity` fixture independently authors a moving/timecoded
source, source Opacity 0/40/80ms -> 0/50/100 and source Mosaic with nontrivial
outer remap. `remapped_opacity_independent_native_source_import_keeps_editable_controls`
asserts imported editable source keys; export regressions assert a retained movie,
inner Opacity/outer remap partition, sibling retention, source endpoints and
negative profiles. These structural assertions do not establish import rendering.

Actual unchanged P019/P028 owner8990 movies are retained and managed full18s
1920x1080/30fps exports completed. Canonical full-resolution RGB24/rgb-hybrid
.25s comparisons (73 samples each) improve historical repair-02 means
0.1158999143 -> 0.7338091732 and 0.1114717506 -> 0.8241671068. The historical
before is not a same-head causal native comparison, and neither original case
passes 0.95 or 0.99.

**Visual proof limitations retained:** the complete independent public comparison
averages **0.9147157880 < 0.95** (minimum0.8403558544). The separate native-rate
onset panel averages0.9908259407 but cannot substitute for full-video fidelity.
Fresh half-Opacity input exports respond at seven of eight visible fixed RGB
samples within the predeclared0.49..0.51 ratio; the earliest sample0.4819727409
still fails. Two additional reviewer-authorized managed numeric operations close
saved/reopened edited-key readback: original0/50/100 versus edited0/25/50 at
identical times; all eight nonzero native samples respond exactly by half within
floating-point precision, including that earliest RGB sample. Numeric controls
are not a claim that the failed lossy RGB measurement passed.
The native comparator is an unmatched fixture: independently authored project
32bpc versus generated8bpc, working space `None` in both, and the FX input lacks
the native8×8 green helper sibling. Native source-time samples, Mosaic controls,
quality/sampling switches match. The full-frame loss cannot all be attributed to
clock/footage behavior; no matched-bit-depth rendering or complete causal pixel
attribution was performed. Alpha/audio and long-term Asset publication remain
unproved. See fixture `README.md`, `proof.json` and `reviewer-proof.json`; no
expected/reference movie is committed.
This is a Ready omission improvement based on semantic/native edit proof and
actual-case gains, **not a passing formal feature-reference case**. Scoring,
pinned references and all historical failed measurements remain unchanged.

## Two-key Linear Rotation offset loops — bounded import correction

**AEP → FX:** a complete `loopOut("offset")` on native Z Rotation with exactly
two finite scalar Linear keys is analytically the same affine line after the
last key, with the ordinary constant value before the first key. Import retains
both native keys and adds at most one constant-prefix and one linear-endpoint
key through the receiving composition interval. Parent-copy preparation uses the
whole composition interval, not the native parent's own lifetime. Signed native
start/stretch clocks and ordinary FX millisecond rounding still apply. There is
no frame sampling, cycle expansion, script, shared FX change or source-name rule.
Nonlinear/Hold/multi-key curves, other loop forms, invalid clocks and nonfinite
results retain the existing diagnosed expression fallback. Transform-member
alias reads also retain that fallback: their sampling interval can exceed the
source composition, so this finite preparation is not advertised as a live alias
curve. Later edits to independent FX keys do not preserve a live native
offset-loop relationship.

The licensed Technology Tittle source SHA-256
`f01be9f6725512ac6502d6cf6fc1e258eac1db38c706558e2b4e9c8ae95bd86a`,
composition 1, layers 153/154, provides opposite-sign native Rotation keys:
0° at 0s and −20°/+20° at 1.8s, with the complete offset expression. The local
source-pinned `technology_native_rotation_offset_loop_keeps_linear_motion` test
was removed; recorded results are historical evidence only (no longer
executable). Licensed bytes are not redistributed. It checks both editable tracks through 10s, not Adobe intermediate
readback or native raster equality. Helper tests separately cover grammar and
unsupported profiles. Execution status belongs to the PR evidence, not this
inventory. Independent native 30fps reference/Asset publication, RGB/alpha/audio
fidelity and Adobe acceptance for this correction remain unproved.

**FX → AEP:** existing scalar-key export is unchanged. A fresh edited export,
independent native control inspection/render and restored offset-loop expression
are unrun; the imported finite Linear keys are not a native expression exporter.

## Omitted shader-only Adjustment does not discard animated Group content

**FX → AEP:** animated Group support analysis excludes a nonempty all-CustomShader
Adjustment stack only when every effect has the same absent native mapping used
by lowering. Lowering still checks that no native effects/styles were emitted and
omits that Adjustment with its precise diagnostic. Custom shaders remain unsupported;
no shader recognition, translation or approximation is added. Empty, mixed, native
and unknown stacks retain existing conservative bounds checks. Supported siblings,
order and editable animation survive; no source content is flattened.

**Evidence:** `omitted_shader_adjustment_keeps_its_containing_group` failed in CPU
job2095 before the fix and passes after it. Native managed numeric control and
fresh generated/edit acceptance job2756511df9cf42aea17fe637bd14264f validates
three supported siblings and two Rotation keys against independent native values
at six times, including the +10 degree source edit. Actual P048/P049 exports retain
Group20500 while Adjustment9 remains omitted. P048 diagnostic full-canvas RGB24
comparison (183 samples, 0.25s) changed from historic mean0.52243074 to0.52595712;
strict0.99 still fails, minimum0.00807028. Historic baseline uses a different
converter base, so this is not isolated causal attribution. Reviewer-authorized
P049 fresh normal export/render jobd8b6d5db19074ec9994ce5d75e6916e4 scored
mean0.52928628 versus historic0.52536100 across181 samples; strict0.99 fails,
minimum0.00910212. Its historical base also differs. An initial copied package
failed media pinning and returned verified READY; the fresh export passed.
Native30fps control Asset publication, alpha/audio and general fidelity remain unproved.

**AEP → FX:** unchanged.
## Skewed Text helpers inside Groups — composition-local export parenting

**FX → AEP:** static planar Text shear still uses the existing three editable
Null helpers and original Text drawable. Structural containment is now finalized
by the enclosing Group plan: a native Null Group parents the placement helper;
a precomposition leaves that helper at its source-local root. Internal
placement/factor/basis/drawable references are unchanged. Previously the outer
Group occurrence could be referenced from inside its own source, and the writer
correctly rejected the whole Group with `native layer reference crosses or
escapes its composition`. The reference validator remains strict.

Actual P046 Group7140 contains twelve Text owners, ten with fixed shear, and
animated Group opacity. Its entire subtree was omitted by this reference error.
The explicit regression reproduces that omission before the fix; targeted tests
now cover both precomposition-local and ordinary Null parenting plus unchanged
foreign-parent and unsupported-skew rejection. These are offline editable-record
assertions, not independent Adobe editable acceptance. A full fixed-original
native render succeeded (959 frames, 1080×1920, 30fps, cleanup/READY). Canonical
RGB24/rgb-hybrid/.25s validation against the unchanged original measured mean
0.16721897064437796, minimum 0.00008168827055770724 over 128 samples, all below
.99. This after-only result does not establish improvement. The latest-main
before render timed out at the caller's default 120-second execution deadline;
a later independent-control assertion failed before generated readback began.
Both author failures returned verified READY. Reviewer managed native acceptance
then passed on Adobe 26.5x89 (job1981ae7e289e4b3d832dc1f6f8cb896b): all twelve
editable Text owners, thirty composition-local helpers, enabled Group occurrence
and four opacity keys were read back in both original and input-edited packages.
The Text7358 skew 6.5→7.5 edit changed its native helper values; the eleven other
Text observations and Group keys stayed identical. An independently authored
source-local three-helper chain also passed. Native foreign-parent assignment
behavior was recorded rather than assumed to reject. The reviewer's first call
failed because ExtendScript lacked JSON.stringify; cleanup/READY was verified,
and the corrected ES3 comparison passed within two managed operations. This is
editable acceptance, not RGB/font fidelity or saved/reopened edit proof. A
comparable main68493c4c before render then passed (joba639312cde3c47c082cceb2dea8f6097,
immutable converter job2270), with the same 959 frames, full1080×1920 canvas and
30fps. Unchanged canonical RGB24/rgb-hybrid/.25s validation measured before mean
0.16720334487995803 versus after0.16721897064437796 (delta+0.00001562576441993),
128 samples each; minimum0.00008168827055770724 in both. This very small measured
whole-case improvement is not a claim of broader fidelity; other P046 omissions
remain independently owned. Both means are below0.95. Font/alpha/audio
equivalence and any new skew range remain unclaimed.
No CustomShader support, runtime/schema/renderer change, flattening or source
asset replacement is introduced.

## Adjustment import diagnostics — false omission correction

**AEP → FX:** the intermediate contentless Adjustment carrier no longer claims
that FX has no cross-layer equivalent or that its contribution was omitted.
The existing final lowering already emits an editable direct `AdjustmentLayer`
with supported effects, timing, masks and matte relationships. Its successful
lowering and actual unsupported-effect/geometry/alpha diagnostics remain.
This corrects a misleading warning only: emitted editable content and pixels
are unchanged. Animated dry-plus-wet alpha behavior, Simple Choker kernel limits
and unsupported native effects are not repaired or hidden by this change.

The unignored regression
`adjustment_import_diagnostics_do_not_claim_retained_effects_were_omitted`
freshly imports the independently Adobe-authored, SHA-pinned
`adjustment/native_controls.aep` composition 1 (`adjustment-scope`) and asserts
retained editable Gaussian Blur 19 plus truthful direct-lowering diagnostics,
without the contradictory omission warning. It failed at the diagnostic assertion
before correction in shared CPU job1786; job1783 failed earlier at the required
case-to-test link and is not semantic RED evidence. Existing source/reference
identities and historical registry proof statuses are unchanged.

**FX → AEP:** implementation and proof unchanged. This diagnostic-only correction
adds no native open/render, RGB/alpha/audio or bidirectional feature-fidelity proof.

## Static native effect control editability — bounded export repair

**FX → AEP:** native plugin roots now use the independently observed hidden
sentinel descriptor instead of a checkbox descriptor. Static integer, checkbox,
popup, fixed-point/angle and floating-point leaves use the existing native plugin
storage variants, as keyed controls already did. Previously Adobe read their
values but rejected edits because a property or parent was hidden. Native bounds,
units, parameter definitions, owner identities and effect stages are unchanged;
no clamps or render approximations are introduced. Static Color and Point leaf
serialization is unchanged. The root correction applies to plugin instances and
canonical project definitions; definitions still contain no owner edits or keys.

**AEP → FX:** implementation and import proof are unchanged by this export-only
repair. A fresh export/own-reader round trip is supplementary structural evidence,
not independent import fidelity proof.

**Evidence:** `effects/native-static-effect-descriptors.rifx` pins independently
Adobe-authored Mosaic, Posterize and Gaussian Blur records with provenance beside
it. `native_static_plugin_descriptors_allow_editable_controls` compares complete
native descriptor/storage bytes for all seven edited controls and their three
roots. The original managed Adobe 26.5x89 discriminator rejected all seven edits
before the repair and accepted Mosaic 48→49/27→28/sharp 1→0, Posterize 7→8,
Gaussian Blur 6→7/dimensions 1→2/repeat 1→0 afterward. The independent source
controls were authored in a disposable project inheriting global definitions;
this is a static-control oracle, not independent proof of the full definition
catalog. Native hard bounds and type/unit readback were preserved. Source artifact
SHA256 `fe3d6c0c3160c057011d97bd416d3521737f7914e26819ab927d096c015d9286`;
fixture SHA256 `0e6c2f9d32b6fdb1734d04e20cec432e2275aff446fcae9d287abbf3fa6e3981`.
Adobe omits native-default static leaves when saving, so the independent edited
revision `effects/native-static-effect-edited-descriptors.rifx` supplements, rather
than replaces, the baseline oracle. Its SHA256 is
`59b6cd7ce73f38567f035d0c0f2179fd61995561b1afd9e6f8e0f495a667e6bc`.
Managed reviewer job `466d7079652e48bc99d25041cbaf9d32` verified all seven native
edits, preserved bounds/types/units, and exact controls after same-host save/reopen.
At this descriptor checkpoint, fresh edited FX exports retained Mosaic
49/28/sharp false, Posterize 8 and Gaussian Blur 7/repeat false. The subsequent
Mosaic center-sampling correction below normalizes the exported checkbox only with a proved raw root canvas domain. Gaussian Blur's FX mapping remains Both-only; a horizontal
native popup edit is independently editable but is not a supported FX dimension
edit or a new mapping. The source artifact SHA256 is
`46d2c9c5f5ff8266ea7948baa3df47aa38e05f6a65bb86f57be9ef859177dc14`.

**Limitations:** those seven controls do not prove native editability of the full
catalog, signed/decimal boundary behavior, Grain or Corner Pin. Same-host save/reopen readback is established for the controls above, not fresh-host
reopen or the entire catalog. Independent full-duration 30fps render/long-term
Asset publication, original-project RGB, alpha and audio fidelity remain unproved.
They remain separate proof requirements, not implied by descriptor equality
or successful native edits. Existing feature approximations and omissions remain.
## Mosaic center-sampling normalization — bounded root-canvas export correction

**FX → AEP:** FX currently ignores `sharpColors` and samples block centers.
Export normalizes false to native Sharp Colors on **only** for an ungated,
full-opacity, normal-blend TwoD root Adjustment on the unchanged document canvas,
with measurable contained raw geometry below it. TwoD owner position, scale,
rotation, skew and anchor are inert in FX and the native Adjustment solid.
ThreeD owners, including a ThreeD position with z=0, are excluded because depth
sorting can change the backdrop before the Adjustment captures it. The first proved profile is
flat static identity-affine, unrounded integer Rects or plain Paths, without
masks/mattes, strokes, generators/modifiers or other child effects. Only Mosaic
may be active on the Adjustment; bypassed sibling effects remain in place.
Existing enclosure arithmetic is used after these exclusions, never as proof
that native painted/masked/projected bounds equal renderer raw geometry.

All other domains retain the authored checkbox and receive contextual unsupported
sampling-domain diagnostics. This **does not** make native averaging faithful to
FX center sampling. Escaped geometry, animation/dependent geometry, shifted or
nested planes, tilt, unknown Text/media, parents, backgrounds and gated Adjustments
are not admitted. Counts, count keys/Hold clocks, bypass, effect order, transforms,
owner bounds and source FX bytes are not rewritten. Premiere admission is unchanged.

**AEP → FX:** unchanged; import retains the saved checkbox and existing
approximations. No FX block averaging or renderer change is implemented.

**Evidence:** `mosaic_root_canvas_normalizes_native_backed_controls_without_changing_keys_or_bypass`
freshly imports pinned Adobe-authored `effects/catalog.aep` composition58's
10×10/false controls, then uses a supplementary explicit edited-FX root Adjustment
profile. It checks normalized native controls, 48→120/27→68 count keys at0/36ms,
Hold interpolation, source true, bypass and sibling order. This is structural
export evidence, not a newly independently Adobe-authored eligible Adjustment.
`mosaic_unproven_leaf_retains_checkbox_counts_clocks_and_scales` checks retained
false/true controls and unchanged keys across escaped/scaled leaf domains.
Source-based negative tests cover masked raw escape, tilt, unknown Text, keyed
escape, gated/background/nested planes and raw cubic control-point escape.
Four unsupported-domain regression symbols fail before the restriction.
`mosaic_root_canvas_rejects_three_d_owner_even_at_zero_depth` checks authored
checkbox preservation when an escaped declared-above ThreeD Rect can enter the
backdrop. `mosaic_root_canvas_accepts_inert_nonidentity_two_d_owner_geometry`
checks combined TwoD owner geometry with the same declared-above Rect.
These are supplementary converter regressions, not executed FX/native renders.
The existing Premiere on-control oracle establishes the native center kernel;
it is not new independent AE export or RGB/alpha evidence.

**Preserved regression:** original Spam's animated escaped domain remains
unsupported. The unconditional checkbox normalization regressed worst similarity
0.5419929018→0.5108567485 and mean0.7342052577→0.7336762409; central displaced cells,
not only outer-edge extension, dominate the visible difference. The restriction
removes that introduced control change without claiming to solve the animated
domain mapping or improve measured pixels. Original scores/references are not
replaced or tuned.

**Limitations:** even eligible profile export RGB, alpha, audio, independent AE
open/render acceptance and native editing that changes eligibility are unmeasured.
An eligible saved false checkbox is normalized with an explicit diagnostic;
unsupported domains preserve controls, not fidelity. No transformed/animated
sampling-domain implementation or full-project conformance pass is claimed.

## Fractional source endpoints from shifted affine Video windows — bounded export

**FX → AEP:** positive canonical `windowed` Linear Video mappings now retain
native rational source in/out points when a visible-window trim or signed input
offset maps its integer parent endpoints to fractional source milliseconds. The
latent mapping, rate and nominal native start are preserved; this is not endpoint
rounding or a shifted source asset. Source selection/intrinsic bounds and native
rational field limits remain checked. Static source Time Remap, Audio, temporal
source selectors and fractional property-key rebasing keep their existing guards.
The existing half-millisecond visibility correction and continuous-native versus
FX sampled-source diagnostic remain; no frame-selection fidelity is claimed.

**Evidence:** `affine_video_shifted_window_preserves_fractional_source_endpoints`
failed before the mapping (Rust1725), then passed (1732). The actual packaged,
hash-pinned 8s `media_native_panel/media/movie.mov` full-export/input-edit test is
`media_video_fractional_affine_window_full_export_and_input_edit` (1745 passed).
Canonical mapping input `[0,3000]` → source `[250,4250]`, offset `+250ms`, visible
input `[1000,2000]` gives stretch `3/4`, start `-7/16s`, nominal source endpoints
`23/12s` and `13/4s`. Editing only the visible input to `[1250,2250]` preserves the
mapping/start and gives nominal source endpoints `9/4s` and `43/12s`.

Independent Adobe26.5x89 author/save/reopen/sourceTime readback was managed job
`72052135202b49ff890c79e9c0fe27b9`; generated-output acceptance and the input edit
were independently read in job `ca821389b5aa4a898e7b8638844a6e95`. Both retained
the byte-identical MOV (SHA256
`a383d0058de7ce9723e263611acd35a438cf1154fbbfe8e0f1a1f78e74546292`) and verified
managed cleanup/READY. Native sourceTime matched the exact positive affine map
at five samples per version; existing corrected native visibility was
`0.9995..1.9995s`, then `1.2495..2.2495s`. Temporary probes were removed and no
source/reference was overwritten. These are native control/editability observations,
not human UI inspection or a render comparison.

**AEP → FX:** implementation/proof unchanged, including independent endpoint
rounding for nonunit imported stretches. A negative Linear output span is not an
FX range; reverse remains the already-supported descending TimeRemap profile.
Original49 applicability, independent discriminating native30fps render/Asset,
RGB/alpha/audio and general affine/hierarchy fidelity remain unmeasured. The
uniform-color movie cannot prove visual source-frame selection. Local provenance
and generated packages remain in ignored `tasks/accuracy-W01/`; no original49
reference or private media is published by this repair.

## Shared zero-speed Point Bezier export — native control acceptance

**AEP → FX:** existing straight zero-speed Point import is unchanged. A new public
independently Adobe-authored source, `properties/point_zero_speed.aep` (SHA256
`340f2fd47d09d9452ae1c511847d511ca222787b8611735e7f99f4b49ab81ef6`, comp1),
freshly imports two editable scalar Point tracks at500/1500ms with original
normalized values and zero-speed Bezier ease. Native author/save/reopen job
`3ff0735957534a60a6c9e69758f0669b`, AE26.5x89, preserves a120x80 owner plane in
160x120/24fps/2s comp; provenance/control samples are in the fixture Markdown.
This is concrete source-based structural/native-control evidence, not render proof.

**FX → AEP:** Point animation now accepts one shared two-axis CubicBezier timing
function with zero endpoint slopes (`y1=0,y2=1`), valid nonzero endpoint influence
and zero spatial tangents, using the existing editable native spatial Point writer.
Unsplit authored spans retain exact endpoint values/easing instead of unnecessary
numeric inversion. Independently eased axes, nonzero-speed cubics, curved spatial
handles and cubic Color tracks remain unsupported; contextual Point base-retention
and supported siblings remain. Differing knots may split into unsupported profiles
and retain the warning rather than gaining a guessed mapping. No frame baking,
endpoint shifts, tolerance relaxation, FX runtime/schema changes or donor replay.

`point_zero_speed_native_source_exports_and_edits_without_static_fallback` fails
before repair on the explicit cubic-Point omission (queue1700), then passes after
repair along with three exact-profile/negative tests (queue1717). It freshly exports
the imported editable document and edits the end Point [72,33]→[96,56] pixels.
Independent managed Adobe acceptance/readback job`bfc8cc763ef5403987f1bed1b6484cfa`
opens both generated projects, resolves exact CompItem IDs1/13 plus name and exact
Point matchName, and confirms500/1500ms keys, zero speeds, one-third influence,
120x80 owner and before/after endpoint holds. Original midpoint is
[66.0000932824579,36.4999455852329]; edited midpoint is
[78.0000746750209,48.0000331888982]. Original output SHA256
`75993b3c7d5314543bdd8b54478758e7e86f19a5ca0045757b488a2456e615a4`;
edited output SHA256`a437f62bfc7c43fc0e9da6ab752cae9281d6b708f327c4105563015d860ea69f`.
Both native operations completed cleanup/READY. Unused boundary interpolation flags
are normalized; byte-identical key metadata is not promised. Adobe's numerical
spatial evaluation is retained in reported samples, not relabeled exact analytical
progress. Underlying Radial Blur mapping remains a diagnosed kernel/mode approximation.

Independent30fps reference, long-term Asset publication, RGB/alpha/audio comparison,
original49 fidelity and general Point/Corner Pin native coverage remain incomplete.
This bounded native-control acceptance is not a complete bidirectional feature proof.

## Editable Source Text stroke controls — export mapping, native acceptance blocked

**FX → AEP:** existing Text `StrokeEnabled`, `StrokeColor` and `StrokeWidth`
layer-property constants and Hold keys now feed the whole Source Text document
instead of the Transform gate, which previously omitted the entire Text layer.
This is distinct from Text Animator `strokeWidth`. The existing native writer
stores enable/color/width; `strokeOverFill` remains the typed document's existing
value. No FX schema/runtime, glyph bounds, font defaults or native writer profile
changed. Width is finite/nonnegative; Boolean and color controls retain exact
existing typed/range checks. Dependent/JavaScript controls remain rejected and
continuous interpolation remains diagnosed/omitted. Disabled animation uses its
runtime-visible constant; dormant stroke values do not require native getters.
The existing writer validates completed documents, including enabled color.

**AEP → FX:** unchanged Hold-segment import. The new independent minimal Point
and Box source [source_stroke_holds.aep](../crates/aftereffects_file/tests/fixtures/text/source_stroke_holds.aep)
(SHA-256 `b9f2c8a8d5c1154d474e8f126a4235b09377a6c1036f64d8e376b966da4fa9ca`,
composition1) has enabled RGB/width3→9 keys at0/.5s and stroke off at.75s.
Adjacent JSX/provenance records native width7→9 editing, save/reopen with fresh
handles, text/fill/baseline/alignment and `strokeOverFill=true` readback. Disabled
color/width/order getters are intentionally unavailable; their saved-byte import
assertions are supplementary, not native getter proof.

`source_stroke_native_import_preserves_enabled_controls_and_disabled_state`
freshly imports this source. `source_stroke_full_export_consumes_editable_hold_controls_and_input_edits`
exports explicit editable controls derived from its enabled states and verifies
width9→11 edits, enable/color/order, sibling styles and Hold times through our
reader. Before the mapping, both Text layers were omitted (queue1693 RED);
afterward six targeted tests passed with zero ignored (queue1710). Final queued
converter check/clippy/fmt passed (queue1720); its AE suite reported1672 passed,
12 failed and411 ignored. The12 failure identities exactly match prior baseline
queue1620; the full suite is **not green**. These are structural assertions, not
Adobe acceptance or fidelity.

Generated acceptance request `accuracy-w03-source-stroke-acceptance-20261004-v1`
(job `87bb8e27ad124281b6039d4e7dcda810`) failed opening the fresh width11 export:
`Error reading the text layer. Skipping the text layer.` Managed cleanup/fresh
READY passed. Generated native control/edit/save/reopen proof is therefore
**blocked**, not passed; the preexisting Text writer/default-profile gap is
untouched and no old4807 dependency is stacked. Source author/readback job
`107a1a12143b4e938b470049b570e847` succeeded. Three total bounded native calls
include the preserved first disabled-color getter failure. No models were used.
Native30fps reference render/long-term Asset, RGB, alpha, audio and original49
proof remain unrun/unmeasured. This partial export repair stays Draft.

## Ellipse with a static close-only Path placeholder

**FX → AEP:** a single Ellipse generator may retain an empty or close-only
static Path placeholder. FX rendering replaces that Path with the Ellipse;
the placeholder contributes no contour. Export retains the existing editable
native Ellipse size, position, paint and transform rather than omitting its layer
as multiple geometry kinds. Actual49 P027 owner2111 and P020/P048/P049 related
buttons contain this profile. Real Path commands, Path animation and combined
Ellipse/PolyStar remain guarded; no bounds are guessed and no source is rewritten.

**AEP → FX:** unchanged. Independent Adobe26.5x89 author/readback job
`0bc520abb76644faab8ab20c8e4accd8` observed an82×82 native Ellipse and an edit to
110×60 with matching native sourceRect dimensions. This establishes the native
representation/control response. Fresh generated-output Adobe acceptance/edit
job `02ec3d7d9cf94b3483e97543ee5dd1b4` independently read82×82, accepted110×60,
and observed matching sourceRect dimensions. Targeted export regression and
negative guards are in `export_document::tests::parametric_placeholder` (RED1/2,
GREEN2/2 in CPU1913). Original P027's fresh export retains the button and reports
no omissions, but full native render job `d8049798126446dda1d21497eee24e9d` fails
with the existing generated-Text reading error and returns verified READY.
Historical before validation was freshly rerun at full1920×1080/RGB24/.25s:
mean0.585773369675115,min0.3360531203136993,41samples. After RGB remains UNRUN;
no score improvement or .99 pass is established. Formal30fps Asset publication,
alpha/audio and broad corpus fidelity remain unverified.

## Static same-composition Source Text content aliases — Intro import correction

**AEP → FX:** exact `thisComp.layer("name").text.sourceText` aliases copy a uniquely
named static target's decoded string into editable consumer Text. Resolution uses
the effective occurrence composition after Essential overrides. Consumer font,
leading, style and supported animators remain local; target styles and cached glyph
layout are never copied. Live linkage is replaced by independent editable text and
diagnosed. No general expression evaluation or `JsScript` is generated.

Admission requires one unkeyed point-text consumer document, identical character
records, and explicit equal paragraph justification codes 0–3 in every run.
Differing unmapped paragraph controls retain the existing consumer mapping and an
owner-local approximation warning: **this is content preservation, not paragraph
fidelity**. Uniform unmapped paragraph fields (including unknown COS controls) are
also explicitly diagnosed as omitted; field equality never establishes support. Missing/duplicate/self, keyed/expression-backed targets and keyed,
box/path/mixed-character consumers retain authored content with diagnostics.
Target static admission also requires the native Source Text `tdb4` signature;
malformed metadata retains best-effort target text but cannot authorize copying
that text into a consumer. The native-fixture-derived malformed-signature case
failed through the production Converter in shared CPU job1478 before correction.
Disabled consumer expressions retain authored content. Unsupported animator
expressions retain separate diagnostics; stale caption-width/cache admission stays
closed even after content resolution.

**Evidence:** licensed Intro source SHA256
`f315ca13611716c924825ac5b16a8a8a5b4f1fdb95261a824f2ab7393a0209f4`, composition
1544/layer1549, target1550. The pinned native
`pinned_intro_source_text_alias_keeps_consumer_style_and_animator_diagnostics` (removed; historical)
failed before implementation (shared CPU job1267: stale blog/opener vs Thank You)
and passed in job1373 with font/leading/point-text plus paragraph/animator warning
assertions. Source/media are not redistributed. Four public supplementary tests
cover parser suffixes, paragraph admission, native-fixture-derived target/consumer
rejections, and actual Converter occurrence override isolation. Job1373 selected
all five tests, five passed. Delivery review additionally exercises the target/
consumer rejection matrix and explicit/missing/mixed/unsupported paragraph codes
through the real Converter, with owner-local unknown-field diagnostics and cache
admission assertions. The licensed native assertion also uses fresh full Converter
output. Mutated fixtures are supplemental, not Adobe proof.

**FX → AEP:** implementation/proof unchanged; live alias restoration is not
implemented. No new Adobe readback/render/acceptance, independent feature MP4/Asset,
RGB/alpha/audio/font comparison or 99% fidelity proof is claimed. These remain
unrun/unmeasured, separately from the native-source structural regression.

## One-vertex closed cubic Path — native editable export correction

**FX → AEP:** after folding a repeated closing endpoint into the first native
vertex, a contour with one vertex remains valid. Its outgoing/incoming handles
can define a drawable closed cubic loop. Export previously rejected fewer than
two distinct vertices and omitted these supported Paths. The writer now retains
the one-vertex closed contour; existing finite-coordinate, native-list-size,
command, seam-folding and topology/easing guards remain. No outlines, runtime
changes, source replay, flattening or invented geometry/bounds are introduced.

The independent native fixture `geometry/one_vertex_loop/native.aep`, SHA256
`9908039bd36629cf8121841cbaea381558bc0c4b5d445395a394d53f1e89f753`,
comp1,320×240,24fps,2s, was authored/saved/reopened with managed AE26.5x89.
Native vertex[0,0],incoming[90,-100],outgoing[-90,-100],closed=true were observed.
`native_one_vertex_closed_cubic_survives_fresh_export_and_vertex_edit` freshly
imports the source, asserts editable cubic geometry/handles/closure, freshly
exports it and checks a40px vertex translation applied to both typed bases and
authoritative constant Path entries. RED1406 diagnosed omitted Path owners;
GREEN1416 passed after repair and correcting the input edit to affect its
runtime-authoritative values.

Managed independent native readback opened fresh unedited/edited exports and
verified all generated contours: one closed vertex[0,0] or[40,0], with unchanged
relative incoming/outgoing handles. This is native editable control/input-edit
proof for a bounded static case, not a count-only/own-reader assertion. Existing
paint splitting emits three native Paths; equal source/export topology ownership
or appearance is not inferred from their counts. Author/readback scripts and
sanitized provenance are pinned beside the fixture.

**AEP → FX:** implementation unchanged; the native-source test establishes the
specific editable MoveTo/CubicTo/Close import and handles. Native Shape Path
animation, arbitrary one-vertex degeneration, paint/raster fidelity, alpha and
audio are not established by this static case. The independent source's native
30fps reference has60 nonblank frames over2s, SHA256
`9e15978b50c922557535c3f8048b15973e7c2898f160a8271522e52bc3a7e5ee`;
source24fps/bytes were preserved. Reference Asset publication/fresh download
and strict RGB/alpha comparisons remain incomplete. No model scoring was used.
This is an independent minimal supported control, not original49 improvement.

## Reversed native Bezier Path values — editable key correspondence repair

**AEP → FX:** native explicit Path direction 3 now reverses the authoritative
constant and every editable Path key along with the static base. Closed contours
retain their first anchor and reverse the remaining vertex order while swapping
cubic handles. Key IDs, time, easing, spatial metadata and typed animator state
are retained; no schema/runtime changes, generated JS or flattening are involved.
The prior base-only reversal was overwritten by the unreversed live keys.

**FX → AEP:** the existing native Path writer exports the corrected editable
values, including edits to those values, rather than replaying source bytes.
The independently authored [three-vertex Bezier control](../crates/aftereffects_file/tests/fixtures/geometry/animated_direction/README.md)
(comp1, native24fps, two Linear keys) has a source-based RED→GREEN regression:
`native_reversed_bezier_keys_keep_vertex_and_tangent_correspondence_through_export_and_edit`.
Fresh import, full export/reimport and a +30X editable input change passed.
Separate managed Adobe opening/readback of both fresh exports passed with two
keys, reversed vertex order and swapped handles, including the edited coordinates.
Own-reader checks remain supplemental to that native readback.

**Proof limits:** the new source was independently authored/saved/reopened in
AE26.5x89; exact controls and hashes are pinned in its evidence file. No new native
30fps reference render, long-term Asset/download, strict RGB comparison, alpha,
audio or original49 movie improvement is claimed. General easing, compound
contours and malformed keys are not established by this tiny control. This is
partial bidirectional editable proof, not a full feature-fidelity pass.
## Aligned shared cubic Effect COLOR — bounded editable export

**FX → AEP:** Effect COLOR accepts exactly aligned original RGB knots with the
same destination easing on every changing channel and constant alpha. Static RGB
siblings retain their values. Native COLOR stores one temporal speed in 255-scaled
RGB vector-distance units. Generic Color, Point and scalar key writers are unchanged.
Unequal knots, differing active curves, mixed active Hold/Linear/Cubic, changing
alpha, invalid handles/nonfinite speed or native tick collisions retain a contextual
omission/static fallback; no resampling, baking, JS or source-byte replay is used.

**AEP → FX:** existing unchanged-alpha shared-vector COLOR import is unchanged.
`aligned_color_cubic_independent_native_import_preserves_edited_rgb` consumes the
new independently authored source `effects_coverage/aligned_color_cubic_native.aep`,
SHA256 `cd84c6fc05ab2c5f960c0660fcc367d18ad013089cc987596e29c185c60e1c2e`,
composition 1. Its saved phase is the red-edited endpoint (.9); original (.8) and
edited observations are separately retained in its provenance JSON, not replaced
by converter output. Managed job `a74a9362e51946ed96e6f8639d69193a`, AE26.5x89,
measured 14 original/edited native control samples; worst channel error 2.39e-8.

`aligned_color_cubic_full_export_retains_keys_and_endpoint_edit` freshly exports
both explicit public FX inputs, asserting editable keys and untouched white/amount/
default Tint alpha. Queue1558 was semantic RED (missing COLOR animation). Own-reader
assertions are supplementary. Queue1583 passed all four targeted COLOR tests,
including fresh native-source import. Fresh generated original/edited AEPs from
build1591 passed managed native acceptance job `1c9d606a100a41498c1371602a06b6a8`:
21 native samples including the unchanged independent source, worst channel error
2.39e-8 versus5e-7 tolerance; native keys/shared ease/input-edit/siblings passed,
cleanup/fresh READY verified. Exact hashes, getter observations and executable
assertions are in [the case record](../crates/aftereffects_file/tests/fixtures/effects_coverage/aligned_color_cubic.md).
This establishes only the bounded control profile, not render fidelity. Final
rebased targeted queue1654 passed37 COLOR-filter tests (5 existing proof-backlog
ignores) plus standalone clippy/fmt. Required workspace gate1634 passed check/
clippy/fmt but failed tests:12 unrelated failures reproduced on exact main
31ba836de in baseline queue1625, plus a test-only duplicate-key-ID error corrected
and passed in queue1654. The full workspace gate is **not green**; later workspace
packages were not run after the failing package. Independent review/formal visual
proof remain pending; this checkpoint is Draft.
This public minimal case is not an actual49 lost-Tint-track claim (none found).
Reviewer native key/ease editing and same-host save/reopen also passed for the
byte-identical accepted original export, with white/amount/default alpha unchanged.
The pinned independent source now has an inspected full-canvas/full-duration native
30fps reference (38 frames/1.266667 seconds from its unchanged24fps/1.25-second
source, without retiming/trimming), SHA256
`35809a40e6d0e32fdffd5d1e16985138e1d8a4275a761e9be3f86301dc36c550`.
The public case's reviewer proof JSON retains control/reference provenance and
sample observations. Queue1714 passed37 COLOR tests/5 existing ignored; required
workspace1715 still failed12 baseline tests (1670 passed/411 ignored), not green.
Long-term Asset publication/fresh hash download and converter/reference RGB
comparison remain incomplete. Alpha/audio and Tint luma/kernel approximations are
unchanged and unproved; native key acceptance is not their fidelity pass.

## Active Index Range-selector aliases — bounded import profile (W03)

**AEP → FX:** complete same-text named Range aliases `.start`, `.end` and
`.offset` now select the native Index leaf when static Units is2, rather than
rejecting it or reading the dormant Percentage leaf. Native aliases return raw
Index numbers; normalization belongs to the receiving FX property. Existing
bounded affine/Linear lowering retains independent editable keys, not live
cross-selector dependencies, expressions or JS. Later edits to a source selector
do not automatically recompute another selector; edit the receiving FX controls.
Static Percentage behavior/defaults are unchanged. Unknown, animated or
expression-backed Units, ambiguous/cyclic aliases and unmaterialized Index
leaves remain diagnosed omissions; sparse Index defaults are not guessed from
text, word or line counts. No schema/runtime/renderer or writer-profile change.

**Independent source evidence:**
[`import_selector_index_aliases.aep`](../crates/aftereffects_file/tests/fixtures/text/import_selector_index_aliases.md)
pins one independently AE26.5x89-authored comp1,320x18030fps1s, four text layers
with Character/ExcludingSpaces/Word/Line bases. Managed native readback observed
Index Start1.25/End3.5/Offset[-.5,1.5] and identical raw values from receiving
Percentage aliases at[0,.5]s; an actual native Offset input edit .5→1.5 preceded
save. Fresh-import regression asserts all four bases, active units, receiving
normalization and editable Linear tracks. Supplementary CPU negatives retain
unit and missing-field guards, including dormant Percentage collisions.

**FX → AEP:** existing editable Index/Percentage export is unchanged. Fresh full
export of this imported FX is tested for active native match names/raw values,
key clocks, absence of expressions and an independent FX Start edit response.
Own-reader generated-project assertions are structural only, not independent
Adobe acceptance or export fidelity. Generated Text-reading baseline/candidate
gap remains; prior enum/key-envelope repairs are separate PRs and not silently
stacked here. Reviewer managed native reopening verified all four saved source
owners and receiving Percentage/Characters controls. A fresh explicit FX Words
Copy Start edit .0125→.0225 was exported; generated native opening failed with
`Error reading the text layer`, before generated-control readback/native edit.
Cleanup/fresh READY succeeded. An independent unchanged-source full320x180/1s
30fps MP4 was rendered and all30 frames decoded nonblank; critical samples show
overlapping two-line text, not a discriminating visual proof of each basis.
Long-term Asset publication/download/hash verification remains pending; fresh
output comparison, generated-native readback/render and RGB/alpha fidelity
remain unrun/unmeasured. Reference audio was off and no audio claim is made.
This is DRAFT partial proof, not completed bidirectional feature fidelity or an
original49 accuracy pass. Exact native jobs, hashes, limitations and test symbols
are adjacent to the fixture; all original sources/references remain unchanged.
## Embedded Video exact-zero gain — export switch correction

**FX → AEP:** Video with source audio and an exactly silent effective AudioVolume
(base zero, constant zero override, or an all-zero track) now disables the native
audio switch, as Audio already does. Picture/source identity, unmodified packaged
media and editable levels are retained. Positive gain or nonzero overriding keys
keep audio enabled; a source without audio is not enabled. No new dB clamp is used.
Mixed zero/nonzero animation retains the existing -192 dB near-silence approximation
at zero: exact keyed switch transitions are **not implemented or proved**.

Targeted regression `video_export_exact_zero_mutes_without_disabling_picture_and_gain_edits_restore_audio`
failed before the fix on gain0 (audio enabled), and exercises positive gain,
nonzero overriding keys, all-zero keys and independent unsupported/no-audio controls.
This is explicit editable-input/own-reader export evidence, not Adobe export acceptance.
One independent managed native author/readback job
`f7d099b380df435c958b30e0f8809a43` (AE26.5x89, cleanup/READY verified) observed
picture enabled for all three controls: audio-disabled/-192 dB had audioActive=false;
audio-enabled/-192 dB and audio-enabled/-6.02059984 dB had audioActive=true.
Native Levels were keyable; audioEnabled was Boolean, not proof of keyed mute.
Unchanged primary movie SHA256
`a383d0058de7ce9723e263611acd35a438cf1154fbbfe8e0f1a1f78e74546292`;
managed script SHA256
`7e999da17284104693edfdd34e3753234ab9e1be21ff40243ad69823d7399c94`.
These initial controls establish switch semantics, not waveforms. Reviewer follow-up
at converter HEAD `1471986ca819720a0a22734164f05c905f8bface` freshly exported two
editable public-tone inputs (volume 0 and 0.5), then independently inspected the
outputs using managed, imported-folder-scoped native selection. Mute composition
3/layer4/source16 and gain composition18/layer19/source31 were distinct, with
picture enabled, audio present, audioEnabled/audioActive false→true and Levels
-192→-6.02059984 dB. Both retained 2s/24fps/1x media clocks. A previous index-based
selector incorrectly selected mute twice; its artifacts remain invalid evidence.

One additional managed Adobe render of a 10s sequential generated/control panel
produced 300 frames at 30fps with PCM audio and preserved the native 24fps source.
The 0.25–1.75s interior of each 2s segment was decoded without retiming: generated
mute and independently authored mute were exactly PCM-zero; generated half-gain
and independently authored half-gain had peak2047 and maximum difference0 PCM16
LSB across144000 stereo sample values per window. The enabled -192dB control was
natively active but quantized to zero; this does **not** prove mathematical zero.
All five sampled full frames retained blue picture (minimum B255, maximum R1/G0).
Both managed operations published only after cleanup/fresh READY. The unmodified
public source SHA256 is
`1cfe566acd3c9e309c59908e2c0e244e595deb8c1b671f4d28889d227a5f2253`;
render job `7fc4d548597f43b5aca3b57358980bd0`, MOV SHA256
`95ca18a6149fd7c74ef4b486ac0e92744423a7d0eaad085c0e68250c0da20bd9`.
This establishes bounded generated-output native acceptance and editable gain
response, not general RGB/alpha fidelity, keyed switch support, or new long-term
reference Asset publication. Original49/full-project fidelity remains unmeasured.

**AEP → FX:** implementation unchanged. Existing independently pinned enabled/muted
native sources and import tests remain the import control evidence; native mute
still projects to zero gain rather than preserving inactive gain controls.
## Static integer unrounded PolyStar Group enclosure — partial export repair

**AEP → FX:** unchanged; independently native
`shapes/import_parametric_shape_controls.aep` compositions92 (Star) and182
(Polygon) retain five points, outer radius210 and zero roundness.
**FX → AEP:** the static Group enclosure now reuses the existing checked all-time
analyzer for explicit integer PolyStar geometry with 3–1000 points (matching
native writer admission), nonnegative finite radii, zero inner/outer roundness
and no competing Ellipse/Path.
This restores editable geometry under Group opacity instead of omitting its
subtree. Other mask, effect, clock, transform and finite-canvas guards remain.
Rounded/fractional/ambiguous static primitives retain the existing omission;
this does not extend those profiles or introduce bounds padding.

**Evidence:** source-based regression
`static_polystar_native_opacity_group_retains_geometry_and_sibling` imports the
native controls, edits Group opacity to50 and outer radius to125, and checks
fresh exported editable geometry plus its independent sibling. Latest-main
RED job1426 omitted the Star Group, leaving one instead of two root layers.
GREEN jobs1438/1448 passed. Managed native readback
`w04-static-polystar-readback-v1` on AE26.5x89 independently observed source
radius210 and edited radius125/five points, native Star/Polygon types, Group
opacity50, retained root sibling/order and250×250 primitive canvases; cleanup
and fresh READY passed. Exact source/export hashes and proof limits are pinned
in `tests/fixtures/static_polystar_enclosure/proof.json`. The two executable
regressions now use dedicated independently Adobe-authored `source.aep`
SHA `11acf3860e86278190cfd083ba51f58a622036ae810c6251c5781eb3d56045ae`,
comp1 (Star) and14 (Polygon): one static Shape each, five points, radius210,
zero roundness, no unrelated geometry/effects/media. Managed authoring request
`r06-dedicated-static-polystar-source-v5` saved and reopened both native controls,
with exact values and verified cleanup/READY; author script and receipt hashes
are pinned. Queue2063 passed both regressions, including radius125/Group50 edits,
sibling retention and unsupported-profile guards. Historical native export/edit
readback above remains explicitly bound to the original multi-feature source,
not relabeled as dedicated-source native export proof. Import/export mappings
are unchanged by the fixture-isolation follow-up. Own-reader assertions
are supplementary; render comparison, 30fps MP4 publication/fresh hash download,
alpha and audio are UNRUN. No native-fidelity
or original49 restoration claim; proof remains incomplete.

Independent review reproduced an empty retained Group for 1001 points: native
leaf writing rejected the geometry after the enclosure had already admitted it.
Reviewer RED1764 and GREEN1766 cover the corrected upper bound plus too-few points,
inner roundness and negative inner radius. The source-derived two-test panel passes;
required1767 check/clippy/fmt pass but ordinary tests still fail12 baseline cases
(1669 passed/411 ignored). The suite and remaining formal proof are not green.

## Straight spatial Position zero-handle geometry — import correction (L01 ordinal 4)

**AEP → FX:** wholly straight native spatial Position tracks retain their authored
editable keys and shared-distance temporal easing, but no longer attach redundant
zero spatial handles. AE uses traveled distance along the straight segment; FX
interprets present zero handles as cubic geometry, applying smoothstep a second
time. Removing that geometry restores the existing endpoint interpolation contract.
Curved tracks retain adaptive preparation; no FX/schema/runtime changes, JS, frame
baking, or native-byte replay were introduced. Millisecond clock rounding and the
existing curved-track approximation limits remain unchanged.

**Structural evidence:** unchanged independently native
`pr4442_native/sources/hierarchy_animated_bounds_precomp.aep`, SHA-256
`3a506b8ac8c96bf7086fcc7a6177983d80762de3fc22fe2c0b72868617ed8d5e`,
composition 16 / layer 29, exercises moving zero-handle Position.
`native_straight_position_does_not_ease_geometry_twice` checks editable scalar
tracks and retained temporal easing in forward and reversed clocks. Shared queue
job 1271 executed one test and failed its spatial-metadata assertion before the
fix; job 1276 passed 109 animation-filter tests (15 ignored), including the new
regression and existing endpoint/mixed curved-loop cases. These are source-based
editable-structure tests, not an independent native-render fidelity pass.

**FX → AEP:** implementation unchanged; fresh edited-FX export, Adobe acceptance,
editable native readback and independent render comparison remain unrun for this
correction. Assigned Bold01 composition 1 source-contract analysis predicts about
47/51 source pixels of Circle01/02 displacement error at 1.5s before correction;
this is numerical evidence, not Adobe sampling. Before/after visual scoring,
30fps independent Adobe proof, alpha/audio/font parity and the strict fidelity gate
remain unmeasured. No new oracle or baseline was created.

## Trailing empty Text paragraphs — export grammar repair, native acceptance blocked

**FX → AEP:** Point and Box Source Text now append one native terminal return
*after* every authored line break. Previously an existing trailing return was
reused, silently losing an editable empty paragraph. CR, LF and CRLF normalize
to native CR; Box character/paragraph run lengths include the additional UTF-16
unit. No font choice, envelope/cache repair, flattening, native source replay or
FX/schema/runtime change is introduced.

Independent managed AE26.5x89 control in
[`trailing_paragraph/README.md`](../crates/aftereffects_file/tests/fixtures/trailing_paragraph/README.md)
pins source SHA256 `7d194b82b6248ce18809210a39892eb8ad652ba80fcb0561102a40a7e70c9979`,
composition `1`, 320×180, square pixels, 30fps, one second. Native author/readback
job `d1244884bbbf4ec1b7fd5d76029e6afe` distinguishes `A` from `A\r` and retains
an edit to `A\r\r`; saved native COS stores `A\r\r\r`, proving the separate
storage marker. Both writer regressions failed in queue `1300`, then passed in
`1305` with workspace check/clippy/fmt. The Box regression checks the shared
grammar and run units; independent Box-native readback is not established.

**AEP → FX:** implementation is unchanged and removes only one terminal CR.
`trailing_paragraph_native_import_preserves_authored_empty_paragraphs` freshly
imports this pinned source and asserts editable Text values `A` and `A\n\n`.
Reviewer queue `1369` passed all three paragraph regressions (zero ignored),
workspace all-target check/clippy with warnings denied, and formatting on
`2aea5dece9a8a470b204f3a906a1719acc0c07f3`. Subsequent evidence edits change only
documentation. After a clean rebase onto `e6ac82fe9` (Basic Text face repair),
queue `1378` passed the same tests/check/clippy/fmt on `7efd031bf`; no new native
acceptance is inferred for that later base. The writer test symbols are
`trailing_paragraph_point_text_matches_native_authored_control` and
`trailing_paragraph_box_text_keeps_authored_breaks_and_terminal_run_unit`.

**Independent acceptance comparison / blocker:** exact-main
`068aec6922b3bb97486ad8a0d48449e1aef4ae6a` build `1349` freshly imports the
unchanged source. Export of the same FX archive by baseline `1349` and fixed
`1310` emits SHA256 `74845c9f0a63e49651a3e8fb01b90564c9794db66a10ef0033d81cdf09b80d11`
and `9eeee44dbdae85b385822ce6de29afcc8ad4409613d9ca384135d82135b6e1a7` respectively.
The fixed bytes equal W03's rejected input. Baseline native open also fails with
`Error reading the text layer. Skipping the text layer.` (managed job
`d2bc71383751466886b2994a0060267e`); fixed job
`49df9fe320b94db5a31dd411f4f9554d` failed identically. Both verified READY.
This establishes a preexisting tested main Text acceptance blocker, not its
precise cause or proof that unmerged PR4807 fixes this fixture. No dependency is
silently stacked. Generated native acceptance/edit response, independently
rendered 30fps long-term Asset proof, RGB/alpha/audio, font equivalence and
original49 retention/fidelity remain unproved. Native source/input-edit evidence
and CPU payload correctness are not an end-to-end pass.

## Signed Text Animator Stroke Width range — export correction (W03)

**AEP → FX:** unchanged existing signed additive `strokeWidth` mapping. Independent
AE26.5x89 source composition1 authors −4px and edits to −2px; actual native
readback confirms both and the control's [−1000,1000] range. The pinned public
[fixture/readback](../crates/aftereffects_file/tests/fixtures/text/stroke-delta/README.md)
provides provenance; fresh current-main import preserves the edited −2px.

**FX → AEP:** this animator's authored range metadata now uses the independently
observed signed bounds rather than [0,100000]. Source Text's nonnegative stroke
width, values/keys, clocks and shared numeric encoding are unchanged. The native-
source-based `native_signed_stroke_delta_preserves_adobe_range` regression fails
before this correction. One managed baseline/candidate comparison on the same
explicit FX input (edited delta −6px) failed both native opens with `Error reading
the text layer. Skipping the text layer.` Cleanup/fresh READY completed. Native
generated acceptance/edit response and independent RGB/alpha/audio fidelity remain
unproved; existing Text-reading/scalar-envelope blockers are not fixed or reattributed. Native30fps reference/Asset and original49
render comparison remain missing. R04 repeated acceptance on rebased code9184c3c3e
with latest main e3bf5dfed, including merged trailing-paragraph grammar but not
PR4807: fresh import retains −2px, explicit FX −6px is freshly exported at30fps.
Managed jobb10d795ab08a4d66aa944f83bb017083 opened the unchanged native source
and independently confirmed −2px and bounds[−1000,1000], while the newly generated
project still failed with the same Text-reading error before editable readback.
Cleanup/fresh READY completed; no failed-input replay or source mutation occurred.
The precise cause and PR4807 sufficiency remain unproved; the shared rich-Point
legacy document route is not migrated in this range-only correction. This is a
structural control-range repair, not full Text export completion.
## Static editable Ellipse finite Group enclosure — export correction (W04)

**AEP → FX:** mapping unchanged. Newly pinned independent AE26.5x89 source
`tests/fixtures/static_ellipse_enclosure/native.aep` (SHA256
`36f27708a9ba41a25fc9246828ffbea33395e5e4069d4e14f66eb6fb7d8553b2`,
composition1,320×240,30fps,1s) supplies the editable Ellipse size `[82,54]`
and center `[11,-8]` asserted by
`static_ellipse_native_source_import_and_edited_group_export`.

**FX → AEP:** the static hierarchy enclosure previously omitted a Shape Ellipse
inside an opacity Group even though the leaf exporter supported it. The narrowly
selected Ellipse-only branch now reuses the existing checked all-time analyzer
with an empty animation index: local hull center±abs(size)/2, existing checked
modifier/stroke reach and transforms. Masks, ambiguous Ellipse+PolyStar, unproved
PolyStar-only geometry, nonfinite/overflow values and other hierarchy/clock gates
are not relaxed. No renderer/schema/JS/baked-media changes or arbitrary padding.
Omitted unsupported subtrees still preserve convertible siblings.

**Evidence:** synthetic omission RED job1304; edited-size export and focused
bounds/guard GREEN job1359. Managed independent author request
`w04-static-ellipse-oracle-v3` and generated-project native readback request
`w04-static-ellipse-export-readback-v1` both completed with verified READY.
Native readback observed `[82,54]` then `[126,70]`, unchanged `[11,-8]`, corresponding
82×54 then126×70 generated child canvases, root Group Opacity50 and an independent
12×12 sibling. SourceRect observations agreed with the edited extents. Exact
native observations and hashes are pinned in the fixture's `proof.json`.
Native-source regression RED job1400 removes only the ellipse enclosure repair
from the current source/test and reproduces the same one-layer omission; the
repair's GREEN retains both root layers. Our reader/export roundtrips supplement,
not replace, the independent readback. Full converter gate1376 passed check but
reported 11 clock/matte failures; untouched main reproduced the identical 11 in
job1390 (1650 passed versus the repair's1654). Clippy job1392 and fmt passed.
These baseline failures are not waived or repaired by this isolated correction.

**Incomplete visual proof:** no independent30fps MP4 publication/long-term Asset,
full-frame pixel comparison, alpha or audio proof is claimed. Native scripting
readback is not human UI inspection or render fidelity. This repairs the isolated
editable finite-ellipse omission, not original P019's unbounded JavaScript or the
separate old PR4807 prerequisite; original49 restoration remains unproved.

## Bounded animated Text Path guides — export correction, partial native proof

**FX → AEP:** Text Path Options now copy supported animated Shape guide geometry
into an editable same-layer native Mask Path track, instead of rejecting every
animated guide and omitting path layout. The existing mask mapper supplies
single-contour Linear/Hold/zero-speed-Bezier admission, relative affine geometry,
clock/parent/coordinate guards and native Path-track validation. No FX/runtime
changes, outlines, frame baking or bounds guesses are added. The cross-layer
live link still becomes an independent native Path copy; later FX edits require
fresh export. Unsupported clocks, geometry, easing and topology remain diagnosed.

`animated_text_path_guide_retains_edited_cubic_keys` uses an explicitly specified
editable FX input, checks two cubic keys, input-edit coordinate response and a
mismatched-clock rejection. The same assertion failed before the mapping repair
(job1309) and passed afterward (job1318). Own-reader/lowering assertions are
structural evidence, not Adobe acceptance.

The full-export followup consumes the independent native source through
`native_text_path_fixture_pins_pixel_cubic_keys`, asserting its open cubic key
records, times and complete points/handles. Native Text mask storage uses
layer-local pixels; the previous generic source-size normalization incorrectly
divided the generated Text Path keys by the output canvas. Only the TextPath
guide now uses an identity divisor, retaining the FX coordinate convention and
ordinary/AV mask normalization. No native source/canvas is resized to1×1.
`text_path_full_export_keeps_native_pixel_keys_across_canvas_and_input_edits`
compares fresh full AEP key geometry against that source across three nonsquare
canvases, an affine guide edit and a second-key-only edit (nine combinations),
plus Mask None/index1, authored selection value1 and consumed-guide nonpainting.
RED1466 catches normalized fractions instead of native pixel bounds; GREEN1474
passes7 selected tests/0failed, with4 existing ignored proof-backlog cases.
The selection check is an own-reader authored-value assertion, not independently
accepted Adobe linkage. The author receipt's reopened getters cover vertices,
key count, closure and index; tangent/time/interpolation settings are pinned
script/saved-source evidence, not additional native getter observations.

The independently Adobe-authored control is
`tests/fixtures/text/animated_path/native.aep`, SHA256
`9479a75522de5368a4a8d7945cc9cb32b34f353ad445d014c04a05ebe82b1010`,
composition1, 640×360, 24fps, two seconds. Managed AE26.5x89 author/save/reopen
observed native Text Path index1 and two Linear open cubic Mask keys. Independent
30fps Adobe MP4 SHA256
`e8b97b4b7dd734b045728e8bde2862f94ca67741ff6f7570ea5af61d70d6490c`
has60 frames; critical samples0/.5/1/1.5s visibly show curved text translating
80 pixels downward during the first second. Source bytes/FPS were not changed.
Author script, explicit FX input and provenance are pinned beside the source.
Reference Asset publication/fresh download remains incomplete.

**Native export proof remains blocked:** managed readback of the pre-fix baseline
export failed with Adobe's existing “Error reading the text layer” error before
reaching the green/edited exports; cleanup returned verified READY. No exported
Text acceptance, native edited-control response, RGB/alpha equality or full-chain
pass is claimed. Do not remove Text guards or substitute native source replay.

**AEP → FX:** unchanged. Fresh import of this new native control exposes separate
Text mask coordinate scaling and masked-Text wrapper/font-bounds limitations;
this export mapping does not repair them. Original49 contains no TextPath controls;
P019 on current base instead loses Group9000 to an unbounded JavaScript enclosure.
This minimal native/explicit-FX correction is not an original49 retention or
fidelity claim.

## Native Text positive scalar representability — bounded export correction

Import is unchanged. Fresh Point/Box font size and explicit manual leading must
remain positive after conversion to their native f32 storage. Positive f64 values
that narrow to zero, and overflow/nonfinite values, now produce field-specific
errors through the existing owner-omission path; no minimum is guessed or value
clamped. Representable positive f32 subnormal values and genuine inactive
zero automatic-leading defaults remain valid. Geometry/color controls are unchanged.
`native_positive_style_scalars_reject_single_precision_underflow` reproduces the
zero-authored bug before the correction; Point/Box size/leading boundaries and
`native_positive_style_scalars_keep_representable_subnormal_values` are
supplementary CPU writer assertions, not independent Adobe acceptance/fidelity
proof. Native boundary render/readback and RGB/alpha evidence remain unmeasured.

## Coherent native Text with editable animator controls — bounded export repair

**AEP → FX:** unchanged by this repair; no new import fidelity claim.
**FX → AEP:** native Point/Box Source Text eligibility no longer falls back to sparse
legacy COS merely because editable animators/path/nondefault anchors are authored.
The complete native document/property/frame/owner envelope remains, with separately
lowered editable sibling groups. Native numeric Text control defaults come from
existing independently authored selector/channel/path/additional-control fixtures;
only descriptor/bound metadata is retained. All values, key lists, text, fonts and
selected clocks come from current editable input. No source project/content replay,
selector removal, flattening, font mapping, schema or shared renderer change occurs.
Native value/key events must precede bound records; generic selector value types,
missing bound records and misplaced events produced `missing data in file`.

Fresh immutable build1180, commit `97765688aedfd61a43778a9a464de61b2df7cd2d`,
binary SHA-256 `4e9b2d5ad0f3c9300334bdcd259de06a845d28ad0fe6847d3912dcadca8ac3c3`,
opened a fresh original P013 animator-only conversion in Adobe26.5x89:
`Hello, World.`, Poppins-Bold170, Boxtrue, animatorcount1; cleanup/fresh READY passed.
Native request `sixtext-p013-native-envelope-1180-v1`, job
`0a4923d7bc5243b18f3c5fd9799e95fa`, AEP SHA-256
`5d76a43ab973dcd98415f28082f9bd3355838a15ee135912265ffa077b3b4b6c`.
This is bounded acceptance, not independently measured raster or exact selector/key
readback. Full6 original conversions succeeded, including full P046754.438s;
full native movies/SBS remain pending at this checkpoint. Font substitution may be
explicitly allowed for diagnostics, not a claim of actual font-binary identity.
RGB diagnostics do not establish alpha/audio parity. Original failed attempts stay
retained; no failed request is replayed and no native reference is overwritten.

CPU queue1186 `make -C opensource/conv check test clippy fmt` passed3543 tests,
408 ignored native/backlog proofs remain unproved. Source-backed regressions
`writer::native_text_controls::tests::native_text_selector_envelope_retains_required_bounds_and_authored_values`
and `writer::text::tests::native_point_empty_path_group_matches_source` preserve
native envelope/event order, authored values, Point/Box richer siblings and clocks.
Selector fixture SHA-256
`7aedddf83a6a2f7afa849d3206d48dab35b6c43b5b66067bc268e1d39821f9b6`.

## Mixed straight/curved Position refinement budget — import correction (L01 ordinal 12)

**AEP → FX:** a spatial Position track with any nonzero handle already uses a
bounded adaptive traveled-distance mapping. Segments whose relevant outgoing and
incoming handles are exactly zero now evaluate position directly as endpoint lerp
at the existing shared temporal distance progress. Their nonuniform cubic
parameterization does not require an arc-length lookup table. Nonzero-handle
segments, including same-endpoint out-and-back curves, retain the existing table
and refinement. Hold, finite geometry, shared-speed admission, destination clocks,
atomic coupled tracks and all work/depth/tolerance limits remain unchanged. This
is not per-axis easing, a new expression/runtime capability, or a budget increase.

The selected Infinity source SHA256
`1373d29f81469e8d5de5e0af38079814ef7f01a77d976cf9a1e9bf8734d43005`, root `2872`,
nested composition `1648`, layers `1652` and `1655`, has two distinct eight-key
Position tracks. Mostly straight spans plus a small nonzero incoming-X handle
previously exhausted the shared refinement budget and omitted both coupled XY
tracks. A fresh unchanged-source import with immutable converter `66d1df929`
reproduced both omissions. The two
`infinity_position_image03_mixed_segments_fit_without_budget_inflation` and
`infinity_position_image06_mixed_segments_fit_without_budget_inflation` regressions
recreate the exact distinct native key fields and assert retained authored
endpoints, nonzero same-endpoint excursion and a valid coupled editable graph.
Both failed before the fix in shared queue job `1057` with the refinement-work
limit diagnostic. These unit models are source-field/math evidence, not an
independent native fixture or Adobe readback; the proprietary source/media are
not redistributed. Both CPU regressions passed after the fix in shared queue job
`1061` (two passed, zero ignored). Public-workspace check/clippy/fmt and all 18
spatial tests passed in job `1066`. A fresh unchanged-source root `2872` import
with immutable converter `965327316` (binary SHA256
`921e185c7548c1c9f3020c18f027c8c74aab8493bcc792a1569bcf683a02a9e4`, job `1067`)
restored both original owners' XY tracks: 437 keys per axis on layer `1652`, 389
on `1655`, with all eight authored endpoints retained at their rounded times
and the small nonzero-control excursions preserved. Both former work-limit
omissions disappeared. This fresh parser/import assertion is structural evidence,
not independent Adobe motion/render proof.

**FX → AEP:** exporter implementation is unchanged; this repair retains ordinary
editable Position keys rather than native source replay. Fresh edited-FX export,
independent native control inspection/acceptance and feature-specific 30fps Asset
proof were not run. Native-render RGB/alpha, font and audio fidelity remain
unmeasured. Existing sampled spatial/clock approximation limits remain; successful
structural import is not a fidelity pass.

## Shape-owner mask pixel bounds — import correction (L01 ordinal 1)

**AEP → FX:** native Shape-layer Mask Shape bounds are already layer-local
pixels. Static guides and editable Path keys now use unit coordinate scale,
instead of multiplying those bounds and handles by the composition fallback
canvas. AV masks retain their source-dimension normalization. Shape masks do
not require footage dimensions; source-local clocks and native easing admission
are unchanged. This adds no expression runtime, JsScript or FX/schema/renderer
behavior. Text and other owner-coordinate profiles are unchanged and unverified.

The existing pinned AI SaaS source
`548a6b8849fcde4ea134140a212d7008722ddb832cbe13144fea29aa3ca71046`,
composition `11544`, Shape layer `11553` (`Grid`), independently read in Adobe
through headless-adobe's fixed adjustment profile, reports the first mask
vertex `[-69.1180114746094, -656.360656738281]`. The prior import instead used
`[-265413.1684728898, -1417739.0185546875]`: exactly the erroneous 3840×2160
canvas multiplication. The native receipt/artifact is retained in private L01
worker evidence; no proprietary project or reference bytes are published here.
The public AV mask fixture and its independent JSON oracle retain 10..50 pixel
vertices. `shape_mask_pixel_bounds_do_not_inherit_composition_dimensions` uses
a synthetic pixel-storage adaptation of that public native framing, not a new
Adobe-authored Shape oracle; it checks editable guides at two canvases and
without footage dimensions. The original AV test separately checks normalization.

**FX → AEP:** unchanged; native Shape-mask export coordinate correctness has not
been independently established. Independent minimal Shape-mask authoring,
30fps reference/Asset publication, animated native readback and RGB/alpha
fidelity remain unproved. Native Grid generation remains unsupported, and
Mask Feather numeric admission is a separate defect; this repair does not claim
the whole Grid matte or AI SaaS scene now matches Adobe.

## Direct static Angle alias in Text Animator controls — import correction (L01 ordinal 14)

**AEP → FX:** scalar Text Animator/selector controls can resolve a complete direct
same-layer `effect("name")(1)` or named value-parameter alias to a uniquely named
native `ADBE Angle Control` with one finite static value. The visible Angle value
is `ADBE Angle Control-0001`, not hidden parameter 0000. This separate exact
profile does not extend the affine Slider-expression grammar: Angle arithmetic,
time-varying or expression-driven Angle controls, ambiguous names, vector owners,
nonfinite values and extra statements retain existing unsupported-expression
fallback and diagnostics. The resolved numeric control remains editable, but its
live controller relationship is not preserved. No scripts or sampled keys.

Logo Reveal 23.x SHA-256
`f8f9c9db3a80ce1b5c3ab686db7efae505f318299ae1b4f403af8715c994695b`,
composition 500/layer 2384 `Brand`, has Text Animator Rotation
`effect("Blur & Fade In - Rotation")(1)`. Its native static Angle is 53°, whereas
the stored pre-expression Text Rotation is −67°. The importer previously retained
the stale −67° because its bounded control grammar accepted only Sliders.
`direct_static_angle_control_replaces_stale_text_rotation` reproduces this mismatch
and checks rejection boundaries; its pre-fix failure is recorded in the PR.
Proprietary source remains local, not a public native fixture.

**FX → AEP:** unchanged existing Text Animator Rotation export; original Angle
expression/controller linkage is not restored. Independent Adobe readback/render,
public minimal native feature fixture/Asset and RGB/alpha fidelity proof remain
unrun/unmeasured. This restores one static editable control, not all Logo23 text
animation, source camera or project fidelity; see PR validation for actual runs.

## Indexed static Slider extrusion Position — bounded import correction (L01 ordinal 14)

**AEP → FX:** the complete native expression profile
`[literalX,literalY,(index-literalOffset)*thisComp.layer("name").effect("name")("Slider parameter")]`
is lowered for unified 3D Position when the uniquely named controller resolves to
one finite static scalar Slider. Native 1-based real timeline order includes
hidden/null/controller layers; generated FX IDs and expanded children do not
participate. Arbitrary expressions, animated controllers, ambiguous/missing
controllers, separated/2D Position and nonfinite results retain the original
best-effort fallback with contextual diagnostics. No script or sampled keys are
generated. The imported Position is independently editable: subsequent FX layer
reordering/controller edits do not preserve the native live linkage.

Logo Reveal 23.x source SHA-256
`f8f9c9db3a80ce1b5c3ab686db7efae505f318299ae1b4f403af8715c994695b`,
composition 1371, contains 25 enabled `(index-2)` expressions and a static
`Extrude CTRL` layer 1484 Slider value −2. Cached pre-expression Z is zero,
collapsing the native authored depth. Source layer 2360 at index 18 requires Z −32;
layer 2385 at index 42 requires Z −80. Proprietary source bytes remain local,
not checked-in feature fixtures. The targeted regression
`indexed_static_slider_position_preserves_native_layer_depth` failed before the
fix. Grammar and unsupported-control regressions supplement this source evidence.
Execution results are recorded in the PR, not inferred from test presence.

**FX → AEP:** unchanged; existing editable 3D Position export is reused, not a
restoration of the original index/Slider expression or controller relationship.
Independent Adobe readback/render comparison, alpha/fidelity proof and a minimal
public native feature fixture/Asset remain unrun. This corrects static editable
depth only; Logo23's animated camera, nested projection and other expressions
remain separate limitations. No renderer, schema, camera or lighting expansion.

## Regular Color Control numeric value aliases — import-only correction

**AEP → FX:** complete static Color Control aliases may select the regular
`ADBE Color Control` value with numeric index `1`, in addition to its quoted
`Color` display name or `ADBE Color Control-0001` match name. This index is not
admitted for pseudo controls or other plugins. Unique composition/layer/effect
ownership, typed static color storage, finite unit components and the consumer's
existing alpha checks remain required. Other indexes, arithmetic and incomplete
expressions retain the authored fallback with contextual diagnostics. Values are
copied into existing editable color fields; live controller linkage is lost.

The licensed Text Animation Kit source SHA-256
`c294f2e1f2e3e3a61f852fdea6b06c9fab7fcb6ddb1758f85582108f2df9e108`
contains composition 7 / layer 4372's Fill alias to composition 3363 / controller
3378's static white Color Control via index `1`, with a stale red Fill cache.
`text_kit_native_fill_color_index_one_uses_white_controller_not_red_cache` (removed; historical)
asserts the source hash, native controller white and editable white endpoints.
`text_kit_converter_document_resolves_color_index_one` (removed; historical) freshly traverses selected
root 3363 through the actual Converter, finds the exact native comp 7 / layer 4372
carrier, and asserts its enabled Fill, all six white endpoints and owner-local
copied-alias diagnostic. Both tests fail on the old parser's stale red cache.
The helper test's supplementary decoded-source guards reject non-value indexes, arithmetic,
pseudo controls and other plugin kinds; they are not independent Adobe sources.
The licensed-source tests were removed; recorded results are historical evidence
only (no longer executable). The source is never redistributed.

**FX → AEP:** unchanged existing editable color export; numeric aliases and
controller linkage are not reconstructed. Fill remains the diagnosed Tint
approximation, including its native mask/feather/alpha limitations. Native Adobe
readback/open, 30fps reference publication, RGB/alpha/audio comparison and export
proof are unrun/unmeasured. This source-bound structural repair is not feature
fidelity or whole-project completion. No shader/schema/evaluator changes.

## Static Point Control aliases to Bulge Center — bounded Intro import repair

**AEP → FX:** A complete same-composition quoted
`thisComp.layer(name).effect(name)("ADBE Point Control-0001")` on an enabled
Bulge Center expression copies a uniquely named, static Point Control into
independent editable center coordinates. Supplied Adobe expression samples keep
precedence. Only source-less planar 2D Shape controllers with a positive owning
composition canvas are admitted; source-backed, Null, Text, 3D and unknown planes
are declined. No controller transform/toWorld conversion is inferred. The copied
pixel vector is normalized by the destination Group, not the consumer source.

The local declaration must uniquely identify Point Control value0001 with a
148-byte kind6 `pard`. Sparse defaults use signed16.16 **percentages** of that
controller canvas, not the generic point-default decoder. Explicit static native
type4/vector2 storage takes precedence, including zero; malformed, duplicate,
keyed, separated, nonfinite or enabled-expression targets decline without default
or base-graph fallback. Disabled expression text is allowed with its static
explicit authored value. Cross-composition aliases are rejected even on equal
canvases. Effective occurrence-local composition borrowing uses the existing
converter context; Essential overrides cannot resolve behind the override to the
base target. Live linkage is lost and diagnosed. Radius programs, pinning/taper
approximations and other unsupported effects remain independently diagnosed.
No JsScript, baking, flattening, runtime/schema/renderer or global point-default
change is introduced.

**Independent control evidence:** Licensed Intro source SHA-256
`f315ca13611716c924825ac5b16a8a8a5b4f1fdb95261a824f2ab7393a0209f4`,
composition366 (1920x1080), Shape controller367 and Bulge consumers371/373.
One retained typed headless-adobe inspection
`w09-intro-point-contract-inspect-v1` succeeded with cleanup/fresh READY on
AE26.5x89: controller and both consumers evaluate `[960,540]`, no keys. Artifact
SHA-256 `ef661121e15fd9d704779d632d2e6def5544b608d86c7fc4cf678d801b66abcd`.
Diagnostic font substitution was allowed; exact font identity is unverified.
Native control inspection is not a new feature-render or visual fidelity pass.

**Executed editable assertions:**
`intro_native_point_alias_uses_controller_not_cached_centers` (removed; historical) produced semantic
RED in shared CPU job1550 (owner371 centerY0.4666667 versus0.5), then GREEN1566
for both owners. Job1602 passed six Point-alias tests, including
`intro_point_alias_actual_converter_essential_override_is_occurrence_local` (removed; historical):
actual Converter expansion applies explicit occurrence values, declines an
expression-backed occurrence, and restores the unchanged original on the next
expansion. Public supplementary native-envelope/profile tests cover grammar,
percentage conversion, explicit/zero precedence, malformed/animated/separated
storage and incompatible planes. These supplementary mutations are not new
independently Adobe-authored feature sources. Captured-sample precedence remains
code-guarded but its counterexample is unrun. Final job1631 passed converter
workspace fmt/clippy and all six selected tests; job1632 passed150 effect-neighbor
tests (139 existing ignored). Duplicate-storage tests are resolver-level, not
whole-Converter tests. Licensed-source tests were removed;
recorded results are historical evidence only (no longer executable).
That source/media is not redistributed.

**FX → AEP:** Existing editable Bulge field export is unchanged and unrun for this
case; no Point controller or live alias reconstruction is claimed. Independent
feature30fps render/long-term Asset publication, fresh RGB comparison, native
edited-export control proof, alpha/audio/font fidelity and whole-Intro99% remain
unmeasured/incomplete. This is a scoped import repair, not bidirectional feature
completion.

## Static Color Control aliases consumed by Tint (L01 ordinal 13)

**AEP → FX:** Tint's Map Black/Map White colors use the existing bounded literal
Color Control resolver, as Fill and Toner already do. Only unambiguous, static,
opaque controls are copied; animation, nonopaque colors and other expression
semantics remain unsupported with diagnostics. The importer reads the controller,
not a stale expression cache. No new expression runtime, kernel, clock, alpha,
effect-order or budget behavior is introduced; live controller linkage is lost.

The licensed Intro source SHA-256
`f315ca13611716c924825ac5b16a8a8a5b4f1fdb95261a824f2ab7393a0209f4`,
composition 366 / owner 370 / controller 367, has a blue static Color Distortion 1
control but a red Tint black-endpoint cache. The source-bound test
`intro_native_tint_static_alias_uses_controller_not_red_cache` (removed; historical) asserts blue,
controller edits including black/white boundaries, nonopaque rejection diagnostics,
and retained MotionTile sibling/order/amount. Edited decoded-source cases are
supplementary, not independently Adobe-authored fixtures.

`thisComp` aliases receive the actual occurrence composition after supported
Essential overrides; explicit `comp("...")` aliases still resolve named project
compositions. The always-run supplementary CPU regression
`tint_color_alias_uses_occurrence_composition_without_changing_named_comp_scope`
checks local green versus original blue, named-composition scope, local nonopaque
and expressed-controller rejection, and captured-sample precedence. The pinned
Intro test also checks a decoded local green edit without changing the original
blue occurrence or its MotionTile sibling. These context assertions do not add
independent Adobe-native override/readback or visual evidence.

Delivery validation: semantic RED queue1250 exercised the actual Converter path
with the old original-composition lookup (blue instead of local green). GREEN1262
passed the same focused occurrence test. Its supplementary v2 sidecar uses the
required 1ms timestamp grid and RGBA samples; sampled yellow is asserted in editable
channel keys while authored red remains the static base. No sidecar validation was
relaxed. Neighbor queue1263 passed three color-alias tests (one licensed Toner test
ignored); final1264 passed converter fmt/clippy. Queue1266 explicitly ran the
SHA-pinned licensed Intro regression: one passed, zero ignored. Queue1265 selected
but ignored that test and is not execution evidence. Adobe remains unrun and visual
fidelity unmeasured.

**FX → AEP:** unchanged existing effect-export support; this fix does not restore
Color Control linkage or prove Tint export fidelity. Existing Tint luma/alpha
approximations and unsupported Emboss remain. This is endpoint snapshot evidence,
not a new native RGB/alpha render, Adobe acceptance, export, or full-transition
fidelity pass. Existing partial-Tint safety handling below is not undone or counted
as a rendering improvement by this change.

## Unsafe partial Tint → image Emboss Adjustment — bounded import omission (L01 ordinal 3)

**AEP → FX:** an enabled HardLight Adjustment whose enabled image-effect chain
is exactly Tint followed by unsupported `ADBE Emboss`, with a readable static
zero `ADBE Emboss-0004` (Blend With Original), is retained **hidden**. Preceding
Pseudo controller effects and disabled stages do not alter the profile. Other
unknown image effects, changed order, subsequent enabled stages, other blend
modes, ordinary layers, disabled Tint/Emboss/global effects, and nonzero,
animated, expression-enabled, missing, unset-path, duplicate or unreadable mix controls
retain the existing best-effort behavior. This is a narrowly diagnosed omission,
not an image Emboss implementation or neutral-effect substitution. It avoids
applying a destructive false-color Tint-only HardLight stage when the omitted
Emboss defines its output. Supported editable Tint, owner timing/masks/guides,
and all sibling content remain; the Adjustment's output is deliberately bypassed.
Users may unhide/edit it, but that does not restore the absent native kernel.

**FX → AEP:** unchanged. No Emboss reconstruction or independent native export
acceptance/readback/render proof is added. This import safety fallback does not
establish bidirectional conversion or Emboss, alpha, audio or whole-project fidelity.

Private licensed source SHA-256
`b8c898e9d995c2680d1f2ab8aad0725224f4cebc42fcd041c474272799a4e21e`,
composition 1, layer 3208 has independently captured native Tint red/blue,
amount 100 and owner opacity 100, then enabled image Emboss (Direction 45,
Relief 0.15571014975978, Contrast 66, Blend With Original 0). Readback artifact
`eebc9eda85e946f6bca2d8b8a341bc1f`, SHA-256
`a16f32cb0eba87b903dbde40aa1ab7f2032c7c34864640a24b69b4032cc7f224`,
completed cleanup/fresh READY; no additional Adobe call or human UI inspection
is claimed. Tint was correctly imported before this fix; it is not an alias bug.
Current-main converter queue 542 versus fix converter queue 607 fresh imports
differ only in that Adjustment's `isHidden=true`; its editable Tint and every
sibling/animation are unchanged. Supplementary CPU regression
`unsafe_partial_emboss_chain_requires_exact_profile` was assertion-RED in queue
579 and both profile/control rejection tests passed in queue 595. The private
source and videos are not distributed as fixtures; these tests are synthetic
profile regressions, not a newly pinned minimal Adobe fixture.

Same fixed renderer queue 477 exported fresh before/after projects in queues
615/616 (1920×1080, 30fps, 60 frames, 2s). Against the unchanged independent
native diagnostic reference, full-resolution canonical RGB24 `rgb-hybrid` at
0.25s intervals produced 9 scorer samples including its clamped 2.0s endpoint:
mean 0.2080722 → 0.5996193, minimum 0.0626547 → 0.2536546. Some transition
samples regress slightly (0.75–1.25s); omission does not reproduce the intended
Emboss animation. At 1.9s the natural-color lake replaces destructive red/blue
false color. Scores remain below the unchanged 0.99 fidelity floor. This is
productive diagnosed safety improvement, not a fidelity pass. Formal native
fixture/Asset publication and independent editable export/alpha proof remain
unprovided under the user-approved scoped bug-fix milestone.

## Keyed Position beneath additive wiggle — bounded import approximation (L01 ordinal 13)

**AEP → FX:** an enabled, complete `posterizeTime(positiveConstant);`
`wiggle(positiveConstant, nonnegativeConstant)` expression on an already-keyed
Position retains its native base keys instead of dropping all motion and using
composition-center position. The expression is not executed. **Jitter and
posterized sampling are omitted**: the resulting smooth base motion is an editable
approximation, not native expression fidelity. Static Position, arbitrary expressions,
extra wiggle arguments/statements, nonpositive rate/frequency and negative amplitude
are not covered. Other channels and convertible siblings are unchanged. No new
FX/schema/evaluator/renderer behavior, JsScript or per-frame keys are introduced.

**FX → AEP:** unchanged. Existing Position-key export can represent the retained
base motion, but does not restore wiggle, posterizeTime or live expressions.
Independent native export/open/readback/render proof for this fallback is unrun;
RGB/alpha fidelity is unmeasured.

Native source evidence is private Intro Opening SHA-256
`f315ca13611716c924825ac5b16a8a8a5b4f1fdb95261a824f2ab7393a0209f4`,
root composition16. Native keyed Position records at offsets10952674,11284482,
11596776,11992554,12351406,12670134 and13769138 contain exactly
`posterizeTime(5);wiggle(3,5)`. Proprietary input stays outside Git.
`keyed_position_wiggle_keeps_authored_base_motion_with_diagnostics` adds that
expression to independently native Position-key storage as **supplementary mutated
coverage**, not an Adobe-authored wiggle oracle. CPU job558 compiled and was RED
at the retained enabled-expression assertion before implementation. Job577 passed
both targeted tests; job591 passed standalone check/Clippy/fmt and82 control-link
regressions (8 ignored). Fresh source imports with baseline receipt542
(`e651f82049`) and changed receipt602 (`63878e16b`) restore38 X/Y tracks for19
Position occurrences; all8 previously emitted dynamics remain unchanged.

Diagnostic exports623/624 each contain15 frames at1920×1080/30fps over
[33.2,33.7)s, using the unchanged renderer receipt477 (`3edcf747cc`) and identical
hash-verified Inter substitutes for133 native font occurrences. At33.2667s,
full-resolution RGB-hybrid similarity to the existing independent reference is
0.030118 before and0.030372 after: negligible improvement, **very poor fidelity**.
The sample does not visually prove restored base movement; unsupported Text
selection, font parity, color and other effects remain. Full-project comparison
was not rerun. Formal independent minimal native fixture/Asset proof is missing,
not a readiness gate for this narrowed bug-fix milestone; no visual pass is claimed.

## Packed Hue/Saturation Master — static import repair (L01 ordinal 2)

**AEP → FX:** static native `ADBE HUE SATURATION-0003` channel records
(`aRbp`, 180 bytes) supply Master Hue/Saturation/Lightness to the existing
editable `hueSaturation` effect. Native numeric UI declarations can describe
stale selected-channel defaults; they no longer replace the render Master's
values. Integral Master Hue is bounded by the existing writer's signed16:16
UI storage contract (`-32768..=32767`), retaining multi-turn values; this is
reader/writer storage consistency, not new Adobe acceptance evidence. Master
Saturation/Lightness remain bounded to `-100..=100`.
An absent packed record retains the existing numeric/default route.
Duplicate, unknown, malformed, animated or enabled-expression packed records
omit only that effect with a diagnostic; unrelated effects/layers survive.
Non-Master channel ranges/offsets are omitted with per-channel diagnostics while
Master remains editable. The existing FX HSV transfer is still an approximation
of Adobe's native Hue/Saturation; this is not native raster equivalence.

**FX → AEP:** unchanged. The existing static Master writer already emits this
record layout. No new exporter capability or native export proof is claimed.

**Evidence:** unchanged App Promo source
`260d5faf19fcd34e2d27841a26d918c30d59cf45659a172d589ea1c94eb2e67c`
has Master hue30 (comp4984/layer4986), hue142 (comp4999/layer5001), and
saturation−75 (comp3133/layer3274). Fresh baseline import defaults all three
to identity. The public Adobe-authored `hue_master_static_adobe.aep` fixture
(SHA256 `e720cfaa8bcebf2fd4ede1ad4116d4e267c3bc37e3a148d5f6c555c2b559be38`,
comp1/layer15) independently contains Master `[50,-60,20]`. Its unchanged
editable assertion passes before/after; the supplementary
`packed_hue_master_overrides_stale_native_ui_defaults` regression modifies only
its three UI declaration caches to zero, retaining the native packed record.
That regression fails before (hue0 versus50) and passes after. Packed-record
unit tests cover static values, independent channel omissions, missing records,
unknown sizes, invalid values, duplicate records and animation/expression
rejection. No new Adobe session, RGB/alpha comparison, Asset publication,
full-project fidelity or native export acceptance is claimed by this repair.

**Disabled-expression follow-up:** the existing property decoder's native enabled
bit is authoritative. Physically retained but disabled expression text no longer
rejects static packed Master values; its omission is diagnosed. Enabled expressions
(including presence with no disabled bit), animation and ambiguous records still
reject the effect. No expression is executed or exported by this repair.
The unchanged private Intro source SHA256
`f315ca13611716c924825ac5b16a8a8a5b4f1fdb95261a824f2ab7393a0209f4`,
root comp16, contains comp64/layer65 Animation Master `[343,0,0]`,
comp64/layer66 Leak 01 `[21,0,0]`, and comp16/layer955 FX `[0,10,0]`.
The first two contain disabled Channel Range expressions (metadata119=1,
metadata120=1); their effects were omitted before this follow-up. Fresh fixed
import retains all three editable values. The runnable local source-hash-bound
assertions and RED/GREEN evidence are retained in the coordinated w06 scratch
(`tmp/w06/assert-intro-hue.py`, `intro-hue-green.json`); the private source is not
published. The regular `packed_hue_disabled_expression_retains_static_master`
regression fails before and passes after, with enabled/presence-only rejection
controls. These are structural assertions, not independent native-render fidelity
or new minimal Adobe fixture/Asset proof. FX → AEP behavior/proof is unchanged.

## Static Stroke Color aliases — consumer repair (Bold03)

**Import semantics and root cause:** Native Shape Fill Color and Shape Stroke
Color can both request the same static Color Control. Fill resolved the existing
bounded alias grammar while Stroke copied its cached `cdat` value. Stroke now
uses exactly the existing Fill resolver and admission rules; no source IDs,
names, timestamps, coefficient guesses, runtime shaders or renderer changes
select this behavior. Only solid Stroke paint changes; width, cap, join, opacity,
dashes, gradients and expression-free styles retain their existing handling.
Missing, ambiguous, dynamic or unsupported controller expressions retain authored
paint with contextual diagnostics. A resolved alias becomes independent editable
paint: **live controller linkage is lost**, just as for Fill.

**Pinned source and causal regression:** Licensed vendor Bold03 SHA256
`8bd5f7b75781a7e134054f5c61ccb6bdc0cae00bde24964a08a00685aa001ced`,
composition 1 / layers 146 and 147, requests `Settings` / `Shapes Color` for both
Fill and an 80px Stroke. Original controller and cached paint are both opaque
cream `[243,242,238]/255`; no untouched-source RGB mismatch is claimed. Changing
only that native controller's static ARGB doubles to `[255,30,180,70]` produces a
fresh-import green Fill but stale cream Stroke before the repair. The changed
native input is an explicit offline controller edit, **not** an independently
Adobe-authored revision, render oracle or runtime readback. Proprietary sources
and media are not redistributed.

`shapes::tests::stroke_color::native_bold03_stroke_color_alias_follows_edited_static_controller`
was removed; recorded results from the hash-pinned licensed source are
historical evidence only (no longer executable). It exercises both owners, original/edited/channel-boundary colors and
unchanged width/opacity/cap/join. The pre-fix test-only commit `37b640580` ran in
shared CPU job 1157 and failed on cream Stroke versus edited green, after Fill
passed. Public supplementary regressions cover ordinary Color Control aliases,
non-expression styles and expression-backed controller rejection. Post-fix
shared CPU job 1162 passed all four targeted tests (including the licensed native
source case). Standalone `make check test clippy fmt` passed in job 1166: 3498
passed, 0 failed, 409 ignored across ordinary test/doc-test result blocks; the
native case ran separately in job 1162, not implicitly in that full suite.
Immutable patched converter receipt 1167 at `ad8d44935` freshly imports the same
edited native input with green Fill **and Stroke** on both occurrences, unchanged
80px width/opacity, and no JsScript. Reused exact-main `55b2d4e75` receipt 1130
freshly imports that input with green Fill but stale cream Stroke. Binary hashes
were checked against both receipts; no new baseline build was requested. These
are offline editable-structure results, not native render evidence.

**Export and proof limitations:** Existing independent editable solid Stroke
paint export is unchanged; reconstructing source controller UI or live alias
linkage is not implemented. Fresh edited FX-to-AEP export, independent Adobe
open/readback/30fps render comparison, alpha and audio proof are **unrun**. This
consumer fix establishes neither bidirectional feature completion nor strict
native fidelity. It is partial source-backed import work under the existing
bug-fix milestone; missing independent evidence remains a blocker to a full
feature-proof claim.

## Static Color Control aliases to Fill — bounded import repair (L01 projects 02/15)

A complete `thisComp.layer(name).effect(name)("Color")` (or native
`ADBE Color Control-0001` parameter name) on an enabled Shape Fill expression
copies one uniquely named same-composition sibling's static native Color Control
into existing editable solid paint. Effect Fill Color uses the same resolver and
also accepts `comp(name).layer(name).effect(name)(parameter)` against one uniquely
named native composition in the current parsed source project. This is not an
expression runtime and emits no `JsScript`, media replacement or hidden-source replay. Controller edits no
longer propagate: live linkage is intentionally replaced by an independently
editable color, with an owner-local diagnostic. Fill opacity, rule and blend
ownership remain unchanged. Arithmetic, additional statements, ambiguous or
missing names, non-Color effects, animated/expression-backed controls and nonfinite
or out-of-unit colors retain authored paint with an explicit diagnostic. Static
control values have no source-clock dependency; dynamic links are not admitted.

**Import:** App Promo source SHA-256
`260d5faf19fcd34e2d27841a26d918c30d59cf45659a172d589ea1c94eb2e67c`,
root composition 3419, nested composition 547 / BG layer 4853, independently
contains the complete alias to Controller 1 / BG 2. Fresh base
`3edcf747cc566e0f99f7e34bb672857cb2942012` import retains editable Rect paint
`[1,0,0,1]`; the local source-based black-paint assertion fails. Native diagnostic
frames at 0.25s and 2s show black rather than the converted red background.
Proprietary source/media remain outside Git. Formal minimal native feature
fixture, independently read native controls, long-term reference publication,
strict feature-specific native-render comparison and alpha proof are pending;
these diagnostics are not a fidelity
pass. Supplementary synthetic-native-chunk CPU route regressions (using an existing
native layer record, not independently Adobe-authored alias cases):
`shapes::tests::static_sibling_color_control_alias_keeps_editable_rect_fill` and
`shapes::tests::unsupported_sibling_color_control_alias_keeps_authored_fill_and_diagnostic`.
All three targeted alias tests passed in shared queue job 531. Standalone
`check`, `fmt` and workspace/all-target `clippy` passed in job 533; its scoped
Shape tests passed 164 with 11 ignored. Scoped Effect tests passed 134 with
136 ignored in job 534. Ignored tests are not passes. Both original source
assertions pass after fresh conversion with fixed receipt job 532, following
failures on fresh base receipt 467. This is real-file editable-structure RED/GREEN,
not Adobe readback.
Supplementary cross-composition Effect regression:
`shapes::tests::static_cross_comp_color_control_alias_keeps_editable_effect_fill`.
Logo Reveal original SHA-256
`5042c5fb645f2712f64427fdc4197d60d73f469ba6571a4c294b32f8eaa0c40f`,
root 1098 / nested 1371 / foreground layer 1972 aliases
`comp("Render").layer("Controls").effect("BG Color")("ADBE Color Control-0001")`.
Current-base foreground Effect Fill fallback is black, hiding the correctly
imported gray background; worker14's independent hide/recolor diagnostics isolate
this cause. Fresh job 532 import maps that foreground's editable Tint endpoints
to the independently decoded native controller RGB
`[0.8363326787948608,0.8482998609542847,0.8854473233222961]` instead of black.

Bounded diagnostic renders use the fixed base renderer receipt job 477, unchanged
hash-verified diagnostic font inputs and original independent videos. At App
frame 8 (8/30s, requested 0.25s) / frame 60 (2s), full-canvas RGB byte MAE drops
86.747 → 8.108 / 105.582 → 48.874 (1920×1080). Its background patch goes red
`[253,0,0]` → black, matching the native patch. At Logo frame 60 (2s), MAE drops
194.155 → 6.620 (3840×2160); background patch native mean
`[210.826,214.204,224.081]`, before black, after `[210,215,225]`.
The Logo before/after ranges are 1900–2200ms; App is 0–3000ms. This is measured
RGB diagnosis, not a full-timeline/threshold/alpha pass. Phone geometry, camera
projection, logo size and text placement remain visibly different. Native Fill
still uses the existing diagnosed Tint/alpha approximation. Essential-override
linkage and other untested expression contexts are not certified by these cases.

Both sources' independent native inspection was font-admission blocked (Outfit
and Montserrat) and remains unrun. No Adobe API changes are included. The user
explicitly narrowed delivery to the demonstrated bug fix with quick existing
proof, allowing a Ready review PR without new minimal-native-fixture, Asset or
export proof; this does not establish broader feature fidelity.

**Export:** existing editable solid Rect/Shape paint export is unchanged; this
repair does not reconstruct a Color Control or live expression linkage. Fresh
edited-FX export, independent Adobe control inspection and render comparison for
this feature are unrun. Bidirectional feature proof remains incomplete.

## Static custom pseudo-color aliases — bounded import repair (L01 Bold06)

**Import:** A complete same/cross-composition color alias can also copy a uniquely
named native `Pseudo/` effect's uniquely display-named numeric parameter. The
parameter must belong to the effect's native four-digit parameter namespace,
carry explicit Color storage and be static, expression-free, finite and in-unit.
No plugin defaults, arbitrary expressions or pseudo-controller runtime are
implemented. Missing/ambiguous/non-color/dynamic values keep authored fallback
paint with contextual diagnostics. The existing editable Shape paint / Fill Tint
mapping is unchanged; live controller linkage is lost, as diagnosed.

Independent vendor Bold06 source SHA256
`f0b0bfdd722e6280fefa26062db4fee308f78c99369e2320cb77573513abeea4`, root
composition1 / Settings layer198, contains effect `Pseudo/NX291ee23e92k`, display
name `Bold Vibrant Brand Identity_06`, slot `Pseudo/NX291ee23e92k-0001` / `Texts Color`.
Its explicit native Color storage is ARGB `[255,0,0,0]`, with no keys/enabled
expression. Eight complete Fill aliases request this slot. A fresh pre-fix import
using immutable converter receipt877 (commit1dae5bf6f9e1) retains red endpoints;
the source-specific assertion fails on `[1,0,0]` rather than black. Proprietary
source remains local and unpublished. Supplementary CPU regression symbols:
`shapes::tests::static_pseudo_color_alias_keeps_editable_rect_fill`,
`shapes::tests::ambiguous_or_non_color_pseudo_alias_keeps_authored_fill`, and
`shapes::tests::expression_backed_pseudo_color_alias_keeps_authored_fill`.
The three targeted tests passed in shared jobs937/940 (including both duplicate
and non-Color negative cases); existing regular Color Control regressions passed
in job942. Standalone workspace/all-target check, clippy and fmt passed in job940.
Fresh immutable converter receipt938 at commitc90bc7257 imports all eight editable
Fill/Tint effects with both RGB endpoints `[0,0,0]`, amount100 and no JsScript;
the same source assertion now passes. The first candidate failed source import
because native Built In Params also appear in the parameter parade; that attempt
is preserved, and the regression now includes this native compositing metadata.

Supplementary fresh Bold04 source assertion preserves ten enabled editable black
Fill consumers106–115 (source SHA256
`d2451f4eadd93f2aa348fd53d95a614a90fb428839d3477ae3c23747d5472a59`).
A six-source SHA-verified fresh import census04–09 against the preserved diagnostic
font revision has 20/32/50/35/41/28 non-font scalar color differences; diagnostic
font substitutions/normalization are separately recorded, not code-only attribution.

**Review correction:** A peer initially alleged Bold01 slot0003 had declaration
`BG Color` but authored storage `Circles Color`. Exact native run-boundary review
retracted that association: slot0003 declares `Circles Color`; `BG Color` belongs
to slot0004. All18 actual Color leaves across sources04–09 have matching declarations.
A conservative declaration-name gate briefly implemented solely for this alleged
conflict was removed; its commits/tests/failed attempt remain in history, not
native conflict evidence or a separate repair. The final converter source is
identical to the validated original repair. No generic native lookup-precedence
or conflicting-name fidelity claim is established by these matching sources.

Diagnostic export955 using immutable renderer876 and the exact preserved six-case
font revision measured Bold06 mean RGB similarity 0.923806183 → 0.929633535 and
minimum0.875548002 → 0.882112135; 38/40 quarter-second samples remain below0.99.
Full-resolution canonical RGB24/common10s sampling uses the existing approved
one-frame duration policy (native301 vs actual300), without altering originals.
This is not strict fidelity, alpha, font, audio or edited-native export proof.

**Export:** Existing independent editable paint/effect export is unchanged; native
pseudo-controller UI or live linkage restoration is not implemented. A supplementary
explicit isolated editable Rect plus imported Tint effect, edited to RGB32/64/128
at both endpoints and amount100, was freshly exported with converter995. Raw native
`ADBE Tint-0001/0002/0003` records retain those edited RGB values and amount. This is
writer-structure evidence, not an independent Adobe oracle or full source restoration;
Tint alpha is unspecified/unproven. Initial invalid bundle construction and an
incorrect alpha/padding assertion were preserved before using the existing bundle
commit API and the bounded RGB-only check. Independent Adobe acceptance/control/render
proof, full-source edited export and alpha/audio proof remain unrun. Native Color
storage inspection is offline source evidence, not Adobe evaluation. Formal minimal fixture/Asset proof and full-timeline strict
render fidelity remain incomplete under the approved partial bug-fix milestone.
Any diagnostic comparison must use the separately versioned six-case font-policy
revision; a mixed panel is not code-only attribution.

## Unsupported-only Text selector stacks — bypass approximation

**AEP → FX:** If a native Text animator has selectors but none can be imported,
its editable properties are retained behind a new editable zero-amount Range
Selector. Previously omission of every selector made FX select every glyph,
so native zero Scale/Opacity reveal operations could erase the entire title.
This bypass retains source text, not native selection or reveal timing; those
operations remain unsupported with owner-local diagnostics. Mixed supported and
unsupported selector stacks retain their existing partial approximation. An
intentionally empty native selector stack is unchanged. Unsupported selector keys
cannot animate the safety gate; ordinary animator-property keys remain editable.
No expression evaluation, scripts, media flattening or FX runtime changes occur.
**FX → AEP:** unchanged; no reconstruction of the original expression selector is
claimed. Export of the edited fallback uses ordinary supported Range controls;
independent native control/render proof of that fallback remains unverified.

`unsupported_only_text_selector_does_not_select_every_glyph` checks native-shaped
numeric/selector records, preserved zero-valued properties, the zero-amount gate,
and keyed-property target isolation. Supplementary CPU regression only; it is not
an independently Adobe-authored feature case. Private Intro Opening source
SHA-256 `f315ca13611716c924825ac5b16a8a8a5b4f1fdb95261a824f2ab7393a0209f4`,
composition16, freshly imported at base `3edcf747c`, contains115 zero-opacity
selectorless animators. The old diagnostic at29.25s lacks the expected oversized
crimson title. Proprietary source/media remain outside Git. Fresh before/after diagnostic exports use the source-bound prepared-media map,
identical permitted Inter font substitutions, base renderer receipt job477 and
converter receipts jobs464/497. All three full videos are1920×1080,30fps,
1230frames,41s; no unmatched tail. Canonical RGB24 `rgb-hybrid` comparison at
0.25s over164 common samples `[0,41)` measures mean0.054404→0.054843 and
minimum0.029390→0.028985 (minimum regresses). At29.25s the diagnostic frame
measures0.029431→0.029560; text returns, but selected letters, typeface, reveal
timing and other effects still differ substantially. These are poor diagnostic
scores, not fidelity passes. Immutable feature-specific Asset proof, independent
native editable readback, alpha and full-project fidelity remain incomplete.
The corrected regression failed before the fix in queue480 and passed493;
check/Clippy passed521, and fmt plus the full AE CPU suite passed535:
1585passed,406pre-existing ignored. An earlier regression attempt475 failed on
incorrect synthetic numeric framing and is not counted as bug RED evidence.

## Corner Pin — bounded straight zero-speed spatial import (L01 ordinal 2)

**AEP → FX:** native relative Corner Pin points with finite two-dimensional,
monotone collinear spatial handles and zero incoming/outgoing temporal speeds
are normalized to straight paths before source-pixel scaling. Authored key times,
values and temporal influence remain editable scalar Corner Pin animations.
Ordered control points on each segment provide a conservative monotonicity bound;
collinearity accepts only floating-point roundoff (32 epsilon at normalized-coordinate
scale), explicitly diagnosed. This replaces redundant spatial parameterization,
not a curved path, and avoids unresolved nonzero-speed unit conversions.
Nonzero speeds, curves, reversing handles, malformed clocks/metadata, enabled
expressions and other effects retain existing rejection/fallback behavior.
No sampling, frame baking, expression runtime, JsScript or FX model change is added.

Private licensed App source SHA-256
`260d5faf19fcd34e2d27841a26d918c30d59cf45659a172d589ea1c94eb2e67c`,
composition 3398, layers 3404/3405, supplied the diagnosis: all affected Corner Pin
keys have zero temporal speeds and one-third influence, but their nonzero straight
spatial handles caused omission. Sources and media are not redistributed.
`corner_pin_zero_speed_straight_spatial_keys_stay_editable` is a supplementary
writer/record-authored reproduction on a public catalog layer, not an independent
Adobe-native fixture: queue 618 failed before the fix with zero tracks instead of
two and the nonzero-spatial-tangent omission diagnostic. Helper tests cover unsafe
profiles without mutating their keys. Queue 646 passed all three focused CPU tests
(four pre-existing native-proof tests stayed ignored). Queue 652 passed standalone
`make check test` (AE: 1590 passed, 406 ignored); its Clippy failure was a redundant
Copy clone in the regression, corrected before queue 659 passed `make clippy fmt`.
Fixed converter queue 651 (binary SHA-256
`96aa95408ee9345b7a7b60f878b185d2c91f3bd88d30a64f30a9d4ec433f565a`)
freshly imports the unchanged licensed source/composition 3419. A source-hash-bound
assertion verifies both native owners' eight Corner Pin scalar tracks each:
16 tracks, 32 keys at 3667/4667ms, original normalized endpoints and one-third
zero-speed Bezier ease; no imported JsScript. The same assertion fails on the
older base archive with zero tracks for the first owner. This is real native-file
parsing/editable structure, not Adobe execution or render fidelity. No visual
comparison or native call is claimed here; receipts precede the later safe rebase.

**FX → AEP:** this Corner Pin checkpoint added no exporter capability. The later
[shared zero-speed Point follow-up](#shared-zero-speed-point-bezier-export--native-control-acceptance)
adds bounded shared cubic Point export with independent Radial Blur Point control
acceptance; Corner Pin-specific generated-output/native acceptance remains missing.
This import checkpoint is not bidirectional Corner Pin feature completion.
Independent Adobe render comparison, formal immutable Asset registration, alpha
and full-timeline fidelity remain unmeasured/unproven.

## Composition-dimension Scale — bounded static import correction (L01 ordinal 3)

**AEP → FX:** the original two complete dimension-only Scale expression profiles are lowered
into independently editable static X/Y values: `width,height`, and uniform
`max(width,height*aspect)` with an explicit finite positive literal aspect ratio.
Only the exact assignment/conditional/vector grammars are recognized, with
optional leading line comments and whitespace. Arbitrary JavaScript, time,
controller access, changed arithmetic, extra statements and zero canvases retain
unsupported-expression diagnostics. Native percentage results are converted to
AV fractions before existing static Transform import; transform-only parent
copies therefore retain the dimension-derived scales instead of identity.
No expression runtime, JsScript, renderer, schema or source-media change is added.
The approximation is loss of live composition-resize linkage: editing the imported
canvas does not re-evaluate these values. Other expressions remain unsupported.

**FX → AEP:** no new exporter mapping; resulting static Transform values use the
existing bounded native export subset. Media/hierarchy export and independent
Adobe acceptance/editable readback remain unverified. This import correction is
not a bidirectional feature-fidelity completion claim.

### Intro separate dimension ratios — static semantic/diagnostic repair

A third complete profile admits `heightScale=thisComp.height/H;
widthScale=thisComp.width/W;[widthScale*100,heightScale*100]`, with the exact
ordinary temporary names/order and finite positive literal reference dimensions.
The independent axes become native AV fractions `width/W,height/H`; extra
statements, changed multipliers, swapped axes, invalid references and zero canvases
remain rejected. Only enabled expressions reach this recognizer; authored disabled
values and the existing captured-expression-sample precedence remain unchanged.
No Point/Motion Tile/Curves/radius expression admission is added.

Private unchanged Intro source SHA-256
`f315ca13611716c924825ac5b16a8a8a5b4f1fdb95261a824f2ab7393a0209f4`,
composition366/layer368 (Global Scale), has H1080/W1920. Native-source test
`intro_native_dimension_scale_separate_ratios_are_admitted` (removed; historical) was assertion-RED in
queue1776: real transform production retained enabled-expression state and
unlowered diagnostics. Its original1920x1080 result is100%/100%, already equal to
the former fallback: **no incorrect original pixels or measured visual improvement
is claimed**. The regression also freshly converts selected root16 through the
actual Converter and checks occurrence-owned diagnostics/editable scale; changed
1920x540 occurrence metadata is supplemental axis/unit/isolation evidence, not
independent Adobe authoring. Public grammar/adverse tests are supplemental too.
Queue1793 passed all five initial selected tests, zero ignored, including the
fresh root16 Converter assertion. Queue1804 passed all seven selected dimension
Scale tests, zero ignored, including native disabled-expression storage isolation
and the Plastic actual Converter case below. Broader proof is not inferred.

Import intentionally snapshots the expression result; native expression/live
resize/controller semantics are not retained. Export uses existing static Scale
writing but new edited-native/export feature proof is **unrun**. Separate-ratio
Adobe readback, independent30fps feature reference/long-term Asset, fresh visual
comparison, alpha/audio/font proof are **unrun/unmeasured**. Earlier bokeh native
readback below proves the older profiles only, not this new profile. The unrelated
root16 Point-expression capture failed missing VintageQuotes/Druk fonts; it is
not separate-ratio readback or a fidelity pass.

### Plastic literal-reference aspect-cover wrapper — same static evaluator

Private independently authored Plastic Transitions source SHA-256
`eb826ff9dd8e006c2d8c9db8fd0698ba38a8a2dc53f80443e6275cc37102ef0f`,
comp2145997613/layer2145997617, wraps the existing cover formula with literal
`userX=1920;userY=1080;` bindings and line comments, followed by
`x=thisComp.width;y=thisComp.height;ratio=userX/userY;`
`if(x/y>=ratio){[x,x]}else{[y*ratio,y*ratio]}`. Only this complete wrapper,
ordinary named bindings/order, positive finite literals and exact branch/vector
operators are admitted. Comments are skipped only before the profile and between
reference declarations and composition-dimension declarations; general JavaScript
comments/bindings/expressions are not evaluated. The result uses the existing
cover evaluator, not a new AE runtime or renderer operator.

`plastic_native_dimension_scale_cover_wrapper_reaches_converter` (removed; historical) was native-source
assertion-RED in queue1797: enabled Scale remained unresolved. It checks the actual
selected-root Converter's editable transform carrier, not just a grammar helper.
Public wrapper/adverse tests are supplementary. Queue1804 passed all seven
selected tests, zero ignored. Final queue1808 passed converter fmt/clippy,
89 control-link neighbors (10 existing licensed tests ignored), and all seven
selected dimension tests (zero ignored), including both Plastic descendant
parent-copy assertions and actual Intro root16 Converter output. Queue1800 exposed
two supplementary test assumptions, not additional
semantic RED: Plastic's source canvas is3840x2160 (the literals are reference
dimensions, not its canvas), and disabled native expression text uses flags119=1,
120=1. Those assertions were corrected without changing product code. The source
has uncollected footage in this offline check; placeholders are not render-ready
content. The expression-derived static result is3840% on both axes instead of the
prior100% fallback, but native readback/render/visual
measurement has **not** run; no pixel improvement or full-film pass is claimed.
Import snapshots static values and loses live resize linkage; export/new edited
native acceptance, independent30fps MP4/long-term Asset, RGB/alpha/audio proof
remain **unrun/unmeasured**, separately from the older bokeh proof below.

Source-based diagnosis uses private licensed source SHA-256
`b8c898e9d995c2680d1f2ab8aad0725224f4cebc42fcd041c474272799a4e21e`,
composition 1, parent layers 5897/5863. Fresh base `3edcf747cc` conversion with
fixed build queue 463 retained 100%/100% instead of source-expression results
1920%/1920% and 1920%/1080%. A targeted external-source editable assertion failed
before this correction. Supplementary integration test queue 476 was assertion-RED
(2 grammar tests passed, the static Transform import regression failed).
Native `inspect_aep(profile=media_timing)` request
`loop-L01-w03-bokeh-scale-readback-20261003-v1`, job
`dda34dcbcc3a4678ac9cafc43826bcf2`, succeeded with cleanup/fresh READY.
Readback JSON SHA-256
`86a53794df65aae82d35d6de4d21dc985e1f87b57c7700a5460ac938b0fdf30b`
confirms parent 5897 Scale `[1920,1920,100]` and parent 5863
`[1920,1080,100]` at 0, 1.9 and 2.0 seconds. Both parents are disabled guide
controls; their independent children must still inherit the transform.
This is native numeric readback, not human UI inspection or export proof.
Inputs and diagnostic videos are not redistributed.
`dimension_scale_reaches_editable_static_transform` and the helper grammar/unit
and rejection tests are supplementary CPU regressions, not independent native
fixtures. Queue 483 passed all three after the hook was added. Queue 489 passed
standalone converter `make check test clippy fmt`. Fixed converter queue 490
freshly imports both parent scales and all six transform-only parent copies;
the same external-source assertion passes, with no imported `JsScript`.

Matched renderer build 477 (base `3edcf747cc`, binary SHA-256
`b5688d80572bcf640bd3d0958e239899ae26d8ff05d27fcab196a924b1a80b9d`)
rendered unmodified before/after imported projects in export queues 499/500.
Both clips are 1920×1080, 30fps, 60 frames, 2 seconds; no unmatched tail.
Eight full-resolution canonical RGB24 `rgb-hybrid` samples at 0..1.75s in 0.25s
steps improved mean 0.0008421880 → 0.2254405129 and minimum
0.0002372161 → 0.0679425515 (both worst at 1.75s). The unchanged reference MP4
and clips were scored with validator SHA-256
`4f3cdab7784f664cdce76b58101bcc2a43263148678883dcec698688295b7842`.
At 1.9s the lake fills the canvas after correction, rather than a tiny central
square; a severe red/blue color mismatch remains and is outside this one-bug
correction. Full-resolution frame 57 (1.9s) comparison improved 0.000226 →
0.065709 (validator's printed precision). All scores still fail the preserved
0.99 floor. This is one-root-cause diagnostic improvement, not whole-project
conversion fidelity. Independent minimal Adobe-authored fixture, 30fps long-term Asset/fresh hash verification,
native export semantic readback, RGB/alpha proof and critical-frame comparison
remain blocked/pending; historical project diagnostics do not satisfy those gates.

## Owner-clock baking and finite Group source domains

Import is unchanged. Export bakes supported scripts in their owner's checked
source-clock domain, retaining inherited remaps, posterization and clamp/window
provenance. Scalar dependencies must have compatible producer types and exactly
matching clock domains; closure and consumer execute in stable graph order in a
shared VM. This is not arbitrary cross-clock or ambient-global JavaScript support.
The isolated clock lane passed 1,601 CPU tests, formatting and clippy (406 native
proofs ignored); combined corpus fidelity is a separate gate.

Group export now propagates checked affine source demands and recognizes static,
uninverted, unfeathered inline Add masks as finite support for eligible mixed Text
sources after child validation. Group Bulge controls use the logical composition
frame, not the selected capture tile: pixel radii remain unchanged and centers
are translated by capture origin. Supported logical-plane input/output bounds
retain ordered children rather than guessing glyph extents. Unsupported spatial
inputs and unproved 3D/world-frame Shadow domains remain diagnosed; this does not
claim all Group omissions or arbitrary effect stacks are repaired.

Independent native mask and Bulge source projects, managed authoring scripts,
exact hashes and proof limitations are retained in
`crates/aftereffects_file/tests/fixtures/group_effect_domains/README.md`.
Crop controls exhibit small channel differences and are not exact pixel-fidelity
passes. The isolated final Group lane passed 89 hierarchy tests, fixture assertions,
formatting and clippy (queue jobs 1075 and 1081). Conversion, native acceptance,
content retention, audio and full-timeline fidelity must still be assessed
separately for each exported project.

## Native Boolean depth policy removal

Native Boolean export no longer rejects nesting beyond 48, including
compound-contour wrappers; every operand and contour is still encoded. Checked
operand accounting, nonempty operands, finite geometry and native field/key
limits remain. Shared script evaluation retains main's existing Boa
100,000-iteration limit, 128-frame recursion guard and 10,240-slot stack guard.
The proposed loop-policy removal and its finite-loop regressions are deferred
to a separate Draft PR; this repair does not change synchronous VM termination
policy or add a supervisor.

Supplementary CPU regression `nested_boolean_exceeds_former_depth_policy`
writes all 49 nested Booleans, compound contours and 51 native Merge records.
It failed before the depth-policy removal (queue911). Historical queue921
passed 162 shape-filtered AE tests (11 existing ignored), plus formatting and
support-ledger checks. These are synthetic structural boundary checks, not
independent Adobe fixtures/readback or RGB/alpha fidelity proof. AE import
mappings are unchanged; native export acceptance and full-project/49-case proof
remain unrun in this isolated depth change.

## Byte-preserving AVC MP4 export and native presentation inspection

Import behavior is unchanged. FX → AEP export now stages genuine `.mp4` footage
with unchanged bytes and the existing native AVC `MOoV` source records. The new
`media_transcode::inspect::inspect_presentation` entry point asks libavformat to
apply container edit lists to packet timestamps; existing `inspect` callers keep
their deliberate unedited-media clock. Neither path decodes, rewrites, remuxes or
transcodes. Sample aspect ratio (including unspecified) and the native display
matrix (including absent) are now read-only inspection facts. There are no
project-specific timestamp offsets, extension disguises or resource quotas.

The export profile currently accepts one AVC stream on a zero-origin constant
presentation grid, square/unspecified pixels and absent/identity display matrix,
with optional mono/stereo AAC. Presentation order is validated using sorted PTS,
not decode order: reordered B-frames and native edit origins are retained in the
unchanged container. Duration and frame-rate fields come from that displayed
clock. Unsupported codecs, nonidentity geometry, ancillary/multiple streams,
irregular/delayed/trimmed grids and unrepresentable native rates remain explicit
unsupported diagnostics; this is not all-MP4 or arbitrary-edit export support.
A narrowly verified final-frame rounding profile also retains the complete
presentation grid: one version-zero nonnegative unit-rate video edit, movie
scale 1000, matching track/movie ceil duration, and edit duration equal to the
exact floor of that grid in movie ticks. The inspected stream duration must be
the exact nearest-tick projection of the edit duration. Other tail trims remain
rejected; the source bytes, timestamps, and native frame rate are unchanged.
Native P030 authoring reported 241 frames at 24fps despite the eight-tick
libav duration discrepancy; generated acceptance retained source times and
responded to an input-start edit. A finite linear Video selection may retain
stale intrinsic metadata only when its entire source range stays inside both
reported source domains, with no source-owned effects or static remap. Audio,
keyed playback, import, and arbitrary intrinsic mismatches are not admitted.
The public `p030-final-frame` fixture contains self-generated testsrc2 pixels,
not original private P030 media. Original-case RGB24/.25s/rgb-hybrid validation
measured mean 0.1499659838 before and 0.7320577702 after (47 samples); all samples
still fall below .99. The before movie is the preserved historical repair-06
native baseline, not a new same-head before render. The improvement therefore
is not isolated same-base causal proof or broad fidelity certification.

Original container facts are checked with the existing streaming atom reader
and video sample-entry validator before trusting FFmpeg's packet grid. Every
version0/version1 authored edit rate must be exactly unit rate; dwell, reverse
and fractional/other rates are rejected even when FFmpeg presents a regular
grid. Existing `clap` validation requires full coded width/height and zero offset;
cropped or shifted clean apertures cannot bypass the identity geometry gate.
Real-container mutation regressions `mp4_original_dwell_reverse_and_nonunit_edit_rates_are_not_ordinary_video`
and `mp4_original_clean_aperture_must_cover_the_full_uncropped_source` failed
before in queue1003 and passed after in queue1008. They reuse parsed atoms and
preserve media sample offsets; no product bytescan or new container framework
was added. Unit-rate B-frame/edit-origin sources and absent/full-size aperture
controls remain supported. These mutated negatives are supplementary admission
proof, not new independent native authoring/fidelity proof.
No audio packets or authored occurrence/Time Remap keys are rewritten. Silent
video now keeps its picture at nonzero gain as well as zero gain, with the native
audio switch off: gain cannot create an audio track.

Structural regression `native_mp4_media_stages_without_rewriting_its_bytes`
uses unchanged `feature_rate_24_blue.mp4`, referenced by the independently native
`media/import_audio_media_controls.aep`. It failed before at the MOV-only gate
(queue892) and passed after (queue901/902). Additional CPU cases exercise genuine
B-frame/edit-origin footage (`video-30fps-10s.mp4`), Check/Write parity, source
references and byte equality, geometry states/exclusions, malformed/unsupported
containers and nonconstant presentation rejection. The old unedited clock is
asserted separately from the new displayed clock. FFmpeg extraction regression
`inspection_reports_non_square_pixels_and_native_display_matrix` passed queue903;
its rotated matrix is an explicitly supplementary mutation, not a native oracle.
`positive_gain_video_without_a_source_audio_track_keeps_its_picture` failed before
(queue917) and passed with the zero-gain regression afterward (queue923).

Fresh source-hash-verified P040/P043 conversions use pinned build queue924;
retained ignored receipts are `tasks/tsrct-aep49-repair/narrow-03/<case>/`.
Both roots have one editable native video source/layer. P040's staged MP4 SHA-256
is `2ae7e13b5aad6841e20b1944bbead9ff20ed4dbaff04bce135d8e7c1a2747ac6`;
P043's is `c34c48035059ce53278c5fbe15a8ec9655023e0848f22c2db324138c30653993`.
Each matches its original archive member exactly. P043 carries source AAC and its
authored 118-point Remap. Native AE26.5x89 `media_timing` inspection and full
30fps render both completed for each fresh AEP through typed headless-adobe,
with input/media hashes, owned cleanup and fresh READY verified. Inspection
samples are 0, 1/30, 0.5, 2, 4 and 5.9s (P040) / 6.1s (P043). Media is online
after the backend's pinned in-memory relink; this is not original-alias or human
UI inspection proof. Source video remains 24fps, 720×720/6s (P040) and
1920×1080/45.958333s (P043). P043's native 118 keys retain every original value
exactly, with maximum time rounding 7.8125µs and native Linear interpolation.
Its AAC track is present in the unchanged source container. The original FX
Video has no `volume` opt-in; existing FX audio collection treats that as muted,
so the native output correctly has no audio stream. No audio was manufactured,
dropped from the container, or enabled just to make an audio check pass.

| Case | Inspection job | Render job / MP4 SHA-256 | Decoded output / RGB diagnostic |
| --- | --- | --- | --- |
| P040 | `1b722a03e22a4f28a673e820361e1c9a` | `ac2b4c45a7e64d7b9162700271008714` / `0f45bcc7aadd02a715bb7bcffee23442449c525ecf0c08ab63da80cebb368c11` | 180 frames, 720×720/30fps/6s, silent; RGB mean0.906594/min0.862073, worst0s |
| P043 | `d306f0c0dde247cabbc195492a69b7b0` | `2852d43a96b649ef921b492ff824dd9c` / `54850b698263648d142c1789f1fa10c124d4282447a5858d5b7cecf4459f6eb6` | 186 frames, 1920×1080/30fps/6.2s, authored mute; RGB mean0.787288/min0.532902, worst0s |

Both full movies decoded; six selected native frames per case are nonblank.
RGB uses the existing frozen baseline TSRCT exports and canonical full-resolution
RGB24/rgb-hybrid comparator at 0.25s cadence, 25 samples each, with the existing
0.99 diagnostic threshold. **All50 samples fail that threshold.** This is
local TSRCT→generated-AEP diagnostic evidence, not an independent native feature
oracle or a visual pass. No pixel-alignment/color workaround or threshold change
was added. Retained analysis receipts are P040 `native-01/analysis/summary.json`
and P043 `native-01/analysis-02/summary.json`. Audio opt-in/PCM parity, alpha,
immutable feature Asset publication and strict independent oracle comparison
remain unverified.

The bounded eleven-formerly-empty conversion probe leaves nine nonempty roots;
P019/P028 remain empty due to existing finite-enclosure rejection on owner9000.
P037 still omits owner60000 for the same reason. Other media-policy, customShader
and Shape omissions remain diagnosed, not declared repaired by a nonempty root.
All staged MP4s in that probe matched their archive members. Evidence is in
ignored `tasks/tsrct-aep49-repair/expanded-01/` and `expanded-summary.json`.
Final offline validation: queue947 passed all7 MP4 package/presentation tests;
queue952 passed the complete AEP crate suite (1592 passed, 406 existing ignored
proof-backlog cases still **unrun**) before encountering obsolete CLI assumptions
that AVC MP4 must take native fallback. Those six CLI fallback cases now use the
existing genuinely unsupported HEVC fixture, with its actual2s intrinsic length;
retention/gap/audio assertions were not weakened and no hybrid product code was
changed. Queue982 passed standalone converter fmt/all-target clippy with warnings
denied and all150 media/CLI tests (48+67+31+4). Queue974 passed all25 media tests
without FFmpeg. No root/GPU suite or Adobe calls were executed by these CPU gates.
Import implementation/proof is unchanged; general alpha/color/audio fidelity and
all formerly empty projects are not claimed passed.

## P047 Box Text native-reading repair — bounded export acceptance

Import is unchanged. Export omits inherited Box glyph/line caches and unknown
requested-font vendor metadata; automatic leading retains the native inactive
manual-leading value 0.01 rather than fontSize×1.2. Explicit leading is unchanged.
Source text, style and box geometry remain editable; no flattening, font alias,
replacement face, hidden source replay, caps bypass or allowlist is introduced.

Independent AE26.5x89 source `box_text_envelope/native_box_menlo18.aep`, SHA256
`9cb3ea4f56f2db5b01f90a6175b1486e74e270ea8c9fb156cf2ce422eec56436`,
was authored through managed native JSX (job87ca4ad7ee29406583c9c24383c86035).
`boxed_automatic_leading_matches_native_without_inherited_layout` fails
before the fix (21.6 versus0.01), then passes. Removing only that independent
source's layout cache retains native nonempty Box readback (jobdfe0439e9c8442c48513e0fba84adade).
Fresh converter output preserving P047's21 original text/font/size states
(17Box4Point) opens and reads all21 without getter errors, native job
e5d29011158e41e394ff6e43a59283f8, JSON SHA256
`3cada05ff6444707f4122110b78bfe58f5dc8df67bc0156b2505e9f3fe85e78f`.
This is bounded acceptance/native readback, not full-project native render,
alpha, box-reflow, actual font-binary identity or general fidelity proof.
Independent30fps render/immutable Asset comparison remains unmeasured.
Older retained-cache evidence below is historical, superseded for fresh Box output.

## Empty Group controls — bounded affine native Null export

Import is unchanged. Export now retains genuinely childless Groups as native
non-rendering Null controls, including short, offset and affine-rate occurrences,
instead of omitting them for absent finite visual bounds or emitting an empty
vector layer. Existing affine source-clock records and numeric-key rebasing are
reused; no painted extent, source canvas, media flattening or schema change is
introduced. Background/layout normalization, effects, masks, geometry and
reference admission remain separate existing checks. Incoming matte consumers
cannot sample a Null's nonexistent composite alpha and remain diagnosed omissions;
their dependent owners are pruned while unrelated siblings survive. Native Null
Time Remap and simultaneous typed-clock/generic timing remain rejected.
The empty-control classifier also requires no group-level fills; painted Groups
remain on the existing background normalization/diagnostic path, not Nulls.
`empty_controls_with_group_fills_are_not_null_eligible` isolates this admission
predicate with empty and solid-filled childless Groups. This supplementary
regression failed against the prior predicate in shared Rust queue job 382.
Independent native proof remains unverified; this is only admission evidence.

Supplementary CPU structural evidence: `empty_controls_affine_windows_keep_native_null_and_visible_sibling`
checks Null flags, exact native start/in/out/stretch and Opacity/Position key times
and values for a 69866ms/500ms window, offset/rate occurrence and full-span control.
`empty_controls_cannot_replace_an_incoming_matte_source` checks dependency guards;
`empty_controls_null_affine_clock_reaches_record` and
`empty_controls_null_rejects_time_remap_and_generic_timing` check writer admission.
All four passed in shared Rust queue job 276 (0.02s test runtime); the two original
regressions failed before the fix in job 268. These synthetic editable inputs and
own-reader checks are **structural-only**, not independent Adobe-native proof.
Full CPU check/tests/Clippy/fmt passed in queue 277: 3,295 passed / 407 existing
ignored. The private immutable source-derived target 2900000 (original empty
Group, unchanged 69866ms/500ms window, no artwork) has baseline finite-bounds
omission at build 255. Slice SHA-256
`7e438d34ddc3301a8a3528c5ef545c4948154ebb265e7ab846cf5c54eb301301`
now exports with fixed build 280 without owner/subtree omission. Fresh AEP SHA-256
`c83b76eaeaa943de2e5c7bc78f40852685697a152312ee6c9dba79d598a497ba`;
own-reader inventory retains its exact source name as one native root layer,
with no unresolved source IDs. This restores editable controls, not missing
visible artwork. Other source-derived empty targets 2300003 and 200700 have not
been separately executed. Adobe opening/control inspection, independent 30fps
render/Asset comparison, RGB and alpha fidelity remain unverified.

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

## Certified final-root HUD Text retention — export structural checkpoint

**AEP → FX:** unchanged; no new import proof.
**FX → AEP:** a clocked identity root already certified as the final output
viewport can retain static plain Text in a mixed paint hierarchy. The existing
reference-closed branch pruner supplies a **bounds-only copy**, after source-clock
normalization; the ordinary hierarchy classifier still validates every non-Text
child before applying the existing certificate. Emission uses all original
children. Neither the Text box nor guessed glyph bounds defines a source canvas.
Nested Text ancestors must be static, full-span identity-clock, full-opacity
Null-compatible Groups with Normal blend mode; effects, masks, mattes, motion blur, native 3D controls,
Text animators/path/anchor options, and unresolved/spatial references decline this
path. Existing guarded omissions remain for those cases, including explicitly
referenced text-only branches. This is not general nested Text/effect support or
permission to crop an input before blur.

The public literal [HUD structural description](../crates/aftereffects_file/tests/fixtures/text/launch_hud_structural.json)
comes from private diagnostic archive SHA-256
`0d55ef5da45bff2e2166d5df5cd1b92618f4028f1a674f80b05f6de3c44a7504`:
root2400000 at60400ms/4000ms, HUD2400078, boxed Text2400081 and original
Shape siblings2400080/2400079. The archive remains ignored/private. This
editable description is **not** an independent Adobe-native fixture/oracle.
`launch_hud_root_text_retention_preserves_source_structure_and_direct_text`
freshly exports and checks own-reader records for words `1080P / 30 FPS`,
Menlo Regular18, right justification,306.5×46.8 box,1516.5/61 planar Position,
parenting, root/source intervals and both Shape names. The corresponding
`keeps_unsafe_text_guards` and `validates_nontext_geometry` tests preserve guarded
omissions. Shared CPU job301 was RED for actual Text omission with both negative
tests passing; job303 passed all three after the repair, reconfirmed in305;
the existing reference guard passed306. Full standalone check/tests/Clippy/fmt
passed307: **3,294 passed / 407 existing ignored**. Independent full-diff review
found no confirmed defects. The later ancestor-blend regression directly checks
bounds safety before reference pruning; it fails on the previous Group guard
(shared Rust queue436). This is supplementary CPU admission evidence, not
independent Adobe fidelity proof.

Fresh fixed build308 exports the pinned private case without Text-branch or
owner/subtree omission. AEP SHA-256
`301eae7057771b3cf2e45a82a340d866d30d3fc10d15ccd2d35402b30d1f2f37`;
own-reader inventory retains Text2400081's exact name, both Shape names and HUD
parent, with no unresolved source IDs. This is content/structure retention, not
native render equality. Vertical alignment is still omitted with a diagnostic;
the deterministic `Menlo-Regular` PostScript candidate and cached Box Text layout
remain unverified. No Adobe operation ran.

Independent Adobe opening/control inspection,30fps render/long-term Asset,
critical-frame comparison, font resolution/layout, alpha and raster fidelity
remain **unrun/unmeasured**. Own-reader structure does not prove native acceptance
or unchanged paint pixels. Existing native Text font/cache approximations remain;
this checkpoint repairs omission, not those limitations or full-project export.

## Singular Opacity temporal easing — export-only sampled approximation

**AEP → FX:** unchanged. **FX → AEP:** enabled scalar owner Opacity tracks
with finite cubic controls and a vertical endpoint (`x1=0,y1!=0` or
`x2=1,y2!=1`) are evaluated from their original curves on an integer-millisecond
grid, including endpoints, and reduced with the existing bounded streaming
`fx_keyframe_bake` fitter to editable Linear keys. The fit tolerance is 0.005
original percentage points; a separate sampled reconstruction check requires
maximum error ≤0.01 percentage points (AV fraction 0.0001). This replaces an
unrepresentable finite-native-speed segment, not its owner or visual subtree.
Original segment endpoints, incoming start easing, ordinary segments, other
tracks, layer content and clocks stay unchanged; only a derived export document
is prepared. Continuous fades do not acquire artificial Hold interpolation.
Native finite/precision/key-count/clock-collision validation remains unchanged.
Diagnostics identify owner, Opacity window, source controls, key count and
measured grid error. Nonfinite controls and disabled/ordinary tracks are not
repaired; failed fitting retains the original track for native diagnostics.
The complete repaired track is checked against the native key limit, including
an untouched ordinary suffix after singular expansion. Over-limit repair retains
the whole original animator rather than sending oversized keys to native lowering.
`singular_opacity_early_expansion_reserves_large_linear_suffix` fails without this
final check (queue439); this is CPU admission evidence, not Adobe fidelity proof.
A new local preparation-wide admission cap reserves at most **65,535 integer-ms
sample-grid points**, counting both endpoints of every singular segment, across
all attempted tracks. Each admitted point is evaluated at most twice (fitting
and reconstruction), at most **131,070 evaluations** per preparation. Checked
whole-track admission happens before fitting; declined tracks consume no allowance,
while failed admitted fits keep their charge. Long or aggregate-over-budget tracks
retain their complete original animator with an owner-local diagnostic for existing
native validation, which may still omit that owner; smaller convertible siblings
continue. This is not a shared script budget, process-wide cap, or native-key-count
inference, and separate linked-scope preparations have separate allowances.
Input traversal still scales with authored track count. Full-grid tolerances and
native clock restrictions are unchanged; no coarser sampling fallback is used.
Out-of-range cubic X controls are never repaired: both the local detector and
existing `fx_schema::animator::keyframes` validation require X in [0,1]; malformed
editable input therefore retains its schema rejection before native lowering.
The 0.01pp check is **pre-native only**: native PropertyClock tick rounding can
increase error. `singular_opacity_fresh_native_grid_reports_quantized_error`
freshly writes/reads both numeric cases and independently interpolates the saved
Linear keys on their original integer-ms grids; this is own-reader numeric
approximation evidence, not a native cap guarantee or Adobe control/render proof.
Queue 338 passed all seven singular-opacity CPU tests; queue 339 passed the
existing `native_key_clock_collision_and_overflow_are_errors` regression.
Queue 340 executed fresh native-grid measurement with captured output:

| Source-derived target | Segment / samples | Total native Opacity keys | Maximum post-tick error (percentage points) |
| --- | --- | --- | --- |
| Wave owner 2200063 | 400..466ms / 67 | 69 | 0.074556984725 |
| Tunnel owner 200270 | 66..266ms / 201 | 71 | 0.020836673218 |

Both post-tick errors **exceed** the separate 0.01pp pre-native reconstruction
cap. No threshold was lowered; native clock quantization and its existing
warnings remain unchanged. These results do not establish sub-ms or Adobe
fidelity. The numeric extracts are not the complete private original projects.

Source-derived CPU regressions reproduce Wave owner 2200063's 400..466ms
100→0 fade and retain both 14-key X/Y tracks, and Tunnel owner 200270's
66..266ms 34→0 fade with preceding 0ms22 Hold / 66ms34 Linear keys.
The public numeric-only extract remaps ownership to an existing public Rect;
it is supplementary structural evidence, **not** an independently Adobe-authored
feature oracle. `singular_opacity_wave_retains_owner_and_xy_keys` failed before
the fix with native temporal-ease rejection and owner omission (queue 329), then
passed after it (queue 331). Additional focused tests are tracked separately
in execution results. Import/native Adobe control readback, independent 30fps
reference publication/fresh hash verification, sub-millisecond fidelity and
RGB/alpha comparison remain **unproven/unmeasured**. No new JS, FX runtime,
model/schema, renderer or per-media-frame baking is introduced.

Fresh source-derived Wave lineage retains original ancestors, both line/halo
children, effects and seven original dynamics entries. Private slice SHA-256
`4d2f2b2ef45f1360346a10aa1073e340a97df8a8bb20f9078609ebd9ce61c2d2`:
build308 omitted owner2200063 for the singular ease; fixed build351 writes it
and its line/halo without owner/subtree omission or unresolved source IDs.
Fresh AEP SHA-256
`1c768782cc6222ec4c4e263f214b306fe84ec38b4e58d31e80847f369109c279`.
This is source/own-reader structure, not Adobe render proof. The full Tunnel
ancestor/matte/shader case has not been exported separately.

Final standalone check passed in342, but its test suite failed at
`audio_package_check_write_and_reimport_preserve_edited_gain_and_wave_bytes`
(`audio.rs:84`, `Option::unwrap()` on None). It fails independently in349 and
on the unchanged base51d5587eb in353; this is a pre-existing validation blocker,
not a green full-suite claim. Clippy/fmt passed355. Independent full-diff review
found no blocking defects; its diagnostic clarification now explicitly labels
the sampled cap pre-native. This repair does not include an unrelated Audio fix.

## Launch MOV metadata admission — export-only structural repair

**AEP → FX:** unchanged; no new import mapping or proof. **FX → AEP:**
whole-source MOV staging admits explicitly described mono/stereo PCM16 `sowt`/
`twos` version 0/1 and one validated `tmcd` ancillary track. PCM version-one
packet/sample/channel fields must agree; unknown precision/layout/extensions,
unknown handlers, duplicate timecode, unsupported flags and malformed clocks
remain rejected with contextual media diagnostics. The native footage writer
uses the extracted sample rate and video codec; it does not decode audio or
rewrite the MOV. Existing source-file copying preserves all raw tracks and
ProRes `ap4h` alpha bytes, rather than AAC encoding or stripping timecode.

Whole-track movie durations now admit only the exact integer ceiling of the
media duration in movie ticks, still equal to the movie duration. The exact
media sample clock and frame count remain unchanged. Identity edits must still
cover that entire track at media start zero and unit rate; floor/excess durations,
shifted/cropped edits, matrices and other malformed profiles remain rejected.
This removes an unnecessary cross-timescale equality restriction, not a floating
tolerance or retiming approximation.

Evidence: the private original Logo MOV SHA-256 is
`025557fd52af5dae558f53a1c2294ec7625323d35750e61e4d75dfb3f54c9de6`;
read-only inspection found PCM16LE/stereo/48000, version-one packet fields
`1/2/4/2`, and timecode `30/1/30`. The independently remuxed 43-frame source
uses media `15360/22016` and movie `1000/1434`. Six `pcm_timecode` synthetic
regressions and `movie_tick_ceiling_preserves_exact_media_clock_and_rejects_edits`
are supplementary metadata assertions, not Adobe-native feature oracles.
The clock test failed before the fix (queue 226) on the exact duration guard;
post-fix queue 241 passed all seven targeted tests (including the clock test).
Fresh source-derived Check now retains Logo Video `1500212` with its original
ancestors, GaussianBlur, source/playback clocks and raw MOV. A fresh AEP Write
publishes that exact 5,330,324-byte MOV with the same SHA-256; own-reader inventory
finds the native footage source and no unresolved source IDs. These are staging
and own-reader results, not Adobe-native acceptance. Native header readback,
Adobe opening/editable inspection, native 30fps references/Assets, and
RGB/alpha/audio fidelity remain **unrun/unmeasured**. No media is committed.

## Import composition assembly performance — regression evidence only

**AEP → FX:** composition assembly now applies motion blur before adding the
converted trees, avoiding a full populated-composition clone. Exact numeric
values and existing structural/on-disk validation remain unchanged. The bounded
three-run CPU panel reduced import time by 14.0% and median peak RSS by 28.5%,
with byte-identical project payloads. Native-fixture differential imports and
new CPU regressions passed; see [case identity, checks and limitations](conversion-performance.md).
**FX → AEP:** unchanged by this import optimization; no new export or independent
Adobe proof is claimed. No feature approximation/omission policy was changed.

## Deep Blue v13 destination-media and typed-text export repair — partial

### Box Source Text native-acceptance follow-up — partial; full project blocked

**Post-recovery native evidence (after the earlier offline checkpoint below):**
operator repair cleared the first fence. At converter build `dd1c2b928`, freshly
generated static and two-Hold Box Text cases with explicit `ArialMT` / empty style
passed typed `headless-adobe` open/render, cleanup and fresh READY. Both native
outputs are 1920×1080 / 30fps / 263 frames / 8.766667s. No failed AEP was reused.

| Case / native target | Generated input AEP SHA-256 | Published MP4 SHA-256 | Executed checks / boundary |
| --- | --- | --- | --- |
| Static `Kasparov won the first match`, target `1` | `fc5f9aab09e04241e62c3de0fb7fab820c58551e17c87c0d937e51f6a98ede6e` | `d496d4eb66b64717de78c6abcf9ff2bbcbe0bb0ac422013465c65cde80b4755e` | At 0.5s: 7,268 nonblack pixels, bounds `[654,938,1267,983]`. Supplemental own-reader of Adobe's normalized save retains the exact input words, 48px font and 660×68 box. No OCR/image recognition or independent native Text-control inspection. |
| `FIRST` → `SECOND`, two Hold documents at source-local 0/700ms, target `1` | `1fa0cb0531f2a8b56ffd7320edde29ca5f54d3c8dabfcc3a54333899e7bb4f47` | `d920d1e0d3062a9c7cd9e8391d3c6479f587e1c766546af4a233d464ce058f17` | Samples at 0.5/1.25s have 1,765/2,841 nonblack pixels and 4,952 changed pixels. Own-reader of Adobe's normalized save retains `FIRST`/`SECOND` and source-local 0/700ms windows. This is a discriminating temporal-change check, not exact event-clock, font/layout or independent raster equality proof. |

Receipts identify AE `26.5x89`; local request IDs are
`deep-blue-generated-static-pinned-dd1c-recovered-v1` and
`deep-blue-generated-hold-pinned-dd1c-recovered-v1`. Native scripts are exclusively
package-owned; no direct Adobe launch, custom runner, UI operation or failed-AEP
repair was used. Videos remain ignored local artifacts; **no long-term Asset ID
or independent Adobe-native 30fps feature oracle/scoring is established**.
The cases explicitly pin a proven native face rather than proving all original
font-family/style resolution.

A separate fresh conversion of the unchanged original `.tsrct` then failed
`inspect_aep(profile="adjustment")` during native open with **“Error reading the
text layer. Skipping the text layer.”** It did not publish readback or render;
cleanup failed verification, recording the former `requires_operator` field for
run `8099ad1b0a3a4999a4c7ef39537dc75c` under the earlier lifecycle. Current recovery
verifies owned cleanup and fresh READY without a manual-unlock latch; old journal
fields no longer gate execution. Malformed-AEP recovery remains unproved.
Its AEP, and every previous AEP with this error,
are **excluded from all subsequent inputs/references**. Only original FX sources
and independently valid native fixtures may generate the next fresh attempts.
The full project, matte, audio, other fonts, empty/larger document timelines and
broader cache invalidation remain unverified; the task is not complete.

Offline comparison identifies a font-request difference: original owner 8000
has the passing probe's words/48px/660×68/52px leading but requests `Arial` /
`Regular`, for which existing candidate lowering emits `Arial-Regular`. The
passing diagnostic input explicitly requests `ArialMT`. A proposed hard-coded
Arial-to-ArialMT alias was **withdrawn at the user's request**, along with its
fixture-name test: the actual requested font binary and FX fallback/custom-font
resolution had not been checked. A native fixture's identifier does not prove
that it represents the same typeface the user's FX runtime resolves. There are
**no added font-name aliases** in this follow-up; existing candidate lowering
remains unchanged and diagnosed, not validated actual-font mapping. No fonts
were installed or original FX font requests edited.

The native default font table retains `Myriad-Roman`, `Helvetica` and
`AdobeInvisFont`. Although not explicit name aliases, these defaults may affect
native fallback behavior; they have **not** been matched to FX fallback/custom
fonts and now carry an owner-local warning. Font-system preservation is an
unresolved converter requirement, not unsupported FX capability and not a
claim of equivalent rendering. The pinned-ArialMT probes are only Text-record /
Hold native-acceptance diagnostics, never original-font or fallback proof.
The full Text-reading failure's cause remains unconfirmed. No additional font
rewriting/fallback policy will be introduced without checking the actual source
font-resolution contract.

The rebased headless API implements native FPS/ranges, MOV/RGBA, fixed readback,
expression capture and separate AME routes. Its bounded native smoke exercised
FPS/ranges/audio, uniform RGBA and empty readback/expression results. An AME encode
failed the requested-30fps gate by retaining 24fps; Premiere-via-AME, positive
control readback, nonempty expression sampling and converter-feature fidelity
remain unproved. Fixed adjustment readback does not expose Source Text document
controls. Missing feature proof is not waived by those interfaces.

#### Earlier offline checkpoint (superseded where noted above)

**AEP → FX:** implementation and independent import proof are unchanged.
**FX → AEP:** a bounded experimental Box Text profile now replaces the
independently authored default's Source Text/font/style/rectangle/whole-document
runs, owner GUID and selected Source Text clock with typed FX values. It covers
Box Text without animators/path/anchor options and constant whole-document font
and box geometry across holds. Point Text and richer Text controls remain on the
older writer and have **no new Adobe acceptance evidence**.

The native source is the public `text_document_box_v3.aep` (SHA-256
`5b6bf16fb87930e38e975602c0848230173c50a179431d2a3eb34f09e0cffdce`),
composition target `1`; this is a structural-default source, **not new feature
render proof**. Independent Adobe-authored Box Text rendered through the typed
`headless-adobe` API at its native 24fps (640×360, 48 frames, two seconds), showing
`Editable AEP` / `PR 4442`. That smoke is not the required pinned 30fps long-term
Asset reference, does not exercise converter output, and is not a fidelity pass.

The generated static Text owner 8000 was retained structurally but **Adobe
rejected it with “Error reading the text layer. Skipping the text layer.”**
The headless cleanup then failed verification and fenced the worker with
`requires_operator`. No recovery or further Adobe launch was attempted. Offline
comparison found the full COS serializer reordered `/98`/`/99` headers and
changed native real tokens such as `1.0` into integers. The header/token regression
failed against that writer and passes with explicit native default templates;
five focused tests now cover typed values, UTF-16 escaping/run lengths, rectangle
ordering, held documents, selected clock and owner identity. PR review additionally
found raw CR/LF bytes inside UTF-16 literals: byte-level regression fails before
escaping and passes afterward. That protects standard COS line-ending handling;
it is not a newly observed Adobe failure or evidence of native acceptance. **The corrected
profile's Adobe open/render acceptance remains unrun**, not fixed by test totals.

The templates retain independent native **cached glyph/line layout**, including
original layout bounds/offsets, rather than recomputing it from edited FX. This is
a contextual export warning and an unresolved native reflow/layout risk. Whole
font/box changes across holds are diagnosed as unsupported and omit their owner;
convertible siblings remain. Existing vertical alignment and font identity
approximations also remain. See the
[profile and sanitization details](../crates/aftereffects_file/src/writer/native_text/README.md).
No native readback/control inspection, held-text render, original-project Text,
matte/audio acceptance, RGB/alpha scoring or 30fps immutable feature reference
has completed for this profile. These are **blocked/unmeasured**, not passes.
CustomShader remains unsupported and diagnosed. Both the native acceptance gate
and the mandatory feature-proof delivery chain remain incomplete.

Final offline checks for this follow-up: standalone converter `check` and full
CPU `test` passed (3,261 passed / 407 ignored); `clippy` and `fmt` passed after a
byte-slice lint correction. A fresh 30fps export of the unchanged user archive
retains 39 direct / 43 reachable native layers, 13 Source Text blobs and nine
media artifacts whose hashes exactly match the earlier package. Own-reader
readback retains 87 static/held Text documents with the input's words (no donor
`Editable AEP` / `PR 4442` content), the luma track matte and three Audio
structures. This is **converter structural/CPU evidence**, not independent
AEP→FX import proof, native acceptance, correct Adobe glyph layout, audible
alignment or full-project fidelity. Ignored proof tests remain unrun.


**Direction: FX → AEP only. Import behavior and import proof are unchanged.** A
local user-authored `.tsrct` at 1920×1080 / 8.76s / 39 root layers generated an
AEP even before this work but lost major layers. This is a bounded best-effort
export repair, **not complete restoration or a passing Adobe-render comparison**.

| Feature / original semantics | Export replacement, reason and impact | Executed evidence / missing proof |
| --- | --- | --- |
| JPEG/PNG stills (including PNG straight alpha) | Decode and emit float RGBA OpenEXR in owned staging; re-parse its native dataWindow and package only reached assets. Native exporter previously accepted only OpenEXR, so five requested JPEG/PNG sources were omitted. Embedded source profiles are not converted; AE colour management and image/alpha appearance may differ (unbounded). Image preparation preserves full source dimensions without a pixel ceiling or ImageReader allocation cap; sequence PNG dimension inspection uses the same unrestricted reader policy. Zero dimensions, malformed data, checked decoder arithmetic and I/O failures remain errors. Actual allocation/address-space failures remain possible. Malformed image decoding or I/O fails explicitly rather than silently dropping siblings. | Small PNG alpha/JPEG EXR pixel and dimension CPU tests passed. `image_past_former_pixel_limit_preserves_dimensions_and_pixels` failed on the former cap, then passed with 4000×3001 metadata and RGBA sample checks; truncated PNG/JPEG rejection also passed. Targeted queued package/media tests passed (52/162; existing ignored cases remain unproved). Fresh hash-pinned P008/P042 exports each retained both formerly rejected 5504×3072 image assets; diagnostics remain visible. This is export-preparation evidence, not native fidelity. Earlier fresh local `--check` includes all five reached image files; two are inside the restored luma-matte group. Adobe open/control, independent native render, RGB and alpha comparison **unrun/unmeasured**. |
| Full-run QuickTime identity edit list | Admit only a single version-0, one-entry, unit-rate edit whose media time equals the complete constant-rate presentation origin and whose duration equals the already-validated native track/movie duration. No `ctts` means origin zero; unsigned version-0 `ctts` must cover every `stts` sample once on one contiguous presentation grid. This retains B-frame reorder delay without trimming content. Shifted, shortened, multi-entry, signed-offset and discontinuous presentation profiles remain unsupported. No video re-encode, playback-key shift or timing normalization is performed. | Presentation metadata RED job1308:3 failed; GREEN job1320. The same pinned MOV export assertion was RED on exact-base7d720ad45 plus test-only additions (job1440: footage omitted), then GREEN with repair (jobs1389/1413), including byte-preserving packaging and1s→.5s authored-rate edit response. Final CPU suite had12 identical inherited failures on base1423 and branch1413;1650 vs1655passed,409ignored; fmt/clippy1425 passed. Independent AE26.5x89 source/readback job `1b71059c835e49109a3d988705c7e151` pins original100%/1s and edited50%/.5s controls, the same24fps/full1s MOV, and source/native hashes in [the B-frame fixture](../crates/aftereffects_file/tests/fixtures/bframe_presentation/README.md). Fresh edited export opened/rendered15frames/.5s/30fps with cleanup/READY (job `d65e5b71310c4698a6d95af107c7648f`, MP4 SHA `f98c6991e27aa1dd7ddf5aac241db6591c4720d47767257d0ff7d23ad10571fa`). Its authored position offsets/crops the picture; identified visible footage frames support2× rate, **not full-canvas/RGB equivalence**. Reviewer corrected only the explicit FX proof input to identity placement; fresh test exports1742/1743 and managed native readback737fe6bbd84b402ba1d215d4b8d0bc36 establish full-canvas placement, unchanged1s24fps source and100%/50% edited controls. Managed3s/90frame/30fps MOV panel ef0eb38e3f0c4afbb1727f8f886fefc5 compared all30 original/15 edited full RGB24 frame pairs: maximum channel difference1LSB, but unchanged rgb-hybrid/.99 **FAILED** (original min.916603, edited min.917811). Both managed operations completed cleanup/fresh READY. See the fixture README for hashes and retained invalid anchor-assertion provenance. This is native acceptance/edit response, **not strict RGB fidelity**; immutable Asset publication/download and alpha/audio remain **unverified**. P043's untouched MP4 has supplementary native24fps/full45.958333s/118-key control evidence, but current-main MP4 admission still omits it before this metadata path: **P043 is not fixed** and4807 is not duplicated. AEP→FX is unchanged/unvalidated by this export-only admission repair. |
| FX Video zero gain with no source audio track | Keep the video picture and author its native audio switch off; an authored mute has no audible content to remove from an intrinsically silent QuickTime source. Nonzero gain without source audio remains unsupported. | `zero_gain_video_without_a_source_audio_track_keeps_its_picture` red-before/green-after CPU regression passed; the local user video now remains among staged media. Adobe render and audible fidelity **unrun/unmeasured**. |
| String-valued `textContent` scripts | Previously rejected by a numeric-only script probe and omitted as entire Text owners. Sample at 4× output FPS, write editable Hold Source Text documents at observed changes and reject non-string/stateful scripts. On a 24fps export, change instants can shift forward by up to one grid interval (~10.42ms before integer-ms rounding); sub-grid events may disappear (unbounded visual error). No `JsScript` is generated in native content. | A source-derived `IBM RETURNED` script failed before and passed after both typed sampling and native text lowering; non-string and call-history guards passed. Fresh local `--check` no longer omits all seven affected caption owners. Source AEP, Adobe-control inspection, independent render and scored RGB/alpha **missing/unrun/unmeasured**. |
| Mono/stereo WAVE_EXTENSIBLE 24-bit PCM | Verify the declared RIFF size, 40-byte extensible `fmt`, PCM subtype, all 24 valid bits, conventional speaker mask, and sample/byte rates; replace only its header with ordinary PCM24 `fmt`. Keep the entire remaining RIFF stream and sample bytes unchanged. Other extensible layouts/subtypes remain unsupported; inconsistent or truncated files fail rather than being published. The last sample can make interpreted duration round up one millisecond beyond the FX-authored duration; permit only that positive difference when the selected FX source range is within the authored length. No samples are trimmed or retimed. | Synthetic PCM and wrong-subtype CPU tests passed; local source `dakota-performance` and FX owner 2 appear in a fresh `--check` and freshly written AEP. Its 1,275,195 PCM data bytes matched the original source exactly (SHA-256 `1801d4a8dd11a72cac77e54ea66840a1503c98f7c13b4a87d2569378d782207f`). Our own `inspect` read the AEP; Adobe audio playback and audible comparison **unrun/unmeasured**. |
| MP3 with one-frame decoder priming | Only the AE destination route may decode a standalone MP3 whose first packet explicitly carries a Skip Samples count matching its positive first decoded timestamp, at an integral offset of at most 1152 samples. The native transcode checks all frame timestamps for continuity after subtracting this offset, compares declared length with decoded length within at most two MP3 frames, and re-probes the PCM WAVE before publication. Ordinary `media_transcode::run` still rejects nonzero source start; unsupported offsets and source inconsistencies do not become zero-start automatically. Normalizing the MP3 decoder's first content sample to PCM time zero is an **audio-clock approximation**: the original FX player's handling of codec priming is unverified and might differ by up to that bounded source-start interval. | A generated, pinned, 48 kHz mono sine MP3 (SHA-256 `bd5e6566402669b7d493417e5ea42cb033eee253541781227dc31c1d6f88e1dc`) exercises ordinary-run rejection, destination decoding and staging; the local `andratx-music` stereo MP3 and FX owner 2001 survive a fresh `--check`. A fresh AEP export and our own `inspect` completed; its staged output is PCM-float 48 kHz stereo / 212.024521s. Adobe editable controls, source playback alignment and audible comparison **unrun/unmeasured**. |
| FX Image Luma track matte in a nested Group | Image owner 20 samples grayscale image provider 21 as a Luma matte inside FX group 24. The previous image-footage gate rejected the matte and pruning omitted its subtree. The existing native editable matte link (mode 3) now connects the two image layers, with the provider disabled for independent painting but retained for matte sampling. No new FX schema or writer effect, no JS and no flattening. The provider's luma/color/alpha interpretation in AE and the order of missing WGSL effects are unverified; this is not a pixel- or alpha-fidelity claim. | `nested_image_luma_matte_retains_both_editable_sources_without_provider_paint` failed on owner 20 before and passed after the targeted change; it asserts native image sources, mode/ID link, disabled sampleable provider and the effectful precomposition boundary. A fresh local `--check` and newly written AEP include owners 20/21/24 and both image media, with no subtree omission; our own `inspect` reads them. The project source SHA-256 remained `9bb0a8152f4e31a482df87f738597a23a29d3d28cfddc07201b4fda11ff3d1b2`. The source is local user-authored FX, **not** a pinned independent Adobe-authored feature oracle. Adobe open/control, native 30fps reference/Asset, RGB/alpha comparison and render are **missing/unrun/unmeasured**. |
| Remaining effects and image appearance | All seven custom WGSL shader instances have no native editable mapping; now all seven are reached and explicitly diagnosed, including one on group 24 and one on image 20. Their missing animated glitch reveal, color/luma and alpha semantics can visibly differ (unbounded). Other effects and exact typography retain existing diagnostics. Do **not** flatten to media, inject JavaScript or replay an original AEP to conceal the gaps. | Local fresh `--check`: nine media artifacts (five EXR, one MOV, three WAV); no owner/subtree omissions, but **59 diagnostics** and unresolved visual fidelity. Adobe-native fixture/reference, immutable Asset publication, editable control readback and RGB/alpha/audio comparison remain **missing/unrun/unmeasured**. |

Source `.tsrct` bytes were unchanged. Our own check, write and inspection are **not** an Adobe render, acceptance
test or proof that any new audio is audible in the composition.
Import behavior and import proof remain unchanged.

## Video motion-blur admission — P024 partial export repair

**FX → AEP:** visible Video motion blur now passes the footage admission check,
which previously omitted the whole owner despite the shared native layer-options
writer already carrying its editable motion-blur switch. Source clocks, CornerPin,
Alpha matte links, media and Transform remain on their existing native paths.
Hidden video, unsupported blend/matte profiles, captions, placement and legacy
Media admission retain their guards. No shader support, source shifting, media
flattening or new FX capability is added. **AEP → FX:** unchanged.

The source-derived P024 check previously omitted owners 10/20/30. Owner 30 now
survives with its native video and CornerPin. Owners 10/20 remain omitted because
their non-unit stretches produce fractional-millisecond property-key times that
the current finalized source-clock representation rejects. This patch does not
round those keys or claim that P024 is completely repaired.

`video_motion_blur_reaches_the_existing_native_options` failed before and passed
after admission repair, checking blur, Alpha matte identity and the hidden-video
negative guard. A separately Adobe-authored Video/CornerPin/Alpha-matte control
reported the blur switch and numeric corners. Fresh generated P024 opened and
rendered natively (1920×1080, 30fps, 300 frames, 10s), with verified cleanup/READY.
Canonical full-resolution RGB24 rgb-hybrid comparison at 0.25s (41 samples)
improved mean **0.0747540906 → 0.4041256341**; minimum **0.0026941184 →
0.0028555237**. This remains below the owner-approved 0.95 acceptance line.
The unchanged-main output hash matched the retained native before artifact.

A generated readback/edit-response script passed root/video/matte/blur assertions
but failed an incorrect requirement that every CornerPin point be keyed; constant
controls may legitimately have no keys. That failed request remains retained,
not counted as a pass. A separately authorized corrected managed readback
`r01-pr4924-generated-blur-edit-accept-v1` (native job
`7a25cb424e07443ba96325c3c9bf3f6b`, queue 2436) passed with verified READY.
Both generated blur-on and FX-edited blur-off variants preserved root inventory,
matte/source/timing fields and all four numeric CornerPin controls at six sample
times (0, 2, 4, 6, 8, 9.5 seconds), before and after native save/reopen. Each
control had one key; constant controls do not require two keys. The blur switch
changed true → false while CornerPin samples/key counts remained identical.
Handles were reacquired after reopen and all raw phases were manually serialized
and validated offline. This closes the bounded generated edit-response proof.
General motion-blur sampling equivalence, alpha/audio fidelity, independent
30fps feature-reference Asset publication and import proof remain unverified.

## Image blend and motion-blur admission — bounded export repair

**FX → AEP:** Image blend and motion blur were redundantly rejected by the
footage admission check, despite the shared native layer-options writer already
representing both controls. The same rejection during animated bounds analysis
could omit a complete containing scene. Images now retain their actual native
blend and motion-blur switches; Screen is not replaced with Normal. Existing
visibility/matte handling, source interpretation, placement, captions, crop and
input-LUT restrictions remain, and Video/legacy media admission is unchanged.
**AEP → FX:** implementation and proof are unchanged.

Original semantics: composite an editable still using its authored layer blend
and motion blur. Replacement: existing native editable controls (Normal `2`,
Screen `6`, and the actual motion-blur flag), not baked media. Reason: remove an
inconsistent Image-only admission gate. Impact: Adobe/FX color spaces, alpha and
motion-blur sampling may differ; RGB/alpha error is unmeasured and unbounded.

`image_native_options_keep_screen_and_motion_blur_inside_clocked_scene`
checks Normal/Screen × blur off/on, a retained scene and independent solid,
source identity, position and native layer-local timing plus start time. The
EXR descriptor is pinned to the native `footage_not_missing.aep` fixture; this
is not an independent Screen/motion-blur feature oracle. The private launch
archive's Image `1200048` supplies a separate source-derived regression retaining
its four ancestors, four effects and thirteen animators. Source-based Check,
CPU execution, Adobe acceptance/control readback, independent 30fps reference
and long-term Asset, RGB/alpha and full-project fidelity must be reported
separately; native feature proof remains incomplete.

Executed CPU evidence: the new regression failed on the motion-blurred Normal
case before the admission fix and passes afterward for all four combinations.
The full converter `check/test/clippy/fmt` passed (3,280 passed, 407 existing
ignored). A fresh source-derived Check for launch Image `1200048`, original
archive SHA-256 `a98269b684137b80d35dbe28c701551942156e808ed51cb1082ba0bc4e145fbf`,
changed from an omitted/empty scene `1200000` to retained content, with all
thirteen script animators prepared and no owner/subtree omission. These are
structural results; Adobe open/render/control, independent reference/Asset and
RGB/alpha fidelity remain unrun/unmeasured for this repair.

## Multiply video print pass — bounded FX → AEP export repair

A visible FX Video with Multiply blend was previously omitted with its entire
parent scene, even though the native layer-options writer already maps Multiply
to an editable AE layer blend record (`5`). The video source, occurrence clock,
transform and supported effects remain on the ordinary editable footage path;
this initial change admitted Normal and Multiply Video blend modes. The later
original-IG repair below also admits the existing native Overlay control (`7`)
and bounded Alpha mattes. Other video blend modes, hidden video, motion blur,
placement and caption options retain their existing rejection. No CustomShader is exported: shader
instances on the retained owner still receive their separate omission warning.

Original semantics: Multiply the print-pass video over the composite below it.
Replacement: AE's native Multiply layer blend. Reason: this is the existing
editable blend control, not a flattened frame or a guessed Normal blend.
**Impact:** AE and FX blending spaces, source color and alpha handling can differ;
pixel and alpha error is unbounded. Local source-derived structural assertion
`multiply_video_keeps_its_editable_footage_and_normal_sibling` checks two
source-backed layers, the shared source identity, the native blend records and
the explicit rejection of unproven video blends. The private IG master is not
an independently Adobe-authored fixture. Adobe-native control readback, a
30fps independent reference and long-term Asset, RGB/alpha comparison and
whole-film audio/render proof are **missing/unrun/unmeasured**. Import unchanged.

### Original IG scene repair — partial export implementation, no Adobe proof

**FX → AEP only; import implementation/proof is unchanged.** Source archive
SHA-256 remains `8a9656b59e50034766d29dac798a73149e18f55ca4e458cb89682a40e2f71144`.
This source is user-authored FX, not an independently Adobe-authored oracle.
The repairs preserve editable content where current native mappings and support
proofs permit it. Retained scene roots are not restoration of missing children.

| Original semantics / feature | Export mapping, limitations and impact | Executed structural evidence / missing proof |
| --- | --- | --- |
| Alpha-matted Video and consumer-bounded planar Text | Existing native Alpha matte links and exact source/occurrence clocks. Consumer support uses all-time inverse transforms, not Text-box glyph guesses. Native effects with unproved sampling reach still require full input; projective Text and true near-plane crossings reject. | Fresh original readback retains B02 four Text and two Video. Native open/control, reference Asset, RGB/alpha/audio proof missing. |
| Video Overlay print passes | Existing editable native layer blend `7`, with original footage and transforms. Geometry analysis uses the existing media content view independently of wrapper blend eligibility. Blending space/color/alpha error remains unmeasured and potentially unbounded. | `source_rect_mask_keeps_overlay_video_and_rejects_its_real_camera_crossing` passed; asserts native source/blend and a genuinely crossing 4000-pixel surface. Original S02's six videos remain absent because their containing Stroke/Shadow title units reject; implementing Overlay alone did not restore them. |
| Compound circle/rectangle mask contours | A narrow positive-winding canonical circle/axis-aligned Rect profile becomes actual native Add masks. Arbitrary curves, reversed winding, unsupported topology/clock/easing remain diagnosed omissions. No mask is discarded merely to obtain bounds. | Original S02 native circle and 761 positive contours survive. Contour/edge/alpha equivalence has no independent native proof. |
| Source-local hard Rect mask with held XY motion | Copy actual native Path keys on the proven source-zero clock and coordinate parent. Constant/Hold XY only, finite square-corner Rect; other guide animation/geometry, foreign clocks/parents and soft-mask crop certification reject. The live guide link is replaced by editable independent Path keys. | `source_rect_mask_preserves_exact_held_placement_and_its_all_key_support` passed. Original door guides2636/2637 retain Y −200 at2133ms → −640 at2143ms. Reader normalizes the first unused incoming easing; actual segments retain Hold. Native edge/alpha and editing proof missing. |
| Exact static rectangular Shape crop guide | Export crop eligibility recognizes a finite positive axis-aligned four-corner straight contour, with either Close or an exact repeated first point then Close, rather than requiring the Rect layer tag. Existing hard-Add, mask properties, static path/modifier, affine ownership and exact guide/owner-clock guards remain. The native mask is emitted, not removed to invent glyph bounds. | `native_rectangle_shape_guide_retains_edited_masked_text_with_an_explicit_matching_clock` imports pinned `import_mask_controls.aep` SHA `01380c1f8c5ebe486cd068e5dee50447870b86e4864fc842590a99386dd9b417`, comp1/layer17. Explicitly edits the source-derived guide to the owner clock; original MAX_TIME guide is not admitted or claimed fixed. RED omission and GREEN retention; Rect/five-command/six-command native mask payload equality and rejection cases are CPU structural evidence only. Import mapping unchanged; independent Adobe acceptance/render/alpha/text glyph proof remains unrun/unmeasured. Live editable guide linkage still lowers to an independent native Path. |
| Static Shape mask with authored affine placement | Fixed contour/anchor/orientation plus prepared native XY/Scale tracks become Path keys at their joint authored times, without additional sampling. Each interval must have one common easing among changing channels: Hold or Linear. Simultaneously changing Hold/Linear, Bezier, spatial tangents, dynamic geometry, unsupported 3D/clock/parent reject. Retains source guide3902's offscreen −100000 X. Original JS preparation still has the existing sampled-clock approximation; this mapping does not restore unsampled events. Live cross-layer linkage is lost. | Six source-mask regressions and five affine tests passed before the spatial handoff; subsequent seven source-mask tests passed. Original S03 Add3903 and Subtract3902 both survive. Independent Adobe semantics/render/alpha unmeasured. |
| Mask-bounded mixed planar Text and spatial siblings | Existing certificate now carries native-mask provenance to classification, separately from root-output and 3D-consumer viewport flags. All known child geometry and own projective Text are validated before accepting support; original camera/lens/near-plane checks remain. First actual hard Add bounds support; only actually retained Subtract/Intersect suffixes may follow. Native effects' unknown input support is not waived. | `source_rect_mask_certificate_reaches_mixed_spatial_source_classification` passed, including a genuine child near-plane crossing negative. Before the handoff camera3900 was omitted; fresh original after it contains all44 source Image layers and one Text `o`. Five AGENTS letters remain omitted with their retained OuterGlow; `f,r` still reject own Text skew at this checkpoint. |
| Time-correlated 3D enclosure | Existing full-track proof first, then bounded authored-key windows retaining complete overlapping segments and all-time child bounds. Every window must satisfy unchanged near-plane guards. This is continuous enclosure, not sample-only acceptance. | Fresh original S06 retains18 editable Text (`makeitgreenredblue`) and four Video. Native render/edge/alpha proof missing. |
| Parse-invalid Group scalar script | Guarded finite authored typed-base fallback for independently reparsed syntax-invalid scalar Transform controls on an ordinary nonempty Group owner, without dependencies, layer references or random targeting. The owner must retain the existing simple clock and have no paint, effects, masks, matte, non-Normal blending or motion blur. Descendant drawable kinds and effects do not determine whether the failed owner control contributes a value; descendants still pass all ordinary export/enclosure safety checks. Empty owners, non-Group controls, unsupported clocks, runtime errors and geometry/Text/effect scripts are not recovered. S04/S05 malformed `1700--0.066` PositionY scripts use diagnosed Y1700; intended motion is lost, not reconstructed. | Static-placement approximation, not restored intended animation. Mixed/vector descendant regression and owner-profile negatives pass. Independent AE vector placement and fresh generated BASE/EDIT readback preserve editable Y1700→1680 (one constant key at time zero, no expression). Actual P046 export retains S04/S05; native RGB comparison remains diagnostic and below the strict .99 floor. No general render-fidelity, alpha or audio claim. |

Later fixed-Skew repair (`d725f2377`, frozen build160) preserves the **constant
shear only** as two native signed-SVD Null factors under a third Position/Rotation
Null. Original Text retains its FX identity, document/font/intrinsic stroke,
anchor, nonuniform Scale (including zero) and Opacity. Existing authored native
Position/Rotation tracks move intact to the placement Null; their interpolation
is not fitted or sampled. Native Null opacity is not inherited, so opacity stays
on the drawable. Animated Skew/axis, own3D/effects/masks/Path Text/animators/motion
blur/inherited transforms remain diagnosed rejections. Live controls are split
between editable native parents and Text, rather than exposed as one Skew control.
Native fonts remain unverified: these four source letters name
`Exposure[-30]-Italic`, a deterministic PostScript candidate, not a proved host
font match.

Executed supplementary assertions:
`fixed_shear_factorization_is_exact_with_zero_overshoot_and_signed_nonuniform_scale`
passed; `static_text_skew_retains_native_drawable_document_and_distinct_helpers`
passed (native names/parents/partial lifetime/anchor/document);
`fixed_text_skew_keeps_animated_placement_and_drawable_tracks_separate` passed in
queue166 (native Position/Rotation/Scale/Opacity values, key times, Linear
interpolation and clocks). The source-clock importer intentionally represents a
partial Text occurrence with a named Group and a `Source content clock` drawable;
its zero source-local anchor is not the native occurrence's anchor.
Non-Linear easing and provider/matte-specific skew proof are still untested.
The original source's Bezier animation is copied by existing native track lowering,
but these targeted assertions do not independently prove those curves in Adobe.
Pre-rebase frozen validation: queue173 full conv clippy passed; queue174 full conv test
passed (**3298 passed, 407 ignored**, ignored native-proof cases remain unrun);
queue175 check, queue176 fmt and queue177 converter build passed. Fresh build177
unchanged-original export has the identical build160 hash and substantive counts.
Original archive SHA-256 was freshly reverified unchanged. No local GPU suite was
run. These offline checks do not turn the unrun Adobe-proof cases into passes.

Fresh unchanged-original build160 export SHA-256
`8a884970de3fe5a589c78402b98ad3c92b76bd3e61b4e158d5758509d051c626`:
S02 now has six Text (`a,n,d,a,n,d`) and **zero of six Video**; 40 Text remain
missing. S03 now has44 Image and three Text (`f,o,r`); the five AGENTS letters
remain omitted in the OuterGlow units. S06/B02/S13 substantive counts below
remain unchanged. Independent Adobe open/control/render/alpha/audio proof is
still missing. The post-repair supported B02 API request
`ig-fix-conv6-143-b02-after-repair-v1` again returned `RUNTIME_UNAVAILABLE` /
`requires_operator` **without launching a host**. No fence was cleared or recovery
performed by the agent; one explicit supported recovery authorization was requested.

A review also found the analysis-only planar-Text validator retained certified
pointwise Adjustments but then rejected them through its Mosaic-only bounds path.
It now clears those effects **only in the analysis clone**, retaining every native
export effect and all-child geometry/near-plane validation.
`final_root_pointwise_adjustment_keeps_planar_text_and_checks_spatial_siblings`
passed in queue159 (both sibling orders and genuine near-plane crossing rejection).
This does not admit spatial Stroke/Shadow/OuterGlow or establish their crop reach.

Historical checkpoint product `6022c4188` (frozen build143), fresh original export SHA-256
`dbe8d576caec701238aceeecc4f901561385437f40f30cad1438729ece435cfe`:
S02 has only four Text (`a,d,a,d`) and **zero of six Video**; 42 Text remain
missing. Retained Stroke9 plus DropShadow blur/spread0, offset12/14 have no
proved native crop-input preimage. S03 has44 Image and only one of eight Text;
AGENTS groups3100/3101 retain OuterGlow with similarly unproved input reach.
Neither guard is relaxed using a UI radius or guessed glyph bounds. S06 retains
18 Text/four Video; B02 four Text/two Video; S13 17 Text/81 Video. S13's Trim and
unsupported effects still have separate omissions. This is **partial repair**.

All counts above are our own fresh native readback, not Adobe acceptance or
independent output equality. The operator-repair fence described at the historical
checkpoints is obsolete: current supported headless operations perform owned
cleanup and a fresh READY check. Actual Text-reading rejection now blocks export
acceptance, independently of worker availability. Adobe open/control inspection,
independent30fps reference/immutable Asset, RGB/alpha/audio and full-film fidelity
remain missing/unrun/unmeasured. No new CI enforcement or proof exemption is claimed.

### Point Text rejection isolation — unsuccessful envelope candidate reverted

**Current export follow-up (bounded acceptance; full proof incomplete):** eligible
Point owners now use independent native semantic/default records with explicit
Point frame/type sentinels, native owner glue, freshly authored text/style/runs,
and **no inherited document PC/F/R/L/S/G glyph/line cache**. Default Character
anchor options retain native empty More Options; absent Animators are absent.
Point font-changing holds have a generic first-seen font registry and matching
per-document indices, preserving native root default-font dependencies. Writer
profile eligibility is shared with owner-local diagnostics. Consistently Point/Box
owners use their corresponding cache-free native envelopes, including nondefault
anchor/path/animator owners with separately authored editable sibling controls.
Mixed Point/Box documents retain the older writer. Bounded richer-owner acceptance
is recorded in the current top-level Text section; general control/raster fidelity
remains unproved.

The new independent empty Point source and matched native64/48 controls are
pinned in the [fixture README](../crates/aftereffects_file/tests/fixtures/point_text_envelope/README.md).
The earlier build908 empty-line-cache64 result is historical, not cache-free
proof. Cache-free build932/revision04f93443f accepts64 but initially rejects48.
Source-backed automatic-leading regression failed before correction (queue954):
absent semantic leading was incorrectly materialized as derived manual leading.
Current Point automatic leading keeps native inactive manual slot0.01 and the
paragraph's1.2 automatic factor; explicit manual leading remains authored.
Commit7eabb5464/build958 passes fmt/seven targeted native-text regressions and
fresh cache-free48 **exactly matches all30 independent Adobe control RGB frames**,
320x180/full1s/30fps,2376 nonblack pixels/frame,strict fonts/READY:true.

Actual P025 original VT323/content/input bytes unchanged now opens/renders a
1920x1080/30fps/30-frame probe with a strict receipt reporting no missing
fonts/substitution and READY:true (not independent actual VT323 resolution proof):
AEP SHA-256 `3fe92a1618bf7b688880e7ee03d800a4f6133c728cf6c3a9fc2ad700429e4df6`,
video SHA-256 `c7db22d3e4b9eddc83b01fce4d2e4e541e01b2d74440eed79a95674dfb0a4725`.
The decoded prompt/arrow are visible. An independent VT323 reference later fails
strict admission as missing/substituted while generated P025/Point-Hold receipts
report none. Actual VT323 availability is therefore unresolved, not repaired by
metadata cleanup. The API's pre-render `usedFonts` check may defer cache-free face
detection; native Text/fontObject readback is needed to establish the cause.
No missing reference is retried, substituted or installed outside the API.

Commit3de7e4c83 omits the unknown root per-font vendor version: the independent
Arial48 source stores `Version 5.01.2x`, but the VT323 source omits that optional
slot. Source-backed regression fails before (queue1071). Fresh build1086 after
omission exactly matches all30 independent Arial48 RGB frames/READY, job
`88f4b57d83f646528898e5c88f06acec`. This establishes bounded acceptance of the
metadata correction, **not true VT323 availability**. Queue1087 passes full conv
fmt/check/test/clippy:3421 passed,0 failed,407 ignored. Legacy raw-token assertions
are replaced by exact decoded COS semantics, with no weakened feature values or
visual thresholds.

This is bounded native acceptance, not
full5s, independent P025 controls/raster fidelity, all19 owner recovery or the
separate Shape2111 repair. Native Text-control readback, long-term Asset
publication/fresh verification, alpha, font-changing hold/richer-owner native
proof and broad fidelity are still missing. Import implementation is unchanged;
new import fidelity is unmeasured. Exact historical/current hashes, requests,
native build/settings, failing-before tests and limitations are in the fixture
README; no failed artifact was replayed, font-substituted or donor-text patched.

**Historical checkpoint below: export repair was blocked; import implementation is unchanged.** Case
`point-text-envelope-v1` is an independently Adobe-authored one-letter Point Text
source, not the IG archive or a converter-created expected result. Exact source
SHA-256 `5e67c21a5c0b3ce9c7f08f33a27189ef1d5078858e4d22f4716f842afffe2d3e`,
compositionID1, `Native point n`,320×180,24fps,1s, requestedArialMT64.
[Source, concrete assertions and limitations](../crates/aftereffects_file/tests/fixtures/point_text_envelope/README.md)
register the target and typed headless authoring provenance. Native creation
succeeded; that is NOT a source render or fresh-export acceptance.

Fresh source import/export via frozen191 emitted SHA-256
`f2671a51f2c381eff4cd03fa7c28b91c9d320aa4627afd834d532accea635873`;
Adobe rejected it with `Error reading the text layer. Skipping the text layer.`
A narrowly source-justified candidate added bare COS root/version14, SimplePaint
class/type and UTF-16 character/paragraph run lengths, retaining typed content,
styles/keys/clocks and not borrowing caches or guessing font format/version.
The proposed-envelope assertion failed before (queue208) and passed after
(queue209:12 passed,2 ignored); latest-base candidate check210/build211 passed.
These are offline structural checks, not native feature proof.

ONE changed minimal export, SHA-256
`aa77358bf2473e0911bb75e2bb6cde102963d3af43beeb4b183dbff72384672f`,
was submitted through typed `Client.render_aep` at30fps, target1, audiooff;
request `ig-point-envelope-211-changed-minimal-v1`, native job
`9cf579569bcf45698c1d26dc742f23b3`. It produced the SAME Text-reading rejection,
`EXECUTION_FAILED`, verified `ready:true`, and no usable video/nonblank pass.
The unproven production candidate was reverted in `f7a2a4155`. Its generated
assertions were withdrawn with it; the separately named source-only diagnostic
`point_text_native_oracle_identity_and_controls` retains exact oracle facts,
not a weakened passing export regression. Post-revert queue212 ran that test
(1 passed,0 ignored); queue213 fmt passed. Full latest-base conv tests/clippy
were not rerun; candidate check/build do not certify native acceptance.
Rejected AEPs are excluded from later
inputs/references; no original IG retry followed this failed gate.

**Partial PR milestone / validation:** the user excluded further Point Text
serialization investigation. Retain source-proven corrections and diagnostics;
this does not establish Point acceptance or authorize dropping title/matte
semantics. Merged-code checkpoint `83b1e0ff3` passed standalone conv
`make fmt check test clippy` (queue350):3354 passed,0 failed,407 ignored.
Historical pending unit/lint notes below refer to their earlier native diagnostic
checkpoints. No new Adobe or visual-fidelity run followed this validation; failed
native gates and missing immutable references/font/alpha/audio/full-film proof
remain unchanged. S02's six Video consumers are alpha-matted by Point Text groups,
so their fidelity is coupled to the excluded issue rather than independently
restored by deleting matte providers. S03's mixed AGENTS groups and unproved
OuterGlow reach remain unresolved; the malformed PositionY static1700 fallback
remains explicitly lossy.

**Latest Point owner-group evidence (still incomplete):** native Anchor Point
Grouping storage now uses the source-proven VectorEnum recipe; complete124-byte
and stored-value comparisons for native targets107/122/137 at24/30fps failed
before the fix (queue263), then passed (queue266). The independent native Point
fixture also proves that an absent path still retains an empty Path Options
group: `native_point_empty_path_group_matches_source` failed on the omission
(queue291), then passed after retaining that exact group (queue292). Authored
paths, clocks, COS, fonts, glyph caches and other enum types were not changed.
Check272/build273 and check293/build294 passed; full tests/clippy were unrun
at those native diagnostic checkpoints (later PR validation is recorded above). Fresh generated273 and294 still received Adobe's Text-reading rejection,
with verified READY and no video. Fresh294 SHA-256
`e7536750897c0c36115d56beadd0348c71feba71dcbfe6910c18d34d6c905724`, request
`ig-point-empty-path-294-changed-minimal-v1`, job
`d52dce4b6b904f9992cf46a69f152a69` is a failed native export gate, not a pass.

A diagnostic-only complete native Text Properties group transplanted into
fresh273 did open/render30fps MP4: request
`ig-point-273-full-owner-boundary-diagnostic-v1`, job
`faf86be870b34e158f69706d4be29690`; diagnostic AEP SHA-256
`d5369846ddb257badd7f1ca579c49f7dbdac386fc0caf415a2341cbbabd0377c`, video SHA-256
`265cb9a75e592b7c11bd985242533c38b079b5acfe365a214d78a1afbb09b238`. The decoded
320×180 first RGB frame was nonblank (min0/max254,429 bright pixels). This is
original donor `n` rendering including its native text data, **not fresh edited
FX export/reflow or font/alpha/audio/full-film fidelity proof**. No diagnostic
transplant is shipping and this local MP4 is not an immutable reference Asset.
A COS+GUID-only diagnostic still failed `missing data in file.`, verified READY:
request `ig-point-273-cos-guid-boundary-diagnostic-v1`, job
`acb653993bf1402bbc67233b178185f5`, AEP SHA-256
`66eae918e5884c13d95f28f50af4fb7ec1a7de55dc6e2435847919270ab93676`.
Thus this case implicates the remaining Text owner-group context/interactions,
not a universally correct outer project or a sole GUID fix. Generated More
Options contains controls not authored by the FX input; native defaults are
empty. Implicit names and empty Animators also differ. The empty Path correction
alone did not establish acceptance. **Import source structure remains inspectable;
Point export acceptance and edited-content fidelity remain blocked.** Exact test
symbols, native identities and limitations are in the
[Point fixture evidence](../crates/aftereffects_file/tests/fixtures/point_text_envelope/README.md).

A subsequent **descriptor-only repair** corrects the local Source Text packed
storage word at bytes56–59 to native `0x00010008`, with byte60 zero, instead of
word1/subtype8. Native Point/Box/older text_ranges agree; general field semantics
remain unknown. `source_text_descriptor_matches_independent_native_point_storage`
compares all124 bytes at24fps and a30fps clock: queue214 failed before the fix;
queue215 passed13 tests,2 ignored; check216/build217 passed. No COS/font/cache or
shared-schema changes were made. Fresh217 output
`65eb785a2f50bf398d3a9df6914819021f5067dd0be5d021b28003bc806a1ab2`
still failed the same Text-reading gate, request
`ig-point-descriptor-217-changed-minimal-v1`, job
`b56901bb1a5541d4ab997cae12f96af7`, verified READY, no video.
The field was wrong, but correcting it does not establish working export.

A nonshipping native-COS substitution diagnostic
`6af3541e4257a683d12ab73833a70a35b7075eb87bdac33d8c836f56cbcb256a`
failed instead with `file is damaged.`, request
`ig-point-217-native-cos-boundary-diagnostic-v1`, job
`c5c5c458ca2c479ba4bc0317a0297af8`, verified READY. Its initial BUSY attempt
occurred before any native job; only one subsequent queued call executed.
This is **not edited-export proof**. Offline identity re-encoding, nested-length
validation and own metadata inspection pass; they do not certify Adobe validity.
Native owner GUID companions and Path/More Options group context differ from
generated Point output, but no discrepancy is uniquely causal. Exact execution
and limitations are recorded in the linked fixture README. No failed input was
replayed and no original IG retry followed these failed gates.

The root cause is unresolved: native COS defaults/layout, Source Text property
metadata/context and data lost in import have not been discriminated. No format
mismatch is uniquely established as the cause, and no cache/font/renderer/schema
change is justified. The earlier full IG B02 attempt also failed on Text, with
verified READY under current cleanup, not a required operator fence. Import
parser/editable readback is supplementary; independent native source rendering,
long-term Asset/hash verification, generated-native editable controls, actual
font resolution and RGB/alpha/audio/full-film fidelity remain missing/unmeasured.
This is partial diagnostic evidence, not a completed feature or fix-all task.

### S11 effectful Text-only branch — explicitly lossy recovery

The same private IG root 12000 still failed with unknown Text glyph bounds after
admitting its two Multiply video print passes. The nested `TALL` group 12411
contains four Text letters under one enabled custom WGSL shader. With the user's
approval to lose this branch, the existing mixed-precomposition fallback may
omit **the entire independent Text-only subtree**, including such a shader,
when every effect on each omitted Group is a recognized, unsupported
`customShader`. It cannot omit a Group with visual non-Text children, an effect
with a native editable mapping, or an effectful Group that remains on the
retained path. Existing parent/matte/mask/text-guide, animation dependency,
layout, motion blur and other compositing guards remain. The same
pruned FX view is lowered and sized; no guessed glyph bounds, flattened image,
JS substitute or hidden AEP content is emitted.

Original semantics: editable moving letters and their shader halo contribute
to the scene. Replacement: no letters or shader halo on the omitted branch;
independent video/vector material stays editable. **Impact:** the words and
shader result visibly disappear, with unbounded visual and alpha error; this
is not a full S11 restoration or CustomShader support. A source-derived CPU
assertion `independent_custom_shader_text_branch_is_diagnosed_without_discarding_paint`
checks explicit branch/subtree IDs, retained sibling content and rejection of
an editable Gaussian Blur. Original FX archive, not an independent Adobe source,
is SHA-256 `8a9656b59e50034766d29dac798a73149e18f55ca4e458cb89682a40e2f71144`.
A fresh local `--check` after both repairs no longer omits root 12000: whole-root
omissions fell from ten to nine out of twenty. It explicitly omits five Text-only
branches within S11 (IDs 12435, 12446, 12457, 12467 and 12472, the latter
containing the effectful `TALL` group 12411); letters, three file-icon labels,
and the `make it` words are missing. S11 also diagnoses independently omitted
Shape Trim owners 12418/12421, omitted masks, color/effect approximations and
remaining unsupported shaders. A fresh ignored local Write from commit `75cc2ce93` produced `project.aep`
(174,573,228 bytes) and 19 media files. Our own metadata
reader reports ten direct root layers and 896 reachable layer occurrences,
including `S11 make it TALL`, both named cyclist print-pass footage layers
and their normal-video siblings; the deleted `TALL` letters are absent. The
AEP SHA-256 is `1885c084be08d52b9f2c793afe8adab462b09cbca5694b8b308a2771319865a0`.
This is internal structure evidence only, not an Adobe open or render result.
Adobe open/control readback, independent 30fps reference, long-term Asset,
RGB/alpha/audio and full-project fidelity remain **unrun/unmeasured**.

## B02 sibling-Shape alpha-matted Video — bounded FX → AEP repair

The B02 owner Videos 9040 and 9060 sample sibling Shape alpha guides 9382 and
9386 inside their own Groups. Export formerly omitted the Video (and then root
9000) because every Video track matte was rejected, although the native layer
options already encode an editable Alpha matte record with its provider ID. The
video source, occurrence clock and footage remain editable; its source-backed
native layer names the same provider retained for sampling with the provider's
paint eye disabled. **Only Alpha Video mattes** are admitted: Luma/Inverted
variants, cross-parent/missing providers, hidden or motion-blurred video, and
unsupported media controls remain diagnosed/rejected by their existing guards.
The source semantics require sampling the provider's geometry/alpha in the
consumer's parent space. Native AE/FX rasterization, time phase, alpha convention
and mask/effect ordering have not been independently compared, so appearance
error is unbounded. No shader or image flattening is authored; CustomShader
instances remain separately unsupported and must not be silently baked.
`alpha_matted_video_keeps_its_editable_source_and_shape_provider` is a synthetic
Shape-provider **structural** regression for the native Video matte record, source
identity, provider eye switch and non-Alpha rejection. The original private B02
Shape provider is not an independently Adobe-authored oracle; target root 9000,
source SHA-256 `8a9656b59e50034766d29dac798a73149e18f55ca4e458cb89682a40e2f71144`.
Fresh original-project Check, generated AEP Adobe open/control readback, 30fps
Adobe reference/long-term Asset, RGB/alpha/audio score are **pending/unrun**.
Import unchanged.

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

### Byte-preserved RGB8/RGBA8 PNG footage export

**Direction: FX → AEP only; import and project color settings unchanged.**
Non-interlaced RGB8/RGBA8 PNG stills are fully decoded for validation, then staged
byte-for-byte with the native `png!` footage envelope. Original transfer/profile
metadata, alpha samples and dimensions are not rewritten or normalized into EXR.
The native still remains an editable file source, with straight-alpha interpretation,
source-relative geometry and the existing occurrence clock/matte/effects path.
Other PNG envelopes (including RGB with `tRNS` transparency) and JPEG retain the
historical EXR preparation and its color limitations. Existing EXR/HDR, video,
audio, Solid and Text paths are unchanged.
No working profile, gamma, scene compensation, bit depth, linearization or blending
setting is changed. Malformed streams, invalid dimensions and operational I/O remain
errors; cancellation and package rollback use the existing staging lifecycle.

This supersedes the earlier PNG-to-EXR entry only for the stated RGB8/RGBA8 profile.
A global sRGB-working-profile experiment was rejected: isolated non-PNG P040 mean
fell from0.9065942118670377 to0.854854382443014 (delta−0.051739829424023664), while
P025 was unchanged. That shared-root change is **not part of this implementation**.

Independent public native swatch control (AE26.5x89) plus a final mixed-source
managed lossless render `5dff7e1c524e406db4a5b26fabc8c28e` proved the semantically
constructed RGB8/RGBA8 PNG envelope under the existing None/gamma2.4 project:
all24 swatch triplets at frames0/15/29 matched authored codes with maximumRGB8
error0, including encoded breakpoint10/11, colored and neutral values. A deliberately
edited RGBA input changed the128 swatch to32 exactly. The unchanged independent
native PNG row was the oracle, not a converter's own reader. Cleanup/fresh READY
completed. The candidate source envelope was constructed offline from public
source semantics; this is native decoding/render/edit-response proof, **not native
property readback or formal long-term Asset publication**. Untagged opaque swatches
do not establish arbitrary ICC, alpha compositing, HDR, audio or all49 fidelity.

CPU `native_png_media_stages_bytes_and_colour_metadata_without_exr_transfer`
was RED on unchanged-main product code (queue2378, old `.exr` staging), then GREEN
on the repair. Seven focused regressions passed in queue2382, including RGB/RGBA
source distinction, native options/source identities, gAMA/alpha byte preservation,
editable Check/Write parity, malformed data and grayscale/high-bit-depth guards.
The authorized seven-case matched panel completed all14 fresh managed renders,
using unchanged original sources/dependencies, full timelines/canvases and matching
complete decoded frame counts/PTS. Baseline build2379 at tests-only commit
`1ddae0d40c2d5e7e9b169b7edd6dcfbc9bba8e3a` has unchanged-main product code;
candidate build2383 is `330850c7d52c859454c67a3490b9f9a0abd7370b`. Both use the
same base. Scoring remains full-resolution canonical RGB24, `rgb-hybrid`, inclusive
0.25s sampling and the unchanged0.99 diagnostic floor. Both sides were freshly
converted/rendered; no historical baseline movie was reused.

| Case | Before mean | After mean | Delta | Samples/side |
| --- | ---: | ---: | ---: | ---: |
| P008 | 0.4057316426 | 0.6789314199 | +0.2731997772 | 25 |
| P010 | 0.3839666914 | 0.6590025100 | +0.2750358186 | 25 |
| P031 | 0.4171093924 | 0.9328757647 | +0.5157663724 | 13 |
| P025 | 0.9762580073 | 0.9762580073 | 0 | 21 |
| P040 | 0.9065942119 | 0.9065942119 | 0 | 25 |
| P043 | 0.7872881160 | 0.7872881160 | 0 | 25 |
| P046 | 0.1709540243 | 0.1709540243 | 0 | 128 |

All three PNG means improve; all four non-PNG means are unchanged, satisfying
the requested no-loss-greater-than0.005 screen. **Every sample remains below0.99**:
this is bounded improvement, not seven fidelity passes or all49 safety proof.
Original/private movies, exact score commands, hash receipts and alignment evidence
remain local under `tasks/accuracy-W10/p008/png-panel/`. Review's JPEG-hint and
RGB-transparency fallback guards do not change these measured inputs (none of the
99 measured staged PNGs has `tRNS`); later main integration was CPU-validated, not
rerendered or falsely attributed to the earlier measured converter SHA.

CPU gate2384 passed formatting/workspace check/Clippy before stopping because its
Python lacked `pytest`. The unexecuted Rust suite was resumed in queue2386:
3,929 passed,426 existing ignored,0 failed; the Python suite separately passed43
tests/9subtests and the support-ledger checker passed. After main refresh,
queue2403 passed all9 PNG regressions; queue2409 passed scoped all-target Clippy
and formatting; immutable converter2410 built successfully on main`114f5888f`.
Earlier2404 used a nonexistent Make target,2406 ran from the wrong workspace,
and2405 exposed main's missing Premiere `numbered_frames` initializer; these are
retained failed attempts, not green gates. Main`114f5888f` fixes that initializer.
Final guard queue2413 at `3f197ea012adf805c6ea51b8bcd11253676a847b` passed
formatting, workspace all-target check/Clippy and all10 PNG regressions, including
RGB-transparency fallback. Converter2414 built the same source successfully,
SHA-256 `095aa89f90a4f6fb4fc0d01afe81958f681abdb2d40974e7ff89f8c0507d55cc`.
The historical full Rust/Python suites were not blindly repeated after rebase;
final main integration uses these targeted tests/static checks/build. No new ignore,
threshold relaxation, Adobe probe or model call was used.

### Foreign absolute Windows-drive media: native aliases and collected images

**Import/inspection only; export unchanged.** On non-Windows hosts, a complete
absolute drive spelling for Image, Video or Audio may use its validated native
relative alias counts. The exact trailing Windows components are joined to the
specified ancestor of the relocated AEP directory, not searched by filename.
An existing foreign-alias candidate must canonically remain inside that ancestor;
symlink escapes, traversal, alternate-stream components and non-fitting counts
are rejected. The foreign spelling itself is never probed as a host filename.
Host-native authored-file precedence is unchanged; native hints precede collection.
In-root symlinks retain the native alias extension for format admission while
canonical target containment remains mandatory: `.ai` pointing to a PDF-named
file still requires the editable vector profile, and `.ai` pointing to PNG bytes
is unavailable rather than silently becoming raster content. The two in-root
symlink regressions complement the existing outside-root rejection.

When the native hint is absent or its destination is missing, PNG/JPG/JPEG and
PDF-compatible AI Images may use the same exact native project-folder collection
lookup. Canonical adjacent `(Footage)` containment, ambiguity/folder checks,
readability/MIME handling and original-byte identity remain mandatory. Foreign
Images require PNG/JPEG or PDF-compatible `.ai` signatures and no Photoshop
selection; disguised PSD content cannot reach normalization. AI still undergoes
all existing vector-profile and source-selector checks, with no raw-AI/PNG fallback.
No placeholder, flattening, media substitution or shared FX/runtime change is used.
Drive-relative paths, UNC/URLs/NUL, unsafe components, foreign PSD/sequence
references and foreign audio/video without a native relative hint remain unavailable.
Resolution continues to diagnose inferred original-to-collected identity outside
pinned cases. This is not a full image decode or native-render fidelity claim.

Licensed native import cases (sources/media stay private; no tracked fragments):
- App Promo ZIP `app-promo-2026-09-24-05-32-06-utc.zip`, member
  `App Promo/App Promo.aep`, SHA-256
  `63f0af605e750132e721564be0d7251ee513e8357054448273f263a21d39e15c`;
  target3288, owner5119, media5118. Original `Envato-Symbol.png` at
  `(Footage)/Footage/Image`, SHA-256
  `e653dd99dc70dc3c67eead9a42b5c63531b9d6b78bdde7815e231a4b38ece702`.
  Test `adapter::tests::native_collected::app_native_windows_collected_media_restores_image_content`
  was removed; recorded results are historical evidence only (no longer executable).
  It used fresh import, exact Image asset reference and
  verified original PNG byte equality. Semantic RED1982: preserved carrier but no
  Image, despite unchanged collected PNG being present. Earlier1942 was owner
  lookup setup failure;1960 selected zero tests and is not semantic evidence.
- Documentary ZIP `modern-documentary-titles-2026-09-24-04-28-18-utc.zip`,
  member `After Effect Project Files/Modern Documentary Titles.aep`, source SHA-256
  `4ed325457c0eed90e9c244e1280b389d18921d3f1421c2264d2f088c0874933d`;
  target2146774151, folder13 `_Footages` /2146774070 `Images`, four owners referencing
  three original assets. Test
  `adapter::tests::foreign_collected::pinned_documentary_collected_foreign_rasters_have_native_content`
  was removed; recorded results are historical evidence only (no longer executable).
  It pinned all three ZIP image hashes and checked fresh
  Check/Write parity, exact owner Image references, asset hashes/bytes and unchanged
  inputs. Native RED1937 established missing Image under preserved raster carriers.

Both native regressions passed in jobs2018/2052 (one selected test each, zero
ignored). Final job2052 passed standalone fmt/check/clippy,36 media tests (one
unrelated licensed sequence test ignored), one existing collected Check/Write test
and20 linked-import/freshness tests. The synthetic ordinary-Image resolution
assertion was corrected; no production workaround was added. The final predicates
assert each Image's exact `source.assetId`, not a serialized substring.

Actual fixed-binary CLI `convert --check --json` and Write both succeeded for
App target3288 and Documentary target2146774151. Check published no output;
Write emitted real Images and original-byte assets matching all four pinned
image hashes. Full App target3419 also succeeded: all seven occurrences of
owner5119 reference the original packaged Envato-Symbol.png. Fixed binary SHA-256
`d1273070d7af034ff98aa36ad71614a6549cc2a5e8cb80c369bc19571cbafb2e`
records the preliminary CLI check before the foreign signature guard. The guard's
supplemental PSD regression reached semantic RED2120: the earlier implementation
normalized a foreign PSD named `.png`. Final guard validation/CLI receipt is
recorded in the PR; the existing host-native PSD path is unchanged.
These are source-based
structural/image-publication assertions, not independent Adobe open/readback,
render comparison, alpha/audio fidelity or editable export proof. No native render
or shared-runtime behavior is claimed. Public synthetic foreign-collection tests
exercise containment, unsafe spelling/type admission, ambiguity and no-search cases.

Native-relative consolidation regressions (licensed bytes remain external):
- Looper source SHA-256 `e9da6828ed077fba2412da7cf587527689971b6a7986f9388cfac43250f60551`,
  targets1081/3889/5023. `adapter::tests::foreign_relative::pinned_looper_foreign_relative_aliases_restore_images_and_videos`
  was removed; recorded results are historical evidence only (no longer executable).
  It used fresh imports, native1/2 alias counts, owner/source
  identities and twelve independently pinned original media hashes. It asserts
  real JPEG Image and MP4 Video leaves, including scratch/brush matte providers.
  Native RED2734 failed on the empty carrier; GREEN2736 restored those leaves;
  final pinned-hash refinement passed in2754.
- Gateway source SHA-256 `17ffacf954ce168c14003806130f6f1e8777e11c926c8c5701fbd08d360a3e59`,
  matte artwork sources1873/1882/1883/1884 for targets966/6855/7204.
  `adapter::media::tests::gateway_collected::pinned_gateway_native_relative_ai_reaches_native_profile`
  and `pinned_gateway_collected_ai_reaches_native_profile` were removed;
  recorded results are historical evidence only (no longer executable).
  They used four pinned AI hashes. RED2732 could not locate source1873; GREEN2739
  locates the real files in both original native-relative and exact collected
  layouts, then explicitly rejects their unsupported optional-content profile.
  Jobs2737/2738 exposed assertion path-normalization and an additional PNG/JPEG
  byte gate; they are not fidelity regressions or successful runs. Job2728 ignored
  its selected test and is not RED/GREEN evidence.

- Powder source SHA-256 `3b0d1ad9613b0047b936da3df38013f478415d43c6e4cb584c3e0fe94e3032d5`,
  roots6/1834/2724, ink owners249/2873→2857,1766/2880→2864,
  2794/2887→2871. `adapter::tests::native_relative_foreign::pinned_powder_foreign_relative_aliases_restore_ink_videos`
  was removed; recorded results are historical evidence only (no longer executable).
  It used five original media hashes, native aliases and fresh
  imports. GREEN2786 checks actual ink Video leaves plus Dust507/Particles974
  asset hashes for all three roots. The consolidated generic locator includes
  w03's Video-only PR4952 behavior; its three supplementary public Video tests
  (exact-tail, unsafe-path rejection, inspection/Check/Write) were retained and
  passed2789. Their original RED2692 is retained as supplementary fixture proof.
  Those public tests mutate alias metadata of an existing native fixture, not
  independently Windows-authored footage. w03's original-source diagnostic
  exports2724–2726 restored ink silhouettes but all123samples still failed.99:
  means root6=.1214117261,1834=.1729781258,2724=.1983564206. These were generated
  with w03's earlier frozen converter, not a current consolidated render. Residual
  brightness/blur/effects, RotoBezier and SetMatte controls remain unisolated;
  no native-reference/threshold replacement or full-project fidelity pass.

Impact cases06–08 are additional same-root witnesses, not a new media root:
original CC2024/2025/2026 sources carry native1/3 aliases for media15
`Particle Dust.mp4` and16 `Smoke_Explosion.mp4` at exact adjacent
`(Footage)/01.Others` paths. `adapter::media::tests::impact_relative::pinned_impact_foreign_relative_aliases_resolve_particle_dust_and_smoke`
was removed; recorded results are historical evidence only (no longer executable).
It used three pinned AEP hashes and two original movie
hashes to check exact file/codec admission without staging or filename search.
Scale/coverage/timing discrepancies remain separate, unisolated leads. Looper's
post-raster-staging low scores/retained but visually absent title also remain
unmeasured after this consolidated repair: restoring Video maps/dust does not
establish title visibility or a fidelity pass.

**Gateway panels are not restored by path GREEN.** The seven inspected original
AI catalogs contain one OCG listed in the default ON array, but their decoded
paint streams also use marked content, non-DeviceRGB/Gray `cs`/`scn`, and (six
sources) authored clipping `W`. Current color/clipping interpretations are
unsupported, and native AE source-selector/default visibility readback is missing.
Merely deleting the OCG guard or painting every group would not establish exact
editable conversion. The guard remains: contextual artwork omission, no guessed
selector, white-matte substitution, rasterization or shader. Static vector FX
capability is not declared absent; converter profile support and independent
native RGB/alpha proof remain incomplete. Procedural effects and typography
remain separate limitations. Import path/content assertions are structural only;
export unchanged, no Adobe open/readback/render, alpha/audio or export-fidelity
proof. References and score policy unchanged.

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

## Constant Solid sources before layer start — source-clock classification correction

Native Solid sources are constant rasters, like still media; their layer's parent
visibility span can begin before `startTime`. The importer previously applied a
sampled nonnegative source clock to Solids and clipped that visible prefix. The
responsible source classification now uses the existing parent-span path for
ordinary AV Solid sources. It does not extend source lifetimes, shift keys, add a
wrapper, or change a renderer/schema. Still-image-specific Geometry2 handling is
unchanged. Null/Adjustment roles, video/sequences/precompositions, Shape/Text
content and authored Time Remap handling are not broadened. Source color/size,
outer occurrence Transform/effect/mask clocks and native effect order remain
unchanged; a constant raster does not need an intrinsic sampled timeline.

**Import evidence:** private independently native-authored Logo sources
`f8f9c9db3a80` and `5042c5fb645f`, selected Render1098, reachable comp1371,
Solid1971 owners2416/1972 have start `0.9009009009`, source in point
`-0.9009009009`, ascending source out point `10.0433767100`, unit stretch and
enabled visuals. Fresh baseline converter1132 (main55b2d4e) imports their content
windows beginning901ms instead of native parent0ms. Public native source
`render/solid_color_1080.aep` (`efa842dfdfdd`) anchors the supplementary clock
regression `tests::media::solid_source_keeps_its_native_parent_span_before_the_layer_start`:
RED1161 fails900ms versus0ms. Positive, reverse and doubled-stretch edits assert
parent spans, constant source color/size and editable structure. GREEN1171 passes
all four Solid clock variants and the still-image neighbor. Check/clippy/fmt and
the video-clock neighbor pass1182. Fresh patch converter1183 restores the two
native parent windows to0..10944ms. All64 constant-Solid content-clock changes in
the selected import affect only `playback`; layer IDs, leaf color/size, Transform,
effects, masks, ordering and the actual `composition.dynamics` are unchanged.
Patched clocks and importer structure are not independent Adobe visual proof.

**Export:** existing export can normalize the resulting supported parent lifetime
rather than restore the original redundant static source offset. A fresh public
native import was edited to green `[0,0.75,0.25,1]` and its occurrence trimmed to
250..1250ms, then exported1183. Own-reader supplementary inspection retains an
editable native Shape/Path with the exact edited color and occurrence window;
this is not independent native acceptance/readback. A separate edit moving the
intrinsic content-clock window to250ms is diagnosed and omitted by the existing
bounded Group-clock exporter; that failed variant is preserved, not a pass.
Independent native acceptance/readback remains pending. No original source bytes
or separate hidden clock are replayed.
Native open/UI, feature-specific30fps immutable Asset, RGB/alpha comparison and
full fidelity proof remain **unrun/unmeasured**. This is a source-clock cause fix,
not a claim that all source-clock/negative-time capabilities are supported.

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

## Unsupported CC Light Sweep Cutout — import-only source fallback

Static `CC Light Sweep-0009 = 3` (native Add/Composite/Cutout popup) removes
ordinary source pixels outside the sweep. Omitting that unsupported effect while
painting the raw opaque source can conceal otherwise convertible lower siblings.
For an enabled occurrence with the layer effect switch on, an unambiguous static
reception value and no authored effect compositing options, import hides the
occurrence's editable source visuals using the existing source-hiding fallback.
Owner controls, masks, other effects and siblings remain. The sweep itself and
suffix effects applied to its pixels are lost, with a contextual diagnostic.
This is an omission approximation, **not** Light Sweep fidelity/support.

Add/Composite, disabled effects, dynamic/expression/separated reception, malformed
or duplicate reception and nondefault/ambiguous compositing are unchanged. Matte
sampling remains unchanged; cutout alpha in matte providers is not repaired.
Export is unchanged and does not restore the omitted sweep. Independent Adobe
readback, alpha fidelity and bidirectional native proof are unrun/unmeasured.

Evidence: the unchanged private Carousel Slides source SHA-256
`853e6b01377cddacdea62fec4e3e130ba1e480c78afcbaf28a6bd1e0f649dccc`, comp 1,
contains 24 Cutout occurrences; offline declarations name Add/Composite/Cutout
and the static big-endian double is `4008000000000000` (3). Existing exact-base
0s diagnostic source hiding restores portraits, without claiming sweep fidelity.
The supplementary CPU regression
`unsupported_light_sweep_cutout_is_guarded_and_preserves_effect_siblings`
checks reception guards and retained supported effect ordering. Its execution
status and fresh corrected-image comparison are reported in the bug-fix PR.

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

## Static cross-composition Slider Scale — import-only correction

An exact `name = comp(composition).layer(layer).effect(control)(parameter); [name, name]`
(with optional `var`) can replace the native cached Scale with independent editable
XY percentages. Composition, layer, effect and Slider-value identities must be
unique. The existing pure Slider-alias resolver may reach a static finite scalar;
animated, separated, non-scalar, cyclic or unresolved controls retain the original
expression and best-effort diagnostic. No script executes and no keys are sampled.

Original semantics: XY Scale follows a controller in another composition.
Replacement: the resolved static value is copied once; later controller edits and
native Z Scale semantics are not retained. Dynamic cross-composition clocks and
other expressions remain unsupported. Export reconstructs ordinary editable FX
Scale, **not** the original controller/expression linkage.

Regression: `control_links::cross_comp::tests::static_cross_comp_slider_scale_replaces_the_cached_transform`
uses a public native layer/composition scaffold with synthetic control/expression
records and asserts exact XY values, expression removal and ambiguous-name fallback.
`control_links::cross_comp_slider::tests::grammar_requires_one_complete_repeated_xy_binding`
covers complete-program rejection. The licensed Logo source exposed native Text
Scale129.8462 versus fresh imported100%, but is not shipped. Synthetic structure
is not native Adobe acceptance or independent render proof. Import RGB/alpha and
export Adobe/control fidelity remain **unmeasured/unrun**; this does not establish
that the missing Logo wordmark is fixed.

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

## Hidden still-image references — bounded export correction

**FX → AEP only.** A hidden `Image` with an otherwise supported source, static
placement/fit and ordinary compositing now uses the existing native footage
writer and `enabled: false` layer-eye record. Its source, layer identity, timing
and transform remain editable; its pixels remain absent until the user enables
that layer. This does not extend hidden Video/legacy media, audio switching,
caption settings, track mattes, placement, blend, motion blur or corner-radius
support. A hidden image's source dimensions may enlarge a precomposition's
conservative geometry bound; this affects resource use, not visibility. As with
other footage, FX/AE color/alpha equivalence and native edit/readback are not
Adobe-verified.

**Evidence (S, executed):**
`export_document::tests::pr4442_media_cases::hidden_reference_image_retains_editable_source_and_native_eye_switch`
failed before and passes after the change. It checks a fresh native disabled-eye
record, unchanged EXR source path and visible sibling, and rejects unsupported
motion blur. A fresh private IG master Check adds five planned media artifacts
(14→19) without increasing its nine retained root scenes: roots 9000 and 12000
still fail on separate track-matte and Multiply-video controls. A fresh Write
to ignored local storage produces an AEP of 211,389,590 bytes (SHA-256
`93bb99138bbb362d20487be137a9fe1b436b72c3648e26ba50031d279c59218f`),
18 reported media artifacts, nine direct root layers and 829 reachable layers by
our own native metadata reader. This is an editable-source improvement,
**not** a repaired film or visual pass. Import is
unchanged. Adobe opening/readback/render, independent 30fps reference, long-term
Asset, and alpha/audio/visual measurements are **unrun/unmeasured**.

## Independent Text-only branches in mixed precompositions — export fallback

**FX → AEP, best effort; visible text is lost.** A mixed Group that first fails
native precomposition sizing solely because FX Text glyph bounds are unknown can
retain independent non-Text children by omitting complete Text-only branches.
The exporter classifies and lowers the **same** pruned source; it does not guess
a glyph enclosure, shorten a native canvas around emitted Text, rasterize a
frame, or modify the original FX document. Contextual warnings identify each
omitted branch. Moving Shapes and their editable keys, source-backed footage,
other eligible children, and external siblings retain their existing lowering
paths; separate unsupported feature diagnostics still apply.

This fallback requires clean Normal-blend Groups along the pruned path: no
masks, mattes, effects, motion blur, fills or layout background. A branch with
an external parent/matte/mask/text-guide, segment, AI-edit or animation-graph
reference is not removed. Other groups keep their earlier whole-subtree omission
rather than risk cropping content, silently breaking references, or changing a
composite input. **Impact:** all painted glyphs in an omitted branch disappear;
visual and textual error are unbounded. Native precomposition content retained
under this rule is not evidence of Adobe render fidelity. This is not native
Text support, and the remainder of each scene can still fail independently.

**Evidence (S, executed):**
`export_document::tests::mixed_scene_keeps_independent_paints_when_text_branch_has_unknown_bounds`
failed before the fallback and passes afterward on a minimal editable FX
mixed-content scene derived from the local S06 hierarchy; it asserts retained
editable animation on the painted sibling, output hierarchy, and the explicit
Text omission. `mixed_scene_pruning_rejects_referenced_or_effectful_text_scope`
checks the reference and compositing guards. The private IG master itself is
not a pinned native Adobe source: fresh `--check` retains five formerly omitted
root scenes (1000, 7000, 13000, 14000, 15000), with 14 planned project/media
artifacts instead of two. Ten Text-only branches are diagnosed as omitted.
Eleven of the original 20 root scenes still omit, including S06 (6000), which
now hits the separate `3D descendant reaches the root camera near plane`
rejection. These counts are for this input, not a general improvement claim. A fresh local
Write created an ignored `project.aep` (211,373,392 bytes; SHA-256
`c077323efd80ec77d04b364255955417adfa3c69e0c1f496f15cf4a216d3ef9b`)
and 13 reported media artifacts. The native metadata reader sees nine root
layers and 824 reachable layers, versus four root layers before this fallback.
That is our reader's structural result, **not** Adobe opening or render proof.
Import is unchanged. Adobe open/control readback, independent 30fps MP4 and
long-term Asset proof, alpha/audio checks and visual measurements are
**unrun/unmeasured**; a Check-mode success is not a complete AEP export.

## Held Time Remap outside authored keys — bounded export correction

**Export only.** A source-backed Video or Group with `before: hold` / `after: hold`
can remain visible before its first or after its last authored remap key. Native
Time Remap endpoint extrapolation has not been established. The exporter now adds
a same-value Hold key at each uncovered visible-window endpoint only when that
side explicitly uses Hold. On a held head, the first original native key's
incoming easing is also authored as Hold (easing belongs to the arriving
segment); its authored FX easing and the original between-key segments stay
unchanged. The original editable keys, source selection, and occurrence
visibility remain unchanged. Non-Hold extrapolation
(including Continue and Loop) still omits the owner with a contextual diagnostic.
The added endpoint is rounded by the existing selected-rate property clock; an
inward-rounded final endpoint also receives the existing invisible Hold guard.
This can add up to two native keys, with at most half a native property tick of
authoring-time rounding. Exact AE endpoint sampling and visual/audio fidelity
remain unverified.

**Evidence (S, executed):**
`writer::source_clock::tests::held_tail_of_source_remap_keeps_visible_endpoint_and_original_keys`
failed before the fix and passes afterward. It uses the final-key state of local
FX Video 8357 (last authored key at 3266ms, 8168ms source; 3271ms visible end;
`after: hold`), checks serialized native key readback at 30fps, unchanged FX
input, and rejection of Continue. The full private archive and its media are not
native Adobe fixtures. `held_head_uses_shifted_window_without_admitting_other_extrapolation`
checks the symmetric Hold head under an input offset, including serialized
native incoming Hold interpolation (red before this review correction). **Import unchanged.**
A fresh Adobe open/control readback, independently rendered 30fps reference,
long-term Asset, visual and audio measurements are **unrun/unmeasured**. This
repair alone cannot convert the private IG master: fresh `--check` still omits
16/20 root layers, and root 8000 now reaches the next blocker (`Text/font glyph
bounds are not known from the FX text box`) instead of failing its held remap.
Mixed Text/mask, JS and media limitations omit the other root layers; a successful
CLI exit is not fidelity.

## Native Time Remap endpoint and selected-clock export repair

**Export only:** a bounded authored key span may now include the visible window's
endpoints. Planning retains integer-millisecond keys without a fixed 24fps exact-tick
gate. Before adding a media or Group owner, export validates the same selected-rate
native property grammar used by serialization. Tick overflow, collision or unsupported
grammar still omit that owner contextually and preserve convertible siblings.

Keys use the established nearest composition property tick (30,720 ticks/second at
integral 30fps), with at most half a tick of authoring error. Inexact keys emit an
owner diagnostic. If rounding moves an end-aligned key inward, one same-value Hold
guard outside the visible interval prevents dependence on post-key extrapolation.
The exact native in/out points, original source values and source/control-hull bounds
remain intact. Media admission and occurrence-owned animated-property restrictions
are unchanged. This is a timing approximation, not new media support or fidelity proof.

Structural regressions cover the unchanged Worlds A40010/A40310 curves (archive
SHA256 `7c775d8c6ce20d3b2571717192c5663e5b8d93940c1b937c892b495b66aafe8e`),
including A40310's 563ms source jump between timeline 1746ms/1747ms. The pinned
native `timing_time_remap_hold.aep`, composition 1, SHA256
`9af84d67acbdf35b4faf800750d8b2c27c3b0d9ed68a62685ba6ad6465b5cdda`,
supplies independent native keys for an explicitly bounded edited FX export input.
`worlds_endpoint_ramps_keep_all_keys_including_the_one_millisecond_jump` and
`native_time_remap_endpoints_export_with_the_selected_composition_clock` are red
under the previous strict endpoint gate and green with the repair. The default24
exact-key gate independently rejects the Worlds control. Endpoint-guard and
selected-rate overflow tests verify exact out points and sibling retention.
Own-reader assertions are supplementary export structure evidence. For the
pinned Worlds case, fresh Adobe open/save/reopen, editable control readback,
independent render comparison, alpha/audio checks and immutable reference
publication remain **unrun/unmeasured**. Import implementation and proof are
unchanged; this repair does not complete the general bidirectional AE feature proof.

A separate FX → AEP opening repair emits the native source-domain `tdum`/`tduM`
bounds after **keyed** Time Remap lists (zero and the source duration in seconds).
The existing Adobe-authored `timing_time_remap_linear.aep` establishes the
0-to-2-second record, and `keyed_time_remap_includes_adobe_source_domain_bounds`
failed before this writer fix and passed afterward. In a separate private,
local-only 124-layer FX export, Adobe 26.5x89 opened the generated AEP without
skipped-section warnings and its project API read back six enabled, editable
Time Remap properties with 8/8/3/3/3/8 keys. It rendered one decodable frame
at 1920×1080/24fps, but **the frame is uniformly black** (Y=16); all 29 file
sources were linked, while three enabled full-span solid overlays remained
above the video at that time. The export warned 154 times, including 140 custom
WGSL effect omissions. The private source, export and media remain ignored local
artifacts: this proves Adobe open/editable remap structure, **not** useful
rendered pixels, independent render-reference fidelity, alpha or audio. A
whole-film side-by-side is unmeasured; changing how unsupported shader owners
occlude the scene is a separate converter policy change, not part of this repair.

## Shader-only Adjustment owners — export-only omission

**Historical checkpoint:** the black-pixel diagnosis and unmeasured comparisons
below predate the subsequent Anchor and visibility repairs. See the later
sections for current implementation, measured failures and remaining proof gaps.

**FX → AEP only:** A user-approved best-effort policy omits an Adjustment owner
whose authored effect list is nonempty and entirely `CustomShader` when lowering
produces no native effects or Layer Styles. Previously the writer kept a native
solid-backed Adjustment after diagnosing every shader omission. The unsupported
shader appearance, timing and any use of its output are lost (unbounded visual
error); a contextual owner-level omission diagnostic now accompanies retained
convertible siblings. An Adjustment with supported native effects alongside a
CustomShader still keeps those native effects and its editable layer. Other
unsupported-only effect stacks and effectless Adjustments are unchanged. This
is not CustomShader implementation, media flattening or fidelity proof.

For the local, unchanged sneaker archive (SHA256
`661fa407f89eb9afc2e21661d9b83ab9b6229d3ad8e5335fe6d25357e4a167dc`),
the shader-only Adjustment IDs are 50010, 1097 and 50011. A fresh export
removed exactly those three native root owners (124 → 121), retaining the six
named Time-Remap video owners and all other native layer names. The former
Adobe API readback of six editable remaps was not repeated on this changed AEP.

A fresh **before-change** generated AEP opened and Adobe 26.5x89 rendered
full-resolution 24fps frames 0 and 138 (5.75s) uniformly black, while the
unchanged FX source rendered visible video at both times. Fresh **after-change**
Adobe frames at the same times were **also uniformly black** (decoded RGB
minimum/maximum both 0); the unsupported Adjustment omission is **not** a
black-frame fix. For diagnosis, an ignored copy containing only the unchanged
FX Video owner 60000, its two Position tracks and its hash-verified original
asset `sky.mov` (SHA256
`8bebd47e38360edf0087e9d92fd786e71fcd42f4ff9ca08cc4c3a9ca69917b74`)
rendered visible pixels in FX at 0s but black in Adobe even as the **sole**
native layer, with its file media reported supported by our preflight. The
own-reader reimport recovered a video source and its 0→800ms/229→1029ms Time
Remap, but that roundtrip is not independent Adobe editable-control or media-
decode proof. The cause of black native pixels remains **unidentified**, and
further shared media writer/clock changes were not attempted. The targeted
synthetic editable-structure regression
`unsupported_shader_only_adjustment_omits_solid_but_keeps_supported_siblings`
passed on 2026-10-02 through shared Rust queue job 86 (one test, zero failures,
1881 filtered out). The accompanying support-ledger check also passed.
Whole-film visual comparison, alpha and audio remain **unmeasured**; this is a
partial structure improvement only.

## Black-video diagnosis — native control and integrated readback

**Initial export diagnosis.** Adobe's normal media import drew
the unchanged isolated case's `sky.mov` in a separately native-authored 320×180,
30fps, one-second control. Creation/rendering and cleanup/fresh READY completed
in 49 seconds total. Output was nonblack at 0/.8s (mean decoded RGB 142.815/
149.787; source 142.154/149.170). Control AEP SHA-256
`9894571ebcd6d936d421d6101c69d57f76c4c98d6133f3b24632bb2eef84fbb8`;
control MP4 SHA-256
`3b46dd3693439806f58aad9fcf86464a1fb4edc11278de7ccfa22a00ebab1307`.
Original media and converter diagnostic AEP hashes were unchanged. The ignored
local video has no long-term Asset; this is not feature-reference publication,
editable export fidelity, human UI inspection, or an alpha/audio comparison.

The subsequent native readback attempt was refused before launch by an existing
shared worker operator-repair fence; no recovery was attempted. After #4729,
the duplicate inspector was removed. Necessary media/visibility/Transform/Time
Remap samples now use the existing worker's fixed `media_timing` profile and its
JSON/provenance/lifecycle. It deliberately retains merged strict media/font
admission and in-memory relinking: unpinned/missing media fails, and reported
paths/state are post-relink rather than proof of original alias resolution.
The public converter has no private-worker dependency. After the user confirmed
operator repair, a bounded native readback and controlled render pair succeeded:

- Copied baseline relative alias was rejected as unpinned `/Volumes/./media/...`
  with cleanup/fresh READY. This does **not** prove offline media in the original
  package's layout; no pin/admission bypass was used.
- Changing only file-alias `fullpath` to the actual pinned movie path allowed
  `media_timing` of native comp1 at0,1/24,.228s (Adobe26.5x89). Media was online,
  1280×720/30fps/13.6s; layer enabled/active, start−.8s/out.229s, scale172.5%,
  opacity100%, no Remap. Adobe Anchor was819200/259200 versus authored FX640/360.
- Two typed MP4 renders used native1920×1080/24fps, inclusive frames0–5 (.25s),
  silent video/audio off. Alias-only copy stayed RGB0 in all6 frames; changing
  only the stored Anchor from640/360 to normalized.5/.5 made all6 visible
  (RGB means150.665–153.846). Original AEP and source movie hashes were unchanged.
- Diagnostic copy AEP hashes: alias-only
  `c53be35f30bb92fe55abf50be4935cb3f27db727aa587f696e01ac6d6c48dfeb`, anchor-corrected
  `a8488696ac833f8eb963cefbfff65302054fceb7efe77352909c47bf96c98f00`.
  MP4 hashes respectively
  `3d33e91f425897d463098d4c623c7a81fc004a26d800522bd0f2db7381585b8a` and
  `1c07c5e61b6d5a4bb0650a50de73cd0f17b9ff38126a977d3eefb430db11b575`.
  Typed receipts/provenance and exact samples were retained only as private,
  undistributed local evidence; the hashes above are the public record.

### Fresh Write export repair and remaining limitations

**Historical Anchor checkpoint:** its unmeasured full-film/audio status below
was later supplemented by failed RGB scoring and a bounded PCM control in the
rounded-visibility section; neither establishes complete fidelity.

**Import implementation/proof: unchanged. Export implementation: static footage
Anchor unit bug repaired**, with supplemental keyed/tangent normalization and
Write-only final-package alias binding. File-footage Anchor XY now follows the
existing Solid source-relative convention after geometry/clock rebasing; Z and
other controls/clocks/FX models remain unchanged. Ordinary Write binds `fullpath`
to its canonical final media path before generation hashing, preserving native
relative relocation hints. **Check and public staging remain relative** and do
not embed private temporary directories. No private-worker dependency was added.

Build queue181 captured commit `4f747b2b374bfbb9c6ffbe28cda2a71d3d1e1487`.
Fresh converter packages (no diagnostic rewriting) were natively inspected and
rendered via five typed operations in170 seconds, Adobe26.5x89, with owned
cleanup/fresh READY. Original Desktop archive SHA
`661fa407f89eb9afc2e21661d9b83ab9b6229d3ad8e5335fe6d25357e4a167dc`,
source media and generated input AEP hashes were unchanged.

| Fresh output / sample | Measured evidence |
| --- | --- |
| Isolated AEP `4c69969e18597c77e8fc62e3485223e71d1fa934277bceeafe435c0beb5857a1` | Native comp1 at0,1/24,.228s: online1280×720/30fps source; Anchor640/360. Readback JSON SHA `59c5bbe05b1edf918c295fabc10ae4f1c4cc4dcde01d6f0cbaff7b965da92cd7`. |
| Isolated frames0–5, native1920×1080/24fps, .25s, audio off | All6 visible, RGB means150.665–153.846; MP4 SHA `260404e4ce777697edda46bb1521096dc8ad5dff47d24de0e84594592307c2d6`. |
| Full AEP `19146fa2b1ba6a9df1ee61b572ed7d55a063d1a236ce846c155cc60109c76b10` | Native root121 layers,29 nonmissing footage items, six editable video Remaps with8/8/3/3/3/8 keys. Readback at0/5.75s, JSON SHA `1609a5180ea1e54971a65c02a736fe986568b5249c772f2f01cf0c1c32c12437`. |
| Full frame0, native24fps single frame | Visible person/sky footage; RGB mean121.791; MP4 SHA `74bbd0ceca97d6c7c897a9b5a4dcad6a26deecb1ee50656cd1f32979d0486e65`. Green-screen panel remains visible. |
| Full frame138 (5.75s), native24fps single frame | Visible two-sneaker footage; RGB mean161.967; MP4 SHA `e6c357a4bcd8713ee5a326e1aaf6b6166152c8489f8657dea38de81dcabd3dbc`. |

Exact typed requests/job provenance and samples are locally retained under
ignored `tmp/sneaker-fresh-native-proof/`; fresh full package is
`tmp/sneaker-repaired-full-aep/`. This is **native open + editable readback +
actual sampled pixels**, not human Adobe UI inspection or independent full-film
fidelity comparison. No long-term Asset was published for these local controls.

All4 new regressions were assertion-red before repair (queue178/179), then green
in the10-test targeted panel (queue180). Symbols:
`writer::footage::tests::anchor::{static_footage_anchor_uses_native_source_relative_storage,footage_anchor_normalizes_after_source_geometry_without_moving_position,animated_footage_anchor_normalizes_values_and_spatial_tangents}`
and `adapter::export::tests::pr4442_package_cases::published_file_alias_points_to_final_package_and_retains_relocation_hints`.
The pinned native `media_video.aep` omits its default Anchor leaf; its real parse
establishes a file owner, not an explicit stored-center oracle. Keyed/tangent
assertions are supplementary structure only, with native rendering **unrun**.

Existing export omissions/approximations remain:153 diagnostics include136
CustomShader effect omissions (owners retained), the3 approved shader-only
Adjustment owner omissions,5 Hue/Saturation Master-transfer warnings
(FX1060/1059/1052/1051/1046),6 Remap tick-quantization warnings (maximum9.441µs),
and root duration12167ms rounded to293 frames. No shader replacement, new
renderer capability, clock/FPS change or threshold change was used. Original
shader appearance—including green-screen removal—is not restored by this fix.
Full-film/native-reference comparison, immutable independently Adobe-rendered
30fps/full-duration feature Asset, alpha/audio fidelity and keyed-anchor native
proof remain **unmeasured/missing**; the requested converter is not claimed fully
complete. Final queue182 validation passed standalone workspace format and
all-target clippy with warnings denied, plus3 additional Check/staging/picture-only
contract tests. No broad Rust/rendering test suite or full-film scoring ran.

### Rounded-millisecond affine Video visibility — partial export repair

**Import implementation/proof: unchanged. Export: bounded visible-window
correction, not exact source-sampling equivalence.** FX samples visibility with
nearest-millisecond `Time`, whereas native AE evaluates continuous composition
time. An affine silent Video starting at10042ms was absent at native24fps
frame241 (10.041667s), despite being visible in FX; its old10167ms out-point also
kept it visible one frame beyond the FX window. The writer now encodes the
half-millisecond parent-boundary correction as independent source-domain in/out
rationals. Nominal startTime/stretch and property-key rebasing are unchanged.
Positive non-unit stretches use the same exact rational mapping. Audio-bearing
Video, Audio, static source overrides and authored Time Remap remain unchanged.

If corrected bounds would require a negative source in-point or exceed native
rational representation, retain the nominal editable Video with a contextual
warning, not omission or a phase shift. Successful correction also warns that
native continuous footage sampling does not reproduce FX source-start clamping
or rounded source-frame selection exactly. No FPS change, added Remap keys,
source retiming, generated JS, media baking or runtime change is used.

The fresh-export `rounded_visibility_matches_fx_cut_frames_without_retiming_the_source`
regression was assertion-red at frame241 in queue247; the Audio/Remap unchanged
control already passed. Source-head diagnostic coverage was assertion-red too.
These tests are supplementary structure, not independently Adobe-authored feature
proof. All6 new tests passed in queue252. Queue253 passed standalone format and
all-target clippy with warnings denied, plus28 source-clock and28 Anchor tests
(1 and4 pre-existing ignored cases respectively, not passes), then built the
immutable converter. No full Rust/rendering suite was run.

A fresh original-archive Write produced AEP SHA256
`4dd5b6dcff9f786a5c1f2b4e1b3242e28e6548a74f385dfd394d8754d6d4509b`.
Of132 layer records,71 changed only the visible in/out rational fields; source
start/stretch, all remaining record bytes and all29 packaged media were identical
to the prior fresh export. All121 complete layer chunks (Transform, effects,
masks and Remap included) matched byte-for-byte after masking only the visible
in/out fields. The6 Remap records and independently read native keys were unchanged. Typed Adobe readback retained121 layers/29 online footage and
unchanged source FPS. Native140 now has in/out10.0415/10.1665, native141
10.1665/10.2915: both owners' activity matched FX at all7 frames239–245. Readback
worker `artifact.json` SHA256
`5c4bc4d14e7a0d08b143756077dfa9296cbd1f0dde0324beac07489b10ef0dc1`
(not the extracted local `inspection.json`).
Two typed calls completed through owned cleanup/fresh READY. See the internal
native evidence ledger for local provenance, not human UI inspection.

The7-frame native lossless render retained1920×1080/24fps; MOV SHA256
`fd4cdbbb1ca4e46bfd3469deb1ef02cb0c3e8d1e2ed77960c140011328f654af`.
Full-resolution RGB comparison against exact matching frames from the existing
nonshader FX diagnostic failed the unchanged0.99 gate: mean0.817928,
minimum0.706833, maximum0.907392; all7 samples failed. Frame241 improved from
0.040224 to0.706833, but this is not equivalent source sampling or a fidelity pass.
The prior full292-common-frame comparison also failed (mean0.782383,
minimum0.040224, maximum0.954231); no new full-film render was run.

Unchanged-AEP native PCM first1s matched the source WAV exactly at48kHz stereo
(correlation/gain/RMS ratio1.0). The observed AAC-path offsets are not a reason to
shift AEP Audio; full-film audio proof remains incomplete. Hue/Saturation transfer,
alpha, keyed-footage Anchor and whole-film fidelity remain incomplete. Missing
independent native feature source/30fps long-term Asset proof is not waived by
these local converter controls. Original CustomShader appearance is explicitly
outside this scope.

## Canonical windowed clocks — revision 67 migration

Video, Audio and Group retain their existing kinds and now carry required
windowed `playback`; Video and Audio retain independent `sourceRange` selection.
The visibility window does not replace the authored mapping or offset. This is
an FX schema migration, not new Adobe fidelity evidence.

| Direction / semantics | Implementation and limits | Current-checkout evidence |
| --- | --- | --- |
| Import: native trim/stretch and authored remap | Existing importers construct the canonical playback directly, retaining editable remap keys. No historical archive migration or additional layer kinds. | `native_precomp_trim_and_stretch_use_content_clock_not_active_range_offset` and `authored_time_remap_uses_parent_visibility_when_affine_source_is_negative` passed against the existing native sources. These are structural assertions, not new render comparisons. |
| Export: independent affine window/mapping/offset | Exact mapped endpoints are computed separately from source selection. Fractional-millisecond endpoints or mapped values outside the authored source selection are diagnosed rather than rounded or silently retimed. | Six hierarchy-clock and three media-clock unit tests passed. New Adobe open/control inspection and render comparison remain **unrun/unmeasured**. |
| Export: remap input offset | Native signed key times subtract the shifted window origin; occurrence visibility, key values and easing are retained. The authored span must cover the visible window; selected native key ticks may round with an explicit diagnostic and an invisible final Hold guard where needed. Source-domain and animated occurrence-transform restrictions still apply. Unsupported cases remain diagnosed omissions, not identity playback. | `remap_offsets_shift_native_key_times_without_changing_values_or_visibility` passed for positive and negative offsets. This is synthetic writer-plan evidence only; independent Adobe proof is **missing**. |
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
comparison images remain local and are **not redistributed**. Local licensed-source tests
were removed; recorded results are historical evidence only (no longer executable):
`pinned_intro_rectangles_keep_zero_start_and_coupled_size_keys` (removed; historical),
`pinned_intro_alpha_defaults_match_independent_adobe_readback` (removed; historical),
`pinned_intro_wall_rotations_counterrotate_halfs_01` (removed; historical),
`pinned_intro_noise_consumers_keep_both_alpha_gates` (removed; historical), and
`pinned_intro_radial_wipe_center_uses_native_pixel_coordinates` (removed; historical).
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

FX → hybrid export now keeps the supported native children of a longer neutral
Normal picture-only Group when AE cannot preserve that scope: native placement
ends at the real child extent, while the original FX window remains available
to AE and a typed Placement approximation reports the transparent tail. Own
compositing/animation controls and nested audio keep their existing boundaries.
This changes native retention only; AE export/import implementation and Adobe
proof are unchanged. See the [source-based regression and limits](hybrid-adobe-export.md#large-project-export-follow-up).

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

## Static solid vector alpha — export correction

FX → AEP vector programs now multiply a static solid Color's alpha into that
individual Fill/Stroke Opacity, including existing paint-presence keys. Native
solid vector Color does not draw its stored alpha; writing fractional alpha
there alone made translucent fills and strokes opaque. The emitted Color is
opaque, preserving RGB and avoiding double application on reimport. Layer/group
Opacity, paint order, geometry, gradients and clocks remain unchanged. This also
keeps prior overrange folding from applying alpha twice.

`writer::shapes::tests::static_solid_alpha_is_carried_by_each_native_paint_opacity`
and adjacent tests cover translucent/zero/opaque paint, independent paint keys,
unchanged keyed-color/gradient paths and invalid-alpha rejection.
`export_document::tests::paint_opacity::static_solid_alpha_keeps_fill_stroke_layer_keys_and_opaque_sibling`
checks fresh edited-FX export with separate fill/stroke alpha, layer keys and an
opaque sibling. These are CPU structural regressions, not independent Adobe
readback, RGB/alpha fidelity or edit-propagation proof. Keyed Color alpha is not
repaired: combining two varying paint curves is outside this static correction.
AE → FX import implementation is unchanged; no new import fidelity claim follows.

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

**Premiere linked-media prerequisite correction (import only):** the host's
aggregate native preflight distinguishes a linked AEP source by its retained
native owner, and permits only `MediaStatus::Missing` linked footage to use AE's
existing contextual omission. Supported editable siblings and the linked
picture survive; no media asset, slate or colour bars substitute for absence.
Direct Premiere video remains strict, as do invalid/unsupported present linked
video and operational I/O/integrity failures. AE inspection now records genuine
native-path `NotFound` separately from unsafe paths, non-files, invalid alias
metadata and unresolved collected identities (`Unassessed`). Malformed or
one-sided `ascendcount_*` values remain `Unassessed` when no local source is found;
absent pairs and integer zero counts are legitimate no-hint aliases. A valid native
alias whose ancestor is unavailable after moving the AEP remains a diagnosed
missing location, not malformed metadata; its authored tail is still validated
before that classification. A later I/O failure, including a selected source
disappearing, is not classified as benign absence.
Standalone AE best-effort omission/publication behavior is unchanged; readiness
still fails for missing footage. The already-admitted native-video fixture drives
`missing_linked_video_preserves_native_solid_through_public_check_and_write` through
public Premiere Check and Write, with no assets and the original editable solid.
`missing_linked_video_with_malformed_alias_counts_fails_public_check_and_write`,
`unavailable_linked_video_is_not_missing_when_the_path_is_invalid_or_present`,
`linked_video_io_failure_remains_fatal` and the existing
`linked_video_preflight_is_fatal_and_accepts_an_outer_project_media_map` retain the
strict negatives. Export is unchanged; new Adobe open/render and RGB/alpha/audio
proof are unrun/unmeasured.

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
| Short canonical-identity collapsed vectors, export | A short identity occurrence may retain the existing checked source/visibility plan without being mistaken for nonidentity playback. The original canonical Linear identity requirement, 3D and collapse-source eligibility guards remain; no Time Remap, offset, arbitrary retiming or geometry replacement is newly admitted. The regression `collapsed_vector_owner_keeps_requiring_canonical_identity` reproduces a 383 ms omitted occurrence and checks its unchanged unit clock plus a 250 ms visibility edit. Fresh original P047 conversion retains owner 1600014 and its vector source. | Independent Adobe 26.5x89 author/save/reopen measured collapse=true, 100% stretch, no Time Remap, native outPoint 0.3830078125 and six identity source-time/key samples. Generated acceptance is **incomplete**: two managed calls selected inline/root-viewport controls rather than the required nested collapse profile and failed; READY verified. Three-operation budget exhausted. Original P047 render/canonical RGB comparison, generated native edit response, long-term Asset, alpha/audio and general fidelity remain **unrun/unmeasured**. Import unchanged. |
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

The export demand guard also accepts a bounded canonical Linear window with zero
input offset, mapping input equal to the visible input range, zero output start,
and equal input/output durations. This maps exactly `t -> t - active.start`,
matching the existing demand clock shift. For example, the legacy archive reader
normalizes a Group visible at 20533–22533ms to that input and 0–2000ms output;
finite demand can now reach its nested 0–2000ms WORLD source. Existing strict
identity behavior remains. Nonunit rates, offsets, nonzero source starts,
independent mapping windows and TimeRemap are not newly accepted. Masks, mattes,
owner blur/effects, external references, transforms and near-plane guards remain.

Supplementary export structure regressions:
`unit_window_consumer_keeps_nested_world_clock_camera_and_internal_matte` and
`unit_window_consumer_rejects_unproved_linear_clocks`. The normalized editable
input is synthetic, not an independent Adobe-native S09 fixture. The viewport
remains the diagnosed raster-domain approximation above, not pixel equality.
Import is unchanged; independent fresh-export Adobe acceptance, native 30fps
Asset/hash proof, RGB comparison and alpha proof for this case remain
**unrun/unmeasured**. Whole private S09 retention is not established by these tests.

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
| `ordinary-intro-108-keyed-cap-alias`: composition 596/layer 644; supporting composition 3/layer 472 | An exact enabled direct Transform alias supersedes valid stored destination keys. The native source X key is536 rather than stale destination300; the imported editable key is536 at4417 ms. Sparse Rotation resolves to zero; split-axis Scale replacement clears obsolete vector keys. `control_links::cross_comp::tests::local_frame108_keyed_cap_alias_uses_source_curve_not_stale_native_keys` (removed; historical) passed, along with malformed/disabled/layout and clock guards. | Fresh isolated596 previews at107/108/109 show joined rounded caps and connector. Master backgrounds and central circle are not part of this isolated composition; this is not whole-frame equality. |
| `ordinary-intro-603-post-layer-geometry2`: composition 724/layers742,743,746; supporting source741 and controls729/730/733 | A sole native Geometry2 effect on a planar non-adjustment Shape becomes an editable Group **after** native layer and ancestor transforms. Native point normalization, Height/Width and Uniform Scale, static Rotation/Opacity, exact same-comp `toComp([0,0,0])` and same-effect Position alias are mapped. `geometry2::tests::local_external_source_restores_three_fold_geometry2_stages` (removed; historical) pins three effect stages, native pivots/reflections, four editable point tracks without JS, and an analytical Center-origin sample. Disabling the new stage made this removed pinned test fail. | Fresh isolated724 previews at master602/603/604 equivalents restore the detached reflected copies around the fold. Green/black regions, missing master color/grain context and unsupported transfer-mode22 remain; neither master geometry nor pixel/alpha fidelity is certified. |

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
`local_ordinary_intro_opening_aliases_keep_curves_clocks_and_hierarchy` (removed; historical),
`local_external_source_restores_frame_108_cross_composition_connector`,
`local_external_source_lowers_noise_from_source_stage_luma` (removed; historical),
`leading_opaque_subtract_inverts_coverage_but_later_subtract_does_not`,
`local_external_source_leading_subtract_keeps_outside_the_native_mask` (removed; historical),
`precomposition_anchor_storage_is_normalized_to_source_pixels`,
`local_external_source_restores_rotated_precomposition_anchor` (removed; historical), and
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
`local_external_source_restores_comp3_guide_parent_scale_cancellation` (removed; historical),
`local_external_source_resolves_the_comp3_property_alias_chains` (removed; historical),
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
| Oversized hierarchy / effects | General consumer-demand cropping, Chromatic Aberration and exact animated HSV saturation are not yet implemented/proved; the bounded saturation-only Vibrance approximation below is not exact HSV restoration. No global canvas clamp, larger limit or silent substitution is introduced. | Fail-closed demand propagation is implemented and CPU-tested. Only the separately listed final-root spatial output case is enabled; oversized nested hierarchies and other unsupported scenes remain omitted. A native Chromatic channel-assembly candidate failed lossless RGB/alpha checks and is not a product mapping. Native Hue/Saturation can store Hold/Cubic composite Channel Range keys, but its color transfer differs from FX's multiplicative HSV saturation; keyability is **not** an exact mapping. CustomShader remains excluded. |

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
  This is **not an Adobe pixel-error guarantee**: easeOut uses AE's cubic Hermite
  curve (unit start slope, zero end slope; replacing an earlier `(0.167,0.167,0.667,1)`
  Bezier approximation that read 0.637 instead of AE's 0.625 at the midpoint), and
  millisecond FX clock quantization/independent native samples remain unverified. Controller edit linkage is not retained.
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

Removed licensed-source CPU assertions (recorded results are historical evidence
only, no longer executable; a skipped case is not a pass):

- `local_external_source_imports_comp538_layer539_as_rigged_box_rect` (removed; historical)
  including disabled fill and editable Rect keys;
- `local_external_source_restores_displaced_circle_and_neighbors` (removed; historical)
  layers 885/882/883 and constant restored X;
- `local_external_source_restores_sh09_scale_curves` (removed; historical)
  layers 729/749 and repeated native Scale curves.

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
`set_matte::tests::local_external_source_restores_two_intersection_matte_stacks` (removed; historical)
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

## Saved Premiere capsule snapshots (bounded graphic preprocessing)

The ordinary typed AEP reader supplies UUID-bound current single-style Text and
static editable Text/Shape layers to Premiere's Source Graphic import. This is
not a new FX → AEP feature or native template/controller exporter. Parent geometry
is retained; responsive Rectangle width is a diagnosed source-average estimate,
not current glyph measurement. Missing declared Slider values use an explicit
neutral additive estimate; masks/mattes, template animation and unsupported
controls have contextual deviations. General numeric-expression evaluation and
ordinary AEP import/export are unchanged. See the
[Premiere capsule limits](../crates/premiere_file/README.md#saved-ae-capsule-graphics).
Native-source controls require external fixtures and are ignored by default;
synthetic CI tests do not establish native semantics or fidelity.
The external-source Premiere import/edit/ordinary-export test does not establish AEP export,
Adobe reopening, 30fps render, alpha, long-term Asset or visual fidelity. These
proof gaps remain incomplete native-fidelity evidence, not passing fixture/feature
targets. Bounded editable Premiere admission is usable with these explicit
approximations; it does not establish generated-project Adobe acceptance.

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

## Embedded physical font package transport — export-only

Export staging copies only embedded physical font files whose verified PostScript
names occur in final emitted editable Text document keys, including nested
compositions and Hold changes. Files are preserved byte-for-byte in `fonts/`; a
manifest records file, family, style, PostScript name and SHA-256. Collection files
are not split. Unsupported/mismatched face identities retain existing diagnostics.
A package-relative folder does not activate fonts in After Effects: an explicit
conversion diagnostic tells the recipient to install them, subject to font licenses.
There is no font substitution, alias, system lookup/installation, backend activation
or schema/renderer change. Import implementation and proof are unchanged.

Regression: `published_package_copies_only_emitted_text_fonts_with_exact_bytes_and_hash`
checks fresh package publication, metadata/hash/original bytes and exclusion of
unused faces. This is transport evidence, not native font availability, glyph
metrics, RGB/alpha/audio fidelity or an independently authored feature oracle.
Native P025/P028 resolution/render evidence is tracked separately in task artifacts.

## Archive-backed Source Text font identity and outline format

**Export-only correction; import unchanged.** Archive staging passes an ephemeral
font context through lowering to the native writer, without changing persisted FX,
renderer/editor behavior or the standalone converter boundary. Unique physical
family/style and typographic metadata, or an empty-style exact PostScript request,
select the indexed embedded face. The archive asset SHA is verified and the face's
actual name table must agree with metadata; actual `glyf` versus `CFF ` tables
select `/2 1` versus `/2 0`. All static/constant/Hold Source Text states are resolved
without changing their content/style/time. Native Point/Box and general COS
writers preserve typed format identity in font indices. Vendor `/5` is not guessed.

Macintosh platform 1 / Roman encoding 0 names now accept their exact ASCII subset
in both registry inference and indexed PostScript verification. This retains
physical faces such as Menlo whose archive name table has no Unicode records.
Non-ASCII Macintosh bytes and other encodings remain unresolved; no aliases,
format/version guesses or installed-font lookup are introduced. CPU regressions
cover physical-face inference, verified outline type, mismatched identity and
unsupported encoding/bytes (RED job2804 → GREEN job2805).

**P047 forward-export repair:** Corner Pin-first Groups preserve the declared
logical input plane, retaining extended editable child content and authored giant
point values. The existing unsupported earlier-spatial-effect guards remain.
Eight Corner Pin scalar script coordinates use zero fitting tolerance on the
existing sampled grid, preventing invisible million-unit observations from
removing visible point keys. This does not establish unsampled/subframe or
motion-blur equivalence. Independent managed point authoring, generated acceptance
and numeric input-edit readback were collected in job
`54b55b32e76642edac2000156f5d65f7`.
After the Macintosh-ASCII identity correction, the original generated P047 export
fully rendered in native job `3e10a2593f544eca9742637cf071e333`, 2336 frames at 30fps,
without logged Corner Pin error516 or AE crash. The 75s RGB mean changed from 0 to
72.1487700938786; unchanged canonical RGB24/rgb-hybrid, 0.25s, full-resolution
validation over 312 samples changed mean 0.35754330034356424 → 0.6978434147296272.
This is not ≥0.99 whole-case fidelity, independent public native-reference proof,
or alpha/audio fidelity. Font substitution remained explicitly enabled. The
font-matching crash stack and failed enabled/disabled-Text controls do not isolate
one necessary Text owner; the claim is verified physical identity plus successful
full generated-output acceptance, not single-layer crash attribution. Import is
unchanged and not revalidated by this export checkpoint.

Missing/ambiguous/malformed/index-mismatched/variable/unknown/CFF2 faces retain the
existing name candidate and omit unproven format with contextual diagnostics.
Archive I/O/hash failures remain errors. No host-font lookup, installation, name
allowlist, substitution, source replay, content pruning or private resource-crate
coupling is introduced. Actual host availability and appearance remain separate.

Independent managed AE26.5x89 controls establish the mapping (not a universal
constant or font-name inference): request `tsrct-aep49-native-outline-control-v3`,
job `e47cbb93efb84dad82755955ee2c83ce`, AEP SHA
`14dcc43919e816f8fa7c30d6dab6fb1010ec0be236a26fab2351438a26b149d0`:
non-substitute Baskerville and AvenirNext-Regular report TrueType technology12413,
native types12613/12619, actual TTC locations, `/2 1`; their byte hashes
`11f63e492099fbcfb50ad4f2ac373485af16c049fd6c975ef4ea79ae53222ef9`
and `98dec241f3ee712a37fad61aafdb83e225ed54c3e5b6e9f0abeb24eba13743ba`
match original archives and every face has glyf outlines. Baskerville Regular's
actual PostScript identity is **Baskerville**, not Baskerville-Regular.
MyriadPro was substituted and is excluded from CFF evidence. Separate request
`tsrct-aep49-native-cff-outline-control-v4`, job
`db4a24e9d7f841999f11c3f316a4dc0d`, AEP SHA
`b448935a8761d88f2ce21ca12f738b817ede6047871ea2f6e559f40825e5b19f`:
non-substitute AdobeClean-Regular, native CFF type12617/technology12412, actual
font-location bytes SHA
`5901f909b937ac108783d2373846a51fc2e0e2dd0c776083ca1ff3b19c84bef8`,
OTTO/CFF table, `/2 0`. Both managed calls completed READY. Proprietary font bytes
and native outputs remain ignored local evidence, not public test fixtures.

Structural regressions cover actual table/name classification, collection face
indices, unknown/mismatched faces and ambiguity, an existing real Arial font,
independent native-source archive staging, Point/Box `/2` serialization and keyed
same-name/different-format indexing. Synthetic directory fixtures are parser
coverage, not independent native proof. Targeted font tests1198:16passed,
1existingignored. Fresh immutable build1199 source
`86ef67f4ecf91f92052b7a95bd429087dd01b60d` opened and fully rendered the **original
P007**:8s/50rootlayers/1920x1080/30fps, generated AEP SHA
`7487f2548f8c8e0056ff43c545fa5eb8ab0173e67071b8f8df881768bc0f9042`,
native job `f02fd046f354488cb73ef67f80627ee1`, movie SHA
`42256c44d8e244e1fde5292add8f4e809bb9225dffa34fc42ab0668c270a5160`,
READY true. The previous missing-format/candidate-name full P007 crashed; the old
1s diagnostic is not this result. Diagnostic font substitution was permitted,
missing-name receipt empty; exact face/render fidelity, alpha and native
editable-control readback of the full project are not established. Remaining
P008/P010/P042/P045 full renders and SBS/publication are in progress, not passed.

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
box`). Unless a row explicitly records an Adobe execution, its evidence is
structural only; no independent Adobe fidelity follows from a native re-read.

| Direction | Mapping | Diagnosed fallback | Evidence |
| --- | --- | --- | --- |
| FX → AE, text-only Null parents | A Group whose single child is a text-only branch (Text, or a nonempty Group of such branches) becomes the existing exact Null parent, as a multichild Group already did. Each nested Group is classified on its own, so an identity clock Group holding the held segments becomes a Null and each segment a native Text layer with its own interval. | Unchanged: a Null does not carry blend mode, opacity, motion blur, masks, effects, mattes or a nonidentity clock, so such owners still need a precomposition and Text there remains omitted with the glyph-bounds diagnostic. | S: `identity_wrapper_of_held_text_segments_exports_each_segment_under_null_parents` (explicit FX input) and `slider_percent_segments_export_as_native_text_under_null_parents` (fresh import of the derived Slider percent storage above) failed before (subtree omitted) and now write two native Text layers at [0,1.5) and [1.5,2) s with their texts, parented to Nulls for the clock and the AE layer. `text_only_wrapper_keeps_the_null_parent_guards` pins the blend, opacity and motion-blur guards. |
| FX → AE, two-key occurrence clocks | A visual Group whose TimeRemap mapping has two Linear keys exactly on its visible input interval and `inputOffsetMs == 0`, the form in which import stores an offset, trimmed or stretched precomposition occurrence, exports as the existing exact affine occurrence record (reduced rational start, in, out and stretch), as audio occurrences already did. Formerly it went to native Time Remap, which needs keys outside the interval, and the subtree was omitted (`Group source clock cannot be represented exactly`). | A nonzero input offset and other keyframe clocks keep the bounded native Time Remap path or its diagnosed omission. A keyed Group-owned Transform channel (3D and skew included) under such a clock is rejected: FX evaluates those keys on the remapped content clock, which occurrence keys cannot express. The clock is only as exact as the imported milliseconds: AE's 3/2 stretch of `timing_stretch.aep` returns as 1900/1267, 0.33 ms late at the source end. | S: `native_offset_trim_and_stretch_occurrences_export_their_affine_clocks` (pinned `pr4442_native/sources/timing_precomp_source_range.aep`, `timing_trim.aep`, `timing_stretch.aep`) and `two_key_linear_group_clock_is_an_affine_record_while_its_transform_is_static` failed before; exported start/in/out lie within 0.5 ms of Adobe's records. Fresh exports of `timing_time_remap_linear`, `timing_time_remap_hold`, `timing_time_remap_bezier`, `timing_time_remap_negative_keys`, `timing_reverse_v2` and `timing_inactive` stay byte-identical. |
| FX → AE, text-only required precomposition | When Text must precompose, typically under such an occurrence clock, and the subtree is plain 2D Text in plain Groups, the precomposition sets native collapse transformations on a root-canvas source (warning: `Text has no FX glyph bounds, …`). The Text keeps its parent-space geometry, the clock stays on the occurrence record, and every nested Group is classified on its own, so held segments stay under Nulls. The Text owner's clock is validated before collapse (only a full-span identity clock, or a supported occurrence clock moved onto the record), so a canonical linear identity mapping, including a mapping domain wider than its visible window, or identity keys over the whole span keeps the same native identity record. Vector collapse alone requires a canonical linear identity mapping with zero input offset; identity-shaped TimeRemap keys and normalized offset occurrences retain its clock rejections. | Mixed Text and vector content remains omitted here except for the explicit plain-Text-leaf **omission** from a certified clocked final root in the following row. Owner effects, occurrence masks, matte consumers, and Text or nested Groups with effects, masks, mattes, non-Normal blend, motion blur or 3D keep the glyph-bounds omission (a masked occurrence reports `Collapsed Text source requires a 2D occurrence without masks or matte consumers`). No glyph bounds are guessed and nothing is rasterized. Hidden-only content under a required precomposition has no visual bounds and keeps the `Precomposition has no finite visual child render bounds` omission. Adobe rendering of collapsed Text, with or without a source clock, is **unverified**; this relies on the writer's existing collapse and clock record fields, not a native probe. | S: `clocked_text_occurrences_export_as_collapsed_precompositions_with_exact_clocks` (explicit FX input: occurrences 0.5 s late and 0.5 s early) failed before (clock subtree omitted) and now writes collapsed precompositions with exact start/in/out/stretch, a root-canvas source and both held segments; `clocked_text_collapse_rejects_mixed_masked_blended_blurred_or_3d_content` pins the omissions. `clocked_text_collapse_keeps_each_supported_occurrence_clock_exact` (offset, negative start, two-sided trim, slow and fast keys, half and double rate, varied canvases) and `nested_clocked_text_occurrences_each_keep_their_own_exact_clock` pin hand-derived start/in/out/stretch records. `hidden_text_owner_with_a_wider_identity_mapping_collapses_like_canonical_identity` and `hidden_text_owner_with_explicit_identity_keys_collapses_like_canonical_identity` retain the prior owner-collapse correction at the root and inside a clocked source under canonical windowed clocks. `collapsed_vector_owner_keeps_requiring_canonical_identity` pins both vector clock rejections, and `clocked_text_exclusions_keep_a_supported_sibling_collapsed` pins effect and mask omissions beside a kept sibling. |
| FX → AE, mixed clocked root with plain Text | An otherwise representable clocked root scene can contain both a HUD Text and vector children. The FX text box does not bound glyphs; the former exporter omitted the entire scene during precomposition sizing. For a certified final-root output viewport, a plain, unreferenced Text leaf under plain Group ancestors is omitted only when a direct root-level, static, unmasked, unmodified, axis-aligned solid-fill Rect spans the entire source clock and has checked positive paint and exact transformed bounds intersecting the actual output canvas. That Rect retains its source clock, composition and native layer structure. Other paint, nested ancestors, unproved or off-canvas geometry, non-certified roots and nested mixed scenes retain the original rejection; no glyph rectangle is guessed or text baked to media. | **Text, its styling and motion are lost** with the Text layer ID diagnosed; the visual scene is a partial restoration, not editable Text support. Text with effects, motion blur, animators, paths, masks/matte or any cross-layer/animation reference is never pruned. Unsupported vector contour/effect fidelity remains separately diagnosed. A previous root-viewport attempt kept Text in a fresh S24 AEP but Adobe 26.5x89 refused it with `Error reading the text layer` and produced no MP4; a successful own-reader test was not Adobe acceptance. | S: `clocked_root_hud_text_keeps_its_vector_scene` failed before this fallback (root omitted), then passed with the clocked vector scene and no Text in the native structure. `clocked_root_matte_text_is_not_pruned` guards references; existing non-root mixed-content tests still reject. Local private FX source `Tesseract_Converters_Launch_full.tsrct` SHA-256 `a98269b684137b80d35dbe28c701551942156e808ed51cb1082ba0bc4e145fbf`: isolated S24 (FX ID `2400000`, 60.4–64.4s) previously diagnosed whole-scene omission; fresh patched AEP diagnoses only HUD Text ID `2400081` while retaining the scene. The matching current `tsrct` CLI range-exported the **original 60.4–64.4s at 1920×1080, 30fps / 120 decoded frames** (local SHA-256 `d6e00affdb08da2802e93a131a0d4ed10a34dddedf4e4c0cb328e63fa0a68210`). A separate AE 26.5x89 `aerender` opened the fresh generated AEP and decoded **one full-resolution 1920×1080, 30fps frame at source frame 1830 / 61s** showing the radar/particle scene instead of the former black frame. Diagnostic PSNR against range frame 18 is 15.89 dB on encoded RGB; the panels differ materially, so this is not a fidelity pass. Its decoded frame matched an explicitly text-removed control byte-for-byte; it did not match the original Tesseract composition (missing Text, persistent overlays, other diagnosed contour/effect differences). This was export acceptance and a **single-frame partial-render observation on the earlier `bc166299f` build**, not evidence that the current narrower root-Rect-only head recovers S24. No fresh AEP/Adobe comparison was run after narrowing at the user's request; S24 recovery on the current head is **unverified**. The previous observation was not an independently authored Adobe oracle, independent editable-control readback, full-scene score, alpha/audio proof or published long-term Asset. Import is unchanged; the requested complete mixed-Text export/proof remains blocked. |
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

## Shared spatial Position path speed — adaptive import approximation (L01 ordinal 5)

**AEP → FX:** planar or three-component Position with native curved spatial
handles and a finite nonnegative shared temporal speed is converted to coupled
editable Linear X/Y or X/Y/Z keys. Native speed is normalized by the whole vector path's arc length, not by
signed axis displacement. Distance progress is inverted through an adaptive
arc-length lookup; time intervals are recursively refined against quarter-point
linear errors (0.25 source-unit sampled tolerance on the receiving FX integer
millisecond clock; adjacent milliseconds have no interior sample). This is not
an exhaustive all-grid or fractional-millisecond error bound. The refinement is geometric,
not FPS-based/per-frame baking, and generates no script or runtime change.
Original endpoints and Hold intervals remain; original spatial tangents and
Bezier ease controls are replaced. Unsampled continuous fidelity is unverified.
A shared 65,536 generated-vector work limit bounds both arc lookup and key
fitting across each whole property, charged before allocation; recursion depth
is bounded separately. Invalid geometry/clocks, unsupported temporal records,
nonmonotone distance handles or failed refinement omit the coupled animation with a contextual
warning, retaining the layer's static values and convertible siblings. Straight
zero-handle tracks and other property types keep the existing mapping.

**FX → AEP:** unchanged existing editable Linear Position-key export. The native
original ease/handle controls are not reconstructed; independent native export
acceptance/render proof for this approximation is **unrun**.

Source/math evidence: vendor-original Bold02 source SHA-256
`48dcb6d46c69977b63f5a62c1736017e5945a6b48567253c19c56296df9274f3`,
composition 1, Circle 01 layer92 / Circle 02 layer93. At1.75s the independent
10,000-chord math diagnostic predicts approximately `(2436.21,446.90)` /
`(1142.31,1852.36)` versus native-reference approximate circle centers
`(2440,444)` / `(1136,1852)`; prior rendered centers were `(2228,564)` /
`(1064,1824)`. These are diagnostic measurements, not a strict fidelity pass,
Adobe control readback, alpha proof or newly published formal feature oracle.
Proprietary source/reference media remain outside Git. CPU regressions in
`animation/spatial_position/tests.rs` check captured source controls, coupled
editable graphs, nonuniform collinear paths, Hold and diagnostic rejection.
A fresh changed-code import/quick comparison and actual test execution results
are reported separately in the PR; missing independent readback/export proof
is not claimed passed under the narrowed demonstrated-bug milestone.

### Three-component Position admission correction (App)

The caller previously admitted only X/Y targets to the existing 2–3-dimensional
adaptive converter. A 3D owner added a PositionZ target, bypassed that conversion,
and then rejected its native tangents, atomically losing the entire X/Y/Z track.
PositionZ now shares the same admission path; the sampler, path-distance model,
work limits, destination-budget transaction and runtime are unchanged. No camera
or perspective semantics are added, and the original controls are still replaced
by the approximation above. FX → AEP remains the existing Linear Position-key
export; native handle/ease reconstruction is unchanged and unsupported here.

Pinned local App source SHA-256
`260d5faf19fcd34e2d27841a26d918c30d59cf45659a172d589ea1c94eb2e67c`,
composition3398 / noncamera Logo layer3405, has authored Z values −153 at0s
and −413 at1s/2s, with a nonzero Z spatial handle. Licensed-source regression
`animation::position_z_tests::app_native_three_d_position_keeps_coupled_xyz_motion`
was removed; recorded results are historical evidence only (no longer executable).
It verified the source hash and native controls,
asserts all three editable scalar tracks, retained authored values/times, no
spatial metadata, explicit approximation diagnostics and atomic zero-budget
omission. It fails before the caller fix because the PositionZ tangent rejection
drops PositionX too. Native source and reference media are not redistributed.
Fresh import structural execution is reported separately in the PR. Independent
Adobe readback/open/render and FX → AEP acceptance/render comparisons, RGB,
alpha, audio, font and formal long-term Asset fixture proof remain **unrun or
unmeasured**. This is an import admission repair, not completed bidirectional
3D fidelity proof; the existing approximation and scene-projection limitations
remain in force.

## Spatial Position adaptive-key millisecond collision — import correction

**AEP → FX:** adaptive spatial Position previously generated fractional native
key times before the receiving affine clock and FX millisecond rounding. Distinct
refinement keys could collide and invalidate both coupled Position tracks. Fitting
now uses the actual receiving millisecond lattice, including rebased, stretched
and reversed clocks. Interior samples inverse-map into the same source curve;
native endpoint values and Hold transitions remain. Original endpoints that
collide on this clock, non-finite/out-of-range clocks and an inverse that cannot
round-trip remain diagnosed coupled omissions; convertible siblings survive.
Quarter-point sampled tolerance is not exhaustive all-grid/continuous proof.
Adjacent millisecond intervals have no addressable interior sample and stop;
submillisecond motion and endpoint rounding remain approximations. The existing
shared work/depth limits remain. No FPS bake, scripts or shared FX changes.

Pinned source-field regression: AI SaaS selected/resaved SHA256
`548a6b8849fcde4ea134140a212d7008722ddb832cbe13144fea29aa3ca71046`,
Scene04 composition11585, Spark layer11591, Position `ldat` at4294872.
Independent existing Adobe adjustment readback confirms source values and parent
key times1001/1201/1502ms; offline native decode captures ease/handles/source clock.
The executable `ai_spark_submillisecond_refinement_keeps_both_position_tracks`
regression reproduces omission at duplicate1159ms on the unchanged importer
(shared Rust job928: **RED**,0vs2 tracks). Shared Rust job949 passed15 scoped
spatial tests; job953 passed standalone workspace check/clippy/fmt and the CPU
suite (1638passed,405ignored). Job956 built immutable converter66d1df929;
a fresh hash-verified actual AEP import of root10350 produced two valid Spark
Position tracks, each135keys with native endpoint values at1001/1502ms.
This is fresh native-file parsing/editable-structure evidence, not Adobe execution
or render fidelity. Supplemental CPU cases exercise fractional/rebased/reversed
clocks, adjacent milliseconds, Hold, endpoint collisions and inverse cancellation.
Proprietary source/media are not published.

**FX → AEP:** existing editable Linear Position export is unchanged; restored
source handles/ease, fresh independent export acceptance and render proof are
**unrun**. RGB causality/full-film comparison, alpha/audio/font fidelity, a new
minimal independent fixture/30fps long-term Asset chain and native acceptance
remain **unmeasured/incomplete**. This is a demonstrated import-mapping repair,
not completion of the broader bidirectional conversion/fidelity requirement.

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
only shows that the latter producer's header is accepted. A third observed pair,
Co-Editor revision96/subtype6 `[0,96,0,6]` / producer `0x0f8a0656`, has five
independent Premiere ImporterPrefs GUIDs matched to native AEP item IDs/names
(2165,41,1,92,121). [Reduced fixture provenance](../crates/aftereffects_file/tests/fixtures/hybrid/format96/provenance.json)
pins both source hashes, original chunk offsets/hashes and extraction changes.
`native_format96_guids_select_editable_composition_siblings` asserts file/GUID
selection, native canvas, editable Text/Rect content and caller-reserved IDs.
Missing `C0459.MP4`, opening video53 `C20250915_0989.MP4` (native layer54), and
`title bg.psd` are contextual omissions in the reduced fixture.
`native_format96_links_keep_all_five_editable_pictures_with_missing_footage`
exercises public Premiere Check/Write, including native video53's `Missing`
classification and the host admission gate; supported siblings survive without
flattened media or invented slates. This relies on the separate generic linked
missing-video prerequisite correction, not profile admission alone; invalid
present footage remains fatal.
Neighboring headers/producers, mixed profile pairs and unproven GUIDs still fail.
This import-only admission changes no feature mapping or export behavior.
Existing text/effect/time approximations remain; offline identity/editable tests
are not Adobe control readback or RGB/alpha/audio fidelity proof. Native comparison
is unmeasured for this increment. Names/ordinals/default roots are never substitutes. Unknown profiles/layouts and absent/non-composition
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
Premiere reports every import note of a composition with its link. Its explicit
linked-audio items use a separate audio projection with visuals hidden, fresh
placement identities and the same source assets. The Premiere host clips each
picture to its composition canvas with a guide rect and Add mask, as AE renders a
composition. No render or Adobe comparison of a converted linked picture has run.

**Linked sound (import and export).** Independent audio items retain placement,
source trim and gain in editable Audio layers under source-clock Groups. Static
AE/Premiere gain products and affine gain clocks are retained. Overlapping gain
curves use original knots, Hold boundaries and at most 4096 added quarter-segment
samples; curvature between samples is a diagnosed approximation, not a measured
error bound. A non-affine gain clock retains AE automation with static Premiere
gain and reports the lost outer gain timing. Export converts sound-only Groups
into editable native Premiere audio clips, retaining their edited source ranges,
placement and volume keys without adding a black nest or another sound copy.
Grouping and the Dynamic Link relationship are diagnosed losses. Nonunit or
non-affine playback uses normal speed from the selected source start or first
source key; fractional source starts round to milliseconds, with diagnostics.
Original sources and convertible siblings remain available.

Evidence: `premiere_file::tests::linked_compositions::audio` imports native
AE26.5x89 composition `Linked audio` (GUID `00000002-0000-0000-0000-000000000000`)
from `tests/fixtures/hybrid/linked-audio.aep`, SHA-256
`c4dcae7e31ba84e0e54dd1f911229779969ef85ec52edc7c66a36cd1a530c373`.
Only media paths change during test setup; its 4-second stereo source is reused
from the audio fixture set. Premiere occurrences are supplementary typed records.
Tests check two placements, muted picture, shared assets, moved/edited sound,
Hold keys and fresh native audio export. Acceptance also uses the existing
`adobe_nest` fixture tests for the shared nest-audio path. Neither path establishes
direct Dynamic Link native reopening or audible fidelity. No runtime onset
compensation is applied.

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
| Export: PNG/PSD at this import-only milestone | This historical milestone added no image-export mapping. A later bounded JPEG/PNG → EXR preparation for AEP export is recorded [above](#deep-blue-v13-destination-media-and-typed-text-export-repair--partial); PSD reconstruction is still absent. | This row's earlier deferred export proof remains unproved. The later CPU preparation is **not** Adobe open/render or alpha fidelity proof. |
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

The receiving Add Grain exporter also accepts persisted `amount` animation as
an alias of authorable `intensity`, including Premiere Modern Noise's imported
strength tracks during genuine linked-AE fallback. Competing alias tracks in
either order reject strength animation with a diagnostic and retain the authored
static strength; seed and sibling effects remain independent. The direct
`grain_strength_amount_alias_and_conflicts_preserve_current_native_controls` and
public `premiere_hybrid_noise_modern_strength_seed_keys_reach_linked_grain`
regressions inspect fresh native control keys, current edits, order and bypass.
The latter derives Intensity10→30 and Seed3→7 keys from the unchanged Noise
records, asserts imported amount4→12, then inspects actual linked Add Grain
Intensity/Seed values at 0/0.5s, before and after independent FX edits. This is
structural evidence, not new Adobe acceptance or pixel/alpha proof.

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

### Path-qualified numeric Shape capture (version 3)

Capture and the converter-internal measurement parser now admit numeric enabled
Shape expressions as `kind: shape` with an ordered `path` of native one-based
`index`/`match_name` segments. The path includes `ADBE Root Vectors Group` and the
leaf; repeated groups are distinct. Path geometry and masks are not numeric
Shape capture targets. Paths require 2..64 segments and bounded nonempty UTF-8
match names. Shape success or atomic error records require explicit sidecar v3;
ordinary-only captures remain v2. New readers retain v1/v2 semantics and scoped
actual-clock validation for v2/v3; old readers fail rather than reinterpret v3.

This is measurement/parser support only: **Shape-to-editable-FX lowering and
FX-to-AEP export are not implemented by this extension.** Original Shape
expression semantics remain unsupported until a destination consumes the exact
measurements. One strict typed Lemon root15368 capture failed font admission on
`Montserrat-Light` and returned to verified READY (request
`w07-lemon15368-shape-native-20261004-v1`, native job
`ef45c1e32c0d42018651ab3e11e04dd3`). No Shape measurements were returned; no retry
or source/font changes were performed. Offline tests are not Adobe acceptance,
rendered motion restoration, alpha or audio proof.
No FX model/schema/renderer or expression runtime is changed; capture resource
limits, native `valueAtTime(false)`, source binding and cleanup remain strict.

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
- `structure_document::animation::expressions::tests::mixkit_evaluated_expression_import_regression` (removed; historical)
  and `mixkit_evaluated_fill_import_regression` (removed; historical): **2 passed** with the exact original
  AEP and sidecar hashes above. Both perform fresh conversion of the native properties;
  the Scale test asserts sparse editable tracks and the Fill test asserts each RGB target.
- `effects::native::tests::mixkit_native_sparse_shadow_regression` (removed; historical): **failed with
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
| Effect Parade | Mapped named plugins/controls become editable EffectRecord values and compatible scalar keys. The bounded `Fade In+Out - frames` preset becomes owner Opacity keys; Geometry2 on a still becomes a source-plane Group. | Fresh plugin records/current FX values and compatible numeric keys, not original plugin bytes. | Unknown plugins/controls default or omit, kernel differences, incompatible coupled Color cubic keys and Point cubic keys, popups, duplicates, expressions, expansion/clipping and owner/clock combinations diagnosed. | Historical 55-case panel: import S **52 pass/3 fail**, export S **55 pass**, Adobe controls **50 pass/2 fail/3 rejected**; RGB measured 55 imports/52 exports, **not quality passes**, alpha unknown. Separate Vignette animation omission; [control-level appendix](formats/after-effects-effects.md) and [results](after-effects-test-results.md#current-effects-panel). |
| CC Vignette | **Unsupported; omitted with an explicit CS Vignette (CC Vignette) diagnostic.** No substitute radial falloff, Amount/radius keys or invented feather. Owner, masks and supported siblings retained. | Unchanged historical static base Amount ×100 and radius ×60; **animated export omitted with a diagnostic**, base retained. | Import substitution withdrawn under the user no-approximation rule (Gaussian-family and generic missing-font → Inter exceptions do not cover CC Vignette). The FX and native kernels/controls differ. Historical export remains approximate, not restored native fidelity. | Native isolated/static/animated import omission regressions and locally supplied pinned Intro comp16 assert absence, warning and complete sibling-document preservation. Historical RGB-I is not current output proof; no new RGB/alpha/Adobe acceptance or export readback/RGB-E. [Control appendix](formats/after-effects-effects.md#vignette--fx-vignette--cs-vignette-cc-vignette); [import regression](../crates/aftereffects_file/src/effects/tests.rs); [unchanged animated export test](../crates/aftereffects_file/src/export_document/tests/effects_edge_coverage.rs). |
| Hue/Saturation Master subset | Signed Master controls have a source-based **static** assertion; older Colorize-on/animated expectations remain red. | Integer static Master default and rendered state repaired; animated Master/Colorize toggle omitted with base retained (except the separately documented saturation-only Vibrance approximation), fractional value rounded, out-of-range default retained. | AE reports four leaves non-keyable; other channel ranges/unselected combinations unproved. | Selected Master-only export 60-frame RGB min/mean **1.0**, Adobe UI/readback proof **unrun**; import RGB **unmeasured**. [Repair/evidence](after-effects-evidence/hue-master-export-repair.md). Older failures remain in checkpoint, not relabeled. |
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
  than assigned an invented Path speed scale. Import still rejects mixed-ease
  segments outside the pinned two-second duration. Export additionally admits
  exactly a one-second Linear-out / zero-speed Bezier-in segment with 90% incoming
  influence, encoded from cubic `(1/6, 1/6, 0.1, 1)`. Other one-second mixed
  profiles, reversed sides and other durations retain rejection; different native
  versions remain unproven. The new explicit forward fixture and independent
  native control/readback evidence are documented in the
  [Path fixture inventory](../crates/aftereffects_file/tests/fixtures/path-animation/README.md#one-second-mixed-path--bounded-forward-only-repair).
  This does not change import admission or any FX clock/evaluator behavior.
- Non-Hold keys require matching command topology; unequal vertex counts or
  open/closed morphs retain the initial import outline / omit the affected
  export content with diagnostics. FX's approximate unequal-topology morph is
  not substituted for AE semantics. Hold may change topology.
- Native export supports bounded coordinates/keys and ordered compound Shape
  Paths. Changing contour count requires Hold; absent slots use an open
  one-vertex native Path. All-empty intervals also hide the shared paint group
  through editable native opacity keys: one-vertex geometry alone can leave
  a round-cap dot in fresh exports. Animated partial disappearance has a bounded,
  diagnosed stroke-only approximation: one static, fully opaque Normal solid
  Stroke, with no dashes or geometry modifiers, is copied into independently
  hidden native contour groups. Normal Fills, when present, retain their complete
  compound geometry, winding rule, ordered paint stack and Fill keys together
  in a first native group; only the Stroke is divided. Paint editing is no longer
  shared, Fill/Stroke Path editing is separate, and overlap / anti-alias coverage
  may differ. Non-Normal Fills, transparent/gradient/multiple/keyed strokes,
  modifiers and static partial-contour outlines retain omission; exact
  shared-paint partial disappearance is not established.
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
| ShapePath JS → AEP | Sample the owner's 4× output-FPS grid, rounded to integer milliseconds and including endpoints; remove equal held samples and jointly reduce continuous coordinate runs to Linear keys within 0.01 geometry-space units. Hold keeps discontinuities and topology changes. Dense working history is reduced in 256-value windows; output remains bounded by the native u16 key field. Window boundaries can retain additional keys. This is a sampled-clock approximation, not a screen-space/submillisecond/final-pixel guarantee. | Scalar/Path adapter tests, including steps, variable contours, linear reduction, stateful-script rejection and the explicit editable disappearing-triangle input. S02's 59 Path scripts became 780 compound editable keys; fresh Adobe export rendered all 55 frames at 1920×1080/30fps, without a black interval. |
| Held compound Path → AEP | Split ordered contours; missing slots use native one-vertex paths and all-empty intervals carry separate native group-opacity keys. Partial disappearance with strokes retains omission except for the diagnosed single-opaque-static-stroke approximation in the following row; empty round-cap slots must have their whole painted group hidden. All affected transitions must be Hold, so slots do not interpolate between unrelated particles. Physical particle identities are not recoverable from unlabeled JS contour arrays. | Pinned Adobe 26.5x89 source, comp 1; four independently read native keys at 0/500/1000/1500 ms with 3/1/3/1 vertices. Its 60-frame native reference hides the shape in [0.5,1) and [1.5,2), including round-cap stroke. Fresh-export geometry records match the native one-vertex records, but initial export without visibility keys left a 56-pixel dot at 0.5s; broad blackdetect alone missed it. The regression now requires explicit editable opacity keys. Final Adobe readback verifies 100/0/100/0 opacity and four Path keys; critical absent frames have no channel above codec-black value 2. Visible fill/stroke appearance still differs (diagnostic aggregate RGB PSNR 19.5688 dB), so this is not full raster fidelity. |
| Native held disappearance → FX | Existing importer remains unchanged; fresh import retains editable Path keys and their one-vertex geometry. | `native_held_disappearance_imports_editable_path_controls` passes. Imported-FX pixel comparison is unmeasured; no new broad import-fidelity claim. |
| Partial-contour Butt Stroke → AEP | Original: held compound Shape Path keys with disappearing contours, shared Butt-cap Stroke paint (including transparency, gradients and dashes). Replacement: preserve the original shared native Path/paint scope and all existing editable width, cap, join, color/gradient, opacity and dash controls; the native one-vertex placeholders for absent contour slots have no segment for Butt caps to extend. Only Butt-only paint stacks without Trim Paths, Offset Paths or Round Corners modifiers use this exact scope-preserving mapping. Round/Projecting caps and geometry modifiers retain their prior guard/fallback; existing topology, easing and native field/clock guards remain. No per-contour Stroke duplication or paint-opacity approximation is introduced. Import and existing Trim Paths mapping are unchanged. | `path_keys_butt_partial_contours_keep_shared_gradient_and_dashed_stroke_controls` failed before with the exact partial-disappearance whole-owner omission, then passed after retaining two held native Path tracks and one unchanged shared transparent gradient/dashed Stroke record, plus fresh geometry-edit response. `path_keys_butt_partial_contours_do_not_bypass_geometry_modifier_or_other_cap_guards` preserves negative caps/modifiers. This is structural evidence, not independent native-render fidelity. The real private P047 inventory has 15 Butt-cap owners and no geometry modifier; a fresh full conversion on a private W03 combination retained all 15 without owner-omission diagnostics. Its actual managed full-project render failed: AE 26.5x89 logged crash `0 :: 42` around 32:21–32:23 and the incomplete MP4 had no readable movie index. Cleanup and fresh READY were verified, but no complete artifact was published. Full-resolution scoring and active-region improvement remain unmeasured; this is not P047 completion or native fidelity proof. Earlier tiny `sampleImage` native controls returned zero even for the known visible line and were invalid evidence, not a Butt semantics failure. Alpha, audio, independent feature-reference publication and native edit/readback proof remain unverified. |
| Partial-contour opaque Stroke → AEP | Original: a single shared Stroke paints a held compound Path whose contour count can change. Bounded replacement: only an animated Path with exactly one static, fully opaque Normal solid Stroke, only Normal Fill paints, no dashes, Stroke keys or geometry modifier is split into ordinal native contour groups, each with a copy of the Stroke and editable Hold visibility. When present, the complete compound Path and original Fill stack (including winding and Fill keys) stay together in a first identity native group, followed by the Stroke slots; Fill is not duplicated per contour. Whole-slot opacity suppresses the one-vertex placeholder; original owner Transform, timing and effect scope remain outside the slots. This is a **diagnosed approximation**, not shared-paint fidelity: native Stroke controls and Fill/Stroke Path edits become separate and overlap/anti-alias coverage can differ. Non-Hold topology changes, other paint scopes and static partial-contour outlines remain omitted contextually; no persistent particle identity is claimed. | Supplementary explicit-FX regression `path_keys_opaque_partial_stroke_retains_two_painted_slots_and_hold_visibility` failed before (Path layer omitted), then passed with two painted native slots and exact 0/500ms Hold visibility, retaining the sibling. `path_keys_partial_stroke_unsupported_controls_keep_omission` guards transparency, gradient, blend, dashes, multiple strokes, modifier and width keys; `path_keys_stroke_all_empty_remains_hidden_without_partial_approximation` preserves full disappearance. `path_keys_partial_stroke_preserves_shared_compound_fill_and_fill_keys` additionally failed before (whole Shape omitted) and passed after, requiring one shared Fill scope, unchanged native compound geometry records, EvenOdd rule, Fill color keys, fill-before-stroke group order and exact disappearing-slot Hold visibility. Targeted Path tests: 26 passed, 1 historical proof-backlog test ignored. One full private launch `--check` on the stroke-only implementation retained all 23 formerly omitted S24 Paths with contextual approximation warnings (26 minutes, planned output only); it exposed another 88 Normal-Fill/opaque-Stroke omissions. A reference-preserving source-derived slice for layer 2300025 (including original ancestors, matte provider, six targeted dynamics entries and original clocks) now retains the scene/Path with the approximation warning and no layer omissions in `--check`. The other 87 filled cases and a post-Fill-fix full-project run remain unrun. Embedded converter workspace `make check test clippy fmt` passed: 3,261 tests passed, 407 existing opt-in/backlog tests ignored; this is not Adobe proof. New independent Adobe-authored compound-case / 30fps long-term reference / control readback / RGB or alpha comparison is **missing**, and fresh Adobe acceptance of this mapping is **unverified**. Current headless-adobe Shape/stroke authoring and independent readback capabilities are absent; an attempted additional native diagnostic was fenced for operator repair, so no direct Adobe fallback was used. Import is unchanged. |

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

## Encoded baked-track accumulation — AEP export memory

AEP script preparation now retains completed animator payloads and untouched
fields as encoded JSON (`RawValue`), rather than a recursive `Value` object for
every generated Path coordinate. Only the document/composition/dynamics/entry
objects needed for replacement are opened. Reconstruction releases each previous
encoded level and uses the existing checked document slice reader. Unknown fields
outside successfully replaced animators, entry ordering and final structural
validation remain preserved; replacement animator fields follow the previous
`known_value()` behavior. Sampling clocks, fitting tolerances, fresh-runtime
validation, seeds, diagnostics and supported mappings are unchanged. Import is
unchanged; no schema or renderer changes were made.

**Bounded local evidence, not a full-project speed or fidelity claim:** matching
dev-profile converter builds at 24fps were compared once per case. A generated
four-track case (1,000 rectangles per Path, 250ms owner duration) completed in
9.81s before and 9.62s after; maximum RSS fell from 961,871,872 to 719,110,144 bytes
(25.2%). Its 31,168,030-byte AEP and diagnostics were identical; AEP SHA-256:
`42699485b10e35b4216ccdb5c77bc094783f78fd117ab35331faffd03e5d4266`.
A separate 3,967ms Path extracted from a private project also produced identical
AEP bytes and diagnostics. Single runs do not establish a CPU-throughput gain,
and dev-profile timings must not be presented as release performance.

A private 77.867s/3,049-script full-input probe stopped at its 4GiB observed-RSS
limit after 74.05s before the change (last heartbeat: 258 tracks; peak
4,319,576,064 bytes). With the change it reached 1,245 tracks by 120s and stopped
at the time limit, with peak RSS 3,112,222,720 bytes. These are different amounts
of completed work, not a full-export speedup ratio. **Neither full-input probe
completed or published an AEP.** Later preparation/writer memory, total conversion
time, release performance and Adobe RGB/alpha/audio/control proof remain
unmeasured. Inputs and generated artifacts remain ignored local files.

Supplementary CPU regressions in
`adapter::export::script_bake::tests::encoded_document` cover equivalence to the
previous document-reader path, compound Path keys, unknown-field retention,
source immutability and rejection of an invalid final graph. They are added but
**unrun** at this checkpoint; the shared queue offers only the full converter
test operation, whose execution awaits approval. The queued converter build,
format check and the two bounded output-equivalence comparisons above ran.

## Frame-rate sampled FX script baking — export performance

Normal conversion uses frame-rate sampled **FX JsScript animator** preparation
for direct AEP export and linked AEP scopes inside Premiere packages. At the
user's explicit request this replaces the former 1ms policy: there is one
behavior, no opt-in flag and no legacy-mode API. It is not Adobe scripting or
expression execution. Native Premiere script preparation and all import
behavior are unchanged.

| Direction / source semantics | Replacement and impact | Evidence / limits |
| --- | --- | --- |
| Export: owner-local scalar and ShapePath JS | Evaluate at 4× the selected native AEP FPS, rounding each offset independently to integer milliseconds and including zero and the exact owner endpoint. The fresh runtime validation uses the same grid plus extra out-of-order probes for both scalar and Path scripts. Scalar fitting uses Linear/Hold keys with the existing relative/capped scalar tolerance, except the eight Corner Pin coordinate parameters use zero fitting tolerance to preserve all sampled visible points despite giant invisible values. Path uses its existing joint Linear/Hold reducer and 0.01 geometry-space tolerance at sampled offsets. | Explicit sampling warning; events between sampled offsets, narrow pulses, topology/state changes and motion blur may differ. This is a deliberate sampling approximation, not equivalent full-millisecond validation or Adobe fidelity. |
| Export: owner-local `textContent` JS returning a string | Independently evaluate its owner grid and export each observed change as an editable Hold Source Text key; fresh out-of-order validation rejects obvious stateful scripts. Numeric/non-string results leave the script for the preexisting diagnosed layer omission. No script or rasterized substitute is authored in AEP. | Change instants round **forward to the first grid observation** (up to one sample interval, ~10.42ms at 24fps, before integer-ms rounding); intervening changes, sub-grid pulses, font/layout/render behavior and arbitrary side effects remain unverified. Source-derived `IBM RETURNED` Deep Blue typewriter script regression failed before this change and passes after it; no independent Adobe native render, UI-control or RGB/alpha proof. |
| Import | No change or new support. | No new import proof claimed. |

**Executed CPU evidence:** `fast_scalar_and_path_evaluate_only_the_grid_and_fresh_validation`
requires a 100ms/30fps case to use 28 evaluations for scalar and Path scripts,
with two linear keys retaining both owner endpoints. The former 1ms path needed
at least 202 evaluations in this case.
`fast_warning_makes_missed_subframe_pulses_explicit` deliberately demonstrates a
4ms pulse missed by the default sampled grid; the warning is required. Grid tests cover
24/30/fractional/high/low FPS, endpoint rounding and invalid rates. Path tests
retain sampled topology transitions and reject a history-dependent script.
`fast_scalar_rejects_a_call_counter_even_on_the_same_sample_grid` prevents a
stateful counter from passing merely because both sparse passes use the same grid.
`source_derived_typed_caption_script_becomes_editable_hold_text` validates text changes,
`source_text_script_returning_a_number_is_diagnosed_not_coerced` retains the prior
omission for invalid typed output, and `source_text_script_depending_on_call_history_is_not_baked`
checks the out-of-order guard. These are structural CPU checks, not independent
Adobe output; they do not cover all seven original captions' exported appearance.
`default_sampled_bake_reaches_direct_and_linked_check_and_write` checks default FPS
propagation through both export routes, Check/Write diagnostic equivalence,
publication and unchanged source bytes. These are supplementary internal tests,
not independent native feature/render proof.

**Historical comparison against the replaced 1ms implementation:** the following
measurements used the earlier opt-in build; that option and the old mode have
since been removed. The user's private launch project
was reduced to one unchanged 1,833ms ShapePath script drawing 600 cells. At the
default 24fps, one release-build run took 18.28s before the change; the new strict
run took 18.49s and produced a byte-identical AEP (SHA-256
`bd01496b7b5107a61d129842585512118248fbcd3403c489a9adfacdda36da86`).
Fast mode took 2.81s wall / 2.01s CPU versus 18.49s / 18.36s for strict. Both
reported 36 editable keys, but their AEP bytes differ: equal key counts are not
fidelity evidence. Peak footprint stayed roughly 203–208MB; this does **not**
establish a memory fix. This is a single local timing, not a broad benchmark.
The source/archive and generated AEPs remain private ignored local artifacts.
A subsequent full-film fast Premiere trial completed script preparation and wrote
a temporary linked AEP, but failed while staging the Premiere sequence: `gaps
require an explicit bottommost opaque black canvas covering each gap`. No final
Premiere package was published. This trial took 973.02s wall / 889.09s CPU;
maximum RSS was 31,192,678,400 bytes and peak physical footprint was
69,481,432,936 bytes. Large-project memory remains a blocker. The trial started
before the final extra out-of-order scalar validation probes were added, so it
is not execution proof for the final guarded build. Adobe opening, editable-
control readback and RGB/alpha/audio comparisons remain **unrun/unmeasured**.
A separate complete `SpamShowreel.tsrct` comparison (15s, 2,215 JS tracks) used
the same optimized release binary at `1154dbb54`, 30fps, one run per policy.
Both Premiere conversions succeeded: sampled 16.7868s versus 1ms 91.2516s,
**5.44× faster / 81.6% less wall time**. Maximum RSS increased from 1.834GB to
2.657GB; peak footprint increased from 1.151GB to 1.866GB. Editable keys increased
from 29,820 to 130,092. This proves neither memory improvement nor equivalent
pixels. Source hash verification confirmed the original file was unchanged.
These are local private inputs, not pinned independent Adobe-native references.

**Single-policy release confirmation:** release `675d99201`, normal conversion
without an opt-in flag, converted the same `SpamShowreel.tsrct` at 30fps in
16.73s wall / 15.54s CPU. It published a Premiere package with the same 130,092
editable keys as the earlier sampled run. Maximum RSS was 2.546GB and peak
footprint 1.879GB. The original source hash was unchanged. This confirms default
routing and execution, not native-render equality or a memory improvement.

No new native-reference pass is claimed, and no existing independent oracle or
threshold was changed.

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

### Sparse RGB Invert defaults — Looper Dust owner 5035

`AE-LOOPER-SPARSE-INVERT-5035` pins original Looper source SHA-256
`e9da6828ed077fba2412da7cf587527689971b6a7986f9388cfac43250f60551`,
composition 5023 / layer 5035 / source 2684. Its readable instance declaration
contains only marker/built-in controls; its global native descriptor declares
Channel popup 1 (RGB) and Blend With Original 0. Native Multiply is enum **5**,
not enum 3 reported in the earlier local dark-owner audit.

**Import:** the existing canonical catalog now includes these defaults from the
already-pinned Cosmic native control excerpt (fixture SHA-256
`bfa10111574878a08e46668505e5758b599077c54660be3caa148c780bfcc4e4`).
This is native binary descriptor evidence, not newly obtained Adobe UI readback.
Absent controls use these proven defaults; explicit controls and instance-local
defaults retain precedence. Duplicate/malformed controls, unreadable declarations
and incompatible local parameter kinds are rejected, not replaced with RGB.
The existing Levels mapping is retained without new FX or shader capabilities.

The ignored test
`adapter::tests::sparse_invert::pinned_looper_sparse_invert_retains_exact_rgb_levels`
was removed; recorded results are historical evidence only (no longer executable).
It checked the unchanged original's hash,
source/owner/blend identity, fresh Check/Write parity, one editable enabled Levels
with input 0/255, gamma 1, output 255/0, absence of the Invert-omission diagnostic,
and unchanged source bytes.
Supplementary synthetic codec coverage tests missing controls, explicit overrides,
malformed/duplicate records and unsafe declarations; existing native Cosmic tests
retain non-RGB and dynamic-control rejection.

**Export/proof:** native Levels export is unchanged; native Invert export is not
implemented by this correction. Fresh Adobe open/readback, native render equality,
alpha/audio and a new long-term feature-reference Asset remain **unmeasured**.
Looper media resolution is a separate change; effect structure on this independent
branch does not prove restored title pixels or fix the remaining layout/effect
mismatches. No reference, threshold, media binding or renderer was changed.

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

The bounded direct static Color Control alias resolver also applies to Toner's
five color slots (Highlights, Midtones, Shadows, Brights and Darktones). This
copies uniquely identified, opaque, nonanimated controller colors before the
existing atomic Toner guard; it does not evaluate arbitrary expressions or retain
live controller linkage. Nonstatic, ambiguous or otherwise rejected controllers
still omit the consumed Toner profile and preserve convertible siblings.

App Promo source SHA-256
`260d5faf19fcd34e2d27841a26d918c30d59cf45659a172d589ea1c94eb2e67c`,
composition 1 / Stroke layer 4827, aliases Highlights/Midtones/Shadows to
Controller 2 layer 4855's BG Gradient 1 / BG Gradient 2 / BG controls. Before this
consumer hook, enabled aliases caused the entire Toner to be omitted despite
static native controllers. The licensed-source regression
`app_native_toner_static_color_aliases_keep_editable_ramp` was removed;
recorded results are historical evidence only (no longer executable).
It verified the source hash and asserted the three exact RGB
curve knots plus surviving Ramp sibling. This is editable import structure only;
new independent Adobe acceptance, feature-specific RGB comparison, alpha and
formal fixture/Asset proof are unrun/unmeasured. FX → AEP Toner reconstruction is
unchanged and unsupported; generic exported Tint/ColorCurves are not restoration
of native Toner controls. The sparse Ramp point-unit ambiguity on this same owner
is unresolved and is not corrected or certified by this repair.

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
malformed/duplicate values, and animation or unresolved enabled expressions on consumed
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


## Explicit Box Blur radius versus declaration default — import correction

Native Logo14 source SHA-256
`f8f9c9db3a80ce1b5c3ab686db7efae505f318299ae1b4f403af8715c994695b`,
root1098, composition1371/layer2237, declares radius default7 and explicitly
stores static instance radius7. The importer incorrectly required a zero
radius declaration default even when an authored instance value superseded it,
omitting a supported Both-dimensions blur. Import now accepts the radius default
variation only with an explicit native descriptor `tdgp` radius row (not a
parameter synthesized by the decoder from declarations); numeric/layout, static ancillary
controls and keyed-radius admission guards remain. Missing explicit radius still
requires the proven zero-default sparse profile; no arbitrary declaration default
is silently replaced with zero.

The previously committed byte-preserving licensed owner fragment and its README
are removed from the current source tree and public export inventory. This deletion
does not erase already-published Git history. Both licensed-source native Logo14 tests were removed;
recorded results are historical evidence only (no longer executable).
They verified the above SHA-256 and composition1371/layer2237.
No licensed source bytes were embedded in these tests.
`native_logo_radius_declaration_does_not_override_authored_radius` (removed; historical) failed in
shared Rust job1256 with the declaration-profile rejection (one executed test).
The companion `native_logo_noncanonical_radius_default_requires_explicit_control` (removed; historical)
checks an edited explicit value and rejection after removing the native instance
row before fresh decoding, while retaining declaration7 and its synthesized
parameter. The original decoded-parameter-removal assertion did not prove native
row absence. Delivery regression job1270 failed semantically because that absent
row was accepted; corrected job1272 passed all six `box_blur` tests (zero ignored).
Job1269 was a test-helper lookup failure, not semantic RED. Original author
job1257 passed six tests and formatting but lacked this source-row counterexample.
These establish editable import semantics only. Gaussian variance,
native kernel/edge-alpha and linear-key interpolation approximations remain as
below; fresh RGB/alpha comparison is unmeasured. **FX → AEP:** unchanged Gaussian
export, not restoration of native Box Blur identity or controls; independent
Adobe acceptance/readback/render proof is unrun. Native READY=false was respected.

## Legacy Gaussian Blur scalar profile — import-only approximation

Case `legacy-gaussian-blurriness-text0412`: licensed Text Animation source from
`text-animation-2026-09-24-04-12-10-utc.zip`, member
`Text Animation/After Effects/Text Animation.aep`, SHA-256
`55a45f6113e305384531cf3eab8bbcad05d730f1d5a638852d14c5fd74ace811`,
composition946 (`comp 3`, 3840×2160, native60fps), layer1144 (`Text 7`).
No licensed source, fragment, font or media from this case is distributed by this
change.

**Import:** the observed `ADBE Gaussian Blur` scalar-only descriptor maps its
explicit `-0001` Blurriness and authored numeric keys to existing editable
`GaussianBlur.blurriness`. The source has two native values0→30. Missing/extra
controls, malformed/nonfinite/negative scalar values or keys, and enabled
expressions are diagnosed omissions; modern Blur Dimensions/Repeat Edge Pixels
are never inferred from missing legacy controls. The existing FX initializer's
`repeatEdgePixels=false` is retained as an approximation, not evidence of a
native legacy edge default; no source canvas bounds are synthesized. Convertible
siblings survive.
**Approximation:** legacy kernel, radius calibration, sampling and edge alpha
are unverified. This restores editable control content, not proven pixel parity;
every imported occurrence carries that limitation. No FX schema/runtime changes,
JsScript or sampled-key baking are introduced.

**Export:** unchanged existing modern `ADBE Gaussian Blur 2` mapping can export
editable Gaussian content. This change does not restore the legacy plugin's
identity or establish feature-specific independent native export proof.

`native_legacy_gaussian_blur_retains_authored_blurriness_keys` was removed;
recorded licensed-source results are historical evidence only (no longer
executable). The test hash-checked the full
source before fresh parsing and selects the exact native composition/layer,
asserting editable base value, key values/times and stable effect-param target.
Queue1849 ran one test and failed on missing editable GaussianBlur before the
mapping. Earlier queue1832 failed test setup and is not semantic RED. Post-fix
queue1951 passed all three targeted tests (including the explicitly enabled
licensed-source case, zero ignored), standalone workspace all-target check,
clippy with warnings denied, and formatting. Intermediate jobs1897/1919/1932
exposed the key-only guard and incorrect test expectations for existing FX edge
initialization and native owner start/stretch; they are not passing evidence.
Supplementary scalar-profile guards and export-mapping identity assertions are
not independent native feature proof.

Native Adobe open/readback, independent30fps MP4/long-term Asset, RGB/alpha scoring
and both directions' SKILL proof are **unrun/unmeasured or missing**. This is a
partial import approximation, not completion of bidirectional conversion or
fidelity proof. Source FPS and bytes remain unchanged in ignored local storage.

## Explicit Box Blur iterations versus declaration default — import correction

Licensed Match Cut source SHA-256
`e44b06dbbb5aea050d7083a631eccacbb2b26e3b4e0219896c5a96e28f44ba34`,
composition18/layer1201, has complete native declarations with Radius7 and
Iterations1, and explicit static instance rows carrying the same values.
The importer accepted the explicit Radius but rejected Iterations because its
local declaration differed from the canonical sparse default3. Import now permits
that Iterations default variation **only when its explicit native instance row
exists**. Declaration length/kind, duplicate controls, Both dimensions, repeat
switch, finite/nonnegative integral values and expression/key guards are unchanged.
Removing the explicit row still rejects the noncanonical declaration; a decoded
parameter synthesized from that declaration is not explicit storage.

`effects::box_blur::iterations_tests::native_match_cut_explicit_iterations_override_local_default_profile`
was removed; recorded licensed-source results are historical evidence only
(no longer executable). The source cannot be redistributed.
Shared Rust job1801 executed it and failed before this repair at the exact
`Box Blur declarations conflict with the native default profile` rejection.
It pins source hash, owner, Radius7/Iterations1, editable Gaussian blurriness
`4 * sqrt(56 / 3)`, repeat-edge/source bounds and rejection after native-row removal.
The public native Box Blur fixtures provide supplementary scalar/adversarial tests.
Final frozen-source shared Rust job1813 passed all8 scoped Box Blur tests, including
this licensed source through the actual Converter path, plus standalone-workspace
formatting and Clippy. Job1807 ran8 passing tests but failed its freeze check after
formatting; job1809 ran only6 old tests due shared-target mtime skew and is not
new-regression proof. Job1813 refreshed changed-source mtimes inside the serialized
job before Make and confirmed both new symbols executed.
The original ZIP/AEP bytes and media are not published by this correction.

**Import proof:** native-source parsing/editable structure only; fresh native
render/30fps Asset comparison, exact kernel/edge alpha, audio and font proof are
unmeasured. The existing Gaussian variance approximation is unchanged; this is
not a new Box Blur raster-fidelity pass. **Export:** existing Gaussian writer
unchanged/unrun, not restoration of Box Blur identity or Iterations controls;
independent edited-native acceptance/readback/render proof remains unrun.
No Adobe operation, reference replacement, renderer/schema change, guessed
coefficient or threshold weakening is part of this repair.

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

Effect Drop Shadow Opacity keys with a static, expression-free Shadow Color
now map to editable Color-alpha keys: native 0..255 opacity becomes alpha
0..1, while RGB remains authored. The original scalar easing and occurrence
clock are retained; final RGBA key payloads participate in ordinary animation
budget admission. Animated RGB, expressions, separated/spatial controls and
out-of-range alpha keys retain the diagnosed initial-value approximation.
A CPU regression (`shadow_opacity_keys_keep_static_rgb_and_scalar_easing`)
failed before this mapping and checks fractional alpha, nonzero-speed Bezier
controls, fixed nonblack RGB, tight-budget rollback and rejected expressions/
coupled color motion. Its writer-framed controls are supplementary evidence,
not an independently authored native feature fixture.
Existing independent private AI SaaS readback identifies Opacity fades
0→184.5 and 0→127.5 on two static-black shadows; fresh main import retained
alpha 0 and no track. This is an import control repair, not a native shadow
kernel, RGB/alpha fidelity, new 30fps Asset reference or independently verified
export claim. Export opacity animation and bidirectional visual proof remain
unverified for these targets; no renderer or persisted FX schema changed.


## Bounded Fractal Noise — import approximations

A uniform Basic Normal `ADBE Fractal Noise` stage on a proven opaque
solid plane with no native masks maps to the existing editable `TurbulentNoise`
effect. The converter validates the Fractal-specific ABI, including Evolution
0023, Opacity 0029 and Blend 0030, independently of Turbulent Noise. Native popup
defaults come from the declaration default slot, not its current cached value.
The pinned declaration has a cached Blend value 5 and a Normal default 2.
Noise Type0002 now retains all four validated interpolation ordinals (1 Block,
2 Linear, 3 SoftLinear, 4 Spline), rather than omitting non-Spline generators.
`native_fractal_noise_interpolation_modes_remain_editable` mutates only that control
on the pinned source. Export uses existing editable TurbulentNoise → native
`ADBE AIF Perlin Noise 3D` controls, not Fractal source replay. Existing Turbulent
static/animated controls and Add Grain five-control mappings remain unchanged;
Fractal-specific seeds/cycles, anisotropy, unmapped animation and other blend modes remain
outside this bounded route. VR Noise is not equated by name. Derived interpolation
variants remain structural-only; their independent Adobe comparison is unmeasured.
Normal import additionally preserves authored Linear/Hold Contrast0004,
Brightness0005, Uniform Scale0010, Offset0013 and Evolution0023 keys through the
existing checked numeric/point animation and layer start/stretch clock machinery.
Disabled saved expression text no longer rejects usable values; live expressions,
unrepresented popup/control animation, malformed/nonfinite data, invalid domains
and unsupported curves still reject the effect with owner/siblings retained.
Scale must remain positive and contrast nonnegative at every authored endpoint;
Linear/Hold intervals keep those bounds without speculative curve clamping.
Staged Multiply/Screen carries the same validated keys on the generated effect ID.
Animation reservations and tracks commit atomically with the existing identity-clock
blend wrappers; rejected wrappers publish no dangling tracks or animation charges.
The Offset0013 declaration admits the two source-backed paired center defaults,
50/50 and normalized0.5/0.5 in16:16 storage. Mixed pairs, other defaults, wrong
kinds/layouts and other Point slots keep their existing strict guards; runtime
Point units still use the native descriptor and shared Point normalization.

Independent `native-fractal-turbulent-keys.aep` (SHA-256
`1087f5662b0dc4129f39f58e0c137b11c1533065d2d85acffa65fde761f1c743`)
pins Fractal comp1/owner15 and Turbulent comp16/owner29, both320×180/2s/30fps.
The source-derived import regression checks six exact targets/times and shifted
clocks, disabled/live expressions and invalid scales. Direct and public export
regressions inspect newly generated canonical Turbulent controls after independent
current FX edits, including offset keys, effect order and bypass. Export uses
Turbulent Evolution0020, not Fractal0023 or hidden source replay. Independent native
source references and bounded RGB diagnostics are available below; canonical
full-resolution scoring and HDR/alpha fidelity remain unestablished.

Independent `native-fractal-multiply-keys.aep` (SHA-256
`7fdf99f33e9c0b635c2e2355019333b85445d41021e2675450688ef2b62b646b`)
pins comp1/owner15,320×180/2s/30fps: Adobe-authored Blend0030=5 (Multiply)
over opaque RGB[0.25,0.5,0.75], with the same five native keyed controls.
`native_fractal_keyed_multiply_screen_publish_tracks_on_generated_effect` uses
these unchanged bytes for Multiply; Screen remains explicitly supplementary,
derived from the Normal source. The public procedural test additionally checks
the native ordinal/nonneutral source, retained Multiply graph and six generated
effect targets, current edits and fresh native Multiply composition/controls,
bypass and order. This source closes the derived-only Multiply fixture gap.
The21 focused Fractal tests and public procedural test passed, including this
native Multiply case; formatting, ledger and workspace Clippy checks passed.

All three independent native source references are long-term Assets, freshly
resolved/downloaded and byte-count/SHA-256 verified. Managed aerender AE26.5x89
produced H.264/yuv420p MP4s at320×180,2s,60frames, inheriting native30fps;
no explicit bitrate preset is claimed.

| Source / composition | Asset ID | Bytes | Reference SHA-256 |
| --- | --- | ---: | --- |
| Original Fractal /1 | `dd5g5GY6y883XBuVZWs0_vid` | 20960 | `29bdadbbeac456aa3edf7ff4aa7cbfc46d3df85de81de9541cbb0c2c9ef06e35` |
| Original Turbulent /16 | `65GrQ7jm0eupb7w09A7C_vid` | 18267 | `93bf2fc611b85e6a6d553b61a4f402c5546726b5ddbd785c3f8ceab75fb7bc0d` |
| Multiply /1 | `JPUkwMa6oorxMrFC1Dod_vid` | 20698 | `5b6078def8a0ad469c09e626581a406809d065ed02719ec0e201dfc68556f3c4` |

Independent Adobe open/save/cold-reopen of current Normal export retained0/1s
Contrast100→170, Brightness0→20, Scale100→180, Offset[160,90]→[192,74],
Evolution0→90, active canonical Turbulent followed by disabled Gaussian.
Current Multiply export independently retained Contrast100→155, Brightness0→15,
Scale100→160, Offset[160,90]→[185.6,77.2], Evolution0→75 at0/1s and exactly one
native Multiply wrapper at independently edited opacity75. Saved AEP SHA-256
`7a29022ab4b66754a3cdcc9459bef3c6fbfbbc6b90b110020ee49f160a2c89a7`;
readback was equal and the managed application returned READY. This establishes
editable current-control export, not Fractal source replay or pixel equality.

Native and freshly imported FX textures were visually inspected. Multiply RGB
diagnostics sampled0,0.5,29/30,1,31/30,1.5s using **area-downscaled720p FX versus
native320×180**. The fixed public renderer CLI offers720p/1080p/4k, not the native
canvas, so these are explicitly downscaled diagnostics, not canonical
full-resolution AE scoring. Kernel, brightness, spatial detail and output range
differ. Eight-bit opaque references establish neither HDR nor alpha fidelity;
Screen remains supplementary derived coverage, not an independent native case.

The current shader uses `1000/max(scale,3)` for feature frequency, converts evolution
with `radians(evolution)`, and clamps output to0..1, unlike native AllowHDR. Numeric
control retention is not scale/kernel/evolution/pixel/alpha equivalence or VR support.

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
admitted. Other blending modes, unrepresented animated controls,
nondefault cycle/seed/sub-transform settings and unsupported sources remain
omissions. Expression-driven coordinates are ignored only in the constant
zero-contrast branch, never evaluated or fitted.

Static Basic Multiply/Screen controls can instead form an independent
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

## Simple Choker renderer-cap diagnostic — import only

Native Simple Choker permits authored Choke Matte values outside the existing
FX renderer's `±10` radius. The cap remains **10**; the proposed runtime-cap
extension was abandoned. Import retains the authored editable `choke` value,
keys, easing, occurrence IDs and convertible siblings without rewriting them.
For retained bases or emitted numeric keys above10 (or below−10), it now reports
`unsupported: choke > 10, clamped to renderer cap ±10` (or `choke < -10`) with
native effect ordinal and composition/layer context. The **renderer**, not the
converter, applies that pre-existing clamp; large-radius native erosion/dilation
can therefore differ from FX output. Exact boundary values±10 have no cap warning.
Emitted fitted-expression keys are inspected by the same diagnostic; unused
source values replaced by expression samples are not misreported as rendered.
This check does not establish continuous Bezier extrema between keys.

Implementation/evidence: AEP→FX diagnostic only; FX→AEP mappings and proof are
unchanged. `structure_document::effects::tests::simple_choker_renderer_cap_diagnostic_preserves_output_bytes`
exercises0/±10/±100 static controls and later±100 keys with an editable effect
sibling. Its pre-fix RED execution showed all four out-of-cap cases lacked a
warning; the preserved effects/animations/ordinals/ID bytes are pinned by SHA256
`f6b97d1a7712e876d7bc0055299ae8b8967c7307b8ebbd44c754a7782f1359d0`.
These are supplementary writer-generated controls on an unchanged Adobe catalog
owner, not new independent native feature/render proof. Previously inspected
native Bold02(comp134/layer242) and Bold05(comp199/layer231) controls each author
two100→0 Chokers and exceed this existing renderer boundary; their converter
values/keys remain retained. No renderer, FX schema, shader, evaluator or export
change is made. Rendered output is intentionally unchanged; fresh Adobe/render
comparison, independent alpha proof and general bidirectional fidelity remain
unmeasured, not passing.

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

### Launch Radial Blur export — Zoom selection (structural correction)

FX `radialBlur` uses a 16-sample ray/Zoom kernel with center and amount:
`uv + (uv - center) * clamp(amount * 0.00625, 0, 0.25) * i / 16`.
Fresh FX → AEP export now explicitly selects native `ADBE Radial Blur-0003`
Type **2 (Zoom)** instead of the writer donor's default **1 (Spin)**. Amount,
center, authored keys, enabled state and neighboring effects remain editable.
Native Zoom's kernel, amount response, quality and random-seed behavior are
still approximations; this mode correction does not establish render equality.
The export diagnostic states these limitations. Import's existing default-Spin
limitation, native reader and canonical writer donor are unchanged.

`radial_zoom_export_preserves_static_and_keyed_controls_and_siblings` starts
from explicitly authored FX, freshly exports AEP, and checks actual native Type,
static/animated amount and center, disabled occurrence and Gaussian sibling.
Its RED execution (shared Rust queue **283**, test commit `9731fbd67`) failed
specifically on native Type `[1.0]` versus Zoom `[2.0]`. This is synthetic-input,
own-reader structural evidence only, not independent Adobe export proof.
GREEN queue **287** passed both static and animated panels. Earlier post-fix
queues 284/286 exposed test-only duplicated key IDs and an invalid expectation
of a separate base value for keyed native properties; those assertions/fixtures
were corrected, not counted as behavioral RED evidence. Full CPU check/tests,
Clippy and fmt passed in queue **288**: **3,292 passed / 407 existing ignored**.

**Import:** unchanged; existing independent Spin sources are not Zoom oracles.
**Export:** scoped Zoom selection implemented; Adobe acceptance/control readback,
independently authored native Zoom fixture/composition, 30fps Adobe reference,
immutable long-term Asset with fresh hash verification, critical-frame RGB
comparison and alpha fidelity remain **missing/unmeasured**. No Spin reference
is reused as Zoom proof and no fidelity threshold is lowered.

### Static Solid Radial Blur source-origin export — bounded point correction

Fresh FX → AEP export now translates the Radial Blur center when an edited
Rect with a nonzero local origin actually selects a native Solid. `solid()`
already subtracts that origin from Anchor; the native effect point must undergo
the same translation, rather than remain in the old Rect-local coordinate plane.
This does not alter native Zoom/FX sampling kernels, amount response or bounds.

The exact point correction admits only an isolated static unparented/uninherited
ordinary 2D root Solid, sole enabled static Radial Blur, positive finite uniform
scale, opaque Transform, Normal blend and no masks/matte/styles. Animated,
nonuniform, gated, mixed, nested and vector profiles are not generalized; an
ineligible nonzero-origin Solid keeps its previous controls with a contextual
source-plane approximation diagnostic. Nonfinite subtraction declines atomically.
Gaussian Blur is unchanged: its FX renderer already scales blurriness by world
scale, so Directional Blur's screen-space compensation would be incorrect.

[The public control fixture](../crates/aftereffects_file/tests/fixtures/effects/radial-solid-origin.md)
pins explicit editable FX, independently AE-authored base/edit native source,
managed script and native-inspected fresh output hashes. CPU RED queue1638
exposed `[20,15]` instead of source-relative `[13,20]`; initial GREEN1667 passed
both targeted tests. One managed AE26.5x89 readback independently opened the
fresh outputs and matched native centers `[13,20]` and `[20,3]`, real Solid,
Anchor/Transform, Amount and Zoom Type against independently authored controls.
Actual FX edits to origin, Anchor and center were freshly exported, not replayed.

**Export:** bounded point mapping and native editable/input-edit acceptance
established. Independent native30fps MP4, long-term Asset/fresh download,
critical-frame RGB/alpha, native Zoom kernel/Amount equivalence and original49
proof remain missing/unmeasured. **Import:** implementation/proof unchanged.
No finite-bounds guard, importer, FX schema/evaluator or renderer was changed.

### QuickTime intrinsic-duration integer bounds — export-only correction

QuickTime interpretation carries both floor and ceil milliseconds derived from
one checked native media-duration numerator and its timescale. Video and legacy
Video accept a positive authored intrinsic duration only within that exact
interval (at most one millisecond wide); malformed reversed/wide bounds and
foreign durations remain rejected. A 43-frame, 30fps source admits the authored
1433ms floor or 1434ms ceil, while native footage retains the actual 1434ms ceil.
This does not retime the authored sourceRange or Time Remap key times/values.
Non-QuickTime bounds equal their existing duration; Audio admission is unchanged.
No import, FX schema, evaluator or renderer behavior changes.

The targeted `exact_video_duration_quantization_preserves_authored_clock` case
was behaviorally RED before this correction (queue job 245), then GREEN in
queue 254. Accepted floor/ceil labels, out-of-interval values, malformed bounds
and separate zero-duration deserialization rejection are exercised. Full CPU
check/tests passed in queue 256: 3,289 passed / 407 existing ignored; Clippy's
two idiom findings were corrected and Clippy passed in queue 257.
The matte-closed private Stripe source case `case-400572-closed-v2.tsrct`, SHA-256
`1b09fbe62108fc811e020386659e1602376e841f6bff6a5cbb7384af2343121c`, retains
provider 400021 and unchanged ancestors, effects and clocks. Build 242 rejected
its 1433/1434 intrinsic mismatch; fixed build 255 Check now retains the scene
without owner/subtree or media omission. The earlier provider-missing v1 slice
is not retention proof. Adobe open/control inspection and independent
RGB/alpha/audio comparison remain **unrun/unmeasured**; this correction is not
native visual-fidelity proof.

## Legacy Basic Text generator — bounded editable replacement

**AEP → FX:** the first active `ADBE Basic Text2` effect on an ordinary Shape
source can become an editable Text sibling above its unchanged source content.
Admission rejects nonempty AE per-effect built-in compositing options (opacity,
mask references or unknown controls) with `nonempty effect compositing options`;
only absent/empty options are admitted, independently of the plugin's Composite
On Original control. Rejected generators retain the existing diagnosed best-effort
unsupported-effect fallback rather than inserting unconditional editable Text.
The full-import supplementary regression covers empty options and explicit
opacity/mask/unknown controls; independent Adobe fidelity remains unmeasured.
Admission requires a unique instance, no authored masks, an observed 1804-byte
packed single-line centered profile, explicit nonempty text/font/style strings,
static finite controls, fill-only Display Options, zero Tracking, and enabled
Composite On Original. Unsupported layouts, alignments, animation/expressions,
paint modes, other source kinds, non-first stages and duplicate controls remain
contextual omissions; convertible source content and effect siblings survive.
No script, media flattening or FX runtime/schema change is introduced.

Generated Text is assembled before unsupported-source fallback hiding. An enabled
downstream Roto Brush therefore hides generated and original ordinary source paint
together; disabled cutout and matte-sampling bypasses retain their existing semantics.
The supplementary full-import regression reproduced the previous visible-Text bypass
in RED job769, then passed with enabled/disabled, lower-sibling and matte guards in
GREEN job783 (seven scoped Basic Text tests). These injected native-record tests are
pipeline evidence, not independently Adobe-authored Roto Brush fidelity proof.

After normal main integration of PR #4788, the same completed-source ordering
also precedes CC Light Sweep Cutout fallback hiding. The supplementary
`basic_text_light_sweep_cutout_hides_all_ordinary_paint_only_when_enabled` checks
all retained original and generated source children, disabled-effect preservation
and the unaffected lower sibling.
`basic_text_light_sweep_cutout_retains_matte_provider_for_diagnosed_fallback`
checks retained generated matte-provider paint and the existing diagnosed fallback.
With generator insertion moved after the hide pass, both unchanged assertions
failed in RED queue **867** (visible generated Text); restored ordering passed all
10 scoped Basic Text tests in GREEN queue **869**, including the conservative
built-in compositing guard. These injected descriptors are supplementary pipeline
evidence, not independent native Light Sweep/Basic Text fidelity proof. No Adobe
execution, new render comparison, alpha proof or export restoration is claimed.

The original semantics generate text over the source before subsequent effects.
The replacement retains this first-stage order and native owner Transform/source
clock. Source-declared PF_PointDef percentage defaults and PF_PopupDef default
choices are read from their typed descriptor slots; the importer does not invent
missing text/font/control defaults. Packed profile interpretation is bounded to
the unchanged source-observed center profile, not a general plugin ABI claim.
The editable Text uses the source's PostScript font name and normalized fill
color. Native vertical glyph centering is approximated by an em-box baseline;
bundled font substitution, exact glyph metrics, rasterization and alpha differ
or remain unmeasured, with owner-local diagnostics. Multiline content is omitted.

**FX → AEP:** no restoration of the legacy Basic Text plugin/control dialog is
implemented. The replacement remains ordinary editable Text and uses the existing
bounded Source Text export path and its existing limitations. An offline whole-case
export preflight with fixed converter622 ran and diagnosed omission of generated
point-Text branches: unknown glyph bounds prevent their inclusion in mixed native
precompositions. Useful replacement export for this case is therefore **incomplete**,
not established by the existence of a Source Text writer. Edited export, Adobe
open/readback and independent native render proof remain **unrun**, not inferred
from import or synthetic round trips.

### Preserved Basic Text PostScript identity — bounded export repair

Basic Text's producer now places the authoritative packed PostScript face in
`fontFamily` with explicit empty `fontStyle`. Family and display-style slots are
still validated, but display labels are not face-identity suffixes. The literal
font-pair writer exception was removed; ordinary Source Text family/style import
and the existing diagnosed writer candidate fallback remain unchanged. This is a
general producer-namespace correction, not font-specific lookup or substitution.
The private Bold06 source SHA256
`f0b0bfdd722e6280fefa26062db4fee308f78c99369e2320cb77573513abeea4`,
generator comp156/layer203, has one pinned packed font payload with familyArial,
styleBold and PostScriptArial-BoldMT; it is expanded through two root occurrences.
Changing the whole PostScript face while retaining empty style exports the edited
face exactly, in static and keyed Text states. Setting a nonempty style on a whole
face is not supported native style selection: unmatched pairs retain the existing
candidate diagnostic. Diagnostic Inter alias, bundled fonts, FX model, editor,
evaluator and renderer are unchanged; corrected import asset identity is intentional.

**Evidence/status:** `basic_text_bold_postscript_identity_exports_without_a_second_style_suffix`
pins the native font fixture and specifies the exact legacy-derived editable pair.
Its pre-fix RED queue1073 fails on the missing correct native face. A fresh isolated
Text derived from Bold06's imported Basic Text using immutable converter995 writes
one `Arial-BoldMT-Bold` COS name and zero `Arial-BoldMT` names. Both attempts are
preserved. This proves a writer identity mismatch, not native rendering or host
font availability. The original literal-pair GREEN is historical, superseded by this producer repair.
`whole_postscript_style_edits_retain_unverified_candidate_diagnostics` protects
unmatched-pair diagnostics. GREEN1080 ran six PostScript CPU tests. Final1082
passed standalone check/clippy/fmt and nine font tests (one ignored). Immutable
converter1083 at18028de21 freshly exports the same source-derived isolated input
with one correct `Arial-BoldMT` COS name and no `Arial-BoldMT-Bold` name. This is
native writer-record proof only, not Adobe acceptance or glyph fidelity.

**Current correction evidence:** queue1236 ran one generator regression and failed
semantically on `Bold` versus empty style. Queue1238 ran the SHA-pinned licensed
`basic_text_native_postscript_identity_uses_explicit_empty_style` (removed; historical) and failed on the
same mismatch after fresh offline import. GREEN queue1242 ran all 13 Basic Text
CPU tests, including that licensed native assertion: 13 passed, zero ignored.
Neighboring packed profiles and edited static/keyed whole-face writer cases are
supplementary, not Adobe-native oracles.

**Direction limits:** import corrects the Basic Text face-identity namespace;
export consumes that exact identity through its existing whole-face convention.
Independent minimal fixture/Asset/alpha/font fidelity remains incomplete. Legacy plugin UI, full-case
mixed-precomposition inclusion, Adobe open/control acceptance, independent native
render and font/alpha/audio fidelity remain incomplete or unrun. Do not infer
those from raw COS records or this limited Source Text writer correction.

Source-based diagnostic case L01/ordinal7: unchanged proprietary Bold04 AEP
SHA-256 `d2451f4eadd93f2aa348fd53d95a614a90fb428839d3477ae3c23747d5472a59`,
root composition1, generator composition186/layer224. Its packed payload contains
`Replace Me !`, `Arial`, `Bold`, and `Arial-BoldMT`; explicit Size254, Fill Color
RGB220/alpha255 and Composite On Original1 are preserved. At1.25s the independently
rendered diagnostic reference shows the placeholder omitted by the previous
conversion. Proprietary source/media stay outside Git. Sanitized layout/control
regressions supplement fresh private-source assertions; they are not independent
Adobe-authored fixture proof. Formal fixture/Asset publication is not a readiness
gate for the explicitly narrowed demonstrated-bug-fix milestone; native RGB/alpha
fidelity and both-direction feature proof remain incomplete.

## Essential effect-control source paths — structural evidence only

**Import:** effect-instance paths now enter the native `sspc` descriptor. The
next parameter's identity and index are verified against its unique `parT`
declaration, rather than interpreting that index in sparse authored `tdgp`
values. A missing declared numeric leaf can receive the override's supplied,
validated storage; no default value or control declaration is invented.
Duplicate descriptors, declarations or value groups, stale indexes, ambiguous
value names and malformed missing-leaf replacements remain diagnosed and leave
the source unchanged. Non-effect groups do not gain descriptor traversal.

The pinned MIT native `essential/multiple_controllers.aep` (SHA-256
`df08145c4be5d3547b1bb5650b48be777f8663d0d88dc472f65990f0ac37c6d3`) and its
upstream AE JSON projection exercise occurrence layer 28 and source layer 15.
`native_effect_overrides_descend_into_descriptor_controls` was RED with a
non-group Brightness diagnostic, then GREEN; it asserts admitted Brightness
`0` and Fill `[1,0,0,1]` overrides without changing the shared source. These
native overrides equal the defaults: this is **path-admission evidence**, not
a discriminating visual-change case. The supplementary
`effect_descriptor_hop_changes_only_the_selected_control_and_rejects_ambiguity`
asserts changed supplied storage, sparse indexing, sibling retention and unsafe
layout rejection. Initial targeted Essential CPU validation: 21 passed / 6 ignored.

A follow-up closes malformed declaration admission for **missing authored leaves**:
identity alone is insufficient. The selected declaration must contain exactly one
148-byte `pard` of a supported numeric kind. Supplied replacement dimensions and
scalar/point/color storage must match that kind; scalar integer/continuous encodings
follow the existing native reader's compatibility and slider-rounding behavior.
Missing, duplicate, opaque or wrong-length `pard`, nonnumeric kinds and mismatched
replacement types fail closed without inserting storage or altering siblings.
Existing authored-leaf replacement behavior is unchanged.
`missing_effect_leaf_requires_numeric_declaration_and_compatible_override` was
RED on the missing-`pard` reproducer (queue 760), then GREEN with valid typed
scalar/integer/point/color positives and malformed/nonnumeric/type-mismatch negatives
(queue 771). The earlier synthetic positive now uses valid typed declarations,
not empty declaration storage. Follow-up Essential CPU validation: 22 passed /
6 ignored, including the pinned native regression above. This is malformed-input
and editable-structure evidence, not new native render or export fidelity proof.

**Export:** unchanged; no new editable Essential Property exporter is claimed.
Independent Adobe open/inspection, fresh native render, 30fps Asset publication,
RGB/alpha comparison and feature-level export proof remain **unrun/unmeasured**.
The motivating Point Control source-path failure is distinct from unsupported
expressions consuming that control; their existing approximation/omission
limitations remain. This repair does not establish restoration of those visuals.

### Native Mask Feather two-component numeric layout — import correction

Native Mask Feather can use the same two-double, integer-tagged layout as plugin
Point controls. That flag describes the native layout, not quantized scalar
semantics. The importer now admits this profile only for Feather, retaining
continuous editable Vector2 values through the existing point decoder. Ordinary
numeric Feather layouts and all other mask-property admission remain unchanged.
Variable feather, native edge kernels and alpha equality retain their existing
limitations; no renderer or FX-schema changes are made.

The unchanged independently Adobe-authored `masks/import_mask_controls.aep`
(SHA-256 `01380c1f8c5ebe486cd068e5dee50447870b86e4864fc842590a99386dd9b417`),
composition **130 / MASK_FEATHER**, supplies the editable **[24,12]** assertion in
`native_mask_feather_integer_tag_retains_continuous_vector`. Before the repair,
queue **487** failed specifically on imported **[0,0]** versus native **[24,12]**.
The licensed Intro source independently exposes the same layout with **[300,300]**;
its proprietary source/media are not committed. Passing structural assertions
will not by themselves establish native edge/render fidelity.

**Import:** property-layout correction implemented. Queue **496** passed all
33 scoped mask tests (five existing ignored); queue **506** passed public-workspace
check/test/Clippy/fmt (**3,416 passed / 407 existing ignored**). Fresh Intro import
retains **[300,300]**. Equal diagnostic font substitutions permit before/after
three-frame slices at 5s; the formerly hard oval becomes soft, but unresolved
background colors and titles remain. Whole-frame RGB MAE against the independent
native frame worsens **88.56 → 89.60**; this is explicitly not a fidelity pass.
Feature-isolated native raster/alpha scoring remains unmeasured.
**Export:** existing native Vector2 Feather lowering is unchanged; fresh
edited-FX Adobe acceptance/control readback
and independent 30fps RGB/alpha proof remain **unrun/unmeasured** for this repair.
No reference replacement, threshold change, expression runtime or fidelity pass
is claimed.

## Authored Fast Box Blur radius keys — bounded import approximation

The static-only `ADBE Box Blur2` adapter previously discarded the entire effect
when Blur Radius carried native keys, including convertible Linear/Hold schedules.
Import now admits expression-free scalar Linear/Hold intervals with finite,
nonnegative integral authored radius values; Iterations, Dimensions and Repeat
Edge Pixels must still satisfy the unchanged static profile. Each existing key
maps to editable Gaussian blurriness using the existing sequential box variance
formula. Key count, layer-local time, effect order and enable switches remain.
Budget/clock/value failures omit the keyed effect atomically rather than silently
freezing a permanent blur; supported siblings survive without identity leakage.

**Approximation:** Linear intervals interpolate Gaussian sigma/blurriness, not
native radius. Nonlinear variance conversion between authored endpoints is not
preserved. Hold values match the existing static approximation; native kernel,
fractional-radius quantization and edge alpha remain unverified. Bezier/unknown
intervals, expressions, separated/malformed values and dynamic ancillary controls
remain diagnosed omissions. No new sampled/fitted keys, JsScript, schema or
renderer/runtime feature is introduced. **Export:** unchanged existing Gaussian
writer may emit those editable Gaussian keys; native Box Blur identity/controls
are not restored. Independent native export/control/alpha proof is unrun.

Evidence: unchanged private Carousel Slides source SHA-256
`853e6b01377cddacdea62fec4e3e130ba1e480c78afcbaf28a6bd1e0f649dccc`, selected
composition 1, contains authored scalar Radius schedules such as Linear
12→0→0→12 and separately rejected Bezier instances. Supplementary CPU tests
`box_blur_authored_linear_hold_radius_keys_retain_variance_endpoints` and
`box_blur_radius_keys_keep_clock_order_and_fail_atomically_on_budget` mutate the
independent public static Box Blur donor; they are not new Adobe-authored motion
proof. Exact execution, fresh-source import and diagnostic image results are
recorded in the bug-fix PR. Full feature fidelity and native bidirectional proof
remain unmeasured, not passed.

## Static Directional Blur source-plane controls — forward export only

**FX → AEP:** an actual native Solid source, isolated top-level unparented
ordinary2D, with one enabled static Directional Blur, static uniform positive
Scale/Rotation, no masks/matte/Layer Styles and Normal blending, now writes
`Direction = FX Direction − Rotation`, `Length = FX Length / (Scale / 100)`.
FX controls act in screen-space; native raster effects act before the owner
Transform. The source-relative Anchor, Position, source dimensions, effect enabled
state/order and clocks remain editable and unchanged. No arbitrary padding,
flattening, donor replay, schema/runtime/import changes or input pruning is used.
Nonuniform/reflected/animated/nested/vector/3D/mixed/gated stages retain the
existing diagnosed approximation. Non-finite derived compensation is declined,
not clamped. Native/FX sampling kernels and alpha remain approximate/unmeasured.

The independently authored [public fixture and exact forward proof](../crates/aftereffects_file/tests/fixtures/effects/directional-export-plane.md)
pins source SHA/targets and managed native readback. CPU RED1517 exported
Direction90 instead of0; GREEN1528 passed3 focused tests. Fresh explicit FX base
and edited exports were independently opened in AE26.5x89: Direction/Length
0/20 at Scale200/Rotation90 and60/30 after FX Scale300/Rotation−45/Direction15/
Length90 edits matched independently authored native controls, including native
Solid identity, enabled effect, Anchor and Position. Native cleanup/fresh READY
passed. Later validation is recorded in the repair PR.

**AEP → FX:** implementation/proof unchanged by this export-only fix. Existing
import plane approximations are not repaired or claimed here; the separate
import-only PR4837 is not an ancestor or prerequisite of this independent branch.
Native30fps MP4/long-term Asset/fresh hash verification, FX/native RGB/alpha/kernel
comparison, animated/mixed planes and original49/P037 fidelity remain missing.
Forward editable-control progress is established, not full bidirectional delivery.

## Isolated static hard Shadow source-plane offset — bounded import correction

**Import:** native Effect Drop Shadow Direction/Distance describe a source-plane
vector; existing FX DropShadow offsets are screen pixels. A sole static hard
Shadow on an unparented ordinary 2D square-pixel Solid in a standalone selected
root now multiplies that vector by static uniform positive owner Scale and
rotates it by static owner Z Rotation. The editable owner Transform, Shadow
identity/color/opacity, source clock and siblings are retained. The diagnostic
identifies the bounded compensation and remaining fidelity gaps. No finite
padding, collapse admission, canvas clamp, kernel, renderer, schema or JS change
is involved.

Only static expression-free Direction/Distance, Softness0 and static
expression-free authored Scale/Rotation with native Auto Orient None enter
this correction. Eligibility reads raw native controls before expression aliases
are lowered. Soft kernels, nonuniform or signed scale, auto-oriented,
parented/nested/linked occurrences, animated controls, mixed effect stacks, 3D
and Layer Styles retain the existing best-effort mapping; this correction does
not establish their source/world-stage correspondence.
**Export:** unchanged existing native Layer Styles writer. Independent managed
native readback now establishes one fresh imported-source export and one FX
Shadow offset edit: editable Distance40/60, Angle90, Global Angle off and Size0,
with companion Scale200%/Rotation90 and explicit30fps preserved. Native getter
inventory also confirms that the eligible AV Solid owner has no Skew/Skew Axis;
Shape-group Skew is a different profile excluded by the Solid/layer-kind guards.
This is control proof, not native Effect versus Layer Style pixel equivalence.

The [independent native control and provenance](../crates/aftereffects_file/tests/fixtures/effects/shadow-static-source-plane.md)
uses AE26.5x89 composition1: Distance20/Direction90 on a red32x24 Solid with
Scale200%/Rotation90. Managed Adobe getter readback and 30fps RGBA rendering
establish a downward40px shadow displacement. The fresh native-source import
regression failed before the fix (queue1315: raw FX[20,0]) and passed after it
(queue1344: FX[0,40]). Static affine and native-source auto-orient guard
assertions remain supplementary CPU proof. Reviewer native job
`453e7d13d4ef4f1face504061d4568cb` independently read the original AV controls,
actual native Scale/Rotation edits and both generated Layer Style control sets;
see the pinned managed readback JSX/provenance above. Fresh FX-render RGB/alpha
comparison, exported native render/alpha, 30fps MP4 long-term Asset publication,
motion blur and original P037 remain unrun.
Original P037's unsupported unbounded/collapse correspondence is not admitted
by this profile; its full-case gap remains. This is a partial supported-control
repair, not complete bidirectional feature delivery or full-project fidelity.

## Range selector keyed float envelopes — structural export correction

**FX → AEP:** Percentage/Index Start, End and Offset, plus selector Amount,
now use the existing native float envelope (`tdsb=1`, flags `0xffffffff`,
mode8/subtype9), not the generic Transform scalar envelope. Existing value
scaling and composition clocks are unchanged. Enum controls are unchanged.
The independent AE26.5x89 keyed Percentage source
`text/import_selector_keyed_float.aep` (SHA256
`8bf318d14b4ef527822e7790cdb7934c13b913dc2f0c454f423a87b4053d9413`,
composition1, 320×180, 30fps, 1s) was authored through managed job
`4fc0ce1869664d79b205991166d8a4a2`. Linear keys at0/.5s are
Start0/30, End80/100, Offset−20/55, Amount25/75. Adobe actually edited the
second Offset key40→55 before saving; these are not converter-derived values.

`keyed_range_float_envelopes_match_independent_native_source` compares all four
complete descriptors/selections and decoded keys against that pinned source;
it failed before correction and passes afterward.
`keyed_range_full_export_matches_native_envelopes_and_edited_input` checks full
fresh editable FX export, descriptor/clock/value/interpolation records and a
second independently specified FX Offset edit55→70. These are structural checks,
not generated-project Adobe acceptance. Static and Index native proof is not
newly established by the keyed Percentage case.

**AEP → FX:** implementation unchanged; the native fixture freshly imports as
editable Text with its Range selector. Import visual equivalence remains
unmeasured. Generated Text native acceptance retains the separately documented
baseline Text-reading failure; no new native opening/render was run here.
Independent 30fps render/Asset publication, full-resolution RGB, alpha/audio,
and actual generated-native edit readback remain unproved. No fidelity pass or
new unsupported-feature omission is claimed.

## Finite direct-media Time Remap carriers — bounded export correction

**Export only; import unchanged.** Imported authored-remap helper Groups have an
unbounded input lifetime inside a separate visibility gate. Previously their
artificial default domain exceeded native signed ticks, omitting the entire
editable remap carrier even when its backing footage was explicitly finite.
A Group inside a finite emitted precomposition now has a bounded export view
only when every direct child is full-source identity Video/Audio with matching
asset and retained intrinsic duration. The enclosing precomposition certifies
reachable input time; intrinsic media metadata separately certifies source time.
No generic child-extent inference, root-duration fallback, importer change,
native-limit increase, key pruning, flattening or source substitution is used.
Empty/nested/mixed children, conflicting source identity/duration, source
selection/clock edits, offsets and foreign parents retain ordinary planning.
Existing key/control-hull/extrapolation/Transform admission still applies.

Pinned native source `media_native_panel/native/fx-export-media-static-remap.aep`,
composition 1, SHA256
`e795d41e2265565f5da6104dfc52893270cb619fac571576eb46c8317c9cb706`,
retains Linear keys `(0,.75),(2,.75),(8,8)` seconds and an 8000ms media source.
`export_document::tests::media_native_panel::native_movie_remap_input_edit_retention`
failed before the repair (rust1532), then passed with fresh full export (rust1561,
1577). The occurrence-only input edit is250..1250ms mapping to500..1500ms;
all authored keys, including the dormant 8s key, survive. The affine native-source
control and `native_movie_remap_finite_domain_certificate_guards` also passed.

One managed Adobe 26.5x89 readback opened fresh exported AEP SHA256
`bf117264ae710dfc4dc64faeaafa56e40918de6b0886605d35da660413a90874`,
independently observed all three Linear keys, an 8s source, and edited native
occurrence `start=-.25,in=.25,out=1.25,stretch=100%` seconds. This is native
open/control and input-edit evidence, not render fidelity. The unchanged MOV
SHA256 is `a383d0058de7ce9723e263611acd35a438cf1154fbbfe8e0f1a1f78e74546292`.
Managed request `accuracy-w01-finite-remap-carrier-readback-v1` completed cleanup,
fresh READY and publication; receipt identity is retained in the PR. Readback
relinked only private copies to the hash-pinned same media. Static-blue frames
are visually nondiscriminating: fresh RGB/render/Asset comparison, alpha, audio,
and original-49 applicability remain unmeasured. This does not complete general
bidirectional Time Remap feature proof.

Reviewer current-HEAD follow-up independently read a fresh export from immutable
converter build1728/commit`ed8b15022c981a077a8b0a9cb286dac2ada771ad`. AEP SHA256
`2c680ebe04e980ab7b951eb23d5ed29877ac858e87409039e2e49cebf1ae4ee9` differs from
the earlier artifact; its native receipt was not silently reused. Managed AE26.5x89
job`91bbd3c06010434fb01fac0bc3026be6` verifies exactly one media/remap/outer target,
all three exact Linear keys, 8s source duration and the edited composition-global
occurrence endpoints. Raw native getters, script, input hashes and converter
provenance are pinned in `media_native_panel/finite_remap_output_readback.{jsx,json}`.
Cleanup/fresh READY completed. The native `.25/1.25` in/out points correspond to
raw layer-relative `.5/1.5` at start−.25/stretch1; these are different clock bases,
not an input-edit mismatch. No source-key pruning, source/reference modification
or new render/fidelity/Asset proof is inferred.
## Oracle-backed 30000/1001 MOV source metadata — export only

FX → AEP now admits constant QuickTime video at exactly 30000/1001 (including
ratio-equivalent timescales) alongside the existing exactly representable rates.
AE26.5x89 independently stores this source as integer29/fraction63570, not generic
nearest16.16 rounding. Its source duration is validated sample count ×100/2997;
this typed rational travels from media admission through package/lowering to
`sspc` without recovering sample count from rounded milliseconds. Other source
rates keep their prior serialization, and malformed/VFR/edit guards are unchanged.
Unsupported neighboring rates, 24000/1001 and60000/1001 remain rejected. No layer,
key, source-remap, composition-duration, runtime or schema policy is changed.

The [pinned native case](../crates/aftereffects_file/tests/fixtures/ntsc_media_clock/README.md)
records exact hashes, independent author/readback provenance, source metadata and
1×/2× controls. Its executable full-export regression consumes that independent
AEP and unchanged movie bytes, asserts native source rate/duration plus editable
start/in/stretch response for two FX inputs. Millisecond windows do not restore
native submillisecond outPoints exactly. AEP → FX implementation/proof is unchanged.
Generated-output Adobe readback also succeeded for both fresh packages, retaining
online oracle-native source metadata and stretch100/50 with successful cleanup
and fresh READY (native job/source-build commit pinned in the case provenance).
Reviewer follow-up freshly exported both named controls at rebased committed
HEAD2ee8b5c36 (targeted NTSC/duration/dedup queues1787–1789 passed). Managed native
readback85c9a064288d4013bdc86568cc01c689 and complete3s/90frame/30fps MOV render
7c535fa2423a40f1a58333612626b9d8 retained the exact source metadata, full-canvas
placement and100%/50% controls; both published after cleanup/fresh READY. All30
original/15 edited full RGB24 frame pairs were nonblank/moving, channel maxdiff1LSB,
not byte equal. The unchanged pinned rgb-hybrid/.99 frame-image gate **FAILED**:
original min.925329 and edited min.926883, all45 below.99. This is native acceptance
and actual output execution, **not strict RGB fidelity**. Exact hashes/means and
unchanged submillisecond oracle endpoints are recorded in the fixture README.
Long-term reference Asset, alpha/audio, editor/source-FX-render and actual49
restoration proof remain unproved.
This is bounded export support, not completed general bidirectional conversion
or an exact frame-selection/fidelity claim.

## Range Selector enum descriptors — bounded export correction

**Export:** Units, Based On, Mode and Shape now use native integer-enum
storage, not floating scalar descriptors. Ordinals and editable FX values remain
unchanged. Units/Based On/Shape use flags0x20000; Mode uses0x20004. Native
leaf storage flags1 and mode4/subtype4 match independent Adobe-authored bytes.
Numeric selector controls, Wiggly Selector Mode and other Text records are
unchanged; this is not a general scalar-envelope repair or native Text acceptance.

The public [selector-enum fixture](../crates/aftereffects_file/tests/fixtures/text/selector-enums/README.md)
pins source SHA256, composition1, exact managed author body and actual native
BasedOn Words3→Lines4 edit/readback. AE26.5x89 job
`34b9ca9ee18a490882688c976b455634` verified cleanup/fresh READY. Regression
`native_range_selector_enums_match_adobe_source` compares complete descriptors,
leaf storage and values at24fps/30fps property clocks: RED queue1435, GREEN1446
(one passed, zero ignored), public check/Clippy/fmt passed.

**Import:** unchanged implementation; fresh immutable converter1452 imports the
native saved selection as Index/Lines/Subtract/Triangle. Native selector Smoothness
still has no runtime equivalent and is explicitly omitted. No new FX/runtime support.

**Proof limitations:** native author/readback and fresh import/structural export
are established, not generated-AEP Adobe inspection or rendering. The earlier
minimal generated Text baseline AND candidate both failed with `Error reading the
text layer. Skipping the text layer.` This new enum-only candidate was not sent
through Adobe; the existing acceptance prerequisite remains unresolved. Native
export input-edit response, independent30fps MP4/long-term Asset, RGB/alpha/audio
and original49 fidelity remain unproved. No reference substitution, relaxed gate,
font alias, flattened content or source-AEP replay is used.

### Authored root composition endpoints — export-only quantization repair

**Export:** current editable document envelope seconds, including duration edits,
are quantized to the nearest selected native 16.16-FPS frame; positive half-frame
ties round upward. The prior integer-millisecond projection plus ceiling could
add a frame. Root ticks retain downward integer encoding and signed bounds.
Layer/key/source clocks and positive child/source-duration constructors are
unchanged. This is a native endpoint approximation, not a source-time shift.

Independent AE26.5x89 controls saved/reopened at30fps establish12.167s→365frames,
959/30s→959,77.867s→2336 and365.499/365.5/365.501→365/366/366.
A separate constructor/setter control establishes1ms→**zero** frames and
0.499/0.5/0.501→0/1/1,1.499/1.5/1.501→1/2/2. Positive inputs that round to
zero therefore retain the native null-frame root endpoint; they are not rejected
or clamped to an invented one-frame minimum. Nonpositive/nonfinite FX inputs
remain rejected. No layer is pruned or shifted to manufacture this endpoint;
existing unsupported-layer diagnostics and active-range checks remain in force.

Managed native request `accuracy-r02-minimum-duration-20261003-v1`, job
`e651f7ab45dc4a89bcbaef0f4a7471c8`; JSX SHA256
`53a5c228e015b530adb941ebc254ea28e36ded20ef866a0b03fb63b9523fa6b0`,
saved AEP SHA256
`efc96e6ef2422db5f9f8679015c5f7aedd08378e5ab7d8ab7cd7a1f28364ac55`,
result JSON SHA256
`e1468af53e084e392f3343dac66e2fd19af0c8c9d8170ce5fee07c327bbed0bb`.
Native files remain ignored local evidence, not public reference Assets.

`root_duration_exports_positive_input_with_native_null_frame_endpoint` was RED
(queue1420:1ms rejected), then GREEN (queue1424). It checks admission, zero native
root ticks and retained editable layers; it does not prove rendered output.
`root_duration_exports_current_duration_edits_without_millisecond_loss` was RED
on the pre-repair exporter (queue1427:366 rather than365frames), then GREEN
(queue1424); it exercises composition-only edits, same-millisecond JSON duration
edits across365→366frames, and explicit duration replacement to7.5s/225frames.
`authored_duration_preserves_native_null_frame_endpoint` covers short boundaries,
invalid inputs and unchanged positive source constructors.

**Import:** unchanged. Independent numeric native authoring/readback is measured;
other native FPS, generated-output rendering,30fps reference Asset,
RGB/alpha/audio fidelity and full bidirectional feature proof remain unproved.
OriginalP046 conversion timed out without output; its full native render proof
is still missing. Latest-main validation queue1437 passed formatting, workspace
clippy and9targeted tests; the required full gate was1654passed/12failed/409ignored.
It retains the11previous baseline failures plus the spatial-position equal-endpoint
failure independently reproduced on untouched latest main7d720ad45 (queue1447).
No failures were ignored or softened. These unchanged-main failures are disclosed,
not a per-PR merge gate under campaign qualification.
No original49 source/reference is modified.

**Generated-output acceptance:** corrected managed AE readback job
`05e03704f4d447c29cd9e8cad6210eb8` independently opens/closes/reopens four
fresh FX exports at30fps, verifies0/365/366/225frame composition endpoints and
retained editable red native SolidSource. Unchanged reopens are identical; the
separate current FX duration edit produces the specified225frame endpoint.
Pinned script, build/input hashes and numeric results:
`proofs/root-duration-native-readback.{jsx,json}`. One authorized corrected
managed operation, zero models/renders/uploads. This closes generated-output
acceptance only; rendering, long-term Asset, RGB/alpha/audio and original49
fidelity remain unproved, not silently promoted to PASS.

## Independent Linear/Hold Anchor knots — necessary partial export repair

Import implementation is unchanged. Export can union differing authored Anchor
X/Y knots only for Linear/Hold keys without spatial metadata, retaining the
existing exact scalar merger and rejecting simultaneous changing Hold/continuous
axes. Native Anchor remains a three-component spatial property with one shared
ease and zero spatial tangents. Aligned/cubic/spatial/Orientation profiles are
unchanged; no frame resampling or runtime/schema changes are introduced.

Independent self-authored source and provenance: properties/p033_anchor_union.md.
The native source proves union key storage and numerical response, not exact
analytical interpolation: native interior drift and the failed separate 2e-5
analytical check remain limitations. Targeted converter RED1893 (different times)
then GREEN1905 (three regressions) established the admission repair and endpoint
edit response; own-reader native-source assertions are supplementary.

This is necessary but NOT sufficient for actual49 P033/P006. Fresh post4807
unchanged-source exports still omit Group100 for unknown Text/font glyph bounds;
prior P033 media admission/bounds omissions are gone. No RGB improvement is claimed.
The admission repair is limited to native Transform sidecars (native3D or planar
separated Position followers); ordinary planar/static Position is unchanged.
Reviewer managed native job397668dd7458454f91864ca1340b2c1e independently accepted
fresh queue2110 control/endpoint-edited output: union0/.5/1s, Linear and zero
spatial tangents, exact Anchor [0,0,0]/[40,10,0]/[40,20,0] and edited60X.
The earlier reviewer static-Position fixture was omitted through the unchanged
paired path and failed target selection; it is not proof. Both calls returned
verified READY. Native interior drift and failed analytical check remain disclosed.
Native30fps reference/long-term Asset, alpha/audio and complete fidelity remain
incomplete. See properties/p033_anchor_union.md for generated-output hashes.

## Isolated static Directional Blur source plane — bounded import repair

**AEP → FX:** On an ordinary unparented selected-root 2D square-pixel Solid with
one active native Directional Blur, static expression-free Direction/Length and
uniform-positive Scale/ZRotation, the native source-plane axis and pixel length
now become existing editable FX screen-space controls: Direction adds owner
Rotation, and Blur Length multiplies by owner Scale/100 once. Native owner
Transform and effect identity/order remain editable. Masks, mattes,
preserve-transparency, nonuniform/reflected/dynamic/parented/nested/linked/3D or
mixed-effect profiles retain the existing approximation. Shape/Text continuously
rasterized planes are not admitted by this raster-Solid profile. No FX schema,
evaluator, renderer, effect support bounds or Group collapse guards change.

Native/FX sampling kernels and alpha-edge behavior remain approximations; the
coordinate-unit correction is not a pixel-equivalence claim. The import warning
retains these limits. Actual49 P047 scaled/rotated Directional Blur owners were
investigation seeds, not full-case native repair/retention proof.

Independent minimal source/control/readback/RGBA axis evidence and concrete
fresh-import regression are pinned in
[`directional-static-source-plane.md`](../crates/aftereffects_file/tests/fixtures/effects/directional-static-source-plane.md).
Source SHA-256 `e2802e7567b6349d83bd837190ad6274a81576bc9274fca39cbbfcd961972fcc`,
composition1, independently authored Direction0/Length20/Scale200/Rotation90,
natively renders a horizontal extension. Regression
`native_directional_blur_uses_rotated_scaled_source_plane` fails before this fix
with [0,20] and passes afterward with [90,40].
`directional_plane_import_responds_to_native_transform_input_edits` supplements it
with fresh-import Scale/Rotation edits and unsupported nonuniform/negative scales.

**FX → AEP:** unchanged existing Directional Blur mapping. Source-plane
reconstruction for transformed raster owners and independently inspected fresh
export/render evidence remain **incomplete/unrun**. Original P047, fresh FX RGB/
alpha comparison, motion/audio fidelity, dynamic planes and the required native
30fps MP4 → long-term Asset → fresh verified download chain remain
**unmeasured/incomplete**. This is a bounded import repair, not bidirectional
feature completion or a strict fidelity pass.

## Owner-approved shared Boa loop-policy removal

This isolated proposal preserves the loop-policy change split out of PR4807:
shared script evaluation uses Boa 0.21.1's `disable_loop_iteration_limit` instead
of main's 100,000-iteration ceiling. The finite regression returns all200,001
iterations; Premiere's failure regression uses a finite loop-body exception
rather than running a nonterminating script. Import mappings, Boolean geometry
and native writer behavior are unchanged by this proposal.

**Owner-accepted safety tradeoff:** the repository owner explicitly approved
removing the loop cap: "撤廃してマージでいい". Synchronous execution has no enforced
deadline or cancellation; nonterminating input can wedge the caller.
Documentation is not enforcement, and this change adds no process supervisor.
The existing128-frame recursion and10,240-slot stack guards remain and do not
bound execution time. Finite CPU tests are not isolation or native fidelity
proof. No new Adobe/render/Asset/RGB/alpha proof is claimed.

## Box Text vertical alignment — export-only omission correction

**FX → AEP implementation:** explicit whole-document Box Text `verticalAlign`
now uses the independently native-authored frame field
`/0/8/0[0]/0/2/13`: integer **1** for Center, integer **2** for Bottom, absent
for Top/unspecified. The JavaScript API ordinals 12812/12813/12814 are not COS
values. Both the cache-free native Box profile and the generic richer-Text writer
emit this field. The frame control is global across Source Text documents, so
changing alignment across Hold keys is rejected rather than silently retaining
the first value. Point Text keeps the contextual diagnostic
`Box vertical alignment is not applicable to Point Text and was omitted`;
variable-font axes, underline/strikethrough, scaleBoxTextWithTransform and
continuous Source Text interpolation remain diagnosed as before. No source
AEP replay, glyph cache, FX schema/runtime, font alias or renderer change.

**AEP → FX:** unchanged. Cached-box-baseline import intentionally normalizes
explicit Top after consuming the native cached baseline; this follow-up does
not claim a bidirectional Center/Bottom editable round trip or repair cached
placement semantics.

**Independent native semantic proof:** public
`tests/fixtures/box_vertical_alignment/native_controls.aep`, SHA-256
`c642695f0c0ff524c1f586ad89c8c84aee716a044687dcc8256e789bef56f854`,
composition ID1 `W09 P009 Box Vertical Alignment`, 1280×720, square pixels,
30fps/1s, three otherwise identical ArialMT48 Box controls. Managed AE26.5x89
save/reopen observed Top/Center/Bottom and the single differing semantic COS
field above. Editing native Center→Bottom changed sourceRect.top from
−43.1756591796875 to113.590637207031. Source/script hashes and limitations are
in the fixture README. Semantic RED queue2068 failed generated Center None
versus native Some(1); GREEN2103/2132 passed the native-field/input-edit
regression and Point/key-change negatives (no ignored regressions).

**Generated P009 control acceptance passed:** transferred from the concurrently
started W09 session after the parent stopped that duplicate and confirmed sole
ownership. Managed job `a12fdb9827fb4e9da4df97b47e44b0fe`, request
`w09-p009-private-worker01a10492-final`, pinned script SHA-256
`9f611a6ac03239fb3323a41474cb13383d11a8e181697702917b78dfc2b28808`,
opened the unchanged fresh2133 export and required exactly one match for each
caption owner8000/8002. Both were Box Center12813; native edits to Bottom12814
changed sourceRect.top16.2384033203125→33.0046844482422 and
16.6837158203125→33.4499969482422. Cleanup/fresh READY and result publication
passed. This proves actual generated control preservation and a native edit
response, not a saved/reopened fresh **FX-edited** output or11 Top-sibling fidelity.

**Separate failed compound probe retained:** managed request
`w09-p009-generated-box-alignment-acceptance-v1`, job
`466b9365d69243c0802b91f2e3d08422`, failed at the compound Box-control
assertion for actual P009 `Caption / THE OPENING GAME.`; cleanup/fresh READY
succeeded. The pinned script had reached that assertion after minimal generated
Center/Bottom save/reopen, nonzero edit-response and independent-oracle geometry
assertions. Those are partial execution evidence, **not a published successful
acceptance result**. The exact mismatching predicate (text at0.5s, alignment,
box state or dimensions) has not been independently isolated; no speculative
converter change or retry of that compound probe was made. This session used its
two initially bounded follow-up calls (failed probe and successful render); the
other W09 had separately completed the accepted probe and another after-render.
The parent stopped that duplicate, transferred its evidence and explicitly
authorized a fresh-main before-render and one ordinary recovery retry. All
attempts are retained; do not claim
the combined concurrent root used only three calls. Zero model calls/image reads.

**Actual P009 export/render/score:** unchanged archive
`deep-blue-editorial-v13.tsrct`, source SHA-256
`9bb0a8152f4e31a482df87f738597a23a29d3d28cfddc07201b4fda11ff3d1b2`.
Immutable converter build2133 (`f6992ddf9`) removed all13 vertical-alignment
omission diagnostics, including Center Arial48 owners8000/8002 and11 Top
siblings. Generated AEP SHA-256
`b61b9a8f0a4f5e8c422314f2a8d77c6b75733e672da3ae6a52431ab2f9278e94`;
managed native render job `e7042e9c05044c31bda3047e3ac1c94c` opened/rendered
root ID1, 1920×1080/30fps/full263frames, with cleanup/fresh READY. Output MP4
SHA-256 `5ae11f0098437db9e0fbe83879723cbe96e0def0ac3211da60fa2e110e5fae62`.
Missing VT323/Press Start2P fonts use the explicitly authorized diagnostic
substitution; their replacement identities and exact metrics remain unproved.

The unchanged TSRCT reference and both native movies were hash-checked, fully
decoded and verified to share all263 frame PTS. Yesterday's pinned
`validation_cli` (SHA-256
`efe27bf526803a2dab9ccf1ac7d3f898e2038abd5fc30201f5515e4935b8bae4`)
was rerun unchanged: canonical RGB24, full-resolution rgb-hybrid, inclusive
0.25s grid, threshold0.99, 36 samples per side. **Historical before**
(`cd9614f5e`, not fresh main): mean0.5479177546946019,
min0.21893171024321498 at3.75s. **Fresh after:** mean0.548392721247455,
min0.2189163361087611 at3.75s. Mean delta+0.0004749665528531;
**36/36 samples below0.99 on both sides**. This is a diagnostic comparison,
not an isolated causal mapping improvement: other converter changes separate
the historical before from current main. The transferred after-render
`7b8b59eee0334a4d8cb7a3fb945198e4`, MP4 SHA-256
`d4d28db17fda9ec41f2da056b9508eee1d0a64fcd7447cd0728617f28b0248ca`,
was independently hash-checked, fully decoded, PTS-checked and rescored with
identical results. No crop/retime/resample/reference change or relaxed floor.

**Fresh-main before recovered on the single authorized ordinary retry:**
immutable main build2192 at `43002a200` exported the unchanged source. Initial
request `w09-p009-latestmain-2192-before-v1`, job
`1f14577917e242f09b034cb42508f5d6`, failed with `RUNTIME_UNAVAILABLE`,
READY=false (owned exit unverified); its failure remains preserved. The user
then explicitly authorized one ordinary managed retry using built-in recovery.
Request `w09-p009-latestmain-2192-before-v2`, job
`db37384172c54579bfd2f5b96de6a609`, published the full263frame30fps MP4,
with cleanup/fresh READY verified. SHA-256
`09b3b479dd887bbb633c7a3170c46e5293a4e04e1891037dfbf9d06943542c8b`.
No manual kill/unlock, transport fallback, staged-output promotion or retry loop.

Fresh-before was hash-checked, fully decoded, matched all263 reference PTS and
rescored with the unchanged pinned validator/settings above: mean
0.5479177546946019, min0.21893171024321498, worst3.75s,36/36below0.99.
The fresh-after result is mean0.548392721247455,
min0.2189163361087611, worst3.75s,36/36below0.99. Mean delta
+0.0004749665528530622; minimum delta−0.000015374134453877142.
Both remain diagnostic FAILs. Before main43002a200 and afterf6992ddf9 build
identities differ because the retained after predates later unrelated main
merges; this is a fresh case comparison, not an isolated same-base A/B.
`p009-private/fresh-before-after-summary.json` retains exact commands/provenance.

Local retained provenance is under `tasks/accuracy-W09/p009-vertical-align/`
(`before-after-summary.json`, score command/stdout files, native failure/render
receipts, source profile and build receipts); transferred control/render evidence
and latest-main failure are under `tasks/accuracy-W09/p009-private/`.
Videos remain ignored/local,
not Git. No complete critical-event/all-frame equality, native UI inspection,
independent minimal30fps reference/long-term Asset, alpha, audio, exact fonts or
full49 fidelity is claimed. This entry separates implemented export control,
independent semantic proof, passed actual generated controls/native edit,
unproved fresh FX-edited save/reopen and measured below-floor fresh-before/after
full-project rendering.

**Final CPU checkpoint:** queue2220 at `009d8a531` passed converter `fmt`,
workspace `check`/`clippy` and support-ledger validation. Normal
`test-aftereffects-file` ran1847passed/1failed/424ignored; the sole failure was
`export_document::hierarchy::tests::static_ellipse_bounds_keep_masks_ambiguous_geometry_and_overflow_guarded`.
Targeted unchanged-main queue2227 at `8eb7daafe` reproduced the identical
`static_bounds(ambiguous).is_err()` failure. No new ordinary-test failure was
observed; the full gate is **not GREEN**. All four new alignment regressions
passed (including generic writer codes, Point exclusion and Hold-key rejection).

Explicit execution of the existing ignored
`pr4442_unsupported_text_layout_is_diagnosed_while_supported_sibling_exports`
failed before export/assertions: `ContinuousEasingRequiresNumericValue` for its
Linear string key. Candidate queue2219 and unchanged-main queue2228 reproduced
the same error. Its old warning expectation was updated, but that proof-backlog
case is not counted as passed. No ignore was added or baseline failure hidden.
## Embedded physical-font outlines — bounded mixed Text export

**Import:** unchanged. **FX → AE:** a mixed 2D Group can retain editable
horizontal ASCII Point Text when its exact, unambiguous physical font is present
in hash-verified archive bytes. OpenType shaping and glyph outlines provide only
a source-local enclosure copy to the existing hierarchy classifier, including
checked source clocks and nested Groups. Original Text, children, composite
opacity, motion blur and clocks are emitted unchanged. Opacity keys do not alter
the outlines and are retained. No host font, text-box estimate, viewport crop,
arbitrary padding, input pruning, rasterization or donor replay supplies bounds.

Missing/ambiguous fonts, variable faces, missing glyphs, ligatures/reordered or
expanded clusters, non-ASCII/multiline/whitespace-only text, box/path/stroked or
decorated Text, layout dynamics (including Tracking/font size), Text effects,
animators, masks, mattes and motion blur retain the existing rejection/fallback.
Owner skew/spatial/mask/effect exclusions and existing clock/3D guards remain;
a rectangle enclosure cannot certify vector-only collapse. Native font
substitution is not certified. CustomShader omissions are unchanged.

**Evidence:** `embedded_font_bounds_retain_a_mixed_motion_blurred_storyboard_card`
was RED (queue1998/2091), then GREEN (queue2160), covering editable Text retention,
Opacity keys, nested short source clocks, absent font and dynamic-layout
negatives. Actual private P032 cards11000–11003 change from whole-card omission
to retention on immutable converter2161. Independent AE26.5x89 control
`5bb6433d1605490d97ee20ef9ff3afe6` establishes the four embedded Avenir Next
Medium42/Tracking20/right labels. Managed generated-output numeric acceptance
`ae9070718e7748498f9f8924f2187588` verifies eight editable Text/enclosure profiles
and their Opacity keys across original and edited FX inputs. Editing one font
size42→420 produces10× native glyph width/height and recomputes its canvas
2079×1138→2124×1326. No Source Text/CUSTOM_VALUE getter was read.
See `proofs/embedded-font-outline-native-readback.{jsx,json}` for sanitized
numeric provenance. This is bounded native acceptance/edit response, not a
font-substitution, general-font shaping, RGB/alpha/audio or formal long-term
Asset fidelity pass. P037 dynamic Tracking and P006/P033 mismatched native3D
key times remain separate omissions; their whole scenes are not claimed fixed.

The bounded actual-case diagnostic panel uses unchanged source references,
1920×1080 canonical RGB24/rgb-hybrid at 0.25-second intervals and the unchanged
0.99 threshold. P032 mean improves 0.2735006146→0.3701449133 (29 samples);
P037 stays 0.3479564393 (30), P033 stays 0.0054418209 (93), and P006 stays
0.0168294002 (59). All samples remain below 0.99: this is measured omission
repair, not scene fidelity. Every native result was published after fresh READY;
the failed P032 request's staged output was never scored. Its separately
approved managed recovery/render used a new request ID.


## P001 Video luma matte and Screen omission repair

**Import:** implementation unchanged; pinned independent native source import
asserts editable Luma/LumaInverted matte relations and Screen occurrence blend.
**FX → AEP:** Video admission now permits Luma/LumaInverted and Screen through
the existing shared NativeLayerOptions envelope, instead of dropping whole
owners/providers despite already having native serialization. No new clock,
shader, renderer, schema or blend implementation; legacy Media admission and
Video placement/captions/corners/source-clock guards remain unchanged. Add and
AlphaInverted Video profiles remain outside this bounded independent proof.

Independent AE26.5x89 author/save/reopen and fresh generated original/edited
acceptance both returned READY; exact5layer/320x180/2second CompItem identity,
matte provider links, disabled provider eye switches, Screen enum and native
Opacity100→50 versus100→20 keys/midpoints75→60 are numerically established.
Native fixture/provenance/readback: properties/p001_video_compositing.*.
Source-derived executable regression RED2309→GREEN2318 retains editable
footage and providers, with no donor replay or flattened replacement.

Full private actual P001 render (1080x1350,10seconds,30fps,300decoded frames)
retains formerly omitted Videos21/22/23/30 and matte union Group120/children.
Only CustomShader omission remains accepted/unsupported. Unchanged-source
canonical RGB24/rgb-hybrid0.25s validation41samples improves mean
0.18347855044688954→0.2141610938806818; minimum
0.1761758728743271→0.20201688819476776. Before is unchanged-main8eb7daafe
score-eval/build2250; after473da79c5+repair/build2325. This measured omission
improvement is NOT a0.95/0.99 scene pass. Full-fidelity, general Video profiles,
alpha/audio, minimal independent30fps reference/long-term Asset and human UI
proof remain unestablished; numeric acceptance is not pixel equivalence.

## Footage outpoints beyond rounded composition duration — P038

A native AV layer's authored outpoint need not be enclosed by its composition
render duration. The old footage validator rejected entire still/movie/audio
occurrences when the root rounded down to a native frame, rather than merely
preserving their ordinary outpoints. For P038, all 29 affected owners end at
12167ms while the 30fps root ends at 365/30 seconds: four full-duration background
Images, 23 late Image copies, one late Video, and one Audio occurrence. Accepted
CustomShader omissions remain explicit; no shader mapping or image-transfer
change is part of this repair.

Independent Adobe-authored `footage_outpoint/beyond_duration.aep` establishes
saved/reopened image, movie and audio outpoints of 12.1669921875s beyond the
12.1666666666667s root. Actual native edits to 12.2s and back preserve source
identities/durations, start/in points and root clock. Managed job
`9c6179d5b65c4d44b9394b1ea95dad29` completed with cleanup and fresh READY.
This is numeric control readback, **not native pixel sampling, alpha/audio or
render-fidelity proof**. The fixture README pins its hash and provenance.

The writer now preserves these outpoints without clipping/shifting source clocks
or inventing a longer composition. Empty/overflow/unrepresentable active ranges,
source selection, source interpretation, source geometry and channel guards
remain. Still records retain their established 24576-Hz truncation; finalized
source clocks retain exact millisecond rationals. Targeted native-fixture and
fresh-writer regression was RED2331 -> GREEN2337 (3 tests, none ignored).
An earlier generated-acceptance request, managed job
`66af1afbdd4d4fc39882558dba029449`, reached all four tolerance-based readbacks
(open, save/reopen, edit to 12.2s, restore/save/reopen), then failed a redundant
exact outpoint-equality assertion; it is not counted as acceptance. The
separately authorized corrected request `r01-pr4913-generated-accept-tick-v2`
(native job `f9fb7d3184574a36832e0cc852f178fa`) **passed** all phases with
verified READY: image/movie/audio outpoints stayed within one 24,576-Hz tick of
the authored value and beyond the root duration, edited to 12.2s and restored,
with unchanged root FPS/duration, identities, source durations and start/in
points. This closes generated editable acceptance only, not render/alpha/audio
proof; see the fixture README for raw phase values.
Actual P038 render job `61ba828f3b0e471583e4844d3b722afe` succeeded with READY;
its fresh export has no composition-enclosure rejection. The same pinned
full-resolution canonical RGB24 rgb-hybrid validator at 0.25s measured 49 samples:
before mean 0.2341939952905118, minimum 0.039050430082756694 at 8s; after mean
0.2301522113747752, minimum 0.039006458872163405 at 8s. Both movies and the
reference are 1920x1080, 30fps, 365 frames. This is a diagnostic regression, not
a >=0.95 fidelity pass or isolated same-base causal A/B (baseline build2250 vs
candidate build2342). Sources, reference, clocks and scoring were not altered.
Queue2366 passed fmt, clippy, support ledger and full CPU tests: 1879 passed,
0 failed, 425 ignored. W05's earlier audio-sibling archive exports both siblings
with both baseline2250 and candidate2342; its historical exact guard symptom is
covered by this removal, but the historical omission was already fixed by prior
main work, so no new W05 causal improvement is claimed.

## Shape owners with inert Trim controls — export retention

FX Shapes without an installed `shape.trim` reject TrimStart/TrimEnd/TrimOffset
writes; playback drops those updates and keeps the authored geometry/paint.
FX → AEP now omits those inert controls with a contextual diagnostic while
retaining the editable untrimmed Shape, rather than dropping its whole subtree.
It does not invent Trim Paths. Installed Shape Trim keys, Boolean Trim behavior,
Round Corners and Offset Paths guards are unchanged. AEP → FX is unchanged.

Actual P046 Shapes12418/12421/16479/16481 reproduce the whole-owner omission on
main `cfe20dce1` (immutable converter2298) and are retained by converter2297 at
`169b9de0f`. `inert_shape_trim_keys_retain_untrimmed_editable_owner` covers direct
and nested owners, all three scalar targets and byte equality with the same
untrimmed editable input; `installed_shape_trim_still_exports_editable_keys`
checks the installed modifier's values and key count. Semantic RED queue2282:
one failure/one pass; bounded GREEN queue2285: two passes.

Independent AE26.5x89 control plus generated/FX-edited readback passed managed
job `a280ab82a5874fc793cef89dca69eac8`: one editable path, one stroke, no Trim
modifier; authored width6→12 and path vertexX100→140 edits read back exactly.
Twenty numeric checks are reported; this is not pixel/alpha fidelity proof.
The initial syntax failure `5bae4be37cb34b57ad3d88b188ee400c` returned verified
READY; the corrected call and full P046 render `53cd04e8f63345cdad9a85648f514757`
also completed cleanup/READY. All three authorized native operations are used.

Canonical full-resolution RGB24/rgb-hybrid at0.25s/.99: historical before mean
**0.16723525426915142**, after **0.1672271134622293**; both minimum
**0.00008168827055770724** at23.5s,128samples, both FAIL and below0.95.
Before is the preserved older converter movie (960frames/32s); after matches
source959frames/31.966666s. No trimming, threshold relaxation or isolated score
improvement is claimed. Fresh-main CPU omission evidence and numeric editability
are established; overall fidelity remains dominated by separate omissions.
Native minimal-reference RGB comparison, formal Asset publication, alpha/audio
and broad import fidelity are unmeasured. Local evidence is retained under
`tasks/accuracy-W06/trim/`, not a public source/reference publication.

## Skewed Text placement with independent planar Position knots

Forward export reuses the established native separated Position followers on the
outer skew-placement Null when authored scalar X/Y knots or temporal easings
cannot share one spatial leader. Keys are not resampled or merged. Rotation also
stays on that placement helper; anchor, scale, opacity and Source Text remain on
the editable Text drawable. Compatible combined Position keeps its existing path.
Spatial tangents with incompatible knots still reject, as do the existing fixed-
skew, 3D, effects, masks, animator and motion-blur unsupported profiles.

Public regressions cover independent cubic knots, an authored constant-X track,
unchanged helper parenting, nonduplicated drawable controls and the spatial-tangent
negative guard. Actual P046 CPU exports reduced this specific omission diagnostic
from 21 owners to zero. Five owners are retained independently; the other 16 also
require the separate parent-reference repair in PR #4888. This is not a full-case
RGB, font, alpha, audio or all-owner native acceptance claim. Native evidence and
combined full-P046 scores are recorded separately in the task PR; source/archive
and full-frame comparison policy remain unchanged.

## Control-link outgoing Hold endpoint admission

**Import:** direct scalar transform aliases, sibling Rotation and cross-composition
Transform aliases now accept a supported incoming Linear/Bezier/Hold flag after an
outgoing Hold. Native Hold controls the outgoing segment; the next incoming ease
is unused there. Unknown interpolation flags, nonfinite values/ease metadata,
invalid clocks/key ordering, and incoming Hold after a non-Hold segment remain
rejected by the existing bounded profile. No expression runtime or FX changes.

Independent AE 26.5x89 save/reopen control `properties/hold_endpoint_flags.aep`
(SHA-256 `70b9804ecc4f95f6ad8fb4d95a976168d4b6184181a763aabbf2778ba6a14f4d`,
composition 1) retains outgoing Hold → incoming Linear and native .25s/.75s
opacity samples 0/100 percent. A separate incoming Hold/outgoing Linear control
samples 85 percent at .75s, confirming endpoint direction. See adjacent fixture
provenance for exact fields and managed native job. Fresh-read regressions exercise
all three admission sites, with unsupported-flag and actual curved-segment negatives.

**Export:** existing exporter unchanged. Canonical minimal FX keys and fresh native
source import/export tests preserve signed -500/500/1500ms clocks, values and
outgoing segment ownership, including a 100→40 percent middle-value edit. The
writer may normalize unused incoming flags to Hold, not preserve exact raw flag
identity. Generated-output own-reader checks are supplementary CPU evidence.
Independent Adobe generated-output readback now passes for two fresh public
minimal canonical opacity inputs (middle100→40 percent), immutable converter
build1692/commit`36ea3119e57754f5f8698c1712edd215b70ab421`. Managed AE26.5x89
job`7ca07da819784cd1ba00d172b07e47a0` opens/closes/reopens each unchanged output;
three signed keys retain exact times/values/outgoing Hold/Linear flags. All14
critical numeric samples match independently authored `control-2` and its
separate in-memory edit, maximum error0 percent, with cleanup/fresh READY.
See `properties/hold_endpoint_output_readback.{jsx,json}` and adjacent fixture
provenance. This does not prove normalized native save, alias-expression
integration or native pixel equality. A separate discriminating public native
revision, `properties/hold_endpoint_render.aep`, now has a full-canvas/full-duration
30fps Adobe reference (60frames,128x64,2s), public immutable long-term Asset
`ROaNxCVfwxIfADbwmQvA_vid`, freshly downloaded/hash-verified. Its nonoverlapping
visible Rotation alias and direct Opacity controls expose the admitted Hold
endpoint transitions; disabled driver cannot mask them. Managed authoring/readback
job`2a9b49b00a0249da99a316a07bec8dd4` and render job
`9849807f2b794a129458e49410713a71` completed cleanup/fresh READY. Fresh-import
Tesseract versus independent native reference full-resolution canonical RGB24
rgb-hybrid .25s/.99 comparison FAILS:9samples,mean0.974632540563,
minimum0.965132273268 at.5s. This is measured diagnostic evidence, not a fidelity
pass or complete bidirectional proof. See `properties/hold_endpoint_render.json`
and adjacent script/provenance for source/Asset hashes, native provenance, exact
structural regression and critical-frame observations. Generated-AEP native
rendering, alpha/audio fidelity and original49 visual improvement remain unmeasured.
## Finite flare footage and affine sampled Opacity clocks

**FX → AEP:** a strictly verified AVC profile with 44 positive samples plus one
zero-duration terminal sample retains the native complete final frame. The
public 45-frame/120fps test and malformed edit/sample-count negatives guard this
exception; original media bytes are unchanged. Finite source selections reuse
existing closed-domain admission. Screen, motion blur and frame-mix switches
remain editable native AV controls; unrelated compositing guards are unchanged.

Non-remapped Video scalar Opacity with LINEAR arriving segment easing now carries
native ticks rather than requiring intermediate integer source milliseconds.
Independent 30fps unit-rate and 61:50 stretched AV authoring/readback controls
show two-stage quantization: occurrence property ticks, then exact affine mapping
and source-property ticks. All 30 discriminator keys and eight earlier controls
match this path; a single-stage ceil rule does not. First-key arriving easing has
no preceding segment. Other Transform tracks, non-linear/spatial Opacity, audio,
effects, styles and remapped/source-owned controls keep their existing guards.
Collision, checked overflow and native count limits remain errors. No general
float clock API or changed source range is introduced.

This is a native tick-quantized sampled-JavaScript approximation, not exact
continuous-time execution or a new universal half-tie rule. The actual P004
61:50 pulse omission has a semantic RED/GREEN regression, including collision,
overflow and unsupported profiles. Independent numeric readback is established.
Generated-output native readback observes 37 baked keys per flare with the proved
clocks and Linear interpolation. These baked JS Opacity values are approximate
within **0.035 percentage points** (observed maximum 0.03494098), a sub-quantization
magnitude of at most 0.08925/255, below one 8-bit level. This bounded acceptance
applies only to these baked keys, not other properties or exact assertions; it
is not a guarantee of identical pixels across rounding boundaries. An isolated
Opacity edit to 37 evaluates to 37 and survives native save/reopen, preserving
all key times and unrelated values. Reviewer managed job
`2267cf36931b4366b1ec41dd1847b41a` completed with verified cleanup/READY; the
original tighter value assertion failed and is retained as historical evidence.
Formal Asset, alpha/audio proof and full-corpus fidelity are not claimed. The unchanged actual
P004 project retains both flare occurrences and completes a native 17s/510-frame
render. Canonical 69-sample full-resolution RGB24 mean improves from 0.648674 to
0.664844; pulse samples at 1.75s/14s improve from 0.296702/0.259671 to
0.676003/0.839025. The before movie is preserved historical repair03 evidence,
not a fresh same-head render. Font substitution is explicitly enabled for this
diagnostic delivery; every sample remains below the strict 0.99 threshold.

## Planar Video Scale and separated Position source ticks — P024 omission repair

**FX → AEP:** the planar Transform sidecar for positive-affine, non-remapped
Video occurrences now retains three-component uniformly Linear/Hold Scale and
independently separated scalar Linear/Hold Position X/Y followers on native
property ticks. Occurrence milliseconds are first quantized on the owning property clock,
then mapped through the exact native rational source clock and quantized again.
Authored times, values, source ranges and Hold flags are not rounded to source
milliseconds or changed. The existing Opacity helper supplies the checked affine
mapping; its Linear-only Opacity admission is unchanged. Combined spatial Position,
3D layers, Position-Z, cubic/mixed-component easing, static source samples,
remapped clocks, other Transform properties, Audio and non-Video sidecars keep
their old guards.
Collisions, overflow and native key-count limits remain errors. **AEP → FX:** unchanged.

The original P024 reader/cards source durations are 3208/6416ms and occurrence
durations 3209/6417ms. Their Scale and separated Position keys first fail at
2008ms/900ms respectively: source times are 6441664/3209ms and 5774400/6417ms.
`rects::finalized_transform3d_animations` previously passed all of these through
`SourceClockPlan::source_time_millis`'s integral-millisecond guard, omitting each
whole Video owner. The narrowly guarded paths now supply final wire units to the
same editable native leaves; all six source layers and all three media remain.

`p024_planar_video_scale_preserves_two_stage_source_ticks` reproduces that sidecar
failure RED and is GREEN with authored-input preservation, exact Scale/follower
wire times and Hold flags. Negative profiles and supplied-unit collisions remain
covered. Independent Adobe26.5x89 author/save/reopen controls
(`1e2dac79569e4acca2e2033d8247dc22`, `d0112598706844d9b8035eea093d7c39`)
match two-stage quantization at **40/40 saved Scale/follower keys**, using each
native descriptor's 30724/30729 ticks/s; single-stage rounding differs. Generated
P024 acceptance uses its 30720-tick/s clock and reads 533 editable Scale/follower
keys per version, preserving planar switches, media, CornerPin and Alpha-matte
identities. A copied FX edit halves only reader Scale-X; native sample readback
responds with maximum relative error 0.000396057, while Scale-Y/Z and Position
remain within the disclosed sampled-JavaScript bounds. Both calls verified READY.

Managed native render `5d1769e0a0264a4cb5293adfd9325f2e` preserves full
1920×1080, 30fps, 300 frames and 10s. Against the unchanged source reference,
canonical RGB24 rgb-hybrid at 0.25s (41 samples) improves the motion-blur-only
repair's mean **0.4041256341 → 0.8772623073**, minimum **0.0028555237 →
0.8289203736** (new worst 6.75s). Reader/cards content is restored, but this is
**not a 0.95 fidelity pass**. Native-tick/sampled-JavaScript approximation,
remaining appearance/motion-blur differences, alpha/audio fidelity and formal
Asset proof remain separate limitations. No private source pixels, original
source changes, shader mapping or pre-rendered replacement are published.

## Emitted Point Text bounds and unpainted whitespace — export checkpoint

**Direction:** FX → AEP only; import is unchanged. This follows the embedded
physical-font classifier projection, without changing FX/runtime or native Text
layout mappings. Bounds consume the exact Source Text timeline returned by the
existing writer lowering: unsupported continuous Tracking/FillColor retain their
original omission diagnostics and typed static base; supported Hold Tracking uses
an outline union over the emitted documents. Existing native Transform tracks
still pass through the ordinary hierarchy analyzer. Whitespace-only ASCII Point
Text contributes no painted enclosure but remains an editable native Text layer,
never a flattened replacement or a guessed/padded rectangle. Text-only source
Groups may use the same verified outline projection as mixed sources.

An already-rejected CustomShader stack contributes no native effect to this
bounds-only view. Its precise unsupported diagnostic and original owner remain;
no shader is translated, approximated or supported. Mapped Text raster effects,
box/path/stroke/decorated/non-ASCII/complex shaping, unsupported layout dynamics,
absent or conflicting physical fonts, and existing Group skew/spatial/mask/effect,
clock/near-plane/collapse guards remain excluded. Group810's Drop Shadow remains
a distinct unknown-glyph-bounds omission; it is not repaired by this profile.
The contextual warning identifies the emitted native Source Text timeline,
existing static-base omissions and unpainted whitespace. Font substitution is
not certified by font-derived bounds.

**Native evidence:** AE26.5x89 independent control81b4a29d verifies zero painted
whitespace extent and Arial-BoldMT64 Tracking20→200 width205.71249→263.3125,
unchanged height59.28125. Generated acceptancecda19881 verifies P006/P033/P037
editable Group100 camera occurrences with motion blur and native Mirage Text
Georgia-Bold128/Tracking180/static Source Text. A separately generated original/
edited FX pair preserves native whitespace, reproduces the same57.6px Tracking
width increase and recomputes source canvas207×60→264×60. All five initial numeric
glyph enclosures passed. Failed duplicate-name selection38f5510e was not used as
proof; its cleanup/READY was verified before one corrected ID+name request.
See `proofs/emitted-text-bounds-native.{jsx,json}` for sanitized evidence.

**Proof limits:** three managed operations exhausted the current budget. Candidate
P006/P033/P037 renders and validation_cli after-scores were subsequently
completed by W03 at bb7c4a1c4: canonical RGB24 rgb-hybrid means improved
P006 0.0168294002 → 0.3152625071 (59 samples), P033 0.0054418209 →
0.3191932303 (93 samples), and P037 0.3479564393 → 0.4456838027 (30 samples).
Evidence: W03 `tasks/accuracy-W03/P037-diagnosis/scores.json`; full resolution,
0.25-second cadence, zero model calls. These are measured improvements, not
0.95 acceptance passes or fresh latest-main render proof. This is bounded native acceptance/edit-response, not RGB≥0.95,
full-timeline clipping, alpha/audio, general shaping/font-substitution or formal
independent30fps long-term Asset proof. The measured omission repair is published
Ready under the task-specific reviewer instruction; broader fidelity remains unproved.
CPU regression2350 was RED against the original outline bounds (unpainted
Text896). Targeted2359 is GREEN after the repair, including emitted Tracking,
whitespace, Hold-outline union and raster-effect/font-size/leading negatives.
After rebasing onto mainf57b333e4, gate2364 passed standalone check/clippy/fmt,
ledger validation and1878 normal AE tests (425 existing ignored, none newly
ignored). The earlier old-base ellipse failure does not reproduce on this main;
its unchanged-main target2361 passed. The native acceptance cohort above predates
this rebase; no fresh post-rebase native render/readback or RGB pass is claimed.

## Static Shape Gaussian Blur source enclosure — P046 export correction

**Import:** unchanged. **FX → AE:** the finite Shape source enclosure now adds
one enabled static Gaussian Blur's authored pixel radius before applying a
static translation-only Shape transform (100% scale, no rotation/skew/3D).
Native Shape continuous-rasterization transform/effect ordering is not proved
for other transforms; those profiles keep the existing approximation. The checked control hull, stroke/modifier reach and embedded
physical-font Text outlines remain analysis-only; native editable Shape, Text,
effects, opacity and timing are emitted from the original source. There is no
canvas cap, guessed padding, input pruning or consumer-demand crop certificate.

Disabled/zero blur does not expand the source. Repeat Edge Pixels, explicit
`layerSize`, multiple blur stages, other effects, Shape animators and effect-owned animators retain the existing
content-only approximation and effect-expansion warning; they are not newly
certified by this profile. Gaussian consumer-demand cropping remains unproved.
The finite source enclosure is not a general Gaussian kernel/alpha equivalence
claim. Import/export control mappings are unchanged.

**Evidence:** the static file-icon shadow regression was semantic RED (minimum
`[8,8]` instead of the required `[-4,-4]`) before this correction and GREEN after.
The static and animated analyzers agree on the 12px enclosure; zero/disabled,
128px arithmetic (no cap), and scaled/rotated/skewed/projective/animated
transforms, multiple stages, repeat-edge/explicit-plane/effect-key negative
profiles are tested. An independently authored AE26.5x89
216x214 Shape with 12px Both-dimensions Gaussian Blur, Repeat Edge Pixels off,
reported alpha1 inside, positive alpha at0.5/4.5/8.5px outside, and zero at
11.5/12.5/13.5/16.5/20.5/24.5/32.5px outside. See
`proofs/static-shape-gaussian-bounds.json`. Sparse native samples establish only
this control, not arbitrary kernels, radii or alpha precision. Two earlier
attempts had invalid edge-coordinate sampling and are not edge-support proof.

A fresh full P046 native export on the embedded-outline prerequisite already
removed the old 29,474² file-icon source/OOM black tail. The additional blur
correction changed its 1:1 source292x518→304x518 and retained editable labels,
shadows and media. Both diagnostic full exports completed without OOM, at
1080x1920/30fps with959 decoded frames and zero exact-black frames. Canonical
full-resolution RGB24/rgb-hybrid/.25s/128samples improved
0.20688199930236412→0.208374178386119. These original-case scores are well below
0.95 and are not a fidelity pass. The older OOM artifact's0.170954024302907 is
historical context, not a successful before render. Missing-font substitution
was explicitly permitted and remains uncertified. Fresh generated native
editable-property readback/edit response, formal Asset publication, general
font substitution, alpha and audio fidelity remain unproved; only the independent
control, editable CPU assertions and fresh generated native full-render evidence
above were executed.

## File-footage Anchor source units — import correction

Import now classifies native File footage with Solid and Composition sources for
Anchor XY normalization. Explicit source-relative static values and supported
key tracks use source width/height, never a numeric-range or media-name heuristic.
Absent Anchor leaves keep the existing pixel-center default; Position, Z policy,
media admission, clocks and export are unchanged. No new approximation is added.

Structural validation includes an executed native regression for explicit static
Anchor values and separate absent/default coverage. Supplementary synthetic
static and keyed cases check source-dimension scaling, unchanged key times and
no new Z track. The full suite retains four inherited failures.

This is import-only structural evidence; export and its proof limits are
unchanged. Independent native 30fps reference and long-term Asset publication,
keyed native control proof and fresh-render comparison remain incomplete.
No render-fidelity pass is claimed.

## Animated saturation-only export approximation

**Import:** Premiere saved Lumetri Exposure → Contrast → Saturation mapping is
unchanged: saved 130→60 becomes editable HSV saturation 30→−40 at 0/2513ms.
`human_lumetri_contrast.xml` remains the verbatim pinned component/parameter source
inside a synthetic host; original placement and colour fidelity are not proved.
Native AEP Vibrance imports through its existing mapping, not reconstructed
Lumetri or HueSaturation. No new import transfer is claimed.

**Export:** A saturation-only HueSaturation effect (Hue/Lightness zero, Colorize
off, exactly one enabled supported Saturation keyframe animator and no other
tracks on that effect) uses canonical `ADBE Vibrance`, with Vibrance=0 and current
Saturation values/keys on real scalar `ADBE Vibrance-0002`. Base and knot values
must be within its independently reported −100…100 range; values are never clamped.
Existing scalar clock, dependency, easing and numeric guards still apply.
Static H/S/L and other combinations retain their previous path and its diagnostics;
this does not add composite Channel Range keys or recover omitted mixed Master
animation. Effect order and bypass are retained. No model, schema, shader,
JsScript, frame baking, media flattening or source-state replay is introduced.

The direct scalar identity is a **declared nearest-editable approximation**, not
a measured colour conversion: native Vibrance Saturation is not FX's multiplicative
HSV kernel or native Lumetri. Native transfer, clipping (including Bezier
overshoot), alpha and continuous-render fidelity are unmeasured. Export emits a
contextual approximation diagnostic. No RGB/alpha threshold or oracle is changed.

**Native capability grounding:** independent AE26 catalog target composition282
(`Effect_20`) and its Adobe readback identify scalar Saturation, bounds −100…100,
and `canVaryOverTime=true`. Animated catalog has actual 0→0.10000000149012 keys
at 0/1s. Static AEP SHA256
`7519496eb44f5ebc0eff5c2ad78476afdd738303cfb18449ac4f3596e82070c2`; animated AEP
`71ba020d675ecab5fc3236730b2b6a90a49bab2cb25f779f1e68ccf47c4134a4`.
`animated_saturation_vibrance_leaf_is_native_and_independently_keyed` checks the
pinned source and receipt. This is a scalar-keyability oracle, not a nonzero
transfer/render oracle for the new lowering.

**Structural regressions:**
- `human_lumetri_import_and_edited_linked_export_keep_controls_and_saturation_keys`
  freshly imports the Premiere source, edits Exposure1→2/Contrast25→40 and
  saturation30/−40→45/−65 with times500/1750ms, and checks linked native reimport.
- `animated_saturation_uses_real_scalar_control_with_order_and_bypass` checks
  native Exposure → Vibrance → Brightness/Contrast order, zero Vibrance and bypass.
- `animated_saturation_serializes_current_float_keys_and_interpolation` reads
  fresh AEP bytes and asserts45.25/−65.75 at500/1750ms, bypass, Linear/Hold/Bezier.
- `animated_saturation_does_not_change_static_or_unrelated_hue_controls` retains
  static/mixed/duplicate/out-of-range fallback behavior with diagnosed animation loss.

Focused tests passed. Generated readback is our reader, **not Adobe acceptance**.
No new Adobe open, native current-edit inspection, independent 30fps render/Asset
or RGB/alpha comparison has run for this replacement. These remain explicit proof
prerequisites, not a backend capability failure or a completion claim for all
Lumetri controls. General animated Master Hue/Lightness/Colorize and per-channel
Levels remain outside this bounded change.

### Saturation native-encoder admission and owner retention

Export now checks the selected Saturation track with the existing native
EffectFloat encoder at the actual owning composition rate before retaining
animation. Legal FX easing can still be unrepresentable natively: vertical
Bezier endpoint handles, non-finite derived native speeds, and native key-clock
overflow lose **only the animation**, with the native encoder's reason in a
contextual diagnostic. Current static Saturation, bypass, picture and adjacent
effects survive. Handles and values are not clamped or silently changed.
This is restricted to the new saturation-only Vibrance replacement; ordinary
Vibrance, static/mixed H/S/L, white balance and Vignette mappings are unchanged.

`animated_saturation_unrepresentable_ease_retains_picture_and_siblings` freshly
exports45.25→−65.75 at500/1750ms with Bezier(0,0.1,0.75,0.9); before this
correction it reproduced loss of the entire picture. It also covers derived
speed overflow and checks static base45.25, bypass and adjacent Exposure1/
Contrast25 at24/60fps. `animated_saturation_validates_the_actual_owner_clock`
checks identical legal FX keys retained at24fps but diagnosed/static at240fps
when the final native key tick overflows. Existing Linear/Hold/supported Cubic
serialized-key and linked current-edit regressions remain in place.

Import and native proof limits above are unchanged: generated native readback
is structural evidence, not Adobe acceptance, continuous-render or RGB/alpha proof.
## Singular nonspatial Transform cubic — finite temporal-ease approximation

**FX → AEP implementation:** enabled independent scalar Scale X/Y and Rotation
keys with finite cubic controls and `x1=0,y1!=0` or `x2=1,y2!=1` now receive a
converter-local finite native temporal-ease substitute rather than losing their
owner/subtree. Singular influences become the independently observed native
minimum **0.1%**; ordinary handles stay unchanged. A deterministic convex minimax
fit chooses the singular endpoint-speed gain on 16,384 native-parameter intervals.
A single singular end minimizes that finite-speed family; two singular ends share
one gain. This is not a global optimum over arbitrary native cubics or an exact
mapping. Key count, IDs, times and values stay authored: no added keys, frame
sampling, source edit, generated expression, flattening or runtime/schema change.
Ordinary/nonfinite/out-of-range cubic validation, spatial Position, Path, Color,
effect-parameter and dependency guards remain unchanged. Opacity retains its
separate existing preparation. Script preparation precedes this substitution so
this approximation does not become an input to source-script evaluation.

**Error/proof:** P047 owner `1800107`, ScaleY 100→0 over 0..200ms, cubic
`(.55,0,1,.45)`, becomes `(.55,0,.999,.44499078028119066)` with native incoming
speed `-277504.609859404` percentage points/second. The diagnostic reports
**0.158180394 scale percentage points maximum measured pre-native-clock error**;
a separate 65,536-interval analytic grid measures **0.158180595** (rounded upward),
and the CPU regression checks 262,144 intervals and the mirrored outgoing case.
This measures the continuous easing substitution **before native timing**,
not an all-time native-render/readback error bound.

Managed AE26.5x89 generated-output readback job
`164790a004f04f9e804689e422f7d486` resolves exact comp ID1/name, verifies two Scale
keys at 0/.2s, reads the expected finite speeds/influences and confirms an actual
FX endpoint edit 0→25 in a separately freshly exported project. Original/edited
AEP hashes are `ab614d50945f53aa78d0b0e26d7cfbc9297769857bdf620a729e00a87d0a38fd`
and `eea360155e10b0eacc6b0373baea18ee1bdd0c2e69affd9b51cb549104646eaa`.
The earlier independent native control established the 0.1% influence minimum and
that infinite native speed produces nonfinite interior values. These are numeric
control/acceptance observations, not a published native video oracle.

**Accepted native timing limitation:** 13 requested
sample times per input include the analytic extrema and near-endpoint queries.
Through .1999s plus the exact endpoint, observed deviation is 0.183071538 scale
points (edited 0.137303653). However, at .19998500963859s native readback returns
endpoint0 rather than the continuous source1.22894923484186: maximum over all
requested samples is **1.228949235** (edited0.921711927). Timestamp quantization is
a hypothesis, not separately proven by this call. Those failing samples are not
shifted/dropped. The user explicitly accepted the measured **approximately 1.23
scale-point near-end native discrepancy** instead of omitting the owner; the former
≤0.209 native requirement is superseded. These are measured samples, not a proven
all-time native bound, and must not be described as exact fidelity. Full original P047 native
render/`validation_cli`, alpha/audio/motion-blur and long-term Asset-backed fixture
proof remain incomplete. Original archive bytes and all failed evidence are retained.

`singular_scale_finite_temporal_ease_retains_owner_and_authored_keys` fails before
repair with owner omission (Rust queue2697) and passes after (queue2718, 26 targeted
singular-profile tests). Disabled/ordinary/spatial/selected/dependency and malformed
profile guards have regressions. The source-shaped minimal CPU case and analytic
curve checks are supplementary, not independent Adobe rendering.

**AEP → FX:** implementation/proof unchanged. The substitute can import as its
finite editable cubic, but the original singular curve is not restored. This is
an explicitly authorized export approximation, not bidirectional exact fidelity.

The release converter also published the full unchanged original P047 project at
30fps with the owner `1800107` approximation diagnostic instead of its former
singular-ease omission. This is offline publication, not native render proof.
Final CPU check passed; lint/format/release build passed (queue2764). The normal
suite (queue2752) encountered four existing failures, all reproduced independently
on clean base `0a44080ee` (queue2758): accumulated evaluation owner availability,
playback-remapped group window, and the two vector2 component-easing import tests.
The remaining workspace tests passed (queue2770), with exactly those four baseline
failures explicitly excluded and no newly ignored tests. No new native operation
was run after the user's acceptance decision.
## Inverted inline compound mask — explicit Subtract-pair approximation (P003)

**FX → AE:** one runtime-opaque static inverted Add inline mask containing
exactly two closed, same-winding contours can retain two editable noninverted
Subtract masks. The bounded profile requires strictly convex endpoint polygons,
monotone cubic handles along each chord, at most 33 commands per contour, no
vertex mirror/corner controls, and finite nonnegative expansion/feather. It
retains original coordinates/handles and existing owner-source normalization.
Multiple source masks, opposite winding (possible holes), open/nonconvex paths,
partial opacity, guide geometry and mask-control animators remain rejected.
Import, FX renderer/schema and the existing single-contour mapping are unchanged.

This representation is **approximate**, explicitly authorized after the native
alternatives discriminator. Per-contour expansion/feather does not reproduce
shared whole-union processing. Independent AE26.5x89 lossless RGB24 controls
eliminate the historical 10,000-pixel overlap hole. Hard controls differ at
200/262144 pixels with maximum channel difference 16/255; expansion40/feather15
controls differ at **34,134/262144 pixels, maximum channel difference 2/255**
versus the independent whole-union native-mask control. These are measured
control deviations, not a bound for all paths/edits, source-renderer equivalence,
alpha fidelity or relaxed canonical RGB thresholds. The alternate vector matte
had a larger soft mismatch and was not selected. Native jobs
`a5f44389dbdd491d9c968b61a988a9c2` (failed readback),
`alternative-native-v2` corrected readback and
`9251bb458b12427fb1984cbd6c394e35` render retain the independent evidence;
no failing readback is counted as a pass.

Executable semantic regression `inverted_inline_compound_subtract_pair` was
RED queue2501 (zero masks instead of two), then GREEN queue2513/2517; negative
profile and geometry guards pass. Earlier queue2482/2496 were test-authoring
errors, not semantic RED. Final queue2534 passed formatting, workspace check,
clippy and support-ledger validation; full aftereffects_file gate passed
1928 tests with zero failures and 424 existing ignored tests. This is CPU
validation, not missing native proof. Immutable release2503 establishes fresh before;
release2523 establishes after. Normal unchanged private P003 export no longer
omits Image4/mask401 and emits the explicit approximation diagnostic. No private
source geometry/assets are published here.

**Ready per explicit owner authorization, with retained proof gaps:** successful
fresh normal P003 exports supersede the earlier renamed-package/relink failures.
Main `69d1048cc1d3ddc379b4302bc20a63414937be24` versus PR
`54767fdc717f7704d7af184e0300375deec0fa81`, using unchanged source and
MP4/30fps/native audio/font substitution allowed, improved canonical full-resolution
RGB-hybrid mean 0.793057453699873 → 0.807409192783812 and minimum
0.411642116571539 → 0.488309509833470 over 49 samples each (0.25s interval,
max50, canonical RGB24). Frame counts/PTS, canvas, clocks and duration match.
This is measured improvement, not a .99 or .95 fidelity pass. Immutable builds
2525/2577 and export/score queues2581/2582 retain receipts; native jobs
`baa57c70956b464fb402aae6a390edfe` / `99db503555a842cc8c7c422918db6d5f`
completed with cleanup/READY. Before movie SHA256
`7534debf6bb4947fc61594522080be04a99a71b2c694143d51a061126673e19f`;
after `f7da90f13fc7ad82c44714bd8f55c70bc2e4658124f620e67ba7a8b3bfc87837`.
No guard bypass, backend changes, source/time edits or threshold relaxation.

Generated Rect original/source-edited/native-edited/save-reopen acceptance remains
unproven. Initial exact-double coordinate assertion failed; two authorized bounded
fixed-point/f32 probes failed native `invalid numeric result (divide by zero?)`,
last localized to original read (`c46f8dcc1bb346e89d2e3ba26f804320` and
`bebd9ffa55784acb9b9bbacd487a3576`, both READY verified). Numeric/editability
acceptance, rendered edit response, alpha/audio fidelity and general topology remain
unmeasured. Fresh RGB scores do not establish these; owner authorization explicitly
permits Ready based on the P003 improvement with these limitations disclosed.

## Set Matte premultiplied Red — bounded editable graph

**Import (AE → editable FX):** a pinned static `ADBE Set Matte3` native
Red profile (Use For Matte1, Invert0, If Layer Sizes Differ1, Composite
Matte with Original1, Premultiply Matte Layer1) now uses the existing bounded
2D shape/precomposition provider machinery. The complete saved default table
and any explicit channel must agree. Unknown channels/controls, animated
selection, expressions, mixed active effect order, unsafe dependencies and
unsupported hosts retain contextual omissions and the original picture.
Existing RGB Invert and Alpha/Luma profiles are unchanged; this does not add
Invert Alpha or pretend ShiftChannels has an editable alpha-output selector.

The sampled provider is an independent editable copy, muted for audio. A new
matte-only Group composites it over an opaque black Rect, then applies public
ShiftChannels (own Red, Green/Blue Full Off) and HueSaturation(Saturation−100).
The current HSV shader emits gray equal to the retained premultiplied Red.
Black backing makes the matte opaque: the renderer's Luma mask multiplies
RGB by alpha, so omitting that backing would incorrectly square source alpha.
The picture's RGB/effects are untouched and its alpha is gated by sampled
`R × provider-alpha`. This is **layer-local** coverage: ordinary AE import
still materializes its preview background as an opaque FX canvas and diagnoses
that separate alpha loss; this mapping does not change that inherited policy.
The consumer and matte helper are siblings inside an identity Group, so the native exporter can keep their reference within one
composition. Source identity/content, clocks and placement remain editable;
the provider copy is **not live-linked** to edits on its original occurrence.

Native effect bypass creates no replacement or helper. To disable an already
imported graph, clear the consumer's trackMatte and hide/remove its dedicated
helper, **not the picture or enclosing Group**; both import bypass and this
current graph edit have fresh-export tests. This is an ordinary multi-node
editable construction, not a new effect toggle or name-keyed reconstruction.

**Export (edited FX → AEP):** current graph controls use existing native
Shift Channels, Hue/Saturation, black vector backing and Luma track matte.
Editing the projection to own Green/Red Off/Blue Off writes native selectors
10/3/10; original Set Matte bytes are never replayed. Native Hue/Saturation's
grayscale transfer differs from the current FX HSV kernel, so **export matte
strength/alpha can differ substantially**. Existing exporter diagnostics retain
that transfer limitation; this is editable approximation, not restored Set Matte.
Composition-space sampling, source/target size differences, transformation,
clipping and edge coverage can also diverge from native pre-transform/stretch
semantics. No schema/engine/renderer changes, shaders, scripts in converted FX,
frame baking or flattened replacement media were added.

**Evidence:** [native fixture/provenance](../crates/aftereffects_file/tests/fixtures/effects/set_matte_red.provenance.json)
pins AEP SHA256`f14eef956f06ce4072d2b273f39db2e6ee57da54075e4bcb5a8f51031340dffc`,
composition20, target47/provider32, plus supporting compositions1/33. It was
independently authored through managed headless-adobe JSX on AE26.5x89.
Distinct RGB cells with opacity100/50/0 distinguish channel selection and
premultiplication. Detailed native render receipts, hashes, samples and READY
results are retained outside product Git in the private oracle evidence.
Native source characterization is **not a converter RGB/alpha comparison**.
No reference was replaced or threshold changed.

Executable assertions:
- `native_set_matte_red_import_keeps_public_editable_channel_projection`: real
  source/defaults, distinct colours, nonuniform opacity, public controls, black
  backing, Luma binding and ownership.
- `native_set_matte_red_bypass_retains_original_picture`: explicitly supplementary
  native-record bypass mutation and fresh export; not native-authored bypass proof.
- `native_set_matte_red_current_green_edit_exports_native_controls`: current
  edited graph and serialized native selectors/Luma reference.
- `native_set_matte_red_edited_graph_bypass_keeps_picture`: clear binding and
  suppress only the helper, preserving the original picture through fresh export.

Generated export readback is our reader. **Adobe acceptance of edited exports,
Jerboa/native RGB/alpha comparison,30fps MP4 and long-term Asset publication
remain incomplete.** The linked Premiere Green/Blue extension is recorded below;
this Red-only source does not establish that endpoint. Premiere clip-native
Set Matte remains unsupported; per-channel Levels is separate. Prepared older Offset/Lumetri packages remain unrun.

### Mixed Set Matte chain scope preservation

For a chain containing the pinned Red projection, every gate now owns an
identity consumer beside its provider, inside an unmasked identity grouping
wrapper. Each consumer contains the entire preceding picture/gated subtree;
no existing binding is replaced. This keeps Alpha/Luma and Red references in
their own native sibling scopes without moving masks onto transformed or
effected pictures. Added consumers use fresh IDs and count toward nesting
budgets. Pure Alpha/Luma and resolved-native-Alpha paths remain unchanged.

`native_set_matte_alpha_then_red_keeps_both_gates` supplements the pinned native
fixture with an independent provider identity and a preceding Alpha selection.
It asserts both bindings, identity carrier clocks/transforms, retained picture
properties/paint, and fresh native same-composition references with suppressed
provider display. This is supplementary graph/export structure evidence, not
an independently Adobe-authored stack or edited-output fidelity measurement.
Detailed regression receipts and scope inventories remain outside product Git.

## Editable warp approximations (JRB-2011)

The independent [static warp fixture](../crates/aftereffects_file/tests/fixtures/effects/warp_static.aep)
(source SHA `533103d97b25dfec382cfc2e30bebb2a5d0a1785449f15d3012e668cefa7d25d`)
pins three 320×180, 2s, 30fps sources saved/reopened in AE26.5x89. Revision 2's
background obscured the grid: its controls are evidence, its movies are **not a
discriminating oracle**. No source/reference is overwritten or relabeled as a
fidelity pass. Revision 3's independent Mirror render confirms the retained
half and reflection-normal convention; converted RGB/alpha remains unmeasured.

| Feature / direction | Current editable representation | Deviations, bounds and evidence |
|---|---|---|
| Mirror → FX | One enabled static `ADBE Mirror` becomes retained and reflected copies of its pre-effect Group/Rect/Shape content. For normal `n=(cos(angle),sin(angle))`, retain `dot(n,p-center)<=0`; reflection is `R(2*angle) diag(-1,1)` about Center. Half-plane path masks clip both copies before reflection; another mask clips the recombined finite canvas. Prefix effects are copied before clipping; suffix effects, owner spatial transform and opacity remain outside once. | Comp1/owner13: Center[144,84], Angle30. Explicit bypass does nothing. Source keys on supported copied layer/effect properties get distinct target/key IDs through the existing graph copier; edits to the copies become independent. Native Mirror control keys/live expressions, multiple Mirrors, nonvector content, 3D/collapsed precomposition/preserve-transparency, nonidentity occurrence clocks, masks/mattes in the copied source and mixed graph-staged native effects are diagnosed rather than risking invalid references/stages. Malformed controls/ID/depth/budget failure preserve owner/siblings. Ordinary native Shape continuous-rasterization flags are admitted (not confused with collapsed precompositions). The exact raw occurrence's built-in Compositing Options are checked separately: absent/default options and empty Effect Mask Parade retain full reflection; static zero `ADBE Effect Mask Opacity` is identity. Explicit nonzero opacity, live/keyed opacity, nonempty per-effect masks and unknown options decline with control-specific diagnostics, retaining owner/Blur siblings; no full-strength substitution. Explicit nonzero built-in opacity storage/gain is not calibrated from the catalog UI readback alone. Native raster bounds, transformed-owner projection and seam antialiasing remain approximate. No flip/tile stand-in. |
| FX Mirror graph → AEP | Existing editable Group, signed scale/rotation, vector and mask writers author the **current** graph. No Mirror effect name or saved native bytes are replayed. | Nested static half-plane masks retain ordinary native PathMasks rather than being substituted with Radial Wipe: that effect's importer rejects transformed/differently sized source planes and would lose clipping on reopening. The geometric rule is independent of names/IDs; root and animated Radial Wipe recognition are unchanged. Edited reflection transform and both source copies remain editable, not a reconstituted Mirror UI control. Native edited-export acceptance/RGB/alpha comparison remains unmeasured. |
| Spherize → FX | Comp14/owner26 Radius70/Center[144,84] becomes Bulge radii `R/W,R/H`, center `x/W,y/H`, positive height `pi-2`. | There is no native strength knob. The chosen height matches the central inverse-sampling slope `2/pi` of a normalized asin sphere model to Bulge's `1-height/pi`; **not calibrated Adobe projection**. Radius is finite, nonnegative and strictly below the destination short edge, protecting Bulge's square-root domain. Zero radius becomes zero strength with nonzero radii (identity, not 0/0). Keyed/live-expression controls are diagnosed/omitted; disabled expression text retains static controls. Destination Group-plane normalization, falloff and edge alpha are approximations. |
| FX Spherize approximation / Bulge → AEP | Existing Bulge writer uses current edited signed height, independent radii, center, pinning, bypass and supported affine keys. | Exports **Bulge**, not restored Spherize. Existing Bulge import/export otherwise unchanged: taper/AA and kernel/plane fidelity remain uncalibrated. A current edited negative-height/bypassed export is tested independently of native source values. |
| Static Wave / Wave Warp → FX | Comp27/owner39 verifies the actual discovered native `ADBE Wave Warp`: Height8, Width50, Direction90, Speed0, Phase30. Existing height, reciprocal wavelength, direction and phase mapping is retained. Zero speed is recognized as static. | No separate same-named Wave plugin is invented. Phase degrees→radians is affine; wavelength animation remains static-only. Nonzero implicit native speed, Wave Type, pinning and AA are not represented. The FX directional sine uses normalized axes; native pixel displacement on nonsquare/transformed planes is not equivalent. Invalid/nonpositive wavelength is rejected, not replaced by an unrelated default. |
| Ripple → FX | Existing amplitude/center/radian phase and static reciprocal wavelength remain; zero speed is recognized. | Distinct radial sine, not Wave Warp. Existing amplitude limiter still bounds amplitude×frequency to 1.25, approximating missing Radius confinement but not preventing all folds. Nonzero speed, radius and conversion mode remain omitted. Native catalog provides independent parameter identities; existing controls/limiter are not rewritten. |
| FX Ripple / Wave Warp → AEP | Current static values and supported phase keys are written. Omitted legal frequency fields use effective FX defaults (Ripple30 / Wave6) before validation and reciprocal export, never donor widths20/40px. Native **Wave Speed explicitly 0** (`Ripple-0004`, `Wave Warp-0005`) instead of donor default1. Phase keys are the only exported clock. | Reciprocal wavelength animation is diagnosed, not fitted. Nonpositive/nonfinite frequency or plane omits only the unsafe effect. Native kernels/modes/pinning/radius confinement and UV projection remain approximate. Bypass and adjacent effects are retained. |
| FX Fisheye → Premiere native Lens | Centered finite amount0..100 on the existing identity canvas Lens host uses the existing seven-control native Lens writer. Fit FX tangent inverse sampling at horizontal radius0.25 to polynomial `r*(1+k*r²)`, clamp `k` to [-1,0], then existing native Curvature=-100k. | A **nearest static approximation**, not Fisheye equivalence. Uses actual existing Premiere Lens control evidence, not invented AE Optics/FOV slots. Tangent central derivative, circular aspect correction, far-corner edge preservation and native gain/fill/alpha are not preserved/calibrated; strong values saturate. Fisheye keys flatten to current static controls with a diagnostic because fitting is nonlinear. Offcenter/unsafe/nonidentity hosts retain siblings and follow normal omission/fallback behavior. Successful approximation diagnostics do not trigger replacement by an empty linked-AE fallback. |
| Native Lens → FX | Existing Premiere Lens import returns editable `lensDistortion`, including supported Curvature keys. | Does not infer or restore Fisheye from native Lens; this roundtrip is intentionally noninvertible. Direct AE Fisheye/Lens mapping is not added by this Premiere-native route. See Premiere Lens ledger for its unchanged source proof and bounds. |

Focused executable contracts: `warp_static_spherize_native_import_is_editable_bulge`,
`warp_spherize_disabled_expression_import_keeps_static_owner`,
`warp_static_spherize_rejects_unsafe_geometry_and_handles_zero_radius`,
`warp_static_spherize_current_signed_bulge_export`,
`warp_static_mirror_native_half_plane_and_finite_support`,
`warp_nested_static_half_plane_keeps_native_masks` (supplementary generic FX regression),
`warp_mirror_order_bypass_and_unsafe_control_preserve_owner_and_blurs`,
`warp_static_wave_native_phase_and_current_edited_export_have_no_implicit_clock`,
`warp_static_and_keyed_phase_export_never_adds_native_speed`,
`warp_native_invalid_wavelength_is_rejected_not_replaced_by_default`,
`warp_invalid_frequency_omits_only_the_unsafe_effect`,
`warp_omitted_frequency_exports_effective_fx_defaults_not_donor_widths`, Premiere's
`warp_fisheye_current_native_lens_fit_and_unsafe_center_keep_siblings`, and CLI
`warp_public_native_graphs_keep_current_edits_through_aep_package` /
`warp_public_fisheye_keeps_native_lens_approximation_instead_of_empty_ae_fallback`.
These are structural/current-edit/public-route contracts, not native-render
comparisons; exact-candidate execution and independent review are reported
separately. No new renderer/model/schema/JS/opaque source-replay route is used.

The public Mirror edit contract follows the reopened named reflection branch to
its editable pre-Mirror source through normalized containers, composes the linear
transform and checks R(80°)diag(-1,1), explicitly rejecting the unchanged60°
matrix. Clipping checks follow each retained/reflected branch to its actual
pre-Mirror content and resolve reopened mask-guide references on that path,
rather than accepting an unrelated masked descendant or only the outer crop.
The common-ancestor mask must resolve to the active Add/non-inverted/opacity-one
320×180 canvas rectangle after carrier coordinate normalization. Removing only
that mask, with both half-plane masks intact, must fail the finite-canvas check. Disabled/live Spherize
expression test variants edit the bounded instance `tdbs/tdb4` run, skipping
the same-named `parT/pard` declaration; the pinned source bytes are unchanged.


## Premiere-linked Green/Blue channel mattes

[Native fixture and provenance](../crates/premiere_file/tests/fixtures/premiere_channel_matte/provenance.json)
pin a genuinely Premiere-saved Dynamic Link occurrence (sequence
`715705e9-74fa-46ba-a637-f2f8bfddd5e3`), not a generated Premiere wrapper.
Its Set Matte is **inside the linked AEP**, not a Premiere clip effect. The
Premiere source is Green composition34/consumer47/provider46; the same native
AEP independently contains Blue composition48/consumer61/provider60. Both were
saved/reopened with their selected channel. Provider cells have different RGB
values and opacity100/50/0. A real video with audio is trimmed from500–1500ms
onto0–1000ms in Premiere.

| Direction | Mapping and executable evidence | Limits |
|---|---|---|
| Premiere linked AEP → editable FX | Native Set Matte3 RGB popup values1/2/3 retain only the selected own channel through Shift Channels, after composing the sampled source over opaque black; desaturation and Luma gating use existing terms. Alpha4/Luminance5 remain direct gates. The previous sparse2=Luma interpretation was incorrect for the saved Green source. `premiere_saved_channel_matte_import_retains_green_projection` pins the real Premiere entry. | No arbitrary channel routing, inversion, general effect stacks or new matte engine. Existing source-stage, eligibility, unknown-control, default-profile, dependency and budget restrictions remain. Composition-space sampling approximates native pre-transform/stretch semantics; provider copies are independently editable, not linked. |
| Current FX → fresh Premiere + linked AEP → FX | `premiere_saved_channel_matte_trimmed_video_current_edit_and_bypass_export` edits Green to Blue, renames graph labels, then separately removes only the matte binding/helper. It checks emitted own/off native Shift Channels controls, same-composition Luma references, no replayed Set Matte channel, fresh reimport, original packaged video bytes and video/audio source clocks at two interior timeline samples. Bypass has no projection or Luma gate. | Native records are inspected offline, not opened in Adobe. Original native Set Matte UI is not reconstructed. Existing Shift Channels/Hue-Saturation/native Luma transfer and alpha differences remain unmeasured. The clock checks allow less than1ms for the existing native boundary rounding; they are not audio waveform or rendered-video comparisons. |
| Mixed gates | `premiere_linked_green_blue_sources_keep_mixed_gate_scopes` checks both native channel identities and supplementary Alpha/Luma/RGB stacks. Every gate in an RGB-containing chain keeps a fresh identity consumer and sibling provider; pure Alpha/Luma handling and writer scope validation are unchanged. | The mixed stacks are native-record mutations, not independently authored stacks. The unmodified native Blue composition is parser evidence; the genuine Premiere occurrence selects Green. |

Source author/save/reopen, actual Premiere import, current-edit export and
reimport are distinct evidence. Edited-output Adobe acceptance, independent
30fps references/long-term Assets, native RGB/alpha equality and rendered
trim/audio synchronization remain **unmeasured/incomplete**, not fidelity passes.
No canonical FX/editor/renderer changes, generated JS, flattened media or saved
source-byte replay are introduced. Unsupported profiles retain contextual
omissions and otherwise convertible content under the existing rules.


Source-stage matte samples normalize only the sampled occurrence's outer blend
to Normal, alongside the existing owner-opacity reset. The original composition
occurrence and blends inside its source composition are retained; AllEffects
sampling is unchanged. Otherwise an admitted Multiply occurrence would multiply
its channel sample by the projection's opaque black backing and erase coverage.
`native_rgb_source_sample_ignores_only_provider_occurrence_blend` derives Multiply
occurrences and an inner Screen layer from the pinned Green/Blue source. It checks
source-graph invariance, unchanged original/inner blends, and unchanged AllEffects
behavior. This is executed structural evidence using the established blend
semantics, not executed pixel equality or an independently saved blend variant.

## Premiere clip Invert Alpha on a linked input

The `premiere_linked_alpha` Premiere-native fixture applies **Premiere
AE.ADBE Invert Channel15/Blend0** to an AEP-composition clip; there is no Invert
inside the AEP. Saved sequence `92d9c159-77c9-442f-8d46-17f128919537` places
source500–1500ms at timeline1000–2000ms. The source has an RGBA image and a
picture-disabled, audio-enabled video layer. Its nonblack AE preview background
is not paint: the existing Dynamic Link picture import purpose already preserves
source transparency and mutes picture-source audio.

Import previously retained picture/audio but omitted Alpha15 on this Group owner.
The bounded ordinary Alpha replacement now admits the static linked-owner profile
through a second independently allocated linked-picture import. Both imported
subtrees retain their source clocks, transforms, inner blends and internal
references; only their outer parent changes. Existing central ID allocation and
linked-source muting are reused, not a root-ID-only clone. Independent Premiere
audio is not imported twice. Identity-clock carrier/sample siblings are limited
to the occurrence window. Disabled Premiere picture creates no replacement
backing; a live owner with hidden source artwork still legitimately inverts zero
source coverage inside its canvas. Unsupported owner masks, source effects,
non-Normal transfer, animated controls, non-unit playback and authored remaps
remain diagnosed omissions retaining original picture/audio.
For an enabled Alpha15 effect with a valid per-effect mask on a linked input,
normal native mask/ownership decoding runs first; the entire unsupported effect
and its mask are then omitted before the reader's physical-host admission. This
is effect-only fallback, not unmasked Alpha or masked-Alpha support. Other effect
masks, RGB0, ordinary media and bypassed-mask safeguards keep their existing rules.
`premiere_linked_alpha_valid_effect_mask_retains_picture_audio_and_sibling`
uses an explicitly derived G4 mask on the pinned linked picture chain, including
G4's deduplicated binary payloads. A same-mask RGB control proves mask decoding;
the Alpha case equals the nonempty original-picture/RGB-sibling baseline, with
source bytes, picture/audio lifetime and no inverse carrier. This is full-entry
structural regression evidence, not a separately Adobe-authored masked case.

Fresh linked export needs three bounded native-source normalizations: hidden
media contributes no static paint bounds (matching animated bounds), known
transparent hidden-artwork subtrees retain a transparent native canvas rather
than dropping their masked owner and exposing its guide, and still-image ranges
that cannot fit the native clock are intersected with their enclosing finite
source-composition interval. Representable authored still tails, video clocks,
unknown geometry and native reference validation are unchanged.

`premiere_linked_alpha_native_input_retains_inverse_coverage` exercises the genuine
Premiere entry. `premiere_linked_alpha_current_edit_bypass_and_lifetime` renames
labels, changes the current matte to Alpha, separately removes the entire carrier
and sample for real bypass, and hides source artwork with the owner still live.
Fresh AEP records check Alpha/AlphaInverted selectors and consumed same-scope
providers; reimport checks original PNG bytes, no exposed canvas backing on
bypass, and single image/audio source clocks outside/start/interior/end of the
occurrence. `premiere_linked_alpha_disabled_picture_retains_independent_audio`
uses an explicitly supplementary native-record mute mutation and verifies fresh
export/reimport keeps sound live without inverted backing. These are structural
and native-record assertions, not rendered lifetime/alpha/audio equality.

Partial-alpha RGB darkening and unavailable hidden RGB retain the existing Alpha
replacement approximation. Native source author/save/reopen is independent source
evidence; generated controls are inspected offline, not Adobe-accepted output.
Edited-output Adobe acceptance, independent30fps references/Assets and native
RGB/alpha/audio comparison remain unmeasured. This is not arbitrary channel
routing, standalone AEP Channel16 support, or general linked/groupAlpha coverage.
