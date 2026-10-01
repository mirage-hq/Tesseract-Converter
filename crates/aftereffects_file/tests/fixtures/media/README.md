# Native AEP media fixtures

Pinned from `forticheprod/py-aep` commit
`e12a451c35bacd3f34a080090265f9370e66162b` under the paths and SHA-256 values
in `provenance.json`. The upstream MIT license is retained in the fixture
parent directory.

Target identities from the independent AE JSON sidecars:

- `footage_not_missing.aep`: composition item 2 (`Comp 1`), layer 14,
  footage item 1 (`sample_motionblur_transparency.exr`, 200×200 still).
- `footage_missing.aep`: composition item 2 (`Comp 1`), layer 14, footage item
  1. This separately pins the source's saved-offline state.
- `audioEnabled.aep`: footage item 13 (`wav.wav`, 5.9431746031746 s);
  composition item 1/layer 14 has audio enabled and composition item 15/layer
  27 has audio disabled.
- `imio_sequence.aep`: native image-sequence source used only to assert bounded
  `StVc`/sequence classification. Upstream has no matching JSON sidecar in this
  sample directory, so it is structural source evidence, not an independent
  editable/render oracle.

The JSON files are upstream Adobe projections, not FX render output. No Adobe UI,
render, alpha, audio-fidelity, or pixel comparison was performed in this task.
The referenced media bytes are not bundled here; adapter tests use synthetic
local files to prove publication behavior.
