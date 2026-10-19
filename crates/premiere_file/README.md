# Premiere file adapter

`premiere_file` reads native `.prproj` projects and converts one selected
sequence to editable FX. It exports current FX content to a new Premiere package,
without launching Adobe. It accepts plain UTF-8 XML and gzip-compressed projects;
it is not a lossless native record archive.

Use the [CLI](../../apps/tesseract-conv/README.md) for commands and the
[Premiere format reference](../../docs/formats/premiere.md) for supported edits.
The CLI owns cross-format linked-AEP coordination; the library's native export
does not automatically route unsupported content through After Effects.

## Rust API

Use the shared conversion traits for explicit options, Check/Write modes,
diagnostics and artifact inventories:

```rust,no_run
use fx_conv::{ConversionMode, ExportFromTesseract, ImportToTesseract};
use premiere_file::{FrameRate, Premiere, PremiereExportOptions, PremiereImportOptions};
use std::path::Path;

fn main() -> Result<(), premiere_file::ConversionError> {
    let import = PremiereImportOptions {
        sequence: Some("GUID-selected-from-list_import_targets".into()),
    };
    Premiere.import_to_tesseract(
        Path::new("source.prproj"), Path::new("checked-import"),
        &import, ConversionMode::Check,
    )?;
    Premiere.export_from_tesseract(
        Path::new("edited.tsrct"), Path::new("checked-export"),
        &PremiereExportOptions {
            frame_rate: Some(FrameRate::Fps25),
            ..Default::default()
        },
        ConversionMode::Check,
    )?;
    Ok(())
}
```

`list_import_targets` returns IDs and metadata without opening media. Select the
intended exact GUID, not `targets.first()` or a display name. `None` is valid only
for a project with exactly one selectable sequence, including nested sequences.

`ConversionError` preserves its source chain. `is_unsupported()`,
`is_missing_media()` and `is_io()` classify errors without parsing display text.
Legacy `premiere_to_tesseract` / `tesseract_to_premiere` helpers remain available;
use the traits when you need export options or full artifact reports.
`PrProjectFile` exposes the supported native project model and read-only inventory.

## Timing

Import projects native ticks into integer milliseconds. Boundaries round to the
nearest millisecond, ties forward; distinct keys can collide and lose animation
with diagnostics. Video export snaps placement boundaries to the selected frame
grid; audio keeps millisecond boundaries. A placement that collapses on the output
grid can reject or omit its enclosing nest.

### Export rate

`PremiereExportOptions.frame_rate` defaults to 30 fps. Supported choices are
23.976, 24, 25, 29.97, 30, 50, 59.94 and 60. The original sequence cadence is not
stored as an export default in FX. Linked-AEP rate restrictions belong to the
[CLI coordinator](../../docs/hybrid-adobe-export.md#linked-scope-rates).

### Nested sequences

Supported occurrences become independent editable groups with their placement,
source window and bounded Motion/Opacity controls. Repeated source sequences do
not retain shared editing identity. Export writes supported groups as generated
nested sequences. Variable/reverse clocks, coverage, source tails and differing
canvases have narrower admission; do not assume every group is exportable.

Constant speed, reverse and explicit frame holds have bounded editable mappings.
Unsupported time-remap curves may use a diagnosed constant-speed current-source
selection where safe; they are not equivalent nonlinear timing.

## Media and fonts

The library authenticates media paths/hashes, checks reached source clocks and
packages admitted bytes. It does not silently transcode or guess relocation by
basename. Active replacement/enhanced media remains distinct from unused originals.
See [media preparation](../../docs/media-preparation.md) for source-bound maps.

### Explicit media relinking

`--media-relink` in the CLI uses the library's explicit relocation APIs before
normal path admission. A manifest identifies the unchanged project, selected
sequence and each media UID/authored path, and binds it to an absolute local file
and SHA-256. A separately prepared media map binds to that resolved original.
Prepared replacements are not valid relocated-original identities.

The version-1 JSON shape is below. Replace every placeholder with the exact
saved identity or a computed lowercase SHA-256; `local_path` must be absolute.

```json
{
  "version": 1,
  "source": {
    "format": "premiere",
    "sha256": "<original project SHA-256>",
    "target": "<selected sequence GUID>"
  },
  "bindings": [{
    "media_uid": "<native media UID>",
    "authored_path": "C:\\media\\source.mov",
    "local_path": "/absolute/local/source.mov",
    "sha256": "<original media SHA-256>"
  }]
}
```

Use exported `MediaRelink`, `MediaRelinkBinding` and `ValidatedMediaRelink` types
for Rust callers. The [declarations](src/media_relink.rs) define validation.
Changed hashes, conflicting candidates or mismatched identities reject.

### Fonts

Imports retain supported PostScript identities but do not fetch font binaries.
Export resolves supported font identities from the document/archive; unusable or
ambiguous faces can omit text. Transporting or naming a font does not install it,
prove redistribution rights, or guarantee Adobe layout. Rich text, box layout,
shadows and Source Text animation have separate restrictions.

## Effect stacks

Mapped effects retain supported order, bypass state, static controls and compatible
keys. Unknown decorative controls can lose only their feature, while unsupported
coverage-changing effects, masks or dependent clocks can omit the affected
picture. Independent siblings remain where safe. Export uses current edits,
not replayed original effect blobs. Exact host/control limits belong in the
[format reference](../../docs/formats/premiere.md#motion-compositing-masks-and-effects).

Film Impact Pop is disabled by default. The explicit consent API is
`PremiereImportOptionsWithConsent`; ordinary import APIs do not enable the
sampled approximation. See the [Pop and Stroke limits](../../docs/formats/premiere.md#film-impact-profiles).

## Saved AE capsule graphics

Supported saved Essential Graphics bindings can recover static Text/Shape content
and selected saved controls through the AE graphic decoder. This is not a general
MOGRT engine, expression/controller restoration, or live Dynamic Link authoring.
Unsupported binding/layout forms diagnose losses; source files are not rewritten.

## Color management

Import emits legacy FX policy. Export accepts validated legacy policy and an
optional `PremiereExportOptions.hdr` PQ/HLG target. ACES policy persists in FX but
native ACES export rejects. Native settings, 8-bit paint storage and point-color
conversion do not establish HDR render precision or effect/interpolation fidelity.
See the [FX policy](../fx_schema/src/color_management.rs) and
[HDR limits](../../docs/after-effects-support.md#hdr-conversion).

## Prepared export for package coordinators

Prepared export retains native conversion results, source locations, diagnostics
and package identities so a caller can add supplied linked picture scopes without
rerunning unrelated conversion. Use the public preparation APIs in
[src/lib.rs](src/lib.rs); the CLI coordinator implements automatic AE routing.
Picture replacement must preserve dependencies and independent native audio.

## Validation and publication

Check and Write use the same conversion/validation. Check returns planned files;
Write publishes `project.prproj` and required media only after validation.
Destinations must be new and their parents must exist. Publication uses hard links
where available, with exclusive-copy fallback for unsupported destination filesystems.
Neither path replaces an existing entry. Ordinary failures clean owned artifacts;
process interruption can leave partial output. This is not a crash-atomic
transaction or protection against a hostile filesystem.

## Unsupported edits

A successful result can contain omitted properties, effects or occurrences.
Not every loss has a diagnostic: rounding, quantization and some native metadata
losses are inherent. Preserve original projects and review output in Premiere.
Native acceptance and rendered picture/alpha/audio comparisons are separate from
parser, writer and internal round-trip tests.

## Tests

From the workspace root:

```sh
make test-premiere-file
make test-premiere-file filter=motion
make clippy-premiere-file
```

See [Contributing](../../CONTRIBUTING.md) for FFmpeg/toolchain requirements and
[the fixture index](../../tests/README.md) for immutable references and proof limits.
