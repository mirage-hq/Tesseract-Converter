# PDF-compatible AI structural fixtures

These three `.ai` files are **specification-built PDF 1.4 samples**, not files
saved by Adobe Illustrator and not native After Effects proof. They exercise the
converter-local restricted whole-artwork profile only:

| File | SHA-256 | Structural purpose |
| --- | --- | --- |
| `spec_case_1.ai` | `2adf359be7da341e586ff67a1335748b84d310116c04bee39eb10a8e73f29c38` | compound rectangles, even-odd fill, cubic path, paint order |
| `spec_case_2.ai` | `1e2dc6daff0d427d5e488357c422ae000946dfeb3b68d130177a616be4d44448` | open strokes with discriminating cap/join values |
| `spec_case_3.ai` | `af90fc16ee8ed66edf96f448eb737fa29efcff608a74d4cc21d687679d5215ed` | nested graphics state, affine transform, fill/stroke order |

The executable tests assert editable FX `Shape` paths and paint controls, stable
order/IDs, affine lowering, and absence of asset/`JsScript` fallback. They do not
establish Illustrator-produced-file compatibility, AE source-selector semantics,
Adobe opening, RGB/alpha fidelity, or FX → AEP/AI export.
