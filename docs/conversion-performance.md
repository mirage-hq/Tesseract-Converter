# Converter CPU performance: bounded regression evidence

These changes reduce converter-private reconstruction and hybrid planning work.
They do not add supported Adobe features or establish Adobe fidelity. Existing
[AE limitations](after-effects-support.md) and
[Premiere/hybrid limitations](hybrid-adobe-export.md) still apply.

## Implementation and preservation

| Direction | Change | Preserved checks and behavior |
| --- | --- | --- |
| AEP → TSRCT | Set motion blur on an empty composition, then assemble layers and animation in one `Value` envelope. Avoid replacing motion blur on an already populated composition. | Checked composition/document readers, exact numeric values, absent versus explicit default fields, typed structural errors, and final on-disk archive validation. |
| Premiere → TSRCT | Reuse the graph's wire envelope across a batch of imported tracks; decode the completed graph once. | Validate **every intermediate insertion**. Preserve first failure, preceding successful mutations, upsert/insertion order, key identities, unknown fields and single/empty-batch behavior. Validation is intentionally not reduced to a final-only check. |
| TSRCT → Premiere | Retain a sparse equivalent of backdrop connectivity and index picture boundary tokens once. | Identical connected sets, explicit references, audio exclusion from picture backdrops, dependency/cycle budgets, boundary order/duplicates, and native staging/publication validation. The native-only early-return path is unchanged. |
| TSRCT → AEP | Follow-up: journal newly reserved IDs instead of cloning entire sets for each lowering attempt. | Nested failure rollback, initial reservations, guide consumption, successful siblings, text fallback and generated-ID order; see the separate measurement below. |

No sampling interval, fitting tolerance, media hash/freshness check, script seed,
schema, evaluator or renderer changes are involved. A first AE implementation
using a raw JSON text envelope introduced one-ULP geometry changes; it was
**rejected**, not accepted under a looser comparison. The final implementation
uses `from_value` without an extra numeric text parse.

## Measurements

Local macOS arm64 **dev-profile** executables; three sequential completed runs
per build and direction, without profiling or overlapping local Cargo work.
Wall time includes complete CLI conversion/publication. RSS is the median of
each run's maximum resident size, reported in decimal MB. These are bounded
stress-case results, not release-build or general-workload forecasts.

| Case | Wall seconds, before → after | Time reduction | Peak RSS MB, before → after |
| --- | ---: | ---: | ---: |
| Path-heavy AEP import, composition `1` | 30.324 → 26.087 | 14.0% | 4101.8 → 2931.5 (−28.5%) |
| Premiere import, 180 video placements / 1,080 animation tracks | 31.484 → 18.961 | 39.8% | 89.0 → 91.1 (**+2.4%**) |
| Hybrid Premiere export, 2,001 roots with a screen-blended stack | 3.329 → 1.102 | 66.9% | 203.6 → 122.6 (−39.8%) |

The Premiere export input forces linked-AEP fallback with an unsupported skew.
This is a **hybrid** improvement, not evidence of a native-only export speedup.
Premiere import improves time but does not demonstrate a memory improvement.

### Case identity and options

The stress inputs are local, generated/model-derived inputs, not distributed
Adobe-native oracles. Hashes identify the exact measured bytes; the large
source projects and local outputs are not included in this repository.

| Input | SHA-256 | Selection/options |
| --- | --- | --- |
| Generated Path-heavy AEP, 31,168,030 bytes | `42699485b10e35b4216ccdb5c77bc094783f78fd117ab35331faffd03e5d4266` | `convert INPUT --to tesseract --composition 1 --output NEW_DIR --json` |
| Generated Premiere project, 56,673 bytes | `3c0a53dad0681ba6673b7a7c2c63ab3fca21ebd5f5f368fa9bb14d96be3b900b` | `--to tesseract`; sole sequence `38df8a1f-0be8-465f-932e-545f92ddb011` |
| Generated hybrid TSRCT, 1,014,644 bytes | `b355553d7f15fd19843116968062c19fcfb5aa2437026bd0d9a932bc42f52ae0` | `--to premiere --fps 30` |

Baseline executable SHA-256:
`70028022af0c712ad6375f0045fc82e8becd312a626620d35b286e29e43d79b0`.
Optimized executable SHA-256:
`1054d6527b2b6da30e2f8a90428bad5b22d381c5a6e46cb0a94ddafbe36dd673`.
Subsequent edits changed tests/documentation only.

## Executed regression evidence

- All stress import runs produced byte-identical `project.json` payloads,
  identical asset payloads/descriptors, diagnostics and artifact inventories.
  The AEP result is 70,824,994 bytes, SHA-256
  `dc0af15ce50fa3d09417223aa62d1b2ba6ef61a5d6a77442cee4823faa54478b`;
  the Premiere result is 1,645,844 bytes, SHA-256
  `eb7f7bf6b780894aaa9d26b8c50823cd4a70a7ddc7ab043362b5dd0cdb91cb34`.
- All stress Premiere exports produced byte-identical linked AEP files and
  identical diagnostics/inventories. The full native XML was compared in order,
  allowing only the destination-directory prefix and an explicit allowlist of
  writer-generated UUID fields. A bijection across definitions, references and
  the UTF-16/base64 media-state UUID preserves identity topology; arbitrary XML
  fields, numeric values and opaque importer payloads were not discarded.
  This is **not** byte-identical `.prproj` output.
- Fresh before/after imports of the checked-in native
  `native_parametric_key_channels.aep`, composition `1` (`E11_ELLIPSE_SIZE`), and
  `feature_keyframe_ids_strict.prproj`, sequence
  `1592feef-89df-4c40-ba9d-fe9088c8f4a5`, also passed exact project/asset/diagnostic
  comparisons. Their source hashes are respectively
  `7c10eecb3b40883418c60fe7760bd0641bd7f6eb244f933b2fc3c298c1c9b56f` and
  `50dad3178651d488f7cdc32632c28f7a2157aaabc544d47dfbb58882b1b698ce`.
- Added differential CPU tests cover AE default/authored/absent motion blur,
  exact nontrivial floats and typed structural rejection; Premiere batch upserts,
  partial failure and unknown fields; 243 backdrop/audio/reference combinations
  for every seed; linear retained backdrop edges; and 4,096 indexed boundary
  selections against the prior scan.
- Full standalone `make test`: **3,254 passed, 0 failed, 407 ignored**. Ignored
  tests remain unrun. Initial added-test fixture/setup failures were corrected;
  no production validation was weakened to make them pass.

**Proof limits:** no new Adobe open/control inspection, native render, alpha,
audio or pixel comparison was run. Import evidence is exact converter-regression
evidence; export evidence is linked-AEP/native-XML regression evidence. Neither
proves unchanged Adobe appearance for every project, new feature fidelity, or
completion/performance of larger full-user-project conversions.

## Follow-up: AE export reservation journals

AE lowering previously copied all occupied IDs and consumed guides before every
layer attempt, including the guide probe and nested text fallback. A private,
LayerId-specific insertion journal now records only newly inserted IDs. Rollback
removes additions since the checkpoint; successful child additions remain
rollbackable by a failed parent. Initial reservations and earlier successful
siblings survive. The probe still owns an independent one-time copy. Two-ID
media-takeover selection uses its existing monotonic candidate order without a
temporary whole-set clone. No mapping, validation or diagnostic was removed.

### Bounded CLI measurements

macOS arm64, dev-profile binaries, three sequential exports per case/build,
`convert INPUT --to after-effects --fps 24 --output NEW_DIR --json`.
Wall time includes process startup, archive reading, conversion and publication;
RSS is the median maximum reported by `/usr/bin/time -l`. These are two bounded
cases, **not a release-build or large-user-project speedup claim**.

| Case | Before seconds (three runs) | After seconds (three runs) | Median wall | Median peak RSS |
| --- | --- | --- | --- | --- |
| Synthetic 4,096 independent Rect roots | 2.561 / 2.520 / 2.482 | 0.646 / 0.660 / 0.689 | 2.520 → 0.660 (73.8% less time) | 121.91 → 119.14 MB |
| Imported native parenting target `56` | 0.443 / 0.330 / 0.072 | 0.963 / 0.322 / 0.108 | Startup noise dominates; no speedup claim | 33.44 → 33.14 MB |

The native input is a fresh import of checked-in
`parenting/import_parenting_cases.aep`, composition `56`; source SHA-256
`3626cf26d4f8aa2e405ec0999b43cc4bf9e2e5f7cb0c2723b35648e0b7771f55`.
Existing export omissions (including two branches lacking finite visual bounds)
remain: this is regression evidence, not a fidelity pass. The generated case
copies the Rect leaf from imported `properties/transform_unseparated.aep`,
composition `1`, into 4,096 parentless roots with IDs `1..=4096`, clears dynamics
and segment IDs, and sets a two-second document/layer duration. It is deliberately
synthetic. Archive metadata length/hash was updated before validation/export.
The output contains one two-second native composition with 4,096 layers.

| Case | Measured input archive SHA-256 | Identical output AEP SHA-256 |
| --- | --- | --- |
| Native-derived | `41dba91a1f2d04f45b336175929d7bca7e0f65a8fca4f03f5de24f3e606dc761` | `866ca6a537a0768e89c3c99261695517deb5393a0db40cbc868c9c92d0b51352` |
| Synthetic wide | `8487c76703eb3c183cdb799ffbb807915ca28933d9a2d8abc91e347b93baafa6` | `e4ad75632b06dd0fe8da3052b2ef8f3c40efac10b507d38e6331997defe03b16` |

Baseline commit `160e56562`, executable SHA-256
`4da8e0335b856dfcfb0755671fb26b5affb7742261e59ecc0eb747fea77d9e48`;
optimized commit `ce446fe17`, executable SHA-256
`b787a0e2faa457ec6ff32ee8f613aaa0567fa94316e55eff3b209f28196c1bbf`.
Later edits are documentation only.

All twelve exports matched exact AEP bytes, ordered diagnostics (six for the
native-derived case, three for the generated case), result metadata and artifact
inventories. Only the destination directory and completion-event elapsed time
were excluded from result/event comparison; diagnostic events were also checked
against the complete result diagnostic list.

### Validation and limits

- Six new tests cover duplicate/initial reservations, nested rollback against
  snapshot semantics, independent probe/publication histories, failed Text guide
  consumption with a successful sibling, mixed-text fallback reusing a reserved
  skew-helper ID, and takeover collisions/overflow. Targeted `filter=reservations`
  passed seven tests (including one existing animation-budget test).
- Standalone `make check`, `make clippy` and `make fmt` passed.
- Full `make test` is **not green**: 1,512 passed, 1 failed, 406 ignored, then
  Cargo stopped before later packages. The failure is the previously reproduced
  unchanged-base audio test
  `adapter::export::tests::audio::audio_package_check_write_and_reimport_preserve_edited_gain_and_wave_bytes`
  at `adapter/export/tests/audio.rs:84` (`strip_prefix("./").unwrap()`). It was not
  suppressed or weakened. Ignored tests and later packages remain unrun here.
- Import implementation is unchanged; the imported archive is only an export
  input. Export performance/preservation evidence is bounded to these cases and
  CPU tests. No Adobe open, native-render, alpha, audio or pixel proof was run;
  [the AE support/limitation ledger](after-effects-support.md) still applies.

## AE import: occurrence-local parent lookup

`Converter::transform_ancestors` previously searched the entire native layer
array at every parent hop. Wide compositions with one shared parent near the
end therefore performed quadratic lookup work. An occurrence-local ID-to-index
map now replaces the existing ID-membership set, after Essential Property
overrides have been applied. It retains the **first** position for duplicate
IDs, like the prior linear search. It never caches layer payloads or ancestry
across occurrences. Parent order, partial missing-parent chains, cycle/depth
checks, warning order and fresh FX identities are unchanged. Export is unchanged.

### Bounded lookup measurements

Local macOS arm64 dev/test executables, one selected test and one test thread.
For each workload, membership construction was measured once and added to the
median of three sequential ancestry traversals. The parented workload has
N−1 children and one unparented final layer; the other workload has no parents.
The synthetic models omit property payloads: these timings measure lookup,
not parsing, FX assembly, archive publication or whole-command conversion.

| Layers | Parents | Before: set + linear search | After: map + indexed search |
| --- | --- | ---: | ---: |
| 1,024 | Shared final layer | 18.727 ms | 0.599 ms |
| 4,096 | Shared final layer | 295.784 ms | 2.238 ms |
| 1,024 | None | 0.310 ms | 0.337 ms |
| 4,096 | None | 1.293 ms | 1.343 ms |

The larger parented case is about 132× faster **at this lookup stage only**.
Unparented cases do not benefit and show a small extra construction cost. The
map stores a position as well as each ID, so it also uses more memory than the
old set; peak RSS was not measured. No release-build or end-to-end speedup is
claimed. The benchmark has no wall-time pass/fail threshold.

Reproduce the bounded measurement with
`RUST_TEST_NOCAPTURE=1 RUST_TEST_THREADS=1 make test-aftereffects-file filter=parent_lookup_bounded_wide_cpu_timings`
from this workspace. The measured pre-index revision is `fbe5cd39c`; the indexed
revision is `6743db44e`.

### Preservation evidence

`structure_document::tests::parent_lookup` contains a frozen linear ancestry
oracle. Differential assertions compare original layer references and complete,
ordered typed diagnostics for duplicate IDs, shuffled order, no parent, missing
parents, retained partial chains, self/multi-layer cycles and depth boundaries.
The targeted `filter=parent` run passed **40 tests**, with **15 ignored** tests
remaining unrun. Existing native-fixture tests are not fresh Adobe proof.

`parent_lookup_native_fresh_conversion_fingerprints` freshly parses and converts
each selected native target twice per build. Before/after exact document JSON
hashes and ordered diagnostic hashes match for all three targets below. Diagnostic
hashing serializes `(limitation code, composition ID, layer ID, message)` tuples
in original order; it does not discard or sort warnings.

| Native fixture under `aftereffects_file/tests/fixtures` | Composition | Source SHA-256 | Document SHA-256 | Ordered diagnostics SHA-256 |
| --- | ---: | --- | --- | --- |
| `parenting/import_parenting_cases.aep` | 1 | `3626cf26d4f8aa2e405ec0999b43cc4bf9e2e5f7cb0c2723b35648e0b7771f55` | `6aa41850fa331ba230f787a8fffbe1bb14e8e4b2f7b6c6da484de7135b730135` | `f57ef13ad3b52653e968c2c0ce9e1a129512514302c30a169369e5439e566d34` |
| `parenting/import_parenting_cases.aep` | 56 | `3626cf26d4f8aa2e405ec0999b43cc4bf9e2e5f7cb0c2723b35648e0b7771f55` | `d2e1328d16c2d42a420a4d9ffe9f2b829e611ca19a49b6d6796d19832a068e31` | `80cce718e2bb76c1cecbc323e9955290685185278fb45f5174377baaa4fb4e4d` |
| `essential/multiple_controllers.aep` | 16 | `df08145c4be5d3547b1bb5650b48be777f8663d0d88dc472f65990f0ac37c6d3` | `ff175675d49aa6384078b8208060d8655f9c9c0ad867beb53eb1dff030619304` | `50e5a9b081a4030e3fa6465e7473fb21fb0e5c5a9c17ad9b0864b10b2d4fad85` |

Standalone `make check`, `make clippy` (`--all-targets`, `-D warnings`) and
`make fmt` passed. The attempted full `make test` stopped in `aftereffects_file`:
**1,509 passed, 1 failed, 406 ignored**. The failing
`adapter::export::tests::audio::audio_package_check_write_and_reimport_preserve_edited_gain_and_wave_bytes`
asserts a `./` media-path prefix at `adapter/export/tests/audio.rs:84`. The exact
same failure was reproduced on the unmodified base `160e56562`; it was not
suppressed or changed in this optimization. Later workspace test packages were
not reached, so this is **not a full-suite pass**.

These are converter-regression checks, not new parenting or Essential Property
feature proof. No Adobe execution, native render, visual/alpha/audio comparison,
new reference media or feature mapping was introduced. The existing
[AE support and proof limitations](after-effects-support.md) still apply.

## Follow-up audit: nested import and conversion I/O

This is a bounded follow-up, not an exhaustive review of every source file.
The current workspace has eight packages; the review inspected import/export
coordination, selected AE/Premiere lowering paths, archive publication, process
I/O, and shared curve/script/media primitives. The FX model, schema, evaluator,
fit tolerances, media hashes and feature mappings are unchanged.

### Implemented

- **Premiere import:** `reader::nested::keep_non_overlapping` indexes interval
  starts and prefix maximum ends instead of scanning all ordinary and accepted
  nested placements for every nest. Strict endpoint semantics, stable same-start
  ordering and ordered omissions are retained, including empty/reversed ranges.
  Media-only tracks do not build an index. `pair_sounds` uses ordered identity
  buckets instead of scanning all sound slots for every placement, preserving
  first-nest ownership, duplicate order, unmatched order and gain/error branches.
  These stages now take O((items + nests) log(items + nests)) and
  O((sounds + nests) log(sounds + 1)) respectively, with linear index storage;
  recursive content copying/conversion costs are not included in those bounds.
- **External media preparation:** `process::lines` waits on its existing bounded
  channel rather than sleeping unconditionally between stdout bursts. It checks
  cancellation between lines and terminates/reaps the child and joins readers on
  callback, read, channel or wait failures. EOF still waits for child exit without
  busy-spinning. `capture` and the library FFmpeg backend are unchanged. No
  end-to-end transcode speedup is claimed.
- **TSRCT publication:** `TesseractFileBuilder::write_with_runtime_assets` drops
  serialized project bytes before reopening the written archive. The final
  on-disk read and all validation are retained. This benefits callers of that
  builder; it is not a new standalone Adobe-export optimization.

### Bounded evidence

Two completed local macOS arm64 dev/test-profile runs compared the old scan
algorithms with the indexes in the same test executable. These are synthetic CPU
microbenchmarks, **not CLI conversion timings or release-build forecasts**.
Fixture construction is outside the timed region; exact ordering/output checks
follow the calls. No timing threshold is used as a flaky test gate.

| Workload | Old scan, runs 1 / 2 | Index, runs 1 / 2 |
| --- | --- | --- |
| 2,048 disjoint nests and 2,048 ordinary items: overlap filtering | 31.637 / 28.545 ms | 0.794 / 0.655 ms |
| 2,048 nested picture/audio pairs | 70.413 / 63.534 ms | 4.363 / 3.724 ms |

Reproducible tests are
`indexed_overlap_matches_strict_scan_for_permutations_and_stress`,
`indexed_pairing_matches_scan_order_branches_and_errors` and
`indexed_pairing_bounded_cpu_comparison` in `premiere_file::format::reader::nested`.
They compare retained placements, model state, ordered diagnostics and sound
identity/order against the former algorithms, including failures and duplicates.

The isolated `tesseract_file/tests/write_memory.rs` allocator regression uses a
4 MiB-plus-whitespace JSON document with a small parsed tree. Before the buffer
release, `write_releases_serialized_project_before_reopening` fails with two
simultaneous large allocations; after it, the peak is one, and the full original
JSON bytes survive reopening. This demonstrates the eliminated duplicate buffer,
**not a 50% reduction of total process RSS**. The initial test fixture lacked the
required `$schema`; that setup error was corrected before recording the failing
allocation assertion.

Executed checks: 41 archive tests, six Unix process I/O/cleanup tests, 143 selected
Premiere nest-related tests, and four `indexed_` tests (including the three above).
Premiere all-targets clippy and standalone formatting passed after correcting
range-literal lints in tests. The process regressions cover burst ordering,
nonzero exit, bounded stderr, EOF, cancellation, callback errors and invalid UTF-8.
Windows process behavior and complete workspace tests/check/clippy were not run
for this follow-up. No new Adobe execution, native render, alpha/audio scoring or
full-user-project conversion was performed. Existing native-fixture CPU tests
are regression evidence, not fresh Adobe fidelity proof.

### Remaining audit candidates (not implemented or measured here)

| Area | Candidate requiring further work |
| --- | --- |
| AE import | `Converter::transform_ancestors` scans all composition layers per parent hop; investigate an occurrence-local ID index without changing overrides, cycles or diagnostics. |
| AE export | `Lowerer::layer` clones occupied/consumed identity sets for rollback; reducing snapshots requires preserving nested failure rollback and generated identities. |
| AE export | `omit_dangling_reference_owners` repeatedly scans and removes dependency-chain owners; any frontier algorithm must retain round/reason/diagnostic ordering. |
| Premiere export | Motion checks repeatedly scan animation entries; an index must retain entry order and every property/dependency check. |
| Premiere export | Native/hybrid placement scans prior track items; an interval index must track every mutation and retain paint order and matte minimum-track rules. |

Most feature-specific parsers/lowerers and native writer codecs remain unaudited.
These remaining candidates and missing end-to-end measurements prevent claiming
that the converter-wide performance review is complete. Import feature support
and export feature support/proof remain as documented in the
[AE ledger](after-effects-support.md) and [Premiere/hybrid ledger](hybrid-adobe-export.md).
