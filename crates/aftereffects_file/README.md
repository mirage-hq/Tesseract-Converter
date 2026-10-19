# After Effects file adapter

`aftereffects_file` reads native `.aep` files, imports one selected composition
into editable FX, and exports supported current FX content to a fresh native AEP.
It does not launch Adobe or restore hidden original AEP records. Import is best
effort; native export is experimental.

Use the [CLI](../../apps/tesseract-conv/README.md) for commands and
[AE support](../../docs/after-effects-support.md) for direction-specific mappings,
losses and known failures.

## Rust API

The `AfterEffects` adapter implements `fx_conv::ImportToTesseract` and
`fx_conv::ExportFromTesseract`. `list_import_targets` lists native item IDs,
names and available composition metadata without media probing.

```rust,no_run
use aftereffects_file::{AfterEffects, AfterEffectsExportOptions, AfterEffectsImportOptions};
use fx_conv::{ConversionMode, ExportFromTesseract, ImportToTesseract};
use std::path::Path;

fn main() -> Result<(), aftereffects_file::AepConversionError> {
    AfterEffects.import_to_tesseract(
        Path::new("source.aep"), Path::new("checked-import"),
        &AfterEffectsImportOptions {
            composition: Some(1),
            ..Default::default()
        },
        ConversionMode::Check,
    )?;
    AfterEffects.export_from_tesseract(
        Path::new("edited.tsrct"), Path::new("checked-export"),
        &AfterEffectsExportOptions { fps: 30.0, ..Default::default() },
        ConversionMode::Check,
    )?;
    Ok(())
}
```

Select the intended composition by exact item ID. Omission is valid only when
there is exactly one composition, including nested compositions. Check reports
planned artifacts without publishing a final directory. Write produces
`project.tsrct` on import or `project.aep` and required assets on export.
Both require an existing parent and a fresh destination.

## Import options

`AfterEffectsImportOptions` contains:

| Field | Contract |
| --- | --- |
| `composition` | Exact native item ID, or `None` only for an unambiguous single composition. |
| `expression_samples` | Optional path to source-bound native sample JSON. |
| `available_fonts` | Optional set of available PostScript names; declares availability, not font bytes. |

### Explicit font availability on AEP import

Without `available_fonts`, authored font behavior remains unchanged. With an
explicit set, absent faces become a declared same-style Inter face when available,
otherwise Inter-Regular, with the original/substitution named in a warning.
An empty set declares no available faces. The adapter does not discover, download,
install or package fonts for this option. Substitution does not prove typography.

### Expression samples

Native sample sidecars are optional and must match the exact source AEP hash,
composition, typed target identities and clocks. Unsupported or conflicting
captures are rejected or diagnosed; they are not relabeled as fresh evaluator
results. Mapping captured values does not restore the original expression program.
See [numeric expression import](../../docs/ae-expression-evaluation.md) and
[the sample declarations](src/expression_samples.rs) for the supported schema.

## Media and source assets

Native resolution authenticates reached local sources and supported collected-file
layouts. It does not search arbitrary directories or infer identities from names.
Missing or incompatible used audio/video may block import. The separate
[inspection and preparation workflow](../../docs/media-preparation.md) can use
explicit source-bound replacement maps; import itself does not silently transcode.

PSD, selected raster formats, bounded OpenEXR and PDF-compatible Illustrator
sources have format-specific restrictions. AI import lowers a narrow vector
subset to editable shapes, not a general PDF/Illustrator interpreter. Review
[media limits](../../docs/after-effects-support.md#media-and-audio) before promising support.

## Export options and scripts

`AfterEffectsExportOptions.fps` defaults to 24 and accepts a positive rate up to
240. `hdr` optionally selects PQ/HLG; omitted means legacy SDR.

Export prepares admitted layer-local scalar, ShapePath and text-content scripts
as editable keys on a four-times-output-FPS grid rounded to integer milliseconds.
Unsupported dependencies, clocks and script results remain diagnosed failures or
omissions. Sampling can miss narrow pulses, intersample geometry or topology
changes. Native expressions/scripts are not restored. VM limits do not provide
wall-clock or heap isolation; process untrusted programs with external limits.

### Embedded fonts

Export uses hash-verified embedded physical fonts to resolve supported native
font identities and bounded text geometry. Missing, ambiguous or unsupported
faces retain diagnostics; arbitrary system-font lookup is not a substitute.
Used font files are delivered unchanged under `fonts/`, with `fonts/manifest.json`.
They are **not activated**: install licensed fonts before opening/rendering in AE.
Font identity and packaging do not establish glyph shaping or pixel fidelity.

## Color management

Import emits legacy FX policy. Optional HDR point mappings and explicit PQ/HLG
export have bounded, experimental support; they do not establish effect, footage,
interpolation, alpha or precision fidelity. Validated ACES policy can persist in
FX, but native ACES export rejects. See the
[FX policy](../fx_schema/src/color_management.rs) and
[HDR/ACES limits](../../docs/after-effects-support.md#hdr-conversion).

## Picture staging and linked projects

Package coordinators can stage picture-only exports, receive file/composition
identities and resolve a linked AEP by its exact GUID. Import allocates independent
editable identities per occurrence. Linked AEP sound is not implicitly recovered.
The [hybrid coordinator](../../docs/hybrid-adobe-export.md) owns Premiere routing;
this crate does not depend on the Premiere adapter.

Staged files and source identities remain owned/checked through publication.
Failures must not delete foreign files. Output freshness and cleanup are not a
crash-atomic transaction or a hostile-filesystem guarantee.

## Diagnostics and tests

Diagnostics name the source composition/layer and describe omissions or
approximations. The [code inventory](../../docs/after-effects-support.md#diagnostic-code-inventory)
is checked against emitted codes by an offline test. A successful conversion can
still lose content; absence of warnings is not full support.

From the workspace root:

```sh
make test-aftereffects-file
make test-aftereffects-file filter=path
make check-aep-support-ledger
```

These are software contract tests, not Adobe rendering. Use the
[fixture inventory](tests/fixtures/README.md),
[effect controls](../../docs/formats/after-effects-effects.md) and
[evidence guide](../../docs/after-effects-test-results.md) to distinguish native
acceptance, control readback and independent render comparisons. Some historical
native exports fail Adobe acceptance; do not infer acceptance from own-reader tests.
