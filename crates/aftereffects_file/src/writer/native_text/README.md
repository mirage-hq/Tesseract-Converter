# Native Source Text profiles

The writer uses pinned native **structural defaults**, not visible donor text or
replayed user AEP records. Typed FX input replaces wording, style/run lengths,
requested font, geometry, keys, property clock and owner identity. Native text
export remains experimental; supported structure is not general layout fidelity.

## Source profiles

| Profile | Independently authored source | SHA-256 |
| --- | --- | --- |
| Point | `point_text_envelope/native_empty_point.aep` | `02ef66ccaae8337b9421ae910842e7c94fcccd0fcbece00eeacf27b461b48707` |
| Box | `pr4442_native/sources/text_document_box_v3.aep` | `5b6bf16fb87930e38e975602c0848230173c50a179431d2a3eb34f09e0cffdce` |

Sources are under `tests/fixtures/` in this crate. Point defaults come from an
empty Point owner, not Box vertices or a donor glyph cache. `point_root.cos`,
`point_document.cos`, `box_root.cos` and `box_document.cos` supply their distinct
native envelopes. Source/property wrappers remain pinned to these profiles.

## Serialization requirements

- Preserve COS dictionary/class-header order, integer versus real tokens and
  the native record envelopes. Generic JSON reserialization is not equivalent.
- Insert typed UTF-16 literals with escaped parentheses, backslashes and CR/LF
  bytes, including those inside non-ASCII code units. User braces are not templates.
- Generate current document/run counts, style lengths and font indices from input.
  Do not carry donor-visible text or glyph/line caches into new documents.
- Keep automatic-leading semantics separate from inactive manual defaults;
  a derived manual value can change native layout.
- Omit unknown requested-font vendor/version/format metadata rather than copying
  a donor's identity or inventing an alias. Verified archive-font context is separate.
- Preserve complete admitted Source Text/numeric control envelopes when adding
  supported path, anchor or animator groups. Values, bounds and keys have native
  ordering requirements.

## Fonts and animation

Point font-changing Holds allocate requested font entries and document indices.
Box profiles require compatible constant font/geometry across supported Holds.
Other font-table defaults include Myriad-Roman, Helvetica and AdobeInvisFont;
their presence does not establish actual font-binary resolution or the FX fallback
system. Requested faces must not be silently substituted.

Nondefault anchor, Text Path and animator controls have separate admission.
Experimental warnings remain owner-local. Embedded font delivery does not install
fonts or establish shaping, selector, reflow or rendered equivalence.

## Validation limits

Selected static Point/Box and animator profiles have bounded Adobe acceptance or
render/readback observations. Other generated profiles have failed Adobe opening.
A name reported before render, an own-parser round trip, or a passing static case
must not be treated as exact-font or general richer-text acceptance.

Keep source hashes and compare the exact exported bytes when obtaining new native
evidence. See [text support](../../../../../docs/after-effects-support.md#text-masks-and-compositing),
[fixture provenance](../../../tests/fixtures/point_text_envelope/README.md)
and [evidence interpretation](../../../../../docs/after-effects-test-results.md).
