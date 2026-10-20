# After Effects (`.aep`)

[Formats](README.md) · [CLI](../../apps/tesseract-conv/README.md) ·
[Library API](../../crates/aftereffects_file/README.md)

AEP import selects one reachable composition and creates editable `project.tsrct`.
Export creates a **new** `project.aep` from current FX content, not a patched copy
of the original native file. Neither direction renders video.

Import is partial/best effort. Native export is experimental: numeric structure
alone does not establish Adobe acceptance or matching rendered output.

## Choose the reference

| Need | Reference |
| --- | --- |
| Composition, timing, transforms, shapes, text, masks, media and audio | [Current AE support](../after-effects-support.md) |
| Plugin/control mappings and animation limits | [Effect reference](after-effects-effects.md) |
| Native expressions and sample sidecars | [Expression import](../ae-expression-evaluation.md) |
| PQ/HLG and ACES boundaries | [Color limits](../after-effects-support.md#hdr-conversion) |
| Linked Premiere picture scopes | [Hybrid packages](../hybrid-adobe-export.md) |
| Test receipts and known native failures | [Evidence guide](../after-effects-test-results.md) |

Use `inspect --metadata-only` and exact `--composition <item-id>` selection;
names are not selectors. Ordinary inspection checks reached audio/video, not
all stills/effects or rendering. Export defaults to 24 fps, configurable with
`--fps`. Read warnings and preserve the whole output package.

A supported static value does not imply animation or all combinations. Unsupported
content may be approximated, omitted or rejected. Font/layout, effects, clipping,
projection, timing and alpha/audio behavior remain bounded. Check the current
feature reference, then verify the converted result in the destination app.
