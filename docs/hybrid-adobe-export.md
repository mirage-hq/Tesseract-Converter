# Editable Premiere + After Effects packages

The CLI can export a `.tsrct` as one Premiere project with editable linked AEP
picture scopes, then import supported linked compositions back into editable FX.
This happens through ordinary Premiere conversion; there is no hybrid flag or
manual layer selector. It does not prerender pictures or launch Adobe.

## Usage and layout

```sh
tsrct-conv convert edited.tsrct --to premiere --output exported --fps 30
tsrct-conv inspect exported/project.prproj --metadata-only --json
# Select the intended GUID from that inventory before importing:
tsrct-conv convert exported/project.prproj --to tesseract \
  --sequence '<sequence-guid>' --output reimported
```

A package may contain:

```text
exported/
  project.prproj
  media/
    ...native media...
    ae-0001/
      compositions.aep
      ...AEP-local assets...
    ae-0002/
      compositions.aep
      ...AEP-local assets...
```

Native-only output needs no AEP folders. Each linked scope is a dependency-connected
picture region, not necessarily one layer. Keep the entire directory tree together.
Open `project.prproj` with both Premiere Pro and After Effects installed; install
any delivered licensed fonts before opening/rendering linked text.

## Export routing

The coordinator retains the native Premiere conversion and its typed picture
losses. When an admissible AEP scope can represent current picture content better,
it stages a linked composition and replaces that native picture scope. Native
supported content and independent native sound remain in Premiere.

Dependencies must stay within a safe contiguous scope. Interleaved matte/guide
references, incompatible canvases/clocks, unsafe nested-audio extraction or invalid
identities can prevent replacement or fail conversion. AEP limitations still
apply: unsupported FX shaders/effects are not made supported by linking.

A candidate that omits an entire requested picture scope does not replace native
content with an empty AEP. For scopes with native picture, the coordinator compares
native losses (omitted picture leaves plus owners of partial picture losses) with
AEP omitted picture leaves. It prefers fewer losses; ties prefer AEP only when
nothing kept by native output is lost. Diagnostics include
`HYBRID-LINKED-PREFERRED` or `HYBRID-NATIVE-RETAINED` as applicable.
This is a **layer-count heuristic**, not an area-, duration- or pixel-fidelity metric.

A native Premiere effect whose only difference is a declared mapping
approximation is not a picture loss. In particular an FX `primaryGrade` or
`vignette` exported as a native Lumetri Color keeps the graded picture in the
`.prproj` at exact 24, 25, 30, 50 and 60 fps too, instead of a linked AEP;
clamps, unwritten keys and other losses still route (see the Lumetri Color
section of [`formats/premiere.md`](formats/premiere.md)).

Genuinely empty source roots have a separate bounded linked representation. They
must not be confused with nonempty sources whose content was omitted.

## Linked scope rates

Linked AEPs use exact **24, 25, 30, 50 or 60 fps**, matching the Premiere sequence.
At 23.976, 29.97 or 59.94 fps, automatic routing retains native Premiere output
with diagnostics rather than silently changing the sequence clock. Unsupported
empty-root clocks fail. Direct AEP export has its separate positive-FPS range.

FX scalar, ShapePath and text scripts used by AEP output sample at four times the
selected AEP FPS on rounded integer-millisecond offsets. These become editable
keys, not native expression programs. Missed subframe behavior, stateful programs
and unsupported dependencies remain diagnosed limits.

## Media and source clocks

Sources retain archive identity and undergo native admission. Selected AEP scopes
can explicitly prepare eligible whole video sources using the library backend,
then recheck their native compatibility. Supported remux/encoding changes media
representation, not the authored editable clock. Unsafe color, timing, audio/data,
alpha or enabled-unmapped-effect cases retain native fallback or explicit failure;
there is no general media-normalization guarantee. See
[destination preparation](media-preparation.md#hybrid-export-destination-preparation).

Uncovered timeline gaps need no invented black layer. Root gaps use Premiere's
native black background; nested gaps can remain transparent. This does not prove
alpha/background equivalence. Audio is not a compositing backdrop.

## Linked composition import

Ordinary Premiere import resolves each supported AEP by its exact local file and
native composition GUID, not a composition name. Each occurrence gets independent
editable identities, placement and clip/source clocks; repeated instances lose
shared editing identity. Linked media is packaged in a namespace per resolved AEP.

Missing files, unsupported link records, mismatched source canvases and unavailable
source ranges diagnose omitted occurrences. Ambiguous identities are not guessed.
Linked AEP **audio is not imported implicitly**. Separately represented Premiere
audio remains independent; grouping/linkage can be lost on another export.

## Validation and publication

Check runs the conversion/staging/validation path but publishes no final package.
Write requires a fresh destination with an existing parent. Sources, linked
identities, file hashes and package-relative paths are checked through publication.
Ordinary failures clean only owned artifacts; uncertain or foreign paths must not
be removed. Publication is not crash-atomic and does not lock a hostile filesystem.
Treat only successful Write completion as a complete package.

The format libraries own their own native conversion. The CLI coordinator owns
cross-format routing; neither adapter depends on the other. Integrators should
use the public prepared-export/picture-staging APIs rather than manually editing
Dynamic Link wire records or inventing GUIDs.

## Compatibility and evidence

Packages remain **HYBRID-EXPERIMENTAL**. Software tests cover generated links,
selected current edits, reimport, Check/Write and failure cleanup. Bounded Adobe
opening/rendering observations exist for selected packages, not every profile.
General edit propagation, relocation, RGB/alpha/audio fidelity and custom-canvas
combinations remain unverified. Some generated native text/effect exports have
historical Adobe acceptance failures.

A file that parses in the converter is not necessarily valid in Adobe. Inspect
the actual output in both applications and report unperformed checks as unverified.
See [AE limits](after-effects-support.md),
[Premiere limits](formats/premiere.md) and
[reference evidence](after-effects-test-results.md).
