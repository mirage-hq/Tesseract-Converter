# Effects blur, glow and shadow audit

This checkpoint owns the 13 cases in audit group
`03-effects-blur-glow-shadow`. It separates editable converter semantics from
renderer-kernel differences. Scores are descriptive canonical RGB24 measurements,
not fidelity passes; references, comparison policy and thresholds were unchanged.
Alpha and audio remain unverified.

## Pinned native sources

| Source | SHA-256 | Owned composition IDs |
|---|---|---|
| `effects/catalog.aep` | `7519496eb44f5ebc0eff5c2ad78476afdd738303cfb18449ac4f3596e82070c2` | 1, 16 |
| `effects/animated_catalog.aep` | `71ba020d675ecab5fc3236730b2b6a90a49bab2cb25f779f1e68ccf47c4134a4` | 1, 16, 198 |
| `effects_coverage/native_static_controls.aep` | `7c65ebe724fc8403399979cc1354adca3abd737b6ad7a76ba8eafc5b61ccf95b` | 66, 157, 183, 456 |
| `effects_coverage/native_animated_controls.aep` | `4e38ded65e44f6a1adfe22713ddb36557afd312959a612ee601b75849d3f73ce` | 53, 144, 170, 443 |

These are independently Adobe-authored sources with immutable long-term Adobe
references registered in `aep_video_references.json`. The coverage sources use a
320×180 Shape plane with 120×80 visible content. Their import assertions pin the
native values, the 0/1000 ms effect-key clocks and Linear interpolation; no
parameter was tuned against a rendered frame.

## Converter correction

`ADBE Drop Shadow-0005` (Softness) was previously consumed by the special static
Drop Shadow importer, even though current FX exposes `blurRadius` as an animatable
scalar effect property. The same pinned animated catalog source, composition 198,
contains native Linear Softness keys `0 → 0.10000000149012` at `0 → 1000 ms`.
The source-backed regression failed before the correction because no editable
`blurRadius` target existed, then passed after generic scalar mapping retained the
two keys. Export uses the same affine mapping, so edited compatible scalar keys
continue through the existing native numeric writer. Independent generated-export
Adobe inspection/rendering was not rerun by this checkpoint.

The correction intentionally does **not** map animated shadow color, opacity,
direction/distance or Shadow Only. Current FX has no effect-instance scalar target
for those controls that preserves their semantics: independently Linear polar
angle/distance tracks are not an affine XY track, and Shadow Only changes the
whole composite. Their initial editable approximation and contextual diagnostics
remain.

## Fresh import/render/compare results

The fixed converter was built from this worktree with SHA-256
`e5b6375ef4df13e5a8b6880ff23ad260b6f3195ae4429889b59380f6a5b688d9`.
The pinned runtime was SHA-256
`7ae0743d8d3e75ecbfd93e4fe25e0414df7387e1c1f66262acd941e30689a66c`
and the comparator was
`a15a8fa61f9816c28b9090ba21838844f368f232fabc6a188046793a71e17803`.
All 13 fresh imports rendered and scored over 60 unique 320×180 frames at 30 fps.
The Softness correction changes editable structure but is below encoded-pixel
resolution in this case. The c198 rendered MP4 remained byte-identical and every
score remained numerically unchanged from the immediately preceding clean run.

| Case ID | Mean | Minimum | Worst time (s) | Disposition |
|---|---:|---:|---:|---|
| `aep-effects-animated-catalog-c1` | 0.7411815098 | 0.7262087486 | 0.966667 | Gaussian Blur Dimensions changes and Repeat Edge motion are unsupported; mapped Blurriness keys are exact. Do not replace the axis-specific native blur with tuned Both-axis values. |
| `aep-effects-animated-catalog-c16` | 0.9203080487 | 0.8441626614 | 1.000000 | Glow threshold/radius/intensity keys are exact; animated source/composite/color controls are unsupported by current FX. |
| `aep-effects-animated-catalog-c198` | 0.9486147117 | 0.9022500927 | 1.000000 | **Fixed structurally:** Softness keys are editable. Animated color/opacity/polar offset and the Hold transition to Shadow Only remain diagnosed omissions; Shadow Only dominates the critical frame. |
| `aep-effects-catalog-c1` | 0.7735211978 | 0.7729430145 | 1.000000 | Axis-specific Gaussian Blur cannot be represented by current Both-axis FX blur; Repeat Edge and Blurriness initial values are retained. |
| `aep-effects-catalog-c16` | 0.9441544274 | 0.9441500220 | 0.000000 | Mapped Glow controls are exact; nondefault source/composite/color controls remain unsupported. |
| `aep-effects-coverage-native-static-controls-c157` | 0.9083960594 | 0.9083644498 | 0.000000 | Gaussian values `12` and Repeat Edge are exact. Remaining difference is kernel/edge/color behavior; the uniform source is not a discriminating radius proof. |
| `aep-effects-coverage-native-static-controls-c183` | 0.8769780685 | 0.8752028160 | 0.100000 | Glow `20/8/0.5` is exact; remaining difference is threshold/bloom kernel and compositing behavior. |
| `aep-effects-coverage-native-static-controls-c66` | 0.9436039144 | 0.9435850581 | 1.000000 | Direction `35°` and length `12 px` are exact native units; remaining difference is sampling/kernel behavior. |
| `aep-effects-coverage-native-static-controls-c456` | 0.8270199161 | 0.8269652839 | 1.000000 | Spin amount `15` and center `(30/320, 60/180)` are exact; Zoom/quality/seed remain omitted and the radial kernel differs. |
| `aep-effects-coverage-native-animated-controls-c144` | 0.8178330860 | 0.7912383311 | 1.000000 | Gaussian `12 → 24` Linear keys and Repeat Edge are exact; no owner-clock defect. Kernel/edge behavior remains measured. |
| `aep-effects-coverage-native-animated-controls-c170` | 0.8568428049 | 0.8434294738 | 1.000000 | Glow `20 → 40`, `8 → 16`, `0.5 → 1.5` Linear keys are exact; no owner-clock defect. Kernel/compositing differs. |
| `aep-effects-coverage-native-animated-controls-c53` | 0.9586323918 | 0.9362571316 | 0.066667 | Direction `35 → 95°` and length `12 → 24 px` Linear keys are exact. The sub-threshold frame is a sampling/kernel difference, not a unit conversion gap. |
| `aep-effects-coverage-native-animated-controls-c443` | 0.7869464906 | 0.7710096687 | 1.066667 | Amount `15 → 30` and independently keyed normalized center components are exact. Point-knot union and owner clock are preserved; the radial kernel differs. |

## Direction and proof limits

Import implementation is corrected only for Drop Shadow Softness animation; all
other owned mappings were already exact at the editable-control boundary. Import
RGB remains descriptive and below 0.95 in most cases because current FX kernels or
explicitly unsupported native modes differ. No parameter scaling was altered to
fit pixels.

Export implementation uses the shared Drop Shadow scalar mapping, but this run did
not create, inspect or render a fresh generated AEP. Existing native-control
readback evidence remains separate. Export fidelity, alpha, arbitrary owners,
other interpolation modes and unsupported controls are unverified.
