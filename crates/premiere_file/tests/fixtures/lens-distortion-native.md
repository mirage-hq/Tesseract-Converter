# Lens Distortion native records

`lens-distortion-native.xml` contains verbatim XML records extracted offline from
the user-authored `human-inputs-20261004.prproj` (Premiere Pro 2026), SHA-256
`e74d088116570ddb7178b127129036755be2f8e553dea80f98aa59b601585b76`.
The source was not changed. Fixture SHA-256:
`ec8b8dc516d336988dec60cde5b8563b70761b8c21fd859c24a82631a1652764`.

Original target: sequence `effects`, V1 `subject.mov`, VideoClipTrackItem 324,
start/end ticks 5208877599084/6945170132112, component chain 384,
Lens component 546, parameters 722–728. Only the component and its seven parameter
records are retained; this is a native-record fixture, not an openable project or
render oracle. Tests place these unmodified records in the existing synthetic
one-clip harness, with synthetic Blur siblings to exercise omission isolation.

Curvature retains both saved Linear keys (0:-40 and 510674274420:40), static -40,
and its original automatic handles. Decentering and Prism FX controls are zero;
index 5 remains unnamed, true; Fill Color remains 18374966859414961920. Private
data is retained as evidence only, never replayed into exports.

Premiere Pro 26.5.2 getter-only inspection of this exact original clip identifies
index 5 as **Fill Alpha**, static Boolean true, with keyframes supported. The
seven native property names match this record order. Keyed Curvature and Fill
Color values were not read through scalar getters; the saved bytes remain their
source evidence. No setters, saves or renders were performed.

Tests preserve native Curvature keys and sibling order, map the bounded form to
editable FX and export deliberately edited values. The support ledger records
the signed slider normalization as an approximation, not calibrated lens physics.
No original media is needed for the offline record test. Native acceptance of
export without private data, fill/alpha behavior and render fidelity are unverified.
