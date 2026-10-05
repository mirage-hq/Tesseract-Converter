# Deterministic AE numeric expression import — bounded phases

## Phase 2: exact Shape Scale/key/helper slice

Bounded named nonrecursive helpers, return/switch/break/update and recoverable
key-probing try/catch are admitted. Explicit `throw` statements are rejected
throughout admitted programs, including unreachable helper branches: exception
payloads must not transport live AE receivers or API functions into untyped catch
bindings. Native recoverable Key/range probing remains supported. `numKeys`, `key` and `nearestKey` expose immutable
native key records; fatal dependency failures remain sticky across caught errors.
Shape Scale identities resolve omitted-default native indices and indexed Contents
occurrences separately. Existing generated Shape groups receive editable ScaleX/Y
keys through the existing observation-constrained fitter. AE and FX Scale are both
percentages (100 = identity); no ratio normalization is applied.

Pinned Lemon source SHA256 `4f91a7b24be96ed4924d68614aef8b40e6d1fad3d4274174eb06714a92488f84`,
composition15368/owner795192752/native VectorGroup1 Scale: fresh evaluation matches
20,002 vectors with maximum native-unit error1.7195134205394424e-12. Independent
serialized sampling of both the original consumer and matte-provider copy passes
in two separate bounded native windows:10,001 vectors/1,438 total keys/error
9.550340841074645e-9, and10,001 vectors/4 keys/error0. Limit remains1e-8 at unchanged
native timestamps/values. Test: `expression_lemon_single_shape_helper_matches_native` (removed; historical).
Licensed source/captures remain private. This gains **one independently measured
Shape Scale target**, not all lexical helper/Shape occurrences. The supplied native
clock is retained as proof input, with values replaced by fresh evaluator results;
it does not imply default composition-FPS import acquires that denser clock.

Bezier-keyed dependency `valueAtTime`, builtin `ease`/`smooth`, other Shape fields,
arbitrary helper/API reads and recursion remain fail-closed unless separately
proved. Case06 WigglePosition frequency's six Bezier keys remain a dependency
blocker; its separate Geometry2 precomp-owner/evaluated-effect-point lowering gate
is not solved by key support. Random behavior remains the separately documented
merged approximation, not this exact deterministic claim. No new Adobe execution,
rendered/font/alpha fidelity or FX→AEP expression-restoration proof is claimed.

## Phase 1 baseline

## Implemented import scope

The converter evaluates a closed numeric subset using Boa through the existing
`fx_keyframe_bake::script::ScriptRuntime`. Whole-program AST admission checks dead
branches too. Supported direct layer Transform/ordinary Effect properties can
reference unique layer/composition/effect controls, `value`, `time`, `valueAtTime`,
Linear/Hold authored keys, deterministic Math, linear/clamp, vector arithmetic,
length/normalize, posterizeTime and bounded loop helpers. Unknown APIs, mutable API
aliases, Property truthiness/strict identity, unsupported native interpolation,
invalid clocks/values, ambiguous lookups and dependency cycles fail closed.
Phase 2 expands only the bounded helper/key/Shape Scale subset above. Other Shape
properties and per-character Text selectors remain unsupported. Random APIs use
the separately merged opt-in approximation policy, not deterministic native equality.

Native input keys are no longer limited to Linear/Hold. Bezier temporal ease,
spatial Position paths, separated Position and 3D X/Y Rotation/Orientation enter
the evaluator through the keyed import's own editable mapping
(`structure_document::editable_native_keys`: `easing_for_key`,
`straight_spatial_easing_for_key`, `spatial_position::prepare`). The shim evaluates
that FX cubic easing with the converter's 24-step solver. See the
[support ledger](after-effects-support.md#expression-input-keys-eased-spatial-and-separated-native-properties--import-increment)
for measured error against Adobe pre-expression samples.

Evaluation occurs **after Essential overrides**, in a fresh occurrence-local
scope. Source-only captures are not reused for overridden occurrences. Errors do
not discard convertible siblings. Fallback remains authored/stored content with
contextual diagnostics; that fallback is not expression fidelity.

Composition-FPS observations flow through `ExpressionSamples` and the existing
expression-to-scalar-key fitter. Original timestamps are checked against fitted
curves. Observations are marked internally, never relabeled as native sidecars.
Each occurrence is bounded to 250,000 sampled vectors; the dense fitting grid is
also limited to 250,000 milliseconds. VM limits do not provide
OS/process isolation. The AST profile is admission, not an Adobe sandbox.
No FX/schema/renderer change, duplicate fitter, generated JsScript, shader,
flattened media or hidden-source replay is introduced.

## Evidence and limitations

Fresh Boa evaluation of the checked-in sampled-Position native source matches all
2,001 independently Adobe-captured v2 vectors at their actual native timestamps:
maximum absolute error **1.1795009413617663e-12** (comparison limit1e-8). Source
SHA `ff5d9d59e13079dffa0eae8153bcfbf3ec01c57227267a0a256fc7c638c8abbd`, sidecar SHA
`4000185e2e5d11cc3fc532a7ec2df8cf7ab18c5b795489de693d56db214a794f`.
This exercises a layer/effect controller alias, Math.sin, time, vector arithmetic
and oracle-proven XY-to-XYZ normalization for a 2D Position destination.
Test: `expression_eval::tests::expression_evaluator_matches_pinned_adobe_position_oracle`.
Before normalization, this independently sourced regression failed on dimensions.

### Implemented opt-in actual-time observation constraints

`fit_scalar_curve_with_observations` extends the **existing** neutral fitter.
Original fractional offsets and affine-mapped values constrain cubic construction,
segment acceptance (including adjacent1ms spans) and key minimization. The two
value controls solve independent hard observations; a single observation constrains
the previous least-squares choice. Remaining observations must independently pass
the absolute numerical criterion. Soft integer probes retain the unchanged0.001
FX-unit cap. The hard criterion is1e-8 in native units, scaled into each FX scalar;
original times/values are never rounded or relabeled. Legacy `fit_scalar_curve`
and an empty opt-in observation set retain identical behavior, tested directly.

Inconsistent constraints or an equal-endpoint pulse that cannot fit an indivisible
1ms interval return `ObservationFitError::Unrepresentable`, identifying the interval
and its sampled integer endpoints. The caller keeps contextual omission/static
fallback, not inaccurate keys. No second fitter, direct per-observation key dump,
dense fallback, new time schema, renderer change or JsScript was introduced.
Subdivision and minimization remain the existing adaptive fitting engine; output
can still be key-heavy. Integer endpoint values are not silently changed.

The original failing fractional source-frame regression now passes through fresh
conversion and serialized editable-track sampling. Both proof grids are independent:

| Fresh input / persisted-key check | Keys (Position X+Y total) | Maximum native-unit error |
| --- | ---: | ---: |
|49 source-frame vectors (default occurrence evaluation)|166|1.4210854715202004e-14|
|2001 actual native-time vectors, freshly evaluated by Boa before lowering|1146|1.1937117960769683e-12|

The2001-vector path retains only oracle identity/clock metadata as input and replaces
all sample values with fresh Boa results, then compares serialized keys against the
independent native values. Static zero Z is separately asserted for this2D fixture.
Test: `expression_eval::tests::expression_actual_time_source_frames_and_native_vectors_lower_independently`.
The default49-frame track's error at the2001 **unsampled** native times is
**0.29683671299235925**: it is NOT the same track as the independently constrained
2001-vector result, and default import does not acquire this denser native clock.
No claim of exact continuous sine, arbitrary-time JS behavior or pixel fidelity is
made. A finite fixed-time cubic/Linear/Hold track cannot exactly reproduce general
continuous sine. The earlier unsuccessful tighter-integer-fit experiment remains
removed; this implementation constrains actual observations rather than retries.

Latest merged-main CPU validation: job2908 passed **83 expression tests,0 failures,
4 ignored**, including the portable unused-Shape regression, and **27 fitter tests,
0 failures**. Native Lemon job2903 independently passed **1 test,0 failures** over
all four bounded batches. Job2910 workspace check passed after refreshing stale
shared-target metadata; final clippy/format results are recorded on the PR. The
Position49/2001 errors/key counts above remained unchanged. No current-head full
suite or rendered-fidelity result is claimed.

Pre-main CPU validation: workspace check/clippy/fmt and27 fitter tests passed in
job2879. Its single expression failure was a stale Bold01 diagnostic-text assertion,
corrected without weakening editable-color assertions. Corrected job2883 passed
**78 expression tests,0 failures,3 ignored** with Bold01 staged and both lowering
grids checked. These are numerical/editable CPU results, not native-render proof.

Historical main6819ffa38376be148ea9c320ea2096705c4a5ab9 passed an isolated converter workspace
check in job2814. The history-preserving merge5f1113a4d passed workspace check and
**77 expression tests,0 failures,3 ignored** in job2815. Both ledger sections were
preserved; the sole support-ledger conflict is resolved. Final standalone
clippy/format status is reported on the PR for the resulting exact head.
The earlier pre-merge job2791 passed check/clippy/fmt and75 expression tests.
Full offline test job2750 passed1846 tests but failed
`export_document::hierarchy::tests::static_ellipse_bounds_keep_masks_ambiguous_geometry_and_overflow_guarded`.
An exact clean-base rerun at9e1498af6 reproduced the same failure in job2757;
that historical export result is not hidden or marked passing. The full suite has
not been rerun on the merged head, so no current-head full-suite result is claimed. Hash-pinned
Bold01 source `2c01c8802df8c38937acdded954a1608fe100f2d76f161d2f03d0cc6c82a326e`,
composition1, tests ten Fill-color consumers/layers96–105 through fresh evaluation
and editable scalar-key lowering. Independent native-byte inspection identifies
black RGBA; this is **not Adobe equality**. A cloned Essential override to blue
checks occurrence isolation against stale source samples; it is supplementary,
not an independently Adobe-authored override fixture. Exact test:
`structure_document::animation::expressions::tests::pinned_bold01_expression_colors_are_fresh_editable_scalar_keys`
(removed; historical). The licensed-source test was removed; recorded results
are historical evidence only (no longer executable).
Licensed sources and program text are not published.

One authorized native capture was attempted with the typed headless-adobe API,
queue2743, exact runner72caf3f3b915e8636f726bb391b5ed7edda01669,
request `w11-bold01-phase1-oracle-20261004-v1`, diagnostic font substitution enabled.
It failed: **temporary native clock did not restore original project item
identities**. The worker returned verified READY; no sidecar was published and no
native sample/equality result is claimed. Queue2742 was only a rejected queue
argument envelope and did not execute Adobe. No automatic retry occurred.
Bold01 native oracle equality, fresh independent rendering, alpha and exact-font
fidelity remain **unproved**. The separate sampled-Position numerical oracle match
above does not stand in for those proofs. Diagnostic font substitution cannot prove exact typography.
This PR is partial progress, not completion of Adobe feature proof.

Lemon (envato2 case12) remains deferred to the Shape evaluator follow-up. The new
numeric-only native oracle covers41 targets over0–20s in four bounded batches
(820082 vectors). All four indexed JSON-payload hashes were verified, and each
sidecar's properties exactly match its native receipt. The index hashes refer to
compact JSON payload serialization, not the pretty files' different byte hashes;
no values/times or oracle were changed. Inventory:38 Shape targets (18 Vector Scale,
6 Rect Size,6 Roundness,2 Stroke Color,3 Fill Color,3 Group Opacity) and3 ordinary
Opacity targets. Merged main3f96a125 adds validated version3 Shape identities;
it does **not** register Shape expression destinations in this import pipeline.
Every reachable captured Shape target now produces an explicit contextual
`captured native Shape expression target ... has no editable expression mapping;
authored fallback retained` diagnostic instead of silently discarding the capture.
This is missing converter mapping, not an absent FX rendering capability.

Executed test `expression_eval::tests::expression_lemon_native_batches_report_exact_or_diagnosed_targets` (removed; historical)
(job2903:1 passed,0 failed) reads the pinned source, validates each bounded batch,
compares typed properties with its native receipt **without tolerance**, freshly
converts the full selected composition and samples serialized editable tracks.
Each batch remains below250000 vectors; no merged oversized sidecar was created.
Across82 target-window records /41 unique targets /820082 vectors:
- **3 ordinary Opacity targets**, owners795192753–795192755, lower to one persisted
  scalar key each; error **0.0** at all original native observations in both windows.
- **38 Shape targets are diagnosed**, not restored, in both windows. This includes
  all **12 data-wedge Scale targets**, owners15517–15528, and all **6 background
  Scale targets** across owners795192752–795192755. **No Shape Scale target is
  claimed exact.** Rect Size/Roundness, colors and Group Opacity are also explicitly
  diagnosed, with complete native paths retained in each diagnostic.

The private per-target report records identities, both windows, vectors, exact
errors/key counts or contextual diagnostics. A portable synthetic-identity regression
checks that unused Shape captures cannot silently pass; it is not native fidelity proof.
Ease-and-Wizz and controller15383's native Bezier(2) remain outside fresh closed
source-program evaluation; captured-value mapping evidence cannot substitute for
fresh evaluation. Shape capture availability is resolved, but the missing mapping
and fresh clock/interpolation admission remain follow-up work. Numeric font
substitution proves neither exact typography nor rendered/RGB/alpha equality. Glow Text's ordinary opacity aliases are
potentially deterministic, but selector programs require native seeded RNG and
per-character context; opacity-only baking cannot repair the omitted reveal
weights. Those selectors remain unsupported.

## Direction

This user-approved milestone is **AEP → editable FX only**. FX → AEP expression
reconstruction is unchanged/unimplemented: edited numeric keys use the existing
native numeric export capabilities where supported; original program semantics
are not restored. See the [single support ledger](after-effects-support.md).
