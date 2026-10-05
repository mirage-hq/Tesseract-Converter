# Native Dynamic Link source — structural evidence only

Case: `premiere_after_effects_dynamic_link_v1`.

These **Adobe-authored sources** were created in a new, task-owned
macOS GUI session on 2026-09-28, not by either converter. Premiere Pro 26.5.1
imported the AE composition with `importAEComps`, placed it above the native
video, and saved the project. After Effects 26.5x89 (build 89) authored the AEP.
The public Premiere fixture anonymizes absolute path metadata. Its original
source identity remains separate from the published derivative in
[the path provenance](../path-sanitization.json). CPU source-record tests do not
require Adobe. No new native acceptance is claimed for the derivative.

| Input | Bytes | SHA-256 |
|---|---:|---|
| `native-linked.prproj` | 8531 | `a6a7a7a662bd8f2b5b9392cd11e97dde16344ccb55a589a1d902b6f40ac88154` |
| `native-title.aep` | 80335 | `d17ed2fd6a118c69e3be3ae41d327a3877036579715dcc6f5dfa8eb7001c9d31` |
| `background.mp4` (local only; not included) | 134521 | `0c8bc4a098980d45287c05a6f3e15b4fd82f50ed3823354f6532d772d4eb6e6a` |

## Exact target and assertions

- Premiere sequence `Hybrid_Link_30fps`, UID
  `49f69765-5ef3-46ac-b9b7-1939bdcea8a8`: 320×180, square pixels, 30fps,
  three seconds. V1 contains `background.mp4`; V2 contains the linked title
  at 0.5–2.5s with source range 0–2s. No native audio clip.
- Linked Media UID `cb0ef24b-d73e-428b-b00e-52eb2abb1038`, stream ObjectID65;
  AEP comp item1 `Hybrid_Title_30fps`, native Dynamic Link GUID
  `00000001-0000-0000-0000-000000000000`, 320×180, 30fps, two seconds.
- Supporting AEP composition: editable text `HYBRID`, ArialMT, 28px;
  linear position keys .25s `[80,90,0]`, 1.25s `[240,90,0]`.
  This composition supports the **link case**; it is not a completed AE text
  import/export feature test. No additional AEP target is claimed.
- The native link importer is `ec341e53-60c2-4d89-abfc-bdb5c0ff2e0b`.
  Its preferences contain the 36-character GUID encoded as UTF16LE/base64,
  with no terminator. Stream codec1145854285, AlphaType1, OriginalFieldType4,
  and `BT.709 RGB Full` are the supported source profile.

Executable source assertion:
`format::reader::video::tests::native_after_effects_link_is_not_decoded_video`
loads the pinned `.prproj` afresh and checks the media identity, dimensions,
source duration/rate, relative path and non-video inspection route. It failed
before the implementation because the link became `PrMediaKind::Video`, then
passed afterward. The surrounding `after_effects_link_*` cases cover malformed
and shared preferences, unsupported stream semantics and supplementary writer
readback. Run `make test-premiere-file filter=after_effects_link` from the public
workspace. Writer cases use synthetic 1920×1080 placements and do **not** prove
Adobe acceptance of generated records.

The native 320×180 **sequence** is no longer rejected for its canvas size, and
ordinary import resolves linked compositions by file and GUID, but no test
imports this full project. Only its media subgraph is tested here; do not
describe this as a successful full-project or editable FX import.

## Independently observed native behavior

Premiere rendered the exact sequence through `exportAsMediaDirect`, using the
installed AME `01 - Match Source - High bitrate.epr` (35474 bytes, SHA-256
`68cf2ab2a0a62193c8d9f4a0b4501c0bd4be72258a13c1e0437539cf35aa4597`).
`native-reference.mp4`: 402730 bytes, SHA-256
`e37efd4ba83d1ecaf1fa0e2c6e9e4a329c1e665168d0a08eb62866c28302a24b`;
90 decoded H264 frames, 320×180, 30fps, three seconds. The incidental AAC stream
is not audio-fidelity evidence.

Inspected frame indices: 0,14,15,22,37,52,74,75,76,89, each at exact `n/30`
seconds. These show title motion over visible video, absent before15 and from75
onward. They do not sample every exact animation-key boundary or establish a
quantitative comparison/alpha pass. A clean native project reopen retained the
online link, composition identity and range.

On disposable copies only, Premiere saveAs/changeMediaPath linked the copied
AEP; AE saved the text `EDITED`. At one second the first and refresh-only captures
still showed `HYBRID`; a subsequent Premiere project reopen showed `EDITED`.
The reopen exceeded a90s BridgeTalk reply timeout but completed without retry;
a fresh read-only receipt confirmed the copied AEP remained the online source.
This establishes **edit reflection after reopening**, not immediate live refresh.
The after-reopen PNG SHA-256 is
`cce05e1c5a2bed672734d9609cb60197c1d0250dc7372d400dd56f93787a02a2`.

Native receipts and media remain in the author's task-local
`hybrid-adobe-scratch/` directory outside Git. **No Asset publication was
requested/authorized**, so neither media nor reference has an Asset ID or a
freshly downloaded Asset hash verification. Cross-machine Adobe replay is
blocked on media availability. Generated-output Adobe inspection, relocation,
RGB scoring, quantitative alpha and audio comparison are **unrun/unmeasured**.
This case is not enrolled in the strict video-reference gate. See the
[implementation/limitation ledger](../../../../../docs/hybrid-adobe-export.md).
