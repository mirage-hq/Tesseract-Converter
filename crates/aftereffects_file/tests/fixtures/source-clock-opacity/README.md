# Independent source-clock Opacity control

`independent.aep` was independently authored through managed `Client.run_jsx`
(job `3d534b131e3b48a29630e6f878ebc0da`, Adobe version in `proof.json`).
`primary-input.mp4` is a CPU-generated moving/timecoded test input, **not an
expected/reference video**. No private case pixels are included. `author.jsx`
is the exact managed function body; it must never be launched independently.

Native target 15, `P019 public dual-clock occurrence`: 320x180, 30fps, 18s.
A noncollapsed occurrence owns remap 2.85/7.75/11.45/17.95/18s ->
0/5.5/8.2/14.958/14.958s. Inner source target 1 has editable Opacity keys
0/40/80ms -> 0/50/100 and source-clock Mosaic keys at 0/2.5/5.5/16s.
The source-owned control import test asserts the actual independent keys;
export regressions assert a fresh movie source, unchanged outer remap, inner
Opacity keys, siblings, and unsupported-profile guards. Import code is unchanged;
this does not establish import rendering fidelity.

`input.json` is the public current-FX export input. The explicit CPU-only
`remapped_opacity_generate_public_archives` test (removed; historical) wrote original
and half-Opacity-edited archives with these same schedules. Convert those archives
with the normal CLI package route before managed rendering; raw writer test
bytes do not have the final package's bound media aliases.

## Results and unresolved proof

- Unchanged original P019/P028 movies are retained and rendered. Historical
  repair-02 before -> fresh after canonical means: 0.115900 -> 0.733809 and
  0.111472 -> 0.824167 (73 full-resolution RGB24 samples each). These are not
  same-head causal comparisons, nor full-project 0.95 passes.
- Full independent public comparison: **mean 0.914716**, minimum 0.840356,
  73 canonical full-canvas RGB24/.25s samples. This **fails** the owner-set 0.95
  acceptance line. The current FX control input also lacks the independent
  native fixture's 8x8 green helper sibling, so fixture equivalence must be
  completed before attributing all full-frame loss to clock/footage behavior.
  A separate 96-frame onset panel averages 0.990826, but does
  not replace the failing full-video result.
- Real fresh input edit halves source Opacity. Eight of nine fixed RGB samples
  meet the predeclared ratio 0.49..0.51 (seven of eight visible samples). The
  earliest visible fade yields 0.481973 and fails; the tolerance is unchanged.
- Reviewer R02 completed two additional authorized managed numeric operations.
  Saved/reopened public original/edited keys are 0/50/100 versus 0/25/50 at
  identical native times. All eight nonzero evaluated responses are exactly
  half within floating-point precision, including the earliest RGB failure;
  the zero sample remains zero. Mosaic and remap keys are unchanged. This closes
  editable-control response/readback, not the unchanged lossy RGB ratio failure.
  Earlier failed auxiliary sampling/alias inspection remains in `proof.json`.
- The author's `sampleImage` alpha result is negative/invalid as a content
  discriminator. The successful independent normal export proves visible
  source-clock fade content; no alpha or audio fidelity is claimed.
- Rendered reference videos remain ignored. Long-term Asset publication is
  incomplete; no expected/reference video is committed to Git.

See `proof.json` for exact hashes, jobs, scores and limitations. This fixture is
not enrolled as a passing formal feature-reference case. Reviewer comparison
found a material fixture mismatch: independent native project is **32 bpc**,
generated public project **8 bpc**, both working space `None`. Their native
source-time samples, Mosaic values and sampling/quality switches match; identity
TRS differs only by equal anchor/position translation. The green helper sibling
is also absent from FX. Thus 0.914716 is not a matched-input clock regression
measurement. Bit-depth loss is outside this bounded omission repair; no claim
that it explains every pixel or that a matched full-video comparison passed.
The reviewer considers the omission repair Ready on semantic/native edit proof
and the original-case gains, with these unmatched-fixture limitations retained.
Do not replace the reference, relax scoring, shift source time or substitute the
onset panel. See `reviewer-proof.json` for raw numeric observations.
