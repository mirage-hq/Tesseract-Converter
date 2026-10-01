# PR #4442 supplementary Rust test cases

## Status

These are authored Rust cases, **UNRUN** and unverified. No build, test,
formatting/lint, Adobe operation, rendering, or correctness review was performed.
They use existing native fixtures, explicitly constructed FX inputs, and some
supplemental synthetic/mutated records. **No new Adobe-authored `.aep` files were
created for this test addition.** They do not complete the requested independent
Adobe fixture authoring, which remains separate work.

All eight modules are registered in normal test builds. Their deferred proof
cases use reasoned `#[ignore]` attributes and appear as ignored in test output.
Explicit proof targets include them. Assertions may expose unfinished behavior;
no passing result is claimed.

## Authored inventory

Paths below are relative to `crates/aftereffects_file/src/`.

| Direction | Test module | Authored coverage |
|---|---|---|
| Import | `structure_document/tests/pr4442_general_cases.rs` | Essential override cardinality, inactive timing and retained identities, mask defaults/omissions, numeric and owner-clock cases |
| Import | `structure_document/tests/pr4442_vector_cases.rs` | Existing pinned Rectangle, paint, gradient, stroke, Boolean, modifier and vector-Group targets; supplemental malformed-input diagnostics |
| Import | `structure_document/tests/pr4442_text_cases.rs` | Existing point/box text sources, text animator and selector families, signed segment-clock copying, text-path references, unsupported semantics |
| Export | `export_document/tests/pr4442_vector_cases.rs` | Explicit edited FX paints/gradients, stroke controls, Boolean operands, vector Groups/modifiers, Rectangle geometry, paint constants, rejection and sibling retention |
| Export | `export_document/tests/pr4442_scene_cases.rs` | Common layer options, hierarchy, masks and precomp origin, analytic bounds, layout/AiEdit, stored 3D and motion options, clock/numeric restrictions |
| Export | `export_document/tests/pr4442_text_cases.rs` | Fresh text documents, Hold content/style timelines, font tables, animator/selector/path channels, unsupported-field diagnostics |
| Export | `export_document/tests/pr4442_media_cases.rs` | Media descriptors/shared sources, fit/crop/3D, source clocks, frame blending, source variants/preflight and bounded image takeover |
| Package | `adapter/export/tests/pr4442_package_cases.rs` | Synthetic metadata staging, unsupported-format sibling retention, malformed-input publication failure, owned rollback and unsafe paths |

Exact test symbols and assertions are in the named modules. Pinned-source
identities used by import cases are recorded in those tests and the existing
fixture provenance. Synthetic export inputs do not acquire native-source
provenance merely by passing through the writer.

## Limits and unfinished authoring

- Fresh export followed by this repository's own reader is supplementary
  structural coverage, not independent Adobe acceptance or editability proof.
- The existing 415-reference inventory is publication metadata, not evidence
  that these tests ran or that any new feature passed.
- The combined native Gradient Fill + solid Stroke Rectangle fixture remains
  missing; separate native halves are not combined-feature proof.
- Positive Group-clock combinations, precise independently recorded clock
  values, nested frame-blending/source-variant combinations, Hold paint toggles,
  and additional dynamic Rectangle/dash combinations still require further case
  authoring. This inventory is not an exhaustive completion claim.
- New feature-specific `.aep` sources must be authored serially through Adobe,
  with source/target identities and authoring readback preserved. No Adobe source
  creation, reference rendering/publication or comparison occurred here.
- Alpha, audio, font/layout, 3D projection, motion blur and pixel fidelity remain
  unmeasured. Correctness verification is deferred to a subsequent PR.

See [the support and approximation ledger](../../../../docs/after-effects-support.md)
and the conversion-feature-tests skill for the independent-evidence requirements.
