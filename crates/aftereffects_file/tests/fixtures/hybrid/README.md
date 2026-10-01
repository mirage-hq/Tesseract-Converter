# Fresh root identity probe (not a rendering oracle)

`rect-identity.fx.json` is an explicitly authored editable input, **not an
Adobe-authored source**. Its single Rect has no effects, text, external assets or
animation. This fixture proves only the bounded generated-root/staging contract;
it is not a completed conversion-feature or fidelity case.

## Executed native observation

On 2026-09-28, the current CLI exported this document at **30fps**. After Effects
**26.5x89 / build89** independently opened the exact generated AEP and returned:

- name `Hybrid_Rect_Identity_30fps`, item ID **1**;
- `CompItem.dynamicLinkGUID`: **`00000001-0000-0000-0000-000000000000`**;
- **1920×1080**, square pixels, **30fps**, **2 seconds**;
- one enabled native **ShapeLayer**, `Editable identity rectangle`, in/out **0–2s**.

The guarded helper opened only the fresh task-owned file, closed it without
saving, and verified unchanged bytes afterward. No render, Premiere operation,
asset upload, or native control-value comparison ran. Numeric IDs for other
compositions and GUID behavior for imported/resaved files were **not tested**.

| Artifact | SHA-256 |
|---|---|
| Exact checked-in editable JSON | `b09bc8627d0c2183045c4b2046e698706cd111b83305114c5b22bb7757e79a08` |
| Local input TSRCT container | `0c2b92475e72cd0e408a96cbae14f94199e7bc3d38e443f7f38cce3a6aa43247` |
| Generated AEP (62,786 bytes; historical) | `c398e1be996bae5c32d8fcb9b3e746cfe8205941b32dde0ad7b863e9eab8d157` |

Raw generated files and receipts remain in task-local
`hybrid-adobe-scratch/generated-rect-identity-1/`, outside Git.

## Re-observation of the current bytes

The writer later moved property clocks to the 30fps composition clock. That
changed 143 `tdb4` clock values of this AEP (24 × 1,024 → 30 × 1,024 ticks),
so the historical bytes above are no longer generated. On 2026-09-29 the same
After Effects build reopened the current bytes and confirmed every root fact
above.

- Verdict: **PASS**, structural reopen only; negative controls evaluated false,
  and the project closed without saving with unchanged bytes.
- Candidate: `eca9410e1`; its merges with main `1404193b7` and `7a41fe07b` emit
  identical bytes.
- Export: generated AEP (62,786 bytes)
  `0fe75b59a4118b48a965ab1001a9ee14b48e48e614c73a191b07b89e69939290`.
- Render: N/A. No render, upload, score or control-value comparison ran.

Probe scripts and timings remain task-local, outside Git. The supplementary CPU
test
`adapter::export::staging::tests::staged_root_matches_the_nonempty_native_observed_rect_bytes`
freshly exports the checked-in JSON, pins these re-observed AEP bytes, and checks
the root metadata and layer inventory. It does not launch Adobe or independently
verify the GUID; that observation is the bounded native readback above.

## Separate failed Text probe

A fresh import/export of the pinned native title from the Premiere hybrid fixture
produced AEP SHA-256
`44b020e6c0848b45a50a67a46f4b3848b281acec4bbdef628d0ba600a3144ea4`
(71,090 bytes). AE rejected opening it with
`Error reading the text layer. Skipping the text layer.` Root cause is unknown.
The partial unsaved project had three non-Text layers instead of four; its GUID
readback is **not successful native acceptance**. The partial project was later
identity-checked and discarded without saving with explicit user approval.

Text compatibility is not repaired or established by the Rect probe. Neither
probe supplies Adobe-rendered MP4/long-term Asset evidence, fresh-download
verification, RGB/alpha/audio measurements, or generated-Premiere link proof.
See the [hybrid ledger](../../../../../docs/hybrid-adobe-export.md).
