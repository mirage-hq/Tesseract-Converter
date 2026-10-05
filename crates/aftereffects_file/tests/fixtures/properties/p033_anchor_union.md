# Independent straight Anchor union control

Self-authored Adobe AE 26.5x89 source, managed job
`925a606cdf454d64a6605488d60438fb`, verified cleanup/READY.
Source SHA256 `3f91447cb871ebe555f1c6ff02b090c95f3be139a5d102c718e63ea6d658f360`;
JSX SHA256 `e26fb67154f9099022eb83422d12fe9557f059d17b71195244d699bec40b31f6`.
Composition ID1, `p033-anchor-union-controls`, 160x120, 24fps, 2s;
one solid `anchor-owner`. No private actual49 assets are included.

Native ADBE Anchor Point uses three-component values even on a planar layer:
0s [0,0,0], .5s [40,10,0], 1s [40,20,0]; Linear incoming/outgoing
(API6612), zero three-component spatial tangents. Independent native samples:
.25s [19.9999170277546,4.99997925693866,0],
.75s [40,15.0000777353816,0]. Endpoints and post-last values are exact.
Interior native sampling has small numerical drift; this is NOT a passed
analytical 2e-5-pixel test, RGB comparison, or exact runtime equivalence claim.

Executable supplementary native-source assertion:
`p033_anchor_independent_native_control_has_union_keys`.
Minimized actual49-pattern converter regressions separately test independent
Linear Anchor/Position knots, endpoint edits, and cubic/spatial/mixed-Hold
negative profiles. The independently authored source already has union keys;
its own reader assertion is not the omission RED regression or proof that
Adobe accepted converter output.

Import implementation unchanged. Export repairs Anchor knot admission in the
native Transform sidecar: native 3D or planar owners using separated Position
followers. Ordinary planar owners with static/aligned Position remain on the
existing paired-transform path and are not newly admitted.

Reviewer generated-output acceptance: managed AE26.5x89 job
`397668dd7458454f91864ca1340b2c1e`, verified cleanup/READY, opened fresh control
and endpoint-edited exports from immutable queue2110 on main9274d1ec3.
Starting from this independently authored source's fresh import, the explicit
minimal inputs use Anchor X knots0/.5s, Anchor Y0/1s, Position X0/.5s and
Position Y0/1s. Adobe observed the exact union0/.5/1s, Linear6612 and zero spatial
tangents; Anchor values [0,0,0]/[40,10,0]/[40,20,0] changed to
[0,0,0]/[60,10,0]/[60,20,0] after the input endpoint edit.
Generated AEP hashes: control
`53bede68d03e86d7d1ed955f8d62b8645bee57c8cb210f28dccdf703647415d4`, edited
`7579c42977a1dc4184794ab5d2a78e2b012c88b82be7fc98b543557635cf27a3`.
The first reviewer request used static Position and reached the unchanged paired
Transform omission before native target selection; it failed with zero targets,
returned READY, and is not proof. The corrected profile above matches the scope
of the executable regression. No native interior sampling/fidelity claim added.

Native30fps MP4, long-term Asset, RGB/alpha/audio and fullcase fidelity remain
unproved. Fresh post4807 P033/P006 Groups still hit Text/font glyph bounds gates;
this repair is necessary, not sufficient, and no score improvement is claimed.
