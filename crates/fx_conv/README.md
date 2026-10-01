# Converter contract

`fx_conv` is the dependency-light boundary between format libraries and frontends.
It does not parse files, change the FX model, load Adobe, or publish files itself.

- `ImportToTesseract` and `ExportFromTesseract` are independent capabilities.
- Import lists native scenes with `list_import_targets`: stable ID, display name,
  dimensions, FPS, duration, and direct layer/track counts where available.
  Unknown metadata is `None`; listing never probes media or converts content.
  Select one target by native ID; omission is allowed only for exactly one
  selectable scene, counting nested scenes too. Dependencies of that scene are
  still converted. Imports produce exactly `project.tsrct`, not a batch.
- Each direction has typed `Options`, `Error`, and `Diagnostic` associated types.
  All settings, including After Effects export FPS, go through the trait.
- `ConversionReport<D>` returns native `diagnostics` and a complete `artifacts`
  inventory. `into_common()` projects diagnostics for a format-agnostic frontend;
  callers that need native fields can keep the original report.
- `ConversionDiagnostic` supplies a stable code, omission/approximation/warning
  kind, contextual target identity, and the complete existing display text.
  Do not parse messages to infer semantics. Legacy AE codes and Premiere's
  `Omission` collection mix omissions and approximations, so their common kind
  remains `Warning` unless the native type proves a more specific effect. This
  does not mean lossless conversion; Premiere scope codes still identify the
  affected unit (feature, occurrence, track or sequence).

## Validation and publication

`Check` and `Write` use the same conversion and validation. Their reports describe
identical files for unchanged inputs/options. Artifact paths are unique and
relative to the requested output directory, without parent traversal. Include
the selected project and all external packaged export media; exclude
directories, temporary files, and media embedded inside a `.tsrct` archive.

`Check` reports **planned** artifacts without creating the final output directory.
It may use temporary staging and must clean it on return. It does not guarantee
that subsequent filesystem writes will succeed. `Write` reports artifacts only
after publication succeeds. Both modes require an existing output parent and a
fresh destination, including rejection of dangling symlinks. Derive inventories
from the conversion/publication plan, not a post-write filesystem scan: Check
must work without published files too.

Format libraries own media identity checks, validation, staging and publication.
Ordinary failures clean up owned partial outputs without deleting foreign files.
This is **not a crash-atomic guarantee**: interruption can leave partial output.
Only successful Write completion establishes a complete package. Success never
establishes Adobe acceptance, editable feature coverage or render fidelity.

## Progress observations

`import_to_tesseract_with_progress` and `export_from_tesseract_with_progress`
accept a borrowed `Progress` observer; existing entry points remain available.
`Progress::default()` does nothing. `Progress::new(&callback)` observes synchronous
`ConversionProgress` events (the callback may execute on a scoped worker, so it
must be `Sync`, prompt and non-panicking).

Start unmeasured operations with `stage(name)`. For a known denominator, call
`phase(name, unit, total)`, then `update(completed)` after each processed unit,
including diagnosed omissions. Use real work such as clips or script tracks,
not a guessed percentage assigned to preparation steps. `started` resets timing,
including consecutive hybrid scopes with identical names/totals. Do not interleave
updates from separate active phases: finish or replace a phase before another
scope starts. Observations are phase-local, not whole-command completion, and
must not influence conversion values or error handling. Only the caller's
successful return establishes command success.

## Adding a format

For a future `svg_file` (not implemented here):

1. Implement import, export, or both against these traits. Keep SVG settings in
   its own typed options; do not add SVG-specific fields or dependencies here.
   Reuse the existing FX/document archive model rather than creating another IR.
   Importers must implement metadata-only target listing and explicit selection
   when multiple scenes exist; names are not unique IDs.
2. Return native typed errors and diagnostics. Implement `ConversionDiagnostic`
   for the native diagnostic type, or use the provided `Diagnostic` directly.
   Preserve unsupported siblings through the format's existing best-effort policy.
3. Enumerate exact planned document/media filenames in the successful report.
4. Add a CLI adapter and one entry to the static registry in
   `apps/tesseract-conv/src/`. The entry owns the format name, extensions and
   optional directional routes (import couples conversion and target listing).
   `inspect INPUT [--json]` exposes the targets; a native inspector may retain
   richer format-specific inventory. Typed option parsing belongs in that adapter;
   the shared runner uses only the traits. New format-specific flags may require
   adding CLI arguments; registration is not a dynamic plugin or arbitrary option
   schema. Unsupported directions must be absent, not handlers that silently
   substitute another conversion.
5. Exercise the shared contract pattern in
   `apps/tesseract-conv/tests/conversion_contract.rs`: Check/Write parity, complete
   artifact inventory, unchanged input, fresh destination/symlinks, malformed
   input and no output on failure. Add independent native feature evidence where
   relevant; contract tests are not feature-fidelity tests.

The CLI deliberately supports only conversions to/from the Tesseract hub, not an
implicit lossy chain between two native formats. Static SVG timing/selection
policy must be designed explicitly rather than inheriting AE or Premiere FPS.

## API migration

- `ConversionReport::omissions` is now `diagnostics`, since successful conversion
  also reports approximations and non-omission warnings. Reports additionally
  contain `artifacts`; there is no empty default success report.
- `AfterEffects` export uses `AfterEffectsExportOptions`, not `()`. Use
  `&Default::default()` for the existing 24fps behavior. The prior explicit
  `export_from_tesseract_with_options` method forwards to the trait for source
  compatibility; it is no longer an alternate implementation.
- Imports now select exactly one target. Premiere no longer imports every root
  sequence, and AE no longer chooses the first root/first composition. Both
  require an explicit native ID when more than one target exists. Premiere's
  ordinal filenames (`001-Name.tsrct`) become `project.tsrct`.
- The CLI offers `inspect INPUT [--json]` for native inventory.
- Premiere's older free functions still return `Vec<Omission>` for compatibility.
  Use the traits for typed export settings and artifact inventories.

This refactor changes neither direction's feature mappings nor their independent
proof status. Consult the format support ledgers for those limitations.
