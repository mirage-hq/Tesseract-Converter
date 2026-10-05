# Experimental native Source Text defaults

## Point profile

`point_root.cos` / `point_document.cos` use an independently Adobe-authored
**empty Point** owner, `point_text_envelope/native_empty_point.aep`, SHA-256
`02ef66ccaae8337b9421ae910842e7c94fcccd0fcbece00eeacf27b461b48707`.
This is Point geometry, not the Box fixture's vertices or glyph cache. No visible
donor text/glyphs exist: the source contains only the native terminal CR and
empty-line defaults. Typed FX content, whole-run style/lengths, requested font,
keys, property clock and owner identity replace the source fields. Document
PC/F/R/L/S/G caches are absent. Cache-free native regeneration is evidenced for
the matched static Point cases below, not assumed for every style/font/hold. Unauthored
Path/More Options remain native empty groups; no empty Animators is added.
An explicitly stored Character/[0,0] anchor without tracks is the native default.
Point font-changing holds have freshly authored, first-seen font entries and
document indices: requested first font0, unchanged native defaults1/2/3, further
requested fonts4 onward. No requested name is substituted; no guessed vendor/face
fields are added. The root's per-font vendor-version slot is also unknown in FX
and omitted, rather than copying the Arial source's `Version 5.01.2x`. Nondefault anchor/path/animators now retain the complete native Source Text envelope and receive separately generated editable sibling groups. Native numeric Text controls now retain independently authored descriptor/bound envelopes and their value-before-bounds order, with all authored values/keys and the selected clock replaced from editable input. The fresh build1180 P013 animator-only export opens in Adobe26.5x89 with original Poppins-Bold170, `Hello, World.`, Box geometry and animatorcount1, followed by verified cleanup/READY. Earlier descriptor-only and bounds-before-value candidates failed `missing data in file` and remain retained. Full-project movies, selector-value/key readback, SBS and richer-control fidelity are separate evidence, not inferred from this minimal acceptance.
Writer routing and owner-local experimental warnings share the same eligibility.

The earlier empty-line-cache candidate's fresh ArialMT64 `Fresh 49` export
opened/rendered in AE26.5x89, with no missing
fonts/substitution, full1s/30fps/320x180 output and verified READY. All30 decoded
RGB frames exactly matched an independently authored native control; each had
3970 nonblack pixels. This demonstrates fresh visible text and reflow beyond the
empty source, **not general font/style/richer-owner acceptance**. The subsequent cache-free64 case also opened/rendered. Cache-free48 initially
failed because absent semantic leading was serialized as a derived manual-leading
value. The independent native64/48 controls instead keep the inactive manual slot
at0.01 while automatic leading is true; the paragraph's1.2 factor controls layout.
The corrected cache-free48 output (commit7eabb5464/build958) exactly matches all30
independent native control RGB frames,2376 nonblack pixels/frame, READY:true.
Original P025 VT32348/content unchanged now also opens/renders a1920x1080/30fps
30-frame probe with a strict receipt reporting no missing faces/READY:true; its
requested prompt text is visible. An independently authored VT323 control later
fails strict admission as missing/substituted, so that receipt does **not** prove
actual VT323 resolution. This is Text acceptance, not full5s or P025 font fidelity.
The source-backed vendor-version regression fails before removal (queue1071).
Fresh build1086 after removal opens/renders READY and exactly matches all30
independent Arial48 RGB frames (job `88f4b57d83f646528898e5c88f06acec`).
A fresh ArialMT→VT323 Point Hold also renders/changing states/READY, but its
independent VT323 reference fails missing-font admission; requested faces and
native keys/editability remain unproved. The API currently checks `usedFonts`
before save/render; cache-free deferred face detection is an unresolved hypothesis,
not permission to substitute or install fonts outside the API.

Previously rejected artifacts were not retried or font-substituted. Fixed native Text control readback, immutable Asset publication,
alpha and general edited-layout fidelity remain missing. See the fixture README
and support ledger for exact identities, failed artifacts and limitations.

## Box profile

These two **structural default profiles**, not an input AEP, come from the
independently Adobe-authored public fixture
`tests/fixtures/pr4442_native/sources/text_document_box_v3.aep`:
SHA-256 `5b6bf16fb87930e38e975602c0848230173c50a179431d2a3eb34f09e0cffdce`.
Their use does not replay the user's project or donor-visible text. The native
property/record wrappers are read from that same pinned fixture, then its Source
Text blob, event clock and owner identity are replaced.

`box_root.cos` preserves the native bare dictionary, `/98` version header,
`/99` object-class headers and integer/real token types. `box_document.cos`
preserves one native document's default fields. Source UTF-16 literals in the
profiles use standard COS octal byte escapes so the files are readable text.
Whole author runs, text, first-font identity (including removal of unknown face
format/vendor version), and rectangle coordinates are replaced by named slots.
Document count and each Hold's Source Text, style and UTF-16 length come from
typed FX input. Replacement bytes are not scanned as templates; braces in user
text cannot insert native records. Typed UTF-16 literals escape parentheses,
backslashes and CR/LF byte values, including bytes within non-ASCII code units,
to avoid COS line-ending normalization. A byte-level regression checks this
without claiming Adobe acceptance. Other font-table entries are native defaults
`Myriad-Roman`, `Helvetica` and `AdobeInvisFont`, not user-requested faces. Their
behavior has not been matched to the existing FX fallback/custom-font system or
actual font binaries. They must not be described as preserving that font system;
this unresolved font/default-table risk carries an owner-local warning. No
font-name alias is added by this follow-up; the proposed Arial-to-ArialMT alias
was withdrawn rather than guessing the user's resolved face.

The whole-document font and box geometry must be constant across holds. Point
Text uses its separate eligible profile above; Text animators, path and nondefault
anchor options retain this native Source Text envelope with editable sibling groups.
The complete numeric-control envelopes have bounded fresh P013 animator acceptance
above; general richer-control raster/alpha/font fidelity remains unproved. FX→AEP emits owner-local
warnings for the experimental defaults.

**Box documents no longer inherit glyph/line caches.** Independently authored
nonempty Menlo18 Box Text uses inactive manual-leading 0.01 when automatic
leading is enabled. Omitting its layout cache was independently accepted by
AE26.5x89. A source-backed regression pins this semantic default; fresh source
semantics from P047's 17 Box and 4 Point layers now open and read back in Adobe.
Unknown requested-font vendor metadata is omitted, not copied from Arial.
This bounded acceptance/readback does not establish full-project rendering,
alpha, box reflow or actual font-binary fidelity.

The prior full `Value` serializer lost native real token types and reordered
class headers; the source-backed lexical regression fails before this correction
and passes afterward. It is a **structural regression only**. The previous
converter output was rejected by Adobe, and its headless cleanup produced a
`requires_operator` fence. After operator recovery, fresh static and two-Hold
cases with explicit ArialMT passed Adobe open/render, decoded nonblank/changing
pixels, cleanup and fresh READY. Supplemental own-parser readback of Adobe's
normalized saves retained their input text. This is bounded native acceptance,
not independent native controls or raster fidelity. A fresh full original-project
AEP still produced the Text-reading error and another fence under the earlier
lifecycle. Current headless recovery performs bounded owned cleanup and fresh
READY without a manual-unlock latch; malformed-AEP recovery remains unproved.
Every AEP that produced that error is excluded from later inputs/references;
failed inputs must not be replayed. See the
[partial support ledger](../../../../../docs/after-effects-support.md#deep-blue-v13-destination-media-and-typed-text-export-repair--partial)
for separate implementation, acceptance and fidelity status.
