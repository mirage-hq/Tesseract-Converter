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
