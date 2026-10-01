# Script-driven paired Motion

This independently specified fixture proves one bounded export: paired Position,
uniform Scale and Rotation on a trimmed and offset video. It does not prove the
other script bindings or remove the earlier Opacity fidelity failure.

## Source and native controls

`source.json` is the original script-driven FX document. It was specified before
native control authoring. The sequence is 1920 × 1080, 30 fps, 3.5 seconds:

- An opaque gray video covers timeline/source 0–3.5 seconds.
- The red video occupies timeline 0.5–3.5 seconds and source 1–4 seconds.
- Five scalar scripts produce three native Motion controls, with Linear keys.

| Source / timeline seconds | Position, pixels | Uniform Scale | Rotation |
| --- | --- | --- | --- |
| 1 / 0.5 | 700, 430 | 35% | −15° |
| 2.5 / 2 | 925, 520 | 42.5% | 0° |
| 4 / 3.5 | 1150, 610 | 50% | 15° |

`proof.json` records source, media, converter, renderer, scorer and reference
hashes. Package `source.json` as `project.json`, the recorded metadata as
`metadata.json`, and the media as `assets/red.mp4` and `assets/gray.mp4`, using
ZIP_STORED. Red is the existing
`crates/premiere_file/tests/fixtures/feature_multi_sequence_red_10s.mp4` fixture.
Gray was generated with:

```sh
ffmpeg -nostdin -v error -f lavfi \
  -i 'color=c=0x505050:s=1920x1080:r=30:d=3.5' -an \
  -c:v libx264 -preset fast -crf 18 -pix_fmt yuv420p \
  -color_primaries bt709 -color_trc bt709 -colorspace bt709 \
  -color_range tv -map_metadata -1 -movflags +faststart gray.mp4
```

A different encoder build can produce different bytes; verify identities rather
than assuming a recreated archive is the recorded source. Videos and tools are
not stored here.

## Adobe evidence

Premiere 26.5.1 authored `control.prproj` from the expected values before candidate
XML inspection. The bridge could not author paired Position keys or retain one
trim operation. Approved XML edits set Position and placement; `control-edit.diff`
records these edits. The control was then reopened and checked.

The control and `candidate-9a915008f.prproj` had one intrinsic Motion owner, one
paired Position, one uniform Scale and one Rotation control. Each has two Linear
keys. Stored endpoints and the midpoint getter match the table. Native Position
uses X/1920 and Y/1080; Scale uses percent; Rotation uses degrees.
`native-readbacks.json` preserves the XML checks, and `proof.json` records the
reported native values and methods.

Both 105-frame H.264 exports used AME 2026 build 85 and the same pinned preset.
The lossless PNG pairs at frames 14, 15, 16, 59, 60, 61 and 104 have equal hashes.
This is sampled agreement, not an all-frame equality claim. Native lead-in alpha
is 255 at frame 0; no full-sequence alpha claim is made.

The independent control render is long-term Asset `Y02F9qv3rukjGwyqGOB7_vid`,
309,648 bytes, SHA-256
`e2cb856ae6553250e2d15490013af7d8d162933d3a5968310a1df118ce7e82b4`.
A fresh empty-cache download was verified before the final comparison.

## Original-FX comparison and final applicability

The original script-driven archive was rendered again with the pinned renderer
at `27f5c6513`. Full-resolution canonical RGB24 `rgb-hybrid` comparison at
0.25-second intervals passes the unchanged policy:

- Mean: **0.997579888617181**, required ≥ 0.99.
- Minimum: **0.9963071806119456**, required ≥ 0.98.
- 15 samples; the 3.5-second sample clamps to frame 104.

`original-fx-vs-control.json` contains the measurements. In `contact-sheet.png`,
Adobe is on the left and FX is on the right. The seven critical frame pairs were
inspected for placement, motion, scale, rotation and missing media.

The final converter at `82979b5b1` produced `candidate-82979b5b1.prproj`.
`native-identity.json` compares it with the Adobe-tested candidate: 913 XML nodes,
36 bijective GUID renames, and only output media paths otherwise differ. Class IDs
and every other field match exactly. Base64 ModificationState GUIDs were decoded
and checked against content state, not discarded. Both media files are
byte-identical. This final candidate was not separately reopened in Adobe.

The renderer, model, schema, native export, codec and public CLI production code,
and root Cargo configuration/lockfile, are unchanged from the fresh render to
`82979b5b1`; only a composition test file changed. `proof.json` records that audit.
Thus the native and original-FX evidence applies to this final Motion output.

## Limits

Effects, graphics, audio, nested owners and arbitrary scripts have no new Adobe
proof here. CPU checks for those bindings must not be presented as visual proof.
The historical two-property comparison (below) still fails: mean 0.943460,
minimum 0.619020. No masking, frame exclusion, renderer change or threshold
reduction was used to turn that failure into this pass.

## History: the earlier two-property candidate

An earlier candidate (`d3005b758`) baked Opacity `20 + 20 * t` and Rotation
`-30 + 20 * t` on the same trimmed, offset video
(`crates/premiere_file/tests/fixtures/script-video.json`, SHA-256
`c78d04c1a5802430e239bd414dd2e6741ed62143be12243b4338d5bf5dc704c3`). Its
editable keys read back natively (source 1 s / 4 s, Opacity 20 / 80, Rotation
−30 / 30, Linear), and all 105 decoded AME frames of that candidate equalled an
independent control (zero byte difference in every region). Its original-FX
comparison **failed** the unchanged 0.99/0.98 policy: mean 0.9434598, minimum
0.6190204 at 0.25 s, 15 full-resolution RGB24 samples; the control reference
was long-term Asset `bdfu4k28bIFeQ7ysH6AO_vid` (284,111 bytes, SHA-256
`0e22a776955e78e8918a1ef6f0253a35e82e6f4777adb3b5ffd676e128cb065d`). That
export path was rewritten; the record is kept here as a failure, not as proof
for the current code.
