# Numeric After Effects expression import

AEP import evaluates a closed numeric expression subset into editable FX keys.
It is not a complete AE expression runtime, and it does not restore native
programs on FX → AEP export. Separate native sample sidecars can supply validated
values for supported targets; captured values are not fresh evaluator results.

## Supported profile

Evaluation uses Boa through the shared `fx_keyframe_bake` runtime. AST admission
checks the whole program, including dead branches. Supported profiles include:

- Numeric `value`, `time`, deterministic Math and bounded vector operations.
- Unique layer/composition/effect control references and admitted `valueAtTime` reads.
- Supported native keyed dependencies, including bounded temporal/spatial easing.
- `linear`, `clamp`, vector length/normalization and bounded loop/posterize profiles.
- Named nonrecursive helpers and bounded return/switch/break/update constructs.
- Recoverable native key/range probing with `numKeys`, `key`, `nearestKey` and
  restricted try/catch; fatal dependency failures stay fatal across caught errors.
- A bounded Shape Scale destination profile; this is not general Shape-property support.

Unknown APIs, mutable API aliases, ambiguous lookups, dependency cycles,
unsupported destinations/clocks/values and recursion fail closed. Explicit
`throw` is rejected, including unreachable branches. Per-character selectors and
other expression destinations have separate narrow profiles; do not infer support
from ordinary layer controls. Random APIs use their own diagnosed approximation,
not deterministic equality with Adobe's seeded behavior.

## Occurrence and key semantics

Evaluation runs **after Essential overrides** in a fresh occurrence-local scope.
Source-only captures cannot replace overridden occurrences. Supported numeric
results pass through the existing expression-sample and scalar-key fitting path;
no generated JS or rasterized substitute is inserted into FX.

Native key interpolation is mapped through the keyed import's existing editable
representation. Matching that representation does not prove exact AE interpolation
between all observations. Shape Scale values remain percentages (100 is identity),
not normalized ratios.

Unsupported evaluation retains authored/stored fallback where safe, with contextual
diagnostics and independent siblings. Fallback is not expression fidelity.

## Sample clocks and fitting

Default evaluation observes composition-FPS times. Explicit validated observations
can constrain the existing fitter at their original fractional offsets. Do not
round or relabel native timestamps, or claim the default track uses an optional
denser capture clock.

`fit_scalar_curve_with_observations` checks hard observations at `1e-8` native-unit
precision after unit mapping; ordinary soft probes retain their fitting tolerance.
Unrepresentable constraints return `ObservationFitError::Unrepresentable`, not
inaccurate success or a new dense fallback. Finite cubic/Linear/Hold tracks do not
exactly represent arbitrary continuous functions.

Each occurrence has a 250,000-vector sample bound; dense fitting is also bounded
to 250,000 milliseconds. These are algorithm limits, not OS resource isolation.
Boa VM limits do not provide wall-clock/heap isolation or an Adobe sandbox.
Use trusted input and external process/resource limits.

## Native sample sidecars

`--expression-samples JSON` is AEP-import-only. Captures must match exact source
hash, composition, typed property identity, occurrence rules and native clock.
Malformed, conflicting or unmapped captures cannot silently become supported keys.
See [sample declarations](../crates/aftereffects_file/src/expression_samples.rs)
and [adapter options](../crates/aftereffects_file/README.md#expression-samples).

## Evidence and limitations

A checked-in sampled-Position source has 2,001 independent Adobe observations.
Fresh evaluation matches that numeric oracle within its `1e-8` bound. Source SHA-256:
`ff5d9d59e13079dffa0eae8153bcfbf3ec01c57227267a0a256fc7c638c8abbd`;
sidecar SHA-256:
`4000185e2e5d11cc3fc532a7ec2df8cf7ab18c5b795489de693d56db214a794f`.
Tests distinguish default source-frame fitting from separately constrained dense
observations; success on one is not a promise at every unsampled time.

Selected helper/Shape controls have bounded numerical evidence. Other Shape
fields, arbitrary controller programs, native random semantics, fonts and
per-character behavior are not generally established. Historical licensed-case
investigations do not expand the public support profile or prove current runs.
Numerical/serialized-key agreement does not prove Adobe rendering, typography,
RGB, alpha or audio fidelity. See [AE support](after-effects-support.md).
