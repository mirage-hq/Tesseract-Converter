# Native Transform property sources (structural evidence only)

These unchanged Adobe-native AEPs come from MIT `forticheprod/py-aep` at
`e12a451c35bacd3f34a080090265f9370e66162b`. See `../LICENSE` for attribution.
`provenance.json` pins source paths, SHA-256, byte counts, composition/layer IDs
and the original AE-exported JSON sidecar identities. AEP and sidecar bytes were
checked against upstream Git LFS pointers before staging. The two Orientation
sidecars are retained verbatim for key-value evidence; earlier cases retain the
projected fields in the manifest.

The manifest's `compositions` entries are a direct projection of each independent
AE JSON sidecar: comp ID/width/height/frameRate; layer ID/sourceId; Transform
matchName/value/numKeys/expressionEnabled/dimensionsSeparated. No expectation was
computed by the FX reader or writer. Sidecars are not committed in full.

- `transform_unseparated`: comp 1, layer 15, source 14; static solid 1920×1080.
  AE omits the ordinary default 2D Transform leaves from the file. The sidecar
  independently establishes source-center anchor, comp-center position, 100%
  scale/opacity and zero rotation. This tests default synthesis, not nondefault
  Transform decoding.
- `transform_separated`: same target IDs; Position leader and X/Y followers
  physically exist. The sidecar proves separate dimensions and 960/540 values.
- `property_2D_position`, `property_scale`, `property_rotation`,
  `property_1D_opacity`: comp 1, layer 15; animated leaves exercise native key
  values, timing, easing, spatial tangents, unit conversion, and editable graph
  validation rather than treating static `cdat` as an animation sample.
  `native_spatial_position_keys_match_independent_adobe_values` additionally
  asserts `property_2D_position` comp 1 / layer 15 Position at 0s `[0,0,0]`
  and 5s `[100,100,0]`, with zero spatial tangents, from the SHA-verified
  Adobe sidecar (`a9d9fcb8bf282e6db4f61c16b2e03d642cbfe5e354ab457446b1bfb6fc2af8af`).
  It fails with the old reader's missing reserved-double skip (`[0,100,100]`).
  This is independent key-value evidence, not an Adobe render comparison.
- `orientation_5_0_0` and `orientation_with_keyframes`: native `otst` wrapper
  evidence for little-endian static angles and sibling `otky/otda` key values.
  The latter preserves authored Euler key values; AE's quaternion interpolation
  is explicitly diagnosed as a between-key approximation.

No external file media is staged or opened. Converted test content is the native
solid, represented by an editable Rect inside its layer occurrence Group.
Native-derived mutations test nondefault anchor, negative/nonuniform scale,
rotation, opacity and malformed optional values. Those mutations are explicitly
supplementary, not Adobe-authored feature proof. Existing layer corpus remains
additional regression coverage.

No Adobe UI inspection, independent Adobe render comparison, alpha scoring or
reference upload was performed. These are NOT visual-fidelity passes. See
`docs/after-effects-support.md` for feature-by-feature limits and proof gaps.
