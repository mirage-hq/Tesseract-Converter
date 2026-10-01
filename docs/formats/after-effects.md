# After Effects (`.aep`): conversion overview

[Formats](README.md) · [CLI](../../apps/tesseract-conv/README.md) ·
[Current feature × direction ledger](../after-effects-support.md) ·
[Effect Parade controls](after-effects-effects.md)

The former private local workflow guide is no longer distributed. Independent
Adobe evidence still requires separately authorized preparation.

**Both directions are best-effort.** AEP → FX imports one selected reachable
composition and creates editable `project.tsrct`; FX → AEP exports the current
edited FX document to a **new** `project.aep`. It does not restore or patch the
original AEP. Neither conversion renders video. A successful `--check` may
contain diagnosed omissions; prefer explicit `--composition <item-id>` over the
ambiguous default. The [current ledger](../after-effects-support.md) and
[Effect Parade appendix](after-effects-effects.md) record control-level limits;
older implementation checkpoints are not the current support state.

| Area | Current bounded mapping in **both** directions | Key restriction / independent proof |
| --- | --- | --- |
| Projects and timing | Selected comp, ordered layers, canvas, supported clocks, finite precomps, hierarchy, source references. Fresh export FPS defaults to 24; source FPS cannot be inferred from FX. | Other roots, unknown clocks, shared-instance identity, camera projection and custom editor state may be lost. Empty-project Adobe probes do not validate nonempty layers. |
| Transforms and motion | Supported 2D/3D numeric controls, compatible keys, selected frame-blend/motion-blur settings. | Coupled easing, ancestor transforms, user lights/cameras and native interpolation may differ. |
| Shapes and paint | Solids, parametric Rect/Ellipse/Star/Polygon, static Path, bounded paints/groups/dashes/gradients and Merge Paths. Eligible Boolean operand Rect/Ellipse/PolyStar **geometry keys export**. | Native Shape/Mask Path keys have a bounded single-contour Linear/Hold/zero-speed-Bezier mapping; unsupported topology, easing and clocks are diagnosed. Variable stroke width, mixed paints/modifiers and shared identity may be lost; Shape raster fidelity remains below the strict gate. Boolean geometry keys have CPU tests but no independent animated Adobe export oracle. |
| Text, masks, matte | Point/box text, selected animators, static masks/matte; supported gates remain editable. | Font/layout differences, animated masks, transformed feather and alpha are not broadly proved. |
| Adjustment | Direct editable FX Adjustment import and fresh native solid-backed Adjustment export. | Ten independently authored cases measured, but RGB minima include mismatches; alpha, extra parent/gate combinations unproved. |
| Media and audio | Bounded packaging, source variants, AV/audio separation, scalar gain keys and mute. | Missing media/codecs, stereo collapse and audio curve differences diagnosed; general audible fidelity and independent export acceptance missing. |
| Effect Parade | Named plugin subset and compatible controls/key tracks in both directions. | Unmapped controls and kernels differ; historical 55-case CPU/readback panel contains failures/rejections and measured RGB is **not** a fidelity pass. |
| Layer Styles | Nine representable styles map through `ADBE Layer Styles`, not Effect Parade, both directions. | Pattern Overlay omitted, blend/ordering/gradient geometry and non-scalar animation approximated; independent RGB/alpha not established. |
| Scripts/expressions | Import does not generate JS. Export can prepare bounded *layer-local scalar* FX scripts into editable keys. | Not a general AE expression runtime; dependent, non-scalar, legacy-clock scripts are diagnosed/omitted; full-project comparison showed missing content. |

Import-only corrections keep modern unselected mattes, still-image lifetimes,
the `Fade In+Out - frames` preset as owner Opacity keys and Geometry2 on a still
as an editable source-plane Group. Their bounded profiles and synthetic coverage
are in the
[ledger section](../after-effects-support.md#mattes-still-lifetimes-frame-fades-and-still-geometry2--bounded-import-correction);
export of that structure is unverified.

For **each** feature's static, animated and combination limits, fallbacks,
structural execution, independent Adobe control readback, RGB, alpha and audio
status, use the [current ledger](../after-effects-support.md). Control-level
Effect Parade details belong in the [appendix](after-effects-effects.md). The
[historical case results](../after-effects-test-results.md) report actual executed,
failed and unrun cases without upgrading any earlier evidence. RGB cannot prove
alpha or audio; Adobe scripting readback is not manual UI inspection.
