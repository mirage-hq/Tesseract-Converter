# W08 P047 candidate — FAILED acceptance, do not merge

This branch preserves an investigation candidate, not a validated converter repair.
No READY PR or auto-merge qualifies from these results.

## Scope and CPU regression

The candidate admits nested identity-2D output viewports only when the propagated
finite consumer domain equals the unchanged root viewport. Opacity-only animation
is permitted as pointwise alpha; spatial animation and unknown/different domains
remain rejected. Authored child geometry, keys, source clocks and effect inputs
are retained. No coordinate/dimension caps, source edits or runtime/backend edits
were used.

- Oversized effect-bearing identity group: semantic RED queue2710, GREEN2717.
- Opacity-only owner: semantic RED2747; intermediate2751 still rejected because
  inverse-demand animation unnecessarily inflated the identity rectangle.
- Final targeted GREEN2755: three tests passed, including retained opacity keys,
  short source duration, oversized editable scale, and negative spatial/domain cases.
- Publication-candidate gate2813: `make -C opensource/conv fmt clippy check test`.
  Formatting, lint and check completed; `aftereffects_file` tests failed:
  1961 passed, 4 failed, 425 ignored. Failures are
  `accumulated_evaluations_do_not_reject_the_next_script`,
  `playback_remapped_group_input_window_is_not_erased_by_equal_content_windows`,
  `vector2_x_only_motion_uses_x_easing_forward_and_reversed`, and
  `vector2_y_only_motion_uses_y_easing_forward_and_reversed`.
  Their baseline/candidate attribution has not been established; they are not waived.

## Actual P047 validation

Private validation combined the candidate with W03's Corner Pin commits
`bbcc957bf` and `0be18bc9c`. Those commits are NOT part of this publication branch.
Immutable release build2761 pins private commit
`e043ed9a948bb3f5926d24c5c4a3f76db9802f42`, based on main `0a44080ee`.
The full unchanged P047 source was converted at30fps.
All17 target groups lose their oversized-parent omission diagnostics. However,
14 newly exposed child groups remain omitted, plus2 earlier S10/S11 omissions.
Parent retention does not establish painted content or fidelity.

Managed Adobe validation returned artifacts despite native allocation failures:

| Operation | Managed job | Native outcome |
| --- | --- | --- |
| Candidate full movie | `4f13126ffe8a47dbac18661ad77a78b4` |106 errors; 2.6GB allocation exceeds internal limits; out of memory at30:13 |
| Candidate source frames900–986 | `8af85feaa3fd4d7ab2b4486f26d627b5` |78 errors; same allocation failure at30:13 |
| W03-only source frames900–986 | `a64ac42e39f14667ad4fb8579215d1b0` |clean native log |

Both window artifacts advertise1920x1080,30fps,87frames,2.9s. Metadata and API
publication alone are therefore insufficient native acceptance proof.

## Diagnostic numerics, NOT accepted fidelity proof

The authorized30.0–32.9s analysis window uses all original frames900–986.
Its direct RGB24 lossless MP4 reference was byte-verified against decoding those
original frames:541209600 bytes, SHA256
`0447c9ee88910f32d24b2495000ef3f8e8c8a9e6c0b3328076b087cd989b6ff8`.
An earlier FFV1/BGR-derived reference did not match RGB24 bytes; its scores are
invalid and were preserved separately, not used here.

Pinned `validation_cli` RGB-hybrid, canonicalRGB24, full resolution,0.25s interval,
12 samples: W03-only similarity0.6112598479771674; candidate0.22590249572802315.
Secondary nearest-frame RGB channel MAE rises18.167371908221877→36.203437861689814;
all12 regions in a4x3 grid worsen. This is diagnostic failed-output comparison,
NOT native fidelity proof. Baseline W03 build2716 uses main06029c47c, so the cohorts
also have different base commits; no pure single-change causality is claimed.
No alpha, audio or full-case fidelity proof is established.

## Local evidence and next boundary

Evidence is preserved under `tasks/accuracy-W08/p047/`, especially
`ACCEPTANCE-FAILED-v2.json`, `after-v2-target-diagnostics.json`,
`numeric-regions-v2.json`, the paired validation JSON, build receipts, commands,
and managed native logs. Exact continuation is `tasks/accuracy-W08/P047-RESUME.md`.

Stop implementation and revise the exact-bound/renderer-allocation design before
another product change. Do not solve this by caps, pruning, altered source clocks,
preferences, reference changes, backend changes or more nested-crop micro-probes.
A finite output rectangle is not yet proof that native effect/input allocation
is bounded. Do not merge the candidate on structural retention or CPU tests alone.

## Offline allocation follow-up (no image reads)

The generated build2761 AEP SHA256 is
`41a3a1f7fbee635fbabeeee50d91d360d0dec6cf31aff30891c05726e9c9bd78`.
All17 restored parent canvases are already1920x1080. Their descendants still
retain all-time source geometry, including enormous offscreen projected paths.
The problem is not simply an oversized restored parent.

| FX parent | Native parent | Descendant native source | Size | Single RGBA8 image |
| --- | --- | --- | --- | --- |
|1600428|4026|4027, vertex nodes dup,24 samples|26162x25542|2672919216 bytes|
|1600361|4131|4132, vertex nodes behind,24 samples|26162x25542|2672919216 bytes|
|1300416|4567|4568, vertex nodes dup,12 samples|26404x27886|2945207776 bytes|
|1300376|4633|4634, vertex nodes behind,12 samples|26404x27886|2945207776 bytes|

Each node source is two precomposition edges below its native scene (three below
Main). Every occurrence in the17 restored subtrees is2D, with native Motion Blur
and Collapse Transformations switches off. The12/24 shutter samples are explicit
add-blended Shape layers, not native motion-blur sampling. All four node owners
have Outer Glow Size12, Spread10%, Range60%, Opacity80%. Both S16 owners also
have Gaussian Blur key values0→15.35→0 over source time0–0.3s, Repeat Edge Pixels
false. Thus the source-sized image is already gigabytes before effect scratch
images, bit depth, concurrent owners, or sample layers multiply allocation.
The native log does not identify a layer; this is a sufficient allocation hazard,
not exclusive attribution of its first reported2.6GB request. Last logged progress
30:13 is not evidence that S16, whose occurrence starts32s, caused that first error.

The nine byte-wall sources are15140x10245 (620437200 RGBA8 bytes each):
S13 IDs4973/4979/4985/4991/4997/5003 and S16 IDs4475/4482/4489. Their restored
parents are1920x1080, but the child image/Corner Pin source and matte boundary are
not independently certified consumer crops. Several front-edge parents remain
empty because their effected child subtrees were omitted; retention is not paint.

The original unchanged JS Shape Path scripts were evaluated offline at every
active30fps timestamp in900–986. The all-sample endpoint/control hulls are:

- S13 duplicate/behind nodes: [-12237.9015,-26863.6597,14157.9015,993.8581].
  Their maximum per-frame viewport-intersected hull area is491048.8954 pixels.
- S16 duplicate/behind nodes: [-16493.1308,-24539.2593,9565.7130,332.7298].
  Their maximum per-frame viewport-intersected hull area is265180.9264 pixels.

These are conservative sampled geometry hulls, **not exact visible silhouettes or
native glow/blur alpha support**. All four ancestor transform chains are identity.
Most all-time geometry is offscreen, but cropping before spatial effects can remove
pixels that contribute to visible output. UI radius alone does not prove a safe
margin. Source scripts use `input.time.seconds`; no image decoding was used.

The full per-frame enabled/in-range layer counts, dimensions, native nesting,
effect controls and flags are in local `tasks/accuracy-W08/p047/allocation-inventory.json`
(SHA256 `1d4e806b31831db3f768b573a2b7402beb4315b5ae16781de5e0eb7e63e7b1af`).
The source hull samples are `allocation-source.json`
(SHA256 `9ce4462ff5e7a709b11dd8a1a99c0398f78462c577b1bcf12fcb694f7a9fd6cd`).
Reproducers are `allocation-inventory.py` and `allocation-source.js` beside them.
For the node targets, subtree counts are13 at S13 frames915–959 (one owner plus12
Shapes),25 at S16 frames960–971 (one owner plus24 Shapes), and zero outside those
occurrence windows. Counts are enabled/timing counts, not positive-alpha paint.
The inventory separately records all87 timestamps and all17 target subtrees;
unrelated whole-root Time Remap counts are only affine timing estimates.

### Exact remaining implementation blocker

`hierarchy/effect_support.rs::effect` deliberately returns Full for Gaussian Blur
and lacks a finite Outer Glow support profile. `child_demand_inner` consequently
marks these sources unbounded; `root_viewport::canvas_inner` rejects effected
owners. The existing shape blur-reach enclosure is not a native consumer-crop
certificate. An exact effect-aware source preimage and native crop-boundary support
are missing. The existing pointwise identity certificate cannot safely size these
children to their visible output. The nine matte/Corner Pin children add a separate
consumer-support boundary. No caps, effect removal, geometry pruning, clock changes,
or guessed padding were introduced.

No product fix, new RED/GREEN, or native render was performed in this follow-up
(0/2 authorized render calls used). No improved similarity is claimed: the last
valid baseline remains0.6112598479771674 and the failed candidate diagnostic remains
0.22590249572802315. Fix/render/publication remain **incomplete** pending a scoped,
proved effect-support design; no READY PR, label, or auto-merge qualifies.

## Subsequent approved intersection: native improvement, source gate blocked

The user approved finite effect-input intersection after the historical stop above.
The narrow identity 2D/unit-clock profile now intersects validated content with
consumer demand expanded by transparent-edge Gaussian and default Outer Glow
support. Analytical scalar-track hulls enclose animated reach; unsupported effects,
repeat-edge/explicit planes, spatial owner dynamics and unknown demand fail closed.
Geometry, effect records, native matte stages and checked source clocks remain.
This is not a canvas cap or a general Gaussian/Glow alpha-equivalence proof.

Semantic RED queue2839 reproduced both oversized-source omissions. GREEN2842:
48 passed, zero failed, 13 ignored, including animated support, native matted
source retention, smaller-content intersection and unsupported-profile negatives.
Formatter/source-guard2840 and compilation2841 failures are not semantic proof.

At the user's request, release2865 combined W08 with exact #4975 head
`8bb499aeb3f4b664dd176e7d9f9eff4290960e77`; private build commit
`8007439f514dde739d5abfe02eb4252571376fb2`, binary SHA256
`26e2f54827dc2b7cb22cb81c6797495dc9011710cb31eaee7da038523dfc90d3`.
A fresh unchanged full P047 conversion completed; AEP SHA256
`0c0613b2b8e586119b9ce693959e7a71ff622471e8c4147cc21303b6f2926a77`.
All17 target chains are retained, which is not proof every layer paints.
The four former allocation-wall node sources now have exact content/support
intersections: S13a behind/dup `26404×27886 → 1944×1019`, S16 behind/dup
`26162×25542 → 1976×384`, retaining12/24 sampled Shape layers. All nine
byte-wall sources changed `15140×10245 → 1920×1080`, retaining their source layer.

Managed native AE26.5x89 window job `85b6650a9cc14bd4ac728e1b3767690a`
rendered87 frames900–986 with no render-log errors or allocation failures and
verified READY. Unchanged full-resolution canonical RGB24/rgb-hybrid/.25s scoring
(12samples) improved `0.6112598479771674 → 0.6754147283819006`.
Separately approved full-movie job `37e7ad8d1c8a4904b2b4c82a759e9536` also
completed allocation-clean with READY:1920×1080,30fps,2336 frames,77.866667s.
The unchanged full scoring policy (312samples) produced `0.7091726417225561`,
above the user's #4975 baseline `0.6978`. Used one of two window renders and one
separately authorized full render. No images were visually inspected or model
scoring calls made. RGB improvement remains below0.95 and does not prove alpha,
audio, font substitution, arbitrary effect kernels or native editability.

Publication-only source was replayed onto main `92404c67a`; #4975 is
already merged, and no private validation commits were replayed. These native
results are pinned to the requested W03-combined build, not a fresh post-rebase
render. Required ordinary source gate2882 completed fmt/clippy/check, then failed:
1997 passed,4 failed,427 ignored. All four failures independently reproduced on
unchanged earlier main `0af9e7575` in queue2884 (not a reproduction on
publication base `92404c67a`):
- `accumulated_evaluations_do_not_reject_the_next_script`: dependency owner unavailable;
- `playback_remapped_group_input_window_is_not_erased_by_equal_content_windows`: None unwrap;
- `vector2_x_only_motion_uses_x_easing_forward_and_reversed`;
- `vector2_y_only_motion_uses_y_easing_forward_and_reversed`: equal-endpoint Bezier excursion.

No failures were waived or repaired outside scope. Later workspace suites were
not reached. Full PR review remains pending. **No READY PR, label or auto-merge**
is created while the required gate is red. Local receipts, manifests, bounds and
scores are preserved in `tasks/accuracy-W08/p047/ACCEPTANCE-v4-SOURCE-GATE-BLOCKED.json`
and the referenced managed job directories. The historical failed evidence above
is intentionally retained.
