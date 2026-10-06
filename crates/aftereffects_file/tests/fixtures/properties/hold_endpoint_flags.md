# Native Hold endpoint controls

- Source: `hold_endpoint_flags.aep`, SHA-256 `70b9804ecc4f95f6ad8fb4d95a976168d4b6184181a763aabbf2778ba6a14f4d`.
- Independently authored and saved/reopened through managed headless-adobe `Client.run_jsx`, AE 26.5x89, job `4ca2825e58944dd38dedfac4853b4a1c`, request `accuracy-w02-endpoint-flags-v1`.
- Authoring script: `hold_endpoint_flags.jsx`, SHA-256 `0a121858bb096f386893a4969050f078cd30a285630340c8a0a0a00c01c53544`.
- Target: composition 1, `endpoint-flags`, 64x64, square pixels, 24fps, 2 seconds. Three native red solids, no external media/fonts/effects. Opacity keys at -0.5/0.5/1.5 seconds with values 0/100/40 percent.
- Native interpolation enum: Linear=6612, Hold=6614. Saved controls:

| Layer | Incoming flags | Outgoing flags | valueAtTime(.25,true) | valueAtTime(.75,true) |
|---|---|---|---|---|
| control-0 | Linear, Linear, Linear | Hold, Hold, Linear | 0 | 100 |
| control-1 | Linear, Hold, Linear | Hold, Hold, Linear | 0 | 100 |
| control-2 | Linear, Hold, Linear | Hold, Linear, Linear | 0 | 85 |

The decoder exposes opacity in 0..1 units. Outgoing Hold determines its
segment regardless of the next incoming Linear/Hold flag; incoming Hold on
control-2 does not change the subsequent outgoing Linear segment.

`native_hold_endpoint_flags_are_admitted_without_weakening_curve_guards` in
all three control-link validator test modules uses a fresh native read. The
export tests `native_hold_endpoint_flags_survive_editable_import_and_fresh_export`
and `native_hold_endpoint_flags_public_minimal_fx_keys_preserve_segment_ownership`
check canonical editable keys, signed clocks, outgoing Hold/Linear ownership,
fresh full export, and an editable FX middle-value edit from 100 to 40 percent
(decoded native values change from 1.0 to 0.4).
The writer may normalize unused incoming flags to Hold; exact raw flag identity
is not a restoration claim. Our reader's generated-output assertions and a
piecewise CPU calculation are supplementary, not independent Adobe playback.

Reviewer generated-output follow-up: managed AE26.5x89 job
`7ca07da819784cd1ba00d172b07e47a0`, request
`accuracy-r02-hold-generated-output-20261003-v1`, independently opened and
reopened two fresh minimal editable FX exports from exact converter commit
`36ea3119e57754f5f8698c1712edd215b70ab421` (immutable Rust build1692).
The canonical opacity input uses -500/500/1500ms, values0/100/40 percent,
Hold then Linear; the second input edits the middle value100→40.
Both outputs retain three native keys, exact times/values/outgoing flags and
seven samples at−.5/0/.25/.5/.75/1.5/1.75s. All14 samples exactly agree with
the pinned independent native `control-2`, including its separate in-memory
middle-value edit; maximum absolute error0 percent. Native cleanup/fresh READY
completed. `hold_endpoint_output_readback.{jsx,json}` pins the script, input/output
hashes, raw native getters, application/build and converter provenance.
Reopen means close without saving and reopen the unchanged generated input in
one owned host, not a native normalized save. Incoming unused flags are not an
identity claim. This is numeric output acceptance/input-edit evidence only.

The original overlapping-solid fixture above remains a numerical oracle, not
a discriminating visual reference. A new public native revision now supplies a
30fps reference, immutable long-term Asset and measured fresh-import RGB
comparison: see `hold_endpoint_render.{aep,jsx,json,md}`. The strict .99 RGB gate
FAILS (mean .974633, minimum .965132); this is diagnostic evidence, not a visual
pass. Generated-AEP native rendering, alpha/audio and original49 visual fidelity
remain unproved; no complete bidirectional proof is claimed.
