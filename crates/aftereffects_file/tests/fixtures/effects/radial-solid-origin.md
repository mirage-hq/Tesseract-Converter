# Static Solid Radial Blur source-origin export control

Direction: explicit editable FX → fresh AEP. Import implementation/proof unchanged.

The independently AE-authored `radial-solid-origin.aep` has SHA-256
`e3f37f8ba1e79f8e338b093225cc5787325b1d38339a5c780e778f5242d85471`.
Compositions base ID1 and edited ID16 are180×160,squarepixel,1s,30fps.
Each owns one opaque40×30 red native Solid, Anchor[20,15],Position[90,80],
Scale100%,Rotation0°, enabled sole `ADBE Radial Blur`, Amount20,Type2(Zoom).
Native centers are respectively[13,20] and[20,3].

`radial-solid-origin.fx.json` explicitly specifies a Rect origin[7,-5],
Anchor[27,10], normalized radial center[.5,.5] → local point[20,15].
Native Solid source origin is zero: native Anchor[20,15] and center[13,20]
preserve both original world locations, including center[83,85].
Actual input edits in the runnable regression change Rect origin to[12,3],
Anchor to[32,18], center to[.8,.2] → local[32,6]; native center[20,3]
preserves world[90,68]. No original-AEP bytes are replayed by export.

Managed `Client.run_jsx` request
`tsrct-aep49-w07-forward-radial-origin-readback-v1`, AE26.5x89,
job`7ea54cf75d1647e4bd55245a4b5353bb`, executed the pinned body
`radial-solid-origin.jsx` SHA-256
`1acd63320513e08a481ca8d5d2c84a09c5d746814cd852d4c9dad5be550c83ab`.
It independently opened/read fresh RED/base/edited exported AEPs, then authored
the native controls without using the converter's reader/writer. Base and edited
native readbacks matched the independent controls exactly: Solid identity,size,
Anchor,Position,Scale,Rotation,effect identity/enabled,Amount,center and Zoom Type.
Cleanup and fresh READY succeeded; this is not human UI inspection.

Native-inspected exported byte hashes:
- RED:`ec390fb98df019b70e41acb668838c97f56ea6959bdae5981f859f6f1ad1d108`
  had wrong center[20,15].
- base:`dd8a08c06b19f5cb5358dd0febb3f000074edf8ad668130b48c4df5b2b0b8fa7`
- edited:`d4ace1b8d1ef0a5023abab7a654b03a3a31dd4c7ae80ae5e086f956619d612f6`

Runnable tests in `export_document::tests::radial_solid_origin`:
`radial_solid_origin_export_rebases_point_and_responds_to_input_edits`,
`radial_solid_origin_export_declines_nonuniform_gated_and_vector_profiles`,
`radial_solid_origin_export_native_oracle_is_pinned_and_feature_specific`.
CPU behavioral RED queue1638: source center[20,15] versus[13,20].
Initial GREEN queue1667:2/2(before adding native-source hash assertion and animated
negative control). Final executions are recorded in the PR, not inferred here.

This exact point translation is guarded to actual native Solid,sole enabled static
Radial Blur,isolated unparented/uninherited ordinary2D root,static positive uniform
scale,opaque transform,Normal blend,no masks/matte/styles and finite arithmetic.
Vector,animated,mixed,nonuniform,gated/nested profiles are not generalized.
Gaussian Blur was deliberately excluded: its existing FX renderer already
multiplies blurriness by world scale; Directional Blur compensation is not reusable.

**Incomplete fidelity proof:** no new native30fps MP4,long-term Asset/fresh download,
FX/native RGB,alpha,Zoom kernel/Amount equivalence,audio,motion or original49 proof.
These are native editable-control/input-edit acceptance results only. Public source
and locally retained native receipt/audit are not a render/Asset certificate.
