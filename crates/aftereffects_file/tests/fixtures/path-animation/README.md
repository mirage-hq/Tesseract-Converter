# Native path-animation probes

Pinned MIT py-aep revision `e12a451c35bacd3f34a080090265f9370e66162b`;
`provenance.json` records immutable source paths, SHA-256 and size. The Rust
regression verifies all four hashes before fresh imports.

- `property_path_orientation_pin.aep`, composition `vat_probe`: `probe_shape`
  / Contents > Group 1 > Contents > Path 1 > Path, and `probe_solid`
  / Masks > Mask 1 > Mask Path.
- `property_path_orientation_ease.aep`, composition `vat_ease`: `mask_mixed`
  and `mask_asym`, each Masks > Mask 1 > Mask Path.
- The corresponding upstream `*_value_at_time.json` files are independent AE
  property-value projections, not converter-generated references.
- `import_path_key_cases.aep`: compositions 1 (`PATH_LINEAR`), 17
  (`PATH_HOLD`), 32 (`PATH_BEZIER`), registered in `aep_feature_cases.json`.

`pinned_native_paths_import_keys_and_match_independent_adobe_values` verifies
all four upstream hashes, imports one Shape and three Mask Path sources,
asserts typed editable keys/times/closedness/vertices/cubic controls, and compares
first-vertex temporal interpolation against the independent per-frame values
(tolerance 0.002 source pixels). No scripts or sampled geometry are generated.
`native_path_key_panel_keeps_linear_hold_and_bezier_authored_keys` asserts
500/2000ms keys and their incoming easing for the three grouped Shape cases.

These are existing independent sources, not newly authored feature cases.
The registry's separate ignored grouped proof tests remain UNRUN. No new Adobe
UI inspection, fresh exported-AEP acceptance/readback, 30fps render publication,
pixel/alpha comparison or native render was performed for this change. Numeric
projections and our writer/reader checks do not establish export or render fidelity.
See the [bidirectional implementation and limitation ledger](../../../../../docs/after-effects-support.md#native-shape-and-mask-path-keys).

## One-second mixed Path — bounded forward-only repair

`mixed_one_second.aep` is independently Adobe-authored (AE26.5x89), composition1
`W06 mixed Path one-second owner control`, 320×240, 24fps, full5s. Four asymmetric,
closed, three-vertex Shape/Mask controls include baseline and +30X variants;
owner start/in2s, out5s, native stored keys0.25/1.25s, composition keys2.25/3.25s.
`mixed_one_second_readback.json` contains the independent managed native readback,
not our reader's output. Source/projection hashes, managed request/script pins and
fresh generated-output acceptance are in `mixed_one_second_proof.json`.
The author call initially selected a Solids folder after saving: its zero-control
result is **invalid semantic evidence**. One corrected read-only call selected
exact CompItem1 and produced28 actual samples; the original AEP was not changed.

`mixed_one_second.fx.json` is an explicitly authored **public editable FX input**,
not an imported source, donor replay or resampled oracle. Two root Shape controls
start2000ms, duration3000ms, keys250/1250ms in each Shape's own animation clock.
The existing FX evaluator maps composition2250/3250ms to those owner-local keys.
An actual +30X input edit shifts all vertices/handles and leavesY unchanged.
The incoming cubic `(1/6,1/6,0.1,1)` corresponds only to the independently observed
one-second Linear-out / zero-speed Bezier-in90% profile. No all-mixed easing
support, arbitrary speed mapping, sampled geometry or clock correction is added.

**Export evidence:** the complete explicit-input export regression failed before
admission (job1761, both Shape owners omitted) and passed after (job1782).
Its native-record clock assertions are supplementary. A fresh full CLI export
from a canonical Tesseract archive was independently opened in Adobe in the
third and final managed call: exactly2 editable owners, two keys each at
composition2.25/3.25s, start/in2/out5, correct mixed interpolation/zero speed/90%.
All14 `valueAtTime` samples at2,2.25,2.5,2.75,3,3.25,3.5s matched the unchanged
independent native controls, maximum geometry-coordinate error
`4.9737991503207e-14`;370 scalar checks included all +30X vertices and unchangedY.
Cleanup and fresh READY succeeded. This proves bounded numeric editable Shape
export, **not a native-render/RGB/alpha pass or Mask export acceptance**.

**Import remains unchanged:** this mixed one-second profile still produces a
contextual omission. The prior source-based RED1663 is preserved as an admission
failure, not relabeled importGREEN. The earlier assumed250ms assertion for an
imported Mask guide was wrong: `NumericAnimationClock::parent_identity` maps
native source-local250ms + native start2000ms to2250ms on the guide's identity
composition clock (guide start0, identity owner playback). Shape source-content
ownership uses a different source-local clock. The new clock characterization
checks those actual fields without changing product import/shared clocks.

Run `make test-aftereffects-file filter=mixed_one_second_path` in
`opensource/conv`: explicit full export/input edit, narrow negative admission,
pinned native stored clock and unchanged import guard, and Mask guide clock.
Independent30fps render, long-term Asset publication, raster/RGB/alpha fidelity,
original49 improvement and general mixed-ease support remain **unmeasured**.
The bounded Shape-control profile can qualify for merge under the campaign policy;
the inherited unchanged-main failures and formal render/Asset gaps remain disclosed,
not a claim of full-suite success or completed feature fidelity proof.
