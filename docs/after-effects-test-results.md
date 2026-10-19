# After Effects verification and evidence

Use [current AE support](after-effects-support.md) and
[effect controls](formats/after-effects-effects.md) for behavior. The records below
are **historical, case-specific evidence**, not a fresh run on the current checkout.
They retain measured failures and do not establish complete support.

## Checks to use

| Check | What it establishes | What it does not establish |
| --- | --- | --- |
| `make test-aftereffects-file` | Ordinary parser, mapper, writer and adapter assertions. | Native-editor execution or rendered equality. |
| `make check-aep-support-ledger` | Every declared AE diagnostic code has documentation. | Exhaustive detection of unsupported native features. |
| `make test-aftereffects-feature-proof` | Explicit opt-in feature assertions, including known failing contracts. | A blanket pass; inspect failures rather than treating registration as success. |
| Native open/control readback | Acceptance and values in the exact inspected export. | Every rendered frame or a differently hashed export. |
| Independent render comparison | The stated picture/alpha/audio measurements for one source/output pair. | Unmeasured channels, arbitrary edits or general feature equivalence. |

Native-editor execution and rendering/scoring need separately authorized tools and
hosts. They are not part of ordinary standalone tests. Do not lower a threshold,
replace an independent oracle or weaken a contract to hide a mismatch.

## Receipts and source identity

| Record | Contents |
| --- | --- |
| [Effects coverage results](after-effects-evidence/effects-coverage-results.json) | Source/composition identities, exact hashes, control assertions, RGB measurements and failures. |
| [Selected native effect panel](after-effects-evidence/effects-native-panel.json) | Bounded independent export control/key readback. |
| [Hue/Saturation evidence](after-effects-evidence/hue-master-export-repair.md) | Separate static Master export evidence; does not retroactively change older failures. |
| [Layer Styles](after-effects-evidence/layer-styles.md) | Native style mappings, replacements and per-style proof limits. |
| [AE fixture inventory](../crates/aftereffects_file/tests/fixtures/README.md) | Native sources, creation/provenance notes and specialized case records. |
| [Path evidence](../crates/aftereffects_file/tests/fixtures/path-keys-proof/README.md) | Editable geometry/control checks and bounded RGB/alpha results, including remaining Shape raster failures. |
| [Native video reference registry](../crates/aftereffects_file/tests/fixtures/aep_video_references.json) | Source IDs, independent references and pinned hashes. |

Compare the exact bytes named by a receipt. Post-run hashes do not authenticate
an unrecorded causal link, and one accepted export does not validate another.
Preserve immutable native sources/references and document any public derivative
without silently replacing an independent oracle.

## Historical effect panel

The pinned 55-case AE 26.5x89 panel reported:

| Evidence | Recorded result |
| --- | --- |
| Import structure | 52 passed, 3 failed. |
| Fresh export structure | 55 passed. |
| Independent Adobe export control readback | 50 passed; 2 control failures; 3 exports rejected. |
| Import RGB | 55 cases measured; no blanket quality pass. |
| Export RGB | 52 measured; 3 rejected/unscored. |
| Alpha/audio | Not established by the RGB panel. |

Brightness/Contrast and Hue/Saturation account for historical import/control
failures. Pixel Motion Blur and Posterize Time geometry-motion exports had Adobe
acceptance failures. Later static-control repairs have separate evidence; these
old results remain results of their recorded revision, not current test totals.

CC Vignette investigations found animated exports with matching stored keys but
frozen continuous rendering. Current export therefore diagnoses static replacement;
import omits the unsupported effect. A matching static image does not turn the
retained animation contract into a pass.

## Interpreting visual results

A high score can hide missing small content or blank/offline output. Validate
source identity, duration/FPS/canvas, intended visible features and critical frames
before interpreting averages. Distinguish unique decoded frames from repeated
endpoint probes. Alpha conventions, color transforms and comparison channels must
be explicit and must not alter the native reference.

RGB MP4 cannot prove alpha or audio. Control readback is not manual UI inspection,
and font substitution cannot prove exact typography. Unknown/unrun states stay
unverified; listing registered cases is not evidence that they executed.
