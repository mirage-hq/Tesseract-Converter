# FX schema

`fx_schema` owns the canonical FX data model and its checked-in JSON Schema
artifacts:

- `fx_composition.schema.json` is the public FxComposition authoring schema.
- `editable_fx_composition.schema.json` is the checked-in editable project-model
  subset consumed by standalone users.

The editable-schema generator is not part of this standalone repository. Its
maintainer-owned generation currently also consumes a private project-wire
schema from the enclosing repository.
Consequently, a standalone checkout can use and validate the checked-in schemas
but cannot regenerate `editable_fx_composition.schema.json` from its source of
truth. Do not copy the private schema into this repository or treat hand-editing
the derived artifact as regeneration. A public source-of-truth and generator are
an unresolved publication blocker requiring separate design and approval.

Readers retain the original JSON alongside immutable, structurally checked data.
Historical records, unknown fields, identifiers and script strings are preserved
without migration or interpretation. Checked edits replace records atomically.
This crate does not evaluate animation, remap clocks, generate identifiers or
scripts, resolve presets, or hold rendering caches.

The declarations in this crate own every persisted FX field/variant inventory,
including layer/source and effect records, styles, color and audio controls,
animators/keyframes, time remaps, document envelopes, and historical wire shapes.
`*_declaration.rs` macros provide reader projections where nested types, strictness,
legacy input handling or omission policies differ. Product callbacks add runtime
state without independently defining persisted fields. Migration, evaluation,
allocation and caches remain outside this public workspace. Identical value types
are shared directly rather than expanded into separate projections.

The product schema generator reads these canonical declarations for Rust docs and
default metadata; callback emission shells and runtime wrappers are not competing
schema sources. Since revision 67, the schema uses the existing Video, Audio and Group kinds with required
`playback: { type: "windowed", inputRange, mapping, inputOffsetMs }`. Video and
Audio also require an independent `sourceRange`. The mapping is either
`{ type: "linear", input, output }` or `{ type: "timeRemap", property }`.
`inputRange` controls visibility; the mapping and offset retain the authored
content clock when that window is trimmed or relocated. These kinds do not
accept `activeRange`, and Group does not accept `sourceRange`. Image and other
layer clocks are unchanged.

Historical Tesseract timing is read at the product boundary, not by the public
archive reader. New `.tsrct` archives use these canonical fields and reopen
without a migration pass. Revision numbers inventory schema changes; they are
not a reader-version gate.

The entire tracked converter workspace is synchronized to the public repository.
Keep only distributable sources and fixtures in this directory; no separate
per-file export list is required.

The repository paths identify source ownership only. Published schema filenames,
URLs, and bucket paths remain unchanged for compatibility.

For a maintainer-owned schema change, update `FX_SCHEMA_REVISION` /
`SCHEMA_VERSION`, the canonical schema, and the agent-facing documentation
together. A maintainer with the complete authorized sources must run the
private editable-schema generator with `--check` and bring the verified derived
artifact through review. Standalone contributors should report schema drift or
propose model changes without claiming they regenerated this artifact.
