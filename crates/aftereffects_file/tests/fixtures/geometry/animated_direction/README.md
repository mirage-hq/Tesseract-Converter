# Reversed animated native Bezier control

Independently authored in Adobe AE26.5x89 through managed `Client.run_jsx`;
`author.jsx` saves and reopens its own private output and reads the actual keys.
`native.aep` SHA-256:
`579b5b9aec9f8f35f8c0782170e4840440f0096771e90273234e274ab2da5d42`.
Composition1, `W06 Animated Bezier Direction`, 320×240, square pixels, native24fps,
full2s, one closed three-vertex cubic Path, native direction3, Linear keys0/1s.
Public authored test artwork; no fonts, footage or private source assets.

Native direction3 reverses traversal while preserving the closed first anchor.
The first FX segment must go from[-80,-40] to[20,70], with controls[-65,-60]
and[-5,55], rather than to[70,-20]. The second key raises the first anchor20Y.
The regression preserves times/closure, fresh exports the imported content, and
edits all authoritative Path coordinates +30X before another full export.

Exact test:
`export_document::tests::animated_direction::native_reversed_bezier_keys_keep_vertex_and_tangent_correspondence_through_export_and_edit`.
RED Rust1493 (forward first segment); GREEN1495. Earlier1489 was a test float32
precision mismatch and is not semantic RED evidence.

`readback.jsx` is a managed function body, never a standalone Adobe launcher.
It independently opens generated and +30X-edited AEPs and checks native keys,
vertex ordering and swapped relative handles. The two generated artifacts and
readback provenance/hashes are in `evidence.json`; they are not native-source
or render oracles. Adobe acceptance/readback passed after cleanup/READY.

No native30fps reference or long-term Asset was produced under this tiny-probe
budget. RGB comparison, alpha, audio, general animation/easing and original49
movie improvement remain unmeasured. This control provides editable semantic
proof only; the PR remains draft for inherited full-suite and proof gaps.
