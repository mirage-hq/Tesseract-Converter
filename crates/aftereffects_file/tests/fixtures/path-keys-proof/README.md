# Independent native Shape / Mask Path panel

**User-accepted delivery milestone; strict visual proof remains incomplete.**
Native controls pass in both directions; both fresh AEP exports and the two Mask
FX renders pass the registered RGB/alpha gates. The two Shape FX renders still
fail. The user accepted these documented residual differences and deferred more
rendering changes. Do not treat publication, parser success, or the passing
export direction as an import fidelity pass.

[`evidence.json`](evidence.json) registers every source/composition target,
feature/direction, executable assertion, committed reference path and historical
measured result. The eight Adobe references live in
`tests/references/aep/path-keys/` (for example, [shape-v2.mov](../../../../../tests/references/aep/path-keys/shape-v2.mov)),
including four lossless-alpha MOVs freshly rendered from the checked-in AEPs.
[`artifacts.json`](artifacts.json) retains identities for generated FX outputs,
fresh exports and control artifacts, not the committed Adobe references. This is
a bounded local evidence record, not a new CI gate.

## Additional held-disappearance case

`hold-disappearance-v1.aep` is a separate, independently Adobe-authored 320×180,
30fps, two-second case (composition 1). It does not replace the historical panel
or relax `policy.json`. The triangle alternates between three and one vertices
at 0/500/1000/1500ms, with green fill and a red 16px round-cap stroke.
`hold-disappearance-v1.fx.json` supplies the explicit editable JS Path input.
[`hold-disappearance-v1-evidence.json`](hold-disappearance-v1-evidence.json)
records the immutable reference, executed checks and remaining differences.

Fresh import asserts the four editable keys and one-vertex controls. Fresh export
asserts native geometry plus separate group-opacity keys: geometry alone left a
small round-cap dot in the first trial. Partial stroked-contour disappearance
remains unsupported/diagnosed. This case establishes neither a new broad import
render pass nor compound-particle identity. Source, export acceptance/control
readback, diagnostic RGB differences and alpha limitations are kept separate.

## Native provenance and identity

- Independently authored in Adobe After Effects **26.5x89**, build 89, via
  [`author.jsx`](author.jsx), without converter output. Four `*-v2.aep` sources;
  each selected root is item **1**, square pixels, 640×360, 24fps, five seconds,
  display start zero, 8 bits/channel. No external footage, fonts or plugins.
- Shape uses a visible cyan **8px Stroke**, initially open with three vertices,
  then closed with four. Mask uses an orange 640×360 Solid and a closed contour
  with a Hold topology change. Five keys at 0 / .5 / 1.5 / 2.5 / 4.5 seconds;
  Linear, Hold and the bounded zero-speed Bezier/mixed-ease subset.
- Explicit FX edit: every anchor/control moves **(+17, −9)** and the first key
  moves to **125ms**. `author.jsx` separately authors the same edited oracle;
  [`edit_fx.py`](edit_fx.py) edits the freshly imported ZIP_STORED document.
- Native render: `aerender -s 0 -e 119 -renderSettings 'Use this frame rate: 30;
  Quality: Best; Resolution: Full' -OMtemplate 'Lossless with Alpha'`. Output is
  150 frames / 30fps / five seconds; source FPS and bytes are unchanged.
  MP4 viewing references are x264 CRF18 derivatives of that native sampling,
  not resampled FX renders. Lossless Animation MOVs are the scoring oracle.
- All eight reference videos are checked into Git. The four MOVs have been
  decoded as 640×360, 30 fps, 150-frame alpha-bearing Animation QuickTime.
  The MP4s remain RGB viewing references; they do not replace alpha proof.
- Adobe's color-working-space setting was not separately captured; do not infer
  color-managed/HDR or arbitrary-project coverage from this 8-bit panel.

## Executable checks and reproduction

`verify.py::native_import` verifies the fresh native import's actual Path binding,
visible Shape / mask-guide ownership, five unique keys, times, exact cubic
controls, closedness and easing. `verify.py::native_exports` compares independent
Adobe-reopened edited oracles with **fresh exported AEP** readbacks: key times,
vertices/tangents, closure, incoming/outgoing interpolation and temporal ease.
Maximum control errors are recorded in `evidence.json`; all are below 0.001px.
These are Adobe-script observations, **not manual UI inspection**.

The artifact directory used by `verify.py` contains `authoring.json`,
`export-readback.json`, `{shape,mask}-oracle-reopen.json`, and fresh
`import-{shape,mask}/project.tsrct`. Produce readbacks with `readback.jsx` in an
explicitly approved, owned empty Adobe session; never replace unknown user work.
Receipts and full generated reports stay in ignored scratch; their identities
are pinned in the evidence record. **Historical attribution is procedurally
observed, not cryptographically attested at execution:** the original Adobe
receipts label projects but do not record opened-file hashes. The post-run
manifest protects against subsequent substitutions; it does not retroactively
prove that causal link. Root ID/name, canvas, FPS/duration and keyed property/
layer identities are additionally checked. No reference videos belong in Git.

Historical scores describe the earlier executed panel; the newly checked-in
Adobe MOVs do not by themselves establish a fresh converter fidelity pass.
Do not change comparison floors to make a case pass. Offline guard tests:
`python3 -m pytest -q test_artifact_identity.py`.

```sh
python3 verify.py <owned-artifact-directory>
python3 edit_fx.py <fresh-import.tsrct> <new-edited-input.tsrct>
# Render each original and edited input with a matching source-built CLI:
tsrct export --project <input.tsrct> --output <new-output.mov> \
  --format prores --fx-solo main:1 --fps 30
python3 score.py <owned-artifact-directory> --validation <validation_cli>
```

The scorer reads the four Adobe-native MOVs from `tests/references/aep/path-keys/`
and expects generated `native/{shape,mask}-export.mov`,
`{shape,mask}-import.mov`, and `{shape,mask}-edited.mov` in the owned artifact
directory. It refuses an existing
`scores/` directory and exits nonzero if any gate fails. Keep failed runs intact.
Never run Python with `-O`; the structural assertions are part of the test.

For exported AEPs, root and nested display names may repeat. The executed proof
selected root **item 1**, validated its canvas/duration, and renamed only that
root in a separate render-only copy (`PATH_PROOF_EXPORT_ROOT_shape` / `_mask`).
The original fresh export was not overwritten. Ambiguous-name trial renders
were retained diagnostically and excluded from the recorded final comparisons.

## Measurement policy and remaining defects

[`policy.json`](policy.json) predates rendering: all 150 full-resolution RGB
frames and 16 critical RGBA/alpha frames must each reach **0.99**. No resizing,
threshold reduction, key baking, JS, source replay or oracle replacement.
The declared comparison convention is premultiplied black: Adobe already uses
it; the scorer derives a premultiplied copy of straight-alpha FX video for RGB
and RGBA. The alpha gate reads the **original lossless alpha planes**, not that
derivative. Early runs omitted convention normalization; their RGB measurements
are diagnostic only and do not establish fidelity.

The proof exposed a native-export bug: ProRes inherited the opaque-video MSAA1
policy. The native API now requests MSAA4 for ProRes only (adapter fallback
preserved). A raw-GPU regression shows 0 → 222 fractional-alpha pixels before
encoding, and policy tests retain MSAA1 for Auto/H.264/HEVC. Both tests passed;
all four FX clips were rerendered. No Adobe oracle was rerendered or replaced.

Shape coverage still fails. Shared renderer tessellation expands strokes by
0.5 screen pixels for historical Skia matching, so an authored 8px stroke is
not tessellated at its authored width. This is a remaining suspected cause;
changing shared PAG/FX behavior is **not yet implemented or validated**. The
edited-Shape RGB residual also remains unresolved. Root-content isolation
excludes the imported opaque document-preview background; full-project alpha
preservation is not claimed. See the [support/limitation ledger](../../../../../docs/after-effects-support.md#native-shape-and-mask-path-keys).
