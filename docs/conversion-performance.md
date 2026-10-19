# Converter performance

Converter performance depends on source graph size, nested copies, animation,
script evaluation, media checks and publication. Optimizations do not add native
feature support or establish rendered equality. See [AE limits](after-effects-support.md)
and [Premiere/linked output](hybrid-adobe-export.md).

## Current implementation

| Area | Work avoided | Behavior that must remain |
| --- | --- | --- |
| AE import assembly | Reconstructing a populated composition for each setting/batch. | Checked document readers, exact numeric values, defaults and final archive validation. |
| Premiere import assembly | Re-decoding the full graph for each inserted track. | Intermediate validation, first failure, mutation order, unknown fields and key identities. |
| AE parent lookup | Full layer-array search at every parent hop. | Occurrence-local overrides, first duplicate ID, missing/cyclic/deep parent behavior and diagnostic order. |
| Premiere nested placement | Repeated full overlap/sound scans. | Strict interval endpoints, stable order, sound identity and error/omission order. |
| Hybrid scope planning | Repeated backdrop/boundary scans. | Dependency connectivity, audio exclusion from picture backdrops and packing budgets. |
| AE native lowering | Copying complete reserved-ID/guide sets for every attempt. | Nested rollback, successful siblings, text fallback and generated-ID order. |
| Script preparation | Retaining every completed Path payload as a recursive JSON tree. | Untouched data, entry order, final structural validation and source immutability. |
| Archive publication | Retaining serialized project bytes while reopening the archive. | Exact source bytes and the final on-disk validation. |
| External preparation I/O | Unconditional sleeps between output bursts. | Bounded buffering, cancellation, child cleanup, error propagation and EOF handling. |

Some optimizations exchange memory for lookup speed. They do not bound total
process memory or remove parsing, native writer, recursive-copy or script costs.
Large projects can still exhaust time/memory before publishing.

## Script sampling is a semantic approximation

AEP output samples admitted FX scripts at four times output FPS, not every
millisecond. This can reduce work but can miss subframe pulses and geometry/text
changes. It can also produce more keys or use more memory on some inputs.
Do not describe it as byte/pixel-equivalent optimization. Native Premiere
preparation and import use their own policies. See
[script support](after-effects-support.md#expressions-and-fx-script-preparation).

## Useful regression checks

From the workspace root:

```sh
make test-aftereffects-file filter=parent
make test-premiere-file filter=indexed_
make test-tesseract-file filter=write_releases_serialized_project_before_reopening
```

Tests compare indexed/frozen algorithms, exact document values, ordered diagnostics,
media identities and retained bytes. Their elapsed time is not a native fidelity
metric. Follow the [development verification requirements](../CONTRIBUTING.md)
for your environment; ordinary checks do not run Adobe.

## Measure changes accurately

- Pin identical input bytes, target IDs, options, dependencies and build profiles.
- Measure completed whole-command work separately from a parser/lookup microbenchmark.
- Repeat runs; state platform, profile, cache conditions, wall time and peak memory.
- Compare exact project/media/diagnostics where deterministic. For generated native
  UUIDs, validate reference topology instead of deleting arbitrary fields from a diff.
- Do not compare different amounts of completed work as a speedup ratio.
- Do not turn single local dev timings into release forecasts, universal memory
  claims or flaky timing pass/fail gates.

Historical stress measurements are not performance guarantees for the current
build. New end-to-end or Adobe performance claims require new recorded measurements.
