# Essential Properties media replacement

Pinned MIT py-aep revision `e12a451c35bacd3f34a080090265f9370e66162b`.
`provenance.json` records exact immutable source paths, sizes and SHA-256.
The Rust regression verifies both native AEP and independent AE JSON hashes.

Composition 15 / layer 27 instantiates source composition 2. Its media-replacement
controller targets composition 2 / layer 14 (`image_with_alpha.png`), replacing
its source with item 30 (`image_with_alpha.png_sequence`). The controller UUID
is the single `Utf8` before `CTyp`; media UUIDs and a cached filename after
`CTyp` are separate metadata. `blsi` identifies the alternate source.

The native regression failed before implementation (controller incorrectly
rejected for multiple Utf8 metadata fields). It now checks occurrence-local
source replacement, byte-exact preservation of every other layer-record field,
unchanged source content, fresh editable graph expansion of wrapper composition
30 / layer 42, and unchanged direct import of source composition 2.
Supplemental malformed storage, non-AV target and missing-item tests verify
isolation and retention of the original source.

The wrapper references a GIF image sequence. Its media bytes are not pinned
here. These tests establish native parsing and editable reference expansion,
**not** image-sequence rendering, Adobe open/inspection, native pixel/alpha/audio
comparison, or export support. No new Adobe fixture authoring or rendering ran.
