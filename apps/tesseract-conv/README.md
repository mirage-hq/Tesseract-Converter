# Converter CLI reference

`tsrct-conv` converts one selected Adobe composition/sequence to an editable
`.tsrct` document, or exports one edited `.tsrct` document to Adobe.
Start with the [quick start](../../README.md); use the
[format tables](../../docs/formats/README.md) for supported edits and losses.
Project conversion does not render timelines or launch Adobe. These options describe the
current source revision; an installed release can differ. Check its `--help`.

## Build and run

From the converter workspace root:

```sh
make build
target/debug/tsrct-conv --help
target/debug/tsrct-conv convert --help
```

Use the pinned Rust toolchain and FFmpeg 7 development libraries. The binary is
under `$CARGO_TARGET_DIR/debug` when set. Installed release packages include their
native libraries; keep the package intact.

## Commands

| Command | Purpose | Output |
| --- | --- | --- |
| `inspect INPUT` | List native targets and inspect reached audio/video. | Human inventory, or one versioned JSON inventory. |
| `inspect INPUT --metadata-only` | List targets without media I/O. | Version-1 inventory; no readiness claim. |
| `convert INPUT --to FORMAT --output DIR` | Convert one document and package its assets. | A fresh directory containing the project and required external assets. |
| `transcode INPUT --output FILE` | Prepare one audio/video source independently of a project. | One new media file; no project or media map. |

Formats are `after-effects`, `premiere` and `tesseract`. Extensions `.aep`,
`.prproj` and `.tsrct` select the source format case-insensitively; use `--from`
for another extension. Only Adobe ↔ Tesseract routes are supported, not native ↔
native or same-format conversion. Inputs are standalone documents, not full
application/web projects.

## Target selection

Use `inspect --metadata-only --json` to discover targets. Select by native ID,
never by name or list order:

```sh
tsrct-conv inspect source.aep --metadata-only --json
tsrct-conv convert source.aep --to tesseract --composition 1 --output imported
```

For Premiere, use `--sequence '<native-guid>'`. Selection may be omitted only
when exactly one selectable target exists; nested targets count too. Selection
includes that target's reachable dependencies, not unrelated scenes.

## Conversion options

| Option | Applies to | Meaning |
| --- | --- | --- |
| `--from FORMAT` | All conversions | Override extension-based input detection. |
| `--to FORMAT` | All conversions; required | Select the destination. |
| `--output DIR`, `-o DIR` | All conversions; required | New directory; its parent must exist. Existing entries, including symlinks, are rejected. |
| `--composition ID` | AEP → TSRCT | Select an exact numeric composition item ID. |
| `--sequence GUID` | Premiere → TSRCT | Select an exact sequence GUID. |
| `--media-map JSON` | Adobe → TSRCT | Use separately prepared, source-bound replacement media. |
| `--media-relink JSON` | Premiere → TSRCT | Bind saved media identities to explicit relocated original files. |
| `--available-fonts JSON` | AEP → TSRCT | JSON array of available PostScript names; missing faces use a declared same-style Inter face or Inter-Regular, with warnings. Does not fetch/package fonts. |
| `--expression-samples JSON` | AEP → TSRCT | Use validated AE-evaluated values bound to the exact source AEP. See the [AE library](../../crates/aftereffects_file/README.md#expression-samples). |
| `--allow-film-impact-pop` | Premiere → TSRCT | Explicit consent to the bounded sampled Pop geometry approximation; disabled by default. |
| `--fps FPS` | TSRCT → Adobe | Set the export rate; see below. |
| `--hdr pq\|hlg` | TSRCT → Adobe | Explicit experimental BT.2100 target; omitted means legacy SDR. |
| `--check` | All conversions | Run conversion and validation without publishing the final directory. |
| `--json` | All commands | Machine-readable inventory/result; conversion/preparation events go to stderr. |

Route-specific options are rejected on unrelated routes. `inspect` accepts
selection, `--from`, `--metadata-only`, `--media-map` and `--json`, not conversion
options. `--media-relink` is a conversion option, not an inspection option.

## Frame rates

| Output | Default | Accepted rates |
| --- | --- | --- |
| Direct AEP | 24 | Positive nominal FPS up to 240. |
| Native Premiere | 30 | 23.976, 24, 25, 29.97, 30, 50, 59.94, 60. |
| Linked AEP inside Premiere | Sequence rate | Exact 24, 25, 30, 50, 60. |

At fractional Premiere rates, automatic routing retains native Premiere output
with diagnostics instead of changing cadence for a linked AEP. At every rate,
including exact 24/25/30/50/60, an FX `primaryGrade` or `vignette` alone no
longer makes a linked AEP: the graded picture stays a native Lumetri Color in
the `.prproj` (its scale approximations are diagnostics, not routing losses);
clamps, keys and other picture losses still route. See the Lumetri Color
section of [`docs/formats/premiere.md`](../../docs/formats/premiere.md) for
which forms Adobe has opened. `--fps` is
rejected on import; the original Adobe rate is not automatically restored from FX.

## Timeline and media handling

Import produces `project.tsrct`, with required assets packaged in the archive.
Export uses current edited content, not hidden original native records.
Conversions do not transcode source media automatically. Before importing:

1. Inspect the selected target without `--metadata-only`.
2. Read aggregate `media_admission` and per-media/unassessed records, not just
   the exit status or `video_admission`.
3. Resolve missing originals. Prepare only eligible incompatible sources.
4. Build a [media map](../../docs/media-preparation.md#single-file-publication-and-separate-map-identity)
   when replacements are needed, then reinspect and convert with that map.

`inspect` can complete successfully with blocked media. Its audio/video check
is not a still-image, effect or render-fidelity assessment. `PATH UNRESOLVED`
means native path resolution found no accepted local identity, not that the file
cannot exist elsewhere. Conflicting live aliases and changed files reject.

Premiere relocation is separate from replacement. `--media-relink` authenticates
original project/sequence/Media-UID/authored-path identities before ordinary media
admission; `--media-map` then binds prepared replacements to the resolved original
paths and hashes. It does not bypass source validation or permit prepared bytes
to masquerade as relocated originals. See the
[relocation manifest](../../crates/premiere_file/README.md#explicit-media-relinking).
Inspection has no `--media-relink` option; validate that bound import with
`convert ... --media-relink ... --check`, rather than claiming that an unbound
inspection assessed the relocated source.

`transcode` uses `library` by default or explicit `external-ffmpeg-command`, with
optional `--ffmpeg-path` and `--ffprobe-path`. There is no automatic backend
fallback. Video outputs are `.mov`/`.mp4`; standalone audio is `.wav`; re-encoded
alpha requires a supported ProRes 4444 MOV profile. Backend and target admission
still apply. See [media preparation](../../docs/media-preparation.md).

## Automatic Premiere + AE packages

Premiere export can automatically package editable picture scopes in
`media/ae-NNNN/compositions.aep`. Native-supported picture and native audio remain
in `project.prproj`; no hybrid flag or manual layer selection is needed.
Premiere import resolves supported linked AEPs by exact file and composition GUID.

Keep the entire output tree together and open it with both Adobe applications
installed. Linked AEPs do not make every FX feature supported. Missing links,
unsafe dependency extraction and unsupported clocks still diagnose losses or stop
conversion. See [hybrid conversion](../../docs/hybrid-adobe-export.md).

## JSON and progress

`inspect --metadata-only --json` returns `schema_version: 1`. Ordinary inspection
returns version 2 with `media_admission`, `video_admission` and `media_preflight`.
Missing metadata is null, not invented. Check these fields before a media-ready
handoff.

`convert --json` writes one successful result to stdout containing `mode`,
`input`, `output`, `artifacts`, `artifactStatus`, `diagnostics` and `message`.
Check artifacts are `planned`, not `published`. No report sidecar is created.
`transcode --json` reports source/output hashes and media preparation facts.

Both commands send newline-delimited JSON (NDJSON) events to stderr:

- `schemaVersion: 1`, `type` and `command` identify the event.
- Types are `progress`, `diagnostic`, `complete` or `error` as applicable.
- Progress includes `phase`, `elapsedSeconds`, `completed`, `total`, `unit`,
  `ratePerSecond` and `etaSeconds`. Unknown quantities are null.
- Conversion adds `percentage` and `etaScope: "phase"`. Transcode counts media
  time in seconds; do not require conversion-only fields on its events.

Progress is phase-local and normally appears about every five seconds. Reading,
finalization and stalled work may have no percentage or ETA. A progress or
completion event is not a substitute for the final result and successful exit.
Runtime errors exit nonzero and do not emit a success result. Help/argument
errors and inspection errors can remain text even with `--json`.

## Supported subset

Use the single [format reference](../../docs/formats/README.md):
[Premiere](../../docs/formats/premiere.md),
[AE](../../docs/after-effects-support.md) and
[AE effects](../../docs/formats/after-effects-effects.md).
Static support does not imply animation support, and import does not imply export.
Film Impact Pop needs explicit consent; it remains approximate and is not plugin
round-trip support. Custom effects, layouts, clocks, fonts and coverage combinations
can lose content even when related features convert.

## Conversion accuracy and limitations

Warnings describe known omissions and approximations, not an exhaustive audit.
A successful command does not establish app acceptance or RGB/alpha/audio fidelity.
FX scripts used by AEP output are sampled at four times output FPS, on rounded
integer-millisecond offsets; missed subframe behavior remains possible.
Untrusted scripts need external process/resource limits; VM limits are not a
wall-clock or memory sandbox.

Check uses the conversion path but publishes no usable project. Write requires
a new destination and rechecks source/media identities. Publication is not a
crash-atomic transaction or filesystem lock. Treat only successful Write
completion as a complete package; preserve sources and inspect the result.

## Tests

From the workspace root, `make test-converter-cli` runs CLI unit/integration
checks without Adobe. `make test-offline`, `make check-conversion-fixtures` and
`make check-aep-support-ledger` cover offline helpers and documentation contracts.
See [Contributing](../../CONTRIBUTING.md) for prerequisites and
[the fixture index](../../tests/README.md) for reference evidence.
