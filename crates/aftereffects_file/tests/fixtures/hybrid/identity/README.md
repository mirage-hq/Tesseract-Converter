# H-IDENTITY-01: file-scoped Dynamic Link selection

**Import prerequisite, not complete hybrid conversion.** Two independently
Adobe-authored compositions intentionally have the same display name,
`Hybrid_Duplicate_Name`, but different item IDs/GUIDs and red/blue editable solids.
The final source bytes are immutable. No reference videos are committed.

| Native target | GUID / sequence | Native reference Asset (long_term) |
|---|---|---|
| AEP item 1, red | `00000001-0000-0000-0000-000000000000` | `nJvJFfZdDt9G3RljQCZW_vid` |
| AEP item 16, blue | `00000010-0000-0000-0000-000000000000` | `4SofoSLCvGzvcN4VZ11X_vid` |
| Premiere supporting sequence | `1650b494-2985-4142-8645-626872e29ad7` | `tPFucth7V8gDcwsUs9nM_vid` |

Exact source/reference hashes, bytes, authoring/readback identities and fresh
Asset-download verification are in `provenance.json`. `cases.json` registers
**both source-hash/composition-ID targets** and their concrete executable
assertions. The Premiere sequence supports this same identity case; it is not
an invented AEP composition or an independent converter-fidelity test.

## Native provenance and critical sampling

AE26.5x89/build89 authored two 1920×1080, square-pixel, 30fps, 2s compositions,
each with one full-frame solid. IDs were read from Adobe, not derived from names
or item encounter order. Premiere26.5.2 imported the initially distinct names
and placed red at timeline0–1/source0–1 and blue at timeline1–2/source0.5–1.5.
Then AE renamed both compositions identically, preserving their IDs/GUIDs.
The original unique-name seed was retained locally; the final duplicate-name
source is the checked-in `linked-compositions.aep`.

The AE render queue selected **actual item objects by ID**, never the ambiguous
`-comp` name option. It rendered a byte-identical disposable source copy with
`Use this frame rate: 30; Quality: Best; Resolution: Full` and
`H.264 - Match Render Settings - 40 Mbps`, audio off. Premiere reopened a
byte-identical disposable project **after** the duplicate-name change and used
`exportAsMediaDirect` with Adobe's `01 - Match Source - High bitrate.epr`.
Original source hashes stayed unchanged. These are native references, not
converter-generated expectations. Control evidence is script readback, not a
claim of manual Adobe UI control inspection.

All three videos were fully decoded: 1920×1080, 30/1fps, 60 frames, 2s. Every
frame's whole-canvas area-mean RGB was inspected numerically. AE item1 gives
`[255,1,0]`, item16 gives `[0,0,255]`; the native Premiere sequence gives red at
frames0–29 and blue at30–59. Thus samples29/30 straddle the exact1s cut and
prove distinct native identity selection despite identical composition names.
All three Assets were freshly resolved/downloaded, without a cache hit, and
matched the published byte counts/SHA-256. No public-CDN mirror/ACL change was
made. Authoring/session scripts and full machine-specific receipts are retained
locally; their hashes are a provenance limitation record, not reproducible
shipped authoring scripts.

The native Premiere fixture refers to the original authoring paths. Its only
ordinary media dependency is the unchanged tracked
`crates/premiere_file/tests/fixtures/feature_linked_av_source.mp4`, copied as
`background.mp4` during native authoring. The source clip is trimmed to0–2s;
source audio clips were removed while authoring this identity-only oracle.
Do not treat this fixture as a portable generated hybrid package. Tests inspect
its pinned GUID payloads without opening external media.

## Executable editable proof and limitations

Run from `opensource/conv`:

```sh
make test-aftereffects-file filter=adapter::linked_import::tests
```

`native_same_name_guids_import_distinct_editable_red_and_blue_compositions`
checks both pinned source hashes and native Premiere GUID payloads, then freshly
imports each selected snapshot in Check/Write. Both retain the same composition
name but distinct editable red/blue Rect colors, HD canvas, 2s duration, native
center position/anchor, fill/no stroke, and no flattened-media assets. Ordinary
numeric import and prepared import diagnostics agree. Six additional cases
cover unknown profiles/GUIDs, missing/non-composition targets, duplicate/malformed
IDs, file binding/source drift, late publication rejection and existing outputs.
All seven passed; the affected existing import/structure panel adds29 passing
cases. No new tests are ignored.

The historical global `aep_feature_cases.json` helper only accepts `UNRUN` and
cannot ingest these execution results without a separate runner integration.
This feature-local registry records the actual execution explicitly; it is not
enrolled in the historical global inventory/scoring route or Premiere's visual
gate. No helper, CI or required check was changed.

**Unmeasured:** fresh-converter RGB comparison, alpha and audio fidelity. Static
colors do not visually prove source-clock offsets. Premiere's reference has an
incidental AAC stream, not audio-ownership evidence. Native renderer project
color-management/bit-depth controls were not separately read back; no color or
alpha equivalence is inferred from primary-color sanity checks.

**Linked picture import (structural only):** ordinary Premiere conversion of this
native package now embeds both compositions as editable pictures, selected by
exact file and GUID, at the native placements (red 0–1s from source 0, blue
1–2s from source 0.5s): `premiere_file`
`tests::linked_compositions::native_links_import_editable_same_name_compositions_by_exact_guid`.
This is not a converted-render comparison; the unmeasured items above remain.

**Unimplemented:** linked-audio occurrence import (such items are omitted with a
reason) and automatic hybrid routing. FX→AEP export is unchanged; there is no new
generated export/Adobe acceptance, edit-propagation or relocation proof. This
fixture pins only the AE26.5x89 macOS header profile; the one other accepted
profile rests on private evidence, and no universal ID/GUID formula is claimed.
