# Point Text native rejection isolation (not a completed feature test)

## Fresh empty-Point default profile follow-up

The later all-failures repair task resumes Point investigation. New independent
source `native_empty_point.aep`, SHA-256
`02ef66ccaae8337b9421ae910842e7c94fcccd0fcbece00eeacf27b461b48707`,
was authored by fixed `headless-adobe create_aep`, request
`tsrct-aep49-native-empty-point-v1`, job `c3cb8a6088eb445da7b89ec90624597c`,
AE26.5x89, verified READY. Composition1 `Empty Point Defaults`,320x180,24fps,1s:
empty Point string, requested ArialMT64,white fill,Position[40,90]. No media,
visible donor content or supporting compositions. Native persisted content is
terminal CR with empty-line defaults; this is not a Box layout/cache.

`writer::text::tests::native_point_empty_path_group_matches_source` now also
requires the independent native empty More Options/no Animators context. It
failed before the change (queue899: extra empty Animators). The fresh Point
profile passes this and
`writer::native_text::tests::point_text_uses_independent_empty_point_defaults_and_fresh_semantics`:
queue907,57 passed,6 existing ignored. It preserves Point geometry/default
classes, writes typed fresh content/style/run lengths and selected clock/owner,
and at that historical build908 stage retained only the source's empty-line/CR
layout defaults. The current writer removes all inherited document glyph/line
caches; the build908 native result below must not be attributed to that later
revision.

Native matched case: explicit editable Point `Fresh 49`,ArialMT64,white,
Position[40,90],320x180,24fps,1s, no anchor/path/animator options. Fresh converter
build908, revision78131a52a, output SHA-256
`de02429d9778ea5e07aa749d100a1191f444fb467277680cc0d75bb7c555eff7`;
request `tsrct-aep49-point-nooptions-908-v1`, job
`daa07906d2614c2488889327298536b9`. Native render full1s/30fps/audiooff succeeded
and returned READY; MP4 SHA-256
`2e65d0ebb2efa983a8e7eb9e5809c32d78647e3bd3478f21f8cbe37019316743`.

Independent matched source (not converter output) was fixed-API authored:
request `tsrct-aep49-native-point-control-v1`, source SHA-256
`5bf9308791ddb3c9af9a6ed26c5d4e98f1b08767095e0a7c88320eee417494d7`,
composition1 `Fresh Point Control`, same controls. Native render request
`tsrct-aep49-point-control-908-v1`, job `1ab89290708d4e01a14e58e78861afce`,
MP4 SHA-256 `ae24a518e04dcb8601aca216b95487eb2156a91f93c57422373baaa262acf298`.
Both receipt fonts were strict/no missing faces. All30 full-resolution decoded
RGB frames were **exactly equal** (mean/max channel difference0); each first
frame has3970 nonblack pixels (threshold16),max254. This is bounded native fresh
text/reflow evidence, not font substitution or the earlier donor-`n` transplant.

The initial fresh import/export of that control materialized Character default
anchor options and took the old richer route. Artifact SHA-256
`c3e871e9b159a22828be29fbc549122fc9e7d392c52d43fed589c552e5a35fa5`,
request `tsrct-aep49-point-fresh-908-v1`, job `bbd228949aa946499ad18106f13b4e34`,
failed Text reading with verified READY. It was not replayed. Default Character
anchor handling is being corrected; nondefault/richer owners remain unproved.

At build908, original P025 was freshly converted without content/font/input changes. Artifact
SHA-256 `9ce5f2f94bf5da4ced16ac19b421f159adaa7c8b9c88d0866dc400fa491d4a18`,
request `tsrct-aep49-P025-point-probe-908-v1`, job
`1dc86c01da4d4f3eab9fdba394a352a1`, still failed Text reading/READY:true. No
replay or font substitution. That historical result does not establish the19
failing corpus owners.

### Current cache-free leading correction and actual P025 acceptance

Independent `native_point_fresh49_size64.aep`, SHA-256
`5bf9308791ddb3c9af9a6ed26c5d4e98f1b08767095e0a7c88320eee417494d7`,
and `native_point_fresh49_size48.aep`, SHA-256
`9067f2f5e03f18702d4456154b3e7f9401f127756ab0618b685b278c00573f4b`,
were fixed-API authored as the matched controls described above, differing only
in requested fontSize64/48. Size48 authoring request
`tsrct-aep49-native-point-size48-control-v1`, job
`dcb8f0d62e25434cb267516eace3a9f8`, verified READY. Their native owner/root
metadata agree; both automatic-leading runs store inactive manual leading0.01.

Cache-free build932/revision04f93443f accepts fresh64 (AEP SHA-256
`2e66cdc22e084c4bff622e878bb8f0ba732bc5742f62b564c5bb1d4a18bd9bc4`)
but fresh48 fails Text reading (AEP
`f8b82bf384b4b49828a3ebb55e54afc4bd05403bd7eeb9b25356661dd48cf3ad`),
both with READY. No cache PC/F/R/L/S/G tree is retained in these files.
The native default regression
`writer::native_text::tests::point_automatic_leading_keeps_independent_native_inactive_default`
failed before correction (queue954:76.8 versus0.01). In commit7eabb5464,
automatic Point leading keeps its native inactive0.01 slot and paragraph1.2
factor; explicit manual leading is unchanged. Build958 fmt/seven native-text
regressions pass. New fresh size48 AEP SHA-256
`aafacc3ff8af13e48c5c0c8558991923e7c93a557e7bc179ca958be289900e95`,
request `tsrct-aep49-point-size48-leading-958-v1`, job
`46c3a6ff16564f7aab93e147c55ed027`, succeeds with READY. Its30-frame MP4
SHA-256 `d74849844cd3ad3f50ffe62103cea8eec923e4e76f699154d41375bd7ac3178e`
exactly matches all30 RGB frames of the independent size48 control:
request `tsrct-aep49-point-size48-control-render-v1`, job
`75652d1028a5480b9d65c47752d7b3f8`, video SHA-256
`9c211298fc961902d82c5b0ffb5f23f6d5621210dd1bf87ca86929c0534c0038`.
Both320x180/full1s/30fps/audiooff,2376 nonblack pixels/frame,strict fonts/no
missing faces. This is the cache-free revision's matched evidence, not build908.

New actual P025 fresh build958, original source/content/VT323 font unchanged:
AEP SHA-256 `3fe92a1618bf7b688880e7ee03d800a4f6133c728cf6c3a9fc2ad700429e4df6`,
request `tsrct-aep49-P025-point-leading-958-v1`, job
`363336a971fa4f0bb2a5f2dc08585c4a`, native1920x1080/30fps/30-frame probe
succeeds,strict receipt reports no missing fonts/substitution,READY:true. This
receipt is not independent actual VT323 resolution proof (see discrepancy below).
Video SHA-256
`c7db22d3e4b9eddc83b01fce4d2e4e541e01b2d74440eed79a95674dfb0a4725`.
Decoded first frame:20836 nonblack pixels,max255,requested prompt and arrow
visible. No original source bytes/font/content were rewritten. Full5s and
independent P025 controls/fidelity remain unverified; Shape2111 omission is
separate and is not repaired by this Text change.

Font-changing Point holds now have a generic native font registry and correct
per-document indices, retaining root default dependencies. The TextSpec-level
`writer::text::tests::native_point_font_changes_follow_semantic_indices`
failed before this correction (queue977:font-change rejection). A structural assertion is not an independently native-rendered hold reference;
the executed Hold and blocked independent reference are recorded below.

Videos/results remain local ignored artifacts, not committed media or published
immutable Assets. Fixed native Text control readback is unavailable in the current
inspection profiles; no own-reader result substitutes for it. Full feature proof,
Asset publication, alpha/general fonts/richer owners remain incomplete. Import
implementation is unchanged; new both-direction fidelity is not claimed.

## Vendor-version correction and font-proof discrepancy

Independent `native_point_fresh49_vt323.aep`, SHA-256
`dd03022c630825da8ecf1f422b9ebfe1942780766915518d875f99b35c72862f`,
was fixed-API authored under `tsrct-aep49-native-point-vt323-hold-control-v1`,
job `457fa761876843199fbd6242c436f925`, READY. It is static `Fresh 49`, VT323-Regular48,
Position[40,90], white,320x180/24fps/1s, not a converter output. Its failed render
is **not a resolved-font/fidelity oracle**. Against the identical Arial48 source,
root metadata outside document/font-table data differs only in the optional
per-font vendor-version slot `/1/5/[0]/4/[0]/1`: Arial has `Version 5.01.2x`,
VT323 omits it. FX holds no vendor version. Regression
`point_root_does_not_replay_unknown_font_vendor_version` fails before omission
(queue1071). Commit3de7e4c83 removes the unknown version from all fresh Point roots;
no font names are aliased or substituted. Box/richer-owner caches remain unproved.

Build1086/HEAD3a8977306 passes fmt,11 native_text and35 pr4442 Text tests.
Fresh Arial48 output AEP SHA-256
`381b4672e51cb7568d39e16ca0128235c08adced5354dace847a979939828a02`,
request `tsrct-aep49-point-size48-no-vendor-1086-v1`, job
`88f4b57d83f646528898e5c88f06acec`, succeeds/READY. MP4 SHA-256
`b2b234b0b64280a33a13eb61a4e22570364cf0454e74a2da3e3ad10e072c4dc8`.
All30 decoded RGB frames exactly match independent Arial48 control75652d10 above;
2376 nonblack pixels/frame. This is fresh bounded native acceptance after the
intentional metadata byte change, not a claim of unchanged whole-AEP bytes.
Full conv `make fmt check test clippy` passes queue1087. Eight older raw-hex or
adjacent-token assertions in queue1037 are replaced with exact decoded COS
content/font/style values; feature expectations and visual thresholds are unchanged.

Fresh Point Hold ArialMT→VT323-Regular at0.5s renders/READY under request
`tsrct-aep49-point-font-hold-1011-v1`, job
`0f556cae963743c59ddd67169e79cd25`; MP4 SHA-256
`45c1a9d8b575c6d097510028b8379a7bc9bfedc13d4fe9ec2eaf8272248b645c`.
States visibly change, but actual faces/native keys/editability are not read back.
The independently authored VT323 reference render request
`tsrct-aep49-point-vt323-hold-control-render-v1`, job
`b27bc2d9c8964776920c59f33b4c78a3`, fails `Missing/substituted fonts: ["VT323-Regular"]`
and returns READY. No video, replay, font substitution or outside-API installation.
Generated P025/Hold strict receipts therefore do **not** independently prove VT323
availability. The fixed API checks `usedFonts` before save/render; deferred
cache-free face detection is a hypothesis requiring native Text/fontObject
readback, not an established cause or permission to bypass admission.

## PR milestone and validation

The user excluded further Point Text serialization investigation from this partial
repair milestone. The source-proven corrections and supplementary regressions
are retained; exclusion is not acceptance or permission to replay native text
or strip title/matte semantics.

Merged-code checkpoint `83b1e0ff3` passed conv `make fmt check test clippy`
(queue350):3354 tests passed,0 failed,407 ignored. This supersedes the historical
pending unit/lint notes below; it does not change any failed Adobe gate or supply
missing independent feature fidelity/reference evidence.

## Latest owner-group boundary and incomplete export proof

Two source-backed writer corrections now have executable regressions:

- `writer::text::tests::native_anchor_grouping_descriptor_matches_adobe_source`
  compares the complete124-byte descriptor and stored selection/value records
  against independently Adobe-authored `text/import_text_path_options.aep`,
  SHA-256 `019db8c748e3b306591bdbade1cbb83ed466481abf860c3488db10a0a6d485ec`.
  Targets107/122/137 exercise native Word2/Line3/All4 at24/30fps clocks.
  Queue263 failed on the Scalar descriptor; queue266 passed after VectorEnum
  correction. Character1 is an unstored native default, not a stored-record oracle.
- `writer::text::tests::native_point_empty_path_group_matches_source`
  compares the complete empty Path Options group against this Point fixture,
  at24/30fps. Queue291 failed because the group was absent; queue292 passed after
  retaining the native empty group. Authored Path Options are unchanged.

Check272/build273 and check293/build294 passed. Fresh generated273 and294
still failed Adobe with `Error reading the text layer. Skipping the text layer.`
Both executions returned verified READY; neither produced a video. Generated294
SHA-256 is `e7536750897c0c36115d56beadd0348c71feba71dcbfe6910c18d34d6c905724`,
request `ig-point-empty-path-294-changed-minimal-v1`, job
`d52dce4b6b904f9992cf46a69f152a69`. No rejected file was replayed.

A **nonshipping diagnostic** replaced the complete native Text Properties group
in generated273. Artifact SHA-256
`d5369846ddb257badd7f1ca579c49f7dbdac386fc0caf415a2341cbbabd0377c`, request
`ig-point-273-full-owner-boundary-diagnostic-v1`, job
`faf86be870b34e158f69706d4be29690`, opened and rendered30fps MP4. Video SHA-256
`265cb9a75e592b7c11bd985242533c38b079b5acfe365a214d78a1afbb09b238`;
first decoded320×180 RGB frame had min0/max254 and429 bright pixels (nonblank).
This is the original native `n` including its native text data, **not fresh edited
FX export, reflow, independent fidelity, resolved-font or whole-film proof**.
The local diagnostic MP4 is not a published immutable reference Asset.

Replacing only native COS plus its GUID companion in generated273 still failed
`missing data in file.`, with verified READY: artifact SHA-256
`66eae918e5884c13d95f28f50af4fb7ec1a7de55dc6e2435847919270ab93676`, request
`ig-point-273-cos-guid-boundary-diagnostic-v1`, job
`acb653993bf1402bbc67233b178185f5`. GUID+COS alone is not the demonstrated fix.
Offline comparison found identical Source Text descriptor/value/COS/GUID in the
accepted full-group and rejected partial-group diagnostics. Remaining group
context differences include implicit names, native empty More Options versus
five generated controls, and an extra generated empty Animators group. The
missing empty Path Options was corrected, but fresh294 still rejects Text.

Import source structure remains inspectable; **editable Point export acceptance
and edited-content fidelity are blocked**. At this native diagnostic checkpoint,
full tests/clippy were unrun; later PR validation is recorded above.
Do not ship diagnostic transplants, borrow Box glyph caches, guess enum types,
or present the native donor render as a feature-export pass.

Case `point-text-envelope-v1`, source `native_point_n.aep`, SHA-256
`5e67c21a5c0b3ce9c7f08f33a27189ef1d5078858e4d22f4716f842afffe2d3e`.
Independently Adobe-authored through typed `headless_adobe.Client.create_aep`,
request `ig-fix-all-native-point-n-oracle-v1`, Adobe AE `26.5x89`.
No converter-created AEP or cached Box donor was used to author this source.

Selected composition native ID `1`, name `Native point n`, 320×180, square pixels,
24fps, one second. One Point Text `n`, explicit requested `ArialMT`, font size64,
white fill, Position[160,90], Scale[100,100], Rotation0, Opacity100.
No external media; native font-table defaults are not a fallback-font proof.
Source bytes/FPS remain unchanged. No supporting compositions.

Retained source diagnostic:
`writer::text_document::tests::point_text_native_oracle_identity_and_controls`.
It pins source hash, actual authored text/font/size, native root/version,
paint class/type and paragraph/character run lengths. It checks source controls,
not import correctness, edited export or Adobe compatibility of our writer.
After the revert, queue212 ran this exact source-only test:1 passed,0 ignored;
queue213 formatting passed. The final retained fixture/test/docs do not change
production serialization at that checkpoint. The subsequent descriptor-only repair
below changes production metadata; full conv tests/clippy were pending at that
historical checkpoint (later PR validation is recorded above).

## Executed unsuccessful repair

Original fresh import/export via frozen converter191 emitted AEP SHA-256
`f2671a51f2c381eff4cd03fa7c28b91c9d320aa4627afd834d532accea635873`.
Adobe rejected it with `Error reading the text layer. Skipping the text layer.`
under request `ig-fix-all-independent-point-fresh191-v1`; runtime returned READY.

Candidate commit `8a3b16c235395dbc0ba8beae839f363d3f86f6d4` added bare root/version14,
SimplePaint class/type headers and UTF-16 author-run lengths without borrowing
native glyph/line caches or guessing font attributes. The then-named
`point_text_cos_native_record_envelopes` failed before the candidate (queue208)
and passed afterward (queue209:12 passed,2 ignored); queue210 check and211 build
passed. This was structural evidence ONLY.

Fresh211 import/export output SHA-256
`aa77358bf2473e0911bb75e2bb6cde102963d3af43beeb4b183dbff72384672f`
was submitted ONCE through `Client.render_aep`, target1,30fps, audio off,
request `ig-point-envelope-211-changed-minimal-v1`.
Adobe again rejected the Text layer before rendering; `EXECUTION_FAILED`,
verified `ready:true`. Native job `9cf579569bcf45698c1d26dc742f23b3`.
No usable video or nonblank-output pass exists. Both failed AEPs are excluded
from later retries/references. No original IG retry followed this failed gate.

The unproven production candidate was reverted in `f7a2a4155`.
Its generated-envelope assertions were withdrawn with it, not relaxed into a
passing export test. The separately named source-only diagnostic remains.
Cause is unresolved: omitted COS defaults/layout versus Source Text property
metadata versus data lost during import have not been discriminated. A native
format difference is not proof of the cause of Adobe's rejection.

## Source Text storage descriptor repair — structural pass, native rejection

Native Point, Box and older `text_ranges.aep` Source Text descriptors store
`00 01 00 08` at bytes56–59 and zero at byte60. The writer instead stored word1
and subtype8 (`00 00 00 01 08`). Commit `833204596` corrects only this local
property recipe; the general meaning of the packed word remains unknown.
No COS defaults, font attributes, glyph/layout caches or shared schema changed.
`writer::text_document::tests::source_text_descriptor_matches_independent_native_point_storage`
compares the complete124-byte native descriptor at24fps and with a30fps clock.
It failed before the fix (queue214); queue215 ran13 passing tests,2 ignored.
Check216/build217 passed. These are structural checks, not Adobe acceptance.

Fresh217 output SHA-256
`65eb785a2f50bf398d3a9df6914819021f5067dd0be5d021b28003bc806a1ab2`
was rendered once at30fps/audiooff under
`ig-point-descriptor-217-changed-minimal-v1`, job
`b56901bb1a5541d4ab997cae12f96af7`. It still produced the Text-reading error,
`EXECUTION_FAILED`, verified READY, and no usable video. The corrected descriptor
is not the sole rejection cause. This failed AEP must not be replayed.

A separate **nonshipping diagnostic**, SHA-256
`6af3541e4257a683d12ab73833a70a35b7075eb87bdac33d8c836f56cbcb256a`,
substituted the independently authored native Point COS into that generated
container, preserving nested lengths. It is not authored editable export.
The first API attempt was BUSY before any job/execution; one queue-timeout retry
actually executed request `ig-point-217-native-cos-boundary-diagnostic-v1`, job
`c5c5c458ca2c479ba4bc0317a0297af8`. Adobe reported `file is damaged.`, returned
verified READY, and produced no video. Offline identity re-encoding preserves
both containers byte-for-byte, nested boundaries validate, and frozen217's
metadata inspection parses the diagnostic. None of these establish Adobe validity.
The changed error does not uniquely identify COS versus owner/context defects.

The native Source Text property run includes a `btgu` companion with two16-byte
`pgui` records; generated Point Text lacks it. Native Path/More Options groups
are empty, while generated More Options contains five numeric properties and
an empty Animators group. The Box writer preserves native group glue and rewrites
its owner GUID, but our reader does not require/validate that GUID. These are
observed differences, not proven causes or permission to borrow Box glyph caches.

Independent Adobe 30fps source reference, long-term Asset publication/fresh hash
verification, generated-native editable control inspection, RGB/alpha comparison
and actual font resolution are still missing. No video is committed to Git.
This case remains diagnostic/incomplete; import and export fidelity are unmeasured.
