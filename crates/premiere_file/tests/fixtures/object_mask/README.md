# Saved Object Mask fixtures

These fragments exercise saved Premiere Object Mask raster decoding and editable
matte import/export. They are source-derived parser fixtures, not independent
Adobe render references or a complete native project.

## Native records

`opacity.xml` contains unchanged records from a human-authored Premiere Pro 26.x
save. The original source SHA-256 is
`e74d088116570ddb7178b127129036755be2f8e553dea80f98aa59b601585b76`.
Sequence `object_mask` has UID `ce705d24-57c6-4595-8562-6da99c0a7c1d`.
Video occurrence `362` names chain `495`, Opacity `665` and active mask `1079`.

| Fragment | Bytes | SHA-256 |
| --- | ---: | --- |
| `opacity.xml` | 16758 | `7878aea60f693c2a0a7a9970c5f306385cc4406f8ab3cabe936877a7e1199bb1` |

The fragment keeps record order, IDs and binary values, with one newline between
records. It has no XML root or media. Active `AE.ADBE AEMask2` v9/c7 mask `1079`
has Type 4 (parameter `1262`, ID 22). Tracker `1237` (ID 24) references the saved
sidecars below. User Interactions `1263` (ID 23) remains opaque; import does not
reconstruct selection or AI propagation controls.

## Raster excerpts

| Fixture | Bytes | SHA-256 | Derivation |
| --- | ---: | --- | --- |
| `initial.prmf` | 18512 | `91a9d2d90d8bf7b0f88f313810de6c87f7e360afbb51355f12a5a210aac3498c` | Unchanged initial-selection sidecar `8abda722-8116-46fc-b431-9aeab7d80730.prmf`. |
| `propagation-first-two.prmf` | 34032 | `5065e52b3ed94e57374a2918cef413ab606d180513678cd02bb84481ba9a3994` | First two compressed frames of sidecar `dd06d550-fb83-4fb0-b9e8-6d3d6fcdedf1.prmf`, with a rebuilt bounded index. |

The original propagation sidecar has 204 frames, 3624784 bytes and SHA-256
`b853ddef0b67f7c53ef00851330aeda9daf9acf6575538d57a5e460ad5516273`.
The excerpt preserves compressed bytes `[32,33816)`:

- Frame 0: offset 32, length 16884, timestamp 0, crop `[284,50,768,670]`.
- Frame 1: offset 16916, length 16900, timestamp 8511237907 ticks,
  crop `[280,51,759,669]`.
- Both source canvases are 1280 × 720. Rebuilt metadata occupies 216 bytes at
  offset 33816. Header metadata offsets and index relocation differ from the
  original sidecar; the excerpt is not a 204-frame source.

The raster describes a person's silhouette. No source picture, audio, private
filesystem paths or decoded-image dumps are included. Inclusion does not imply
an independent licence for the original picture or an Adobe alpha-fidelity pass.

## Import and edited export

Import recovers saved coverage as ordinary numbered PNG matte assets when the
physical source and referenced sidecar pass admission. The original source video
remains editable. Invalid or missing coverage omits the affected picture safely.
Export writes the current supported matte placements and edits; it does not
replay the original controller state or enable re-propagation.

Public API tests use a supplementary 30-frame index built from the unchanged
compressed crop and a checked-in test video's canvas/clock. They test local
Motion, trims, inversion, source bytes, path selection and edited matte export.
That scaffold is not the original Adobe timeline. Separate sampling tests retain
the original 8511237907-tick cadence; a reachable saved Object Mask can opt into
30 fps sampling with repeated frames. Other sequences retain ordinary native
clock handling.

See the [converter limitations](../../../../../apps/tesseract-conv/README.md#conversion-accuracy-and-limitations).
Structural, crop-pixel and publication tests do not establish independent Adobe
RGB, alpha or audio equality. Export accepts uncovered timeline gaps without
requiring an authored black canvas or changing logical coverage. Sampled-matte
membership, sequence duration, audio and physical source endpoints retain their
existing rules.

## Decoder and safety limits

The internal decoder uses `gdeflate-sys` 0.4.1 (MIT/Apache-2.0), with its bundled
upstream MIT and NVIDIA Apache-2.0 notices. A small wrapper checks bounded slices,
decoder status and exact decoded byte count. Per-architecture CLI builds avoid
the dependency's host-selected CPU-source cross-compilation limitation.

The decoder caps frame counts at 100000 and one-byte canvases at 64 MiB. It
checks crop/payload boundaries, metadata bounds, known slots, nonoverlapping
payloads, dimensions, referenced paths and Tracker/index cadence/count. It
decodes one frame at a time. Vertical wrapping addition of the preceding crop
row continues across compression-tile boundaries.

The C API does not report consumed compressed-byte counts, and tiles have no
checksum. Corrupted tiles can produce incorrect pixels despite successful status
and exact output length. Guard-page, malformed-table, truncated-tile and hash
regressions exercise this boundary; successful tests are not a soundness or
Adobe-fidelity proof.
