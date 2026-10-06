# Opaque modern run-style metadata

`feature_text_point_style_metadata_derived.prproj` derives from the public native
`feature_text_point.prproj` (SHA-256
`47ac41b7f2481c5b034d1adecb3423b61846ef493fb46fef94f8bcef07a88ddc`).
It retains sequence `c8acf9c1-34b2-4086-9f55-d528950a7059`, native wording `py`,
Arial-BoldMT, 160px size, white fill, transforms and the two-second placement.

Only Source Text record10176's empty style21 table is replaced with a well-framed
opaque table carrying own scalar values0/1 at sub-slots1/2; the changed payload's
`BinaryHash` is removed. Neither sub-slot is assigned a meaning or replayed. No
licensed project, wording or payload was copied into this fixture.

This is a supplementary native-derived profile regression, not a new Adobe save
or render oracle. Unit tests assert known text/style retention and consumed-field
bounds. `modern_run_style_metadata_publishes_meaningful_editable_native_text`
checks normal editable publication and a local metadata diagnostic. Native visual
fidelity and font packaging remain separate, unmeasured requirements.
