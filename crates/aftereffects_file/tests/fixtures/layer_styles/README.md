# Independent Adobe Layer Styles fixtures

`styles_static_adobe.aep` was authored in Adobe After Effects 26.5x89 (build89),
not by this converter. SHA-256:
`0160adcfd09c70f4f13b7eedbb539b57e855312fbb0c7b2b44513aef8f418273`.
`styles_static_adobe.readback.json` records exact nondefault controls, enabled
native identities, source/composition/layer identities and authoring provenance.

Each of the nine compositions has one 96×64 blue solid in a 320×180, two-second,
24fps square-pixel composition. Apply the named style using Adobe's **Layer →
Layer Styles** menu, then set the controls recorded in the readback to reproduce
these cases. A fresh AE session's scripted `executeCommand(9000..9008)` silently
failed to activate styles; actual native menu selection in an exclusively owned
session succeeded. Merely enumerating default hidden style groups is not proof.

| Composition | ID | Subject layer ID |
| --- | ---: | ---: |
| DropShadow | 1 | 15 |
| InnerShadow | 16 | 29 |
| OuterGlow | 30 | 43 |
| InnerGlow | 44 | 57 |
| BevelEmboss | 58 | 71 |
| Satin | 72 | 85 |
| ColorOverlay | 86 | 99 |
| GradientOverlay | 100 | 113 |
| Stroke | 114 | 127 |

All nine targets are linked to concrete fresh-import assertions in
`structure_document::tests::native_layer_styles`; none is merely a supporting or
unregistered composition. References preserve native bytes and source FPS, but
sample Adobe at30fps: 60 frames, complete duration/canvas. The MP4s are immutable
long-term Assets, freshly downloaded and hash-verified, indexed in
`../aep_video_references.json`. **Do not add reference videos to Git.**

`<Style>.edited.fx.json` files are separate explicitly edited FX export inputs.
They change the imported controls and must be exported from current FX content;
they are not Adobe-native sources or hidden AEP replay data. Their independent
Adobe export readback and its limits are recorded separately in
[`layer-styles-results.json`](../../../../../docs/after-effects-evidence/layer-styles-results.json).

Run the bounded CPU assertions through the standalone converter Makefile:

```sh
make -C opensource/conv test-aftereffects-file filter=layer_styles
```

See [Layer Styles evidence](../../../../../docs/after-effects-evidence/layer-styles.md)
and the [support ledger](../../../../../docs/after-effects-support.md) for actual
execution, source/reference hashes, approximations, unsupported Pattern Overlay,
remaining animation work and unmeasured RGB/alpha fidelity. Native reference
publication and our own structural round trips are not render-equality proof.
