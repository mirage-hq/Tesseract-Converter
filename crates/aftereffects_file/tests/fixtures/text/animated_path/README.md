# Animated Text Path export control

Independent native source authored through managed headless-adobe `Client.run_jsx`,
request `w06-animated-textpath-author-v1`, AE26.5x89. `author.jsx` is the exact
feature function body (not a standalone Adobe runner). Native source SHA256:
`9479a75522de5368a4a8d7945cc9cb32b34f353ad445d014c04a05ebe82b1010`.
Script SHA256: `f1bc04aece51217f4bc996b76924ac5644e81a1dda99b815e1b2c635ccce2c49`.

Target composition1 `W06 Animated Text Path`:640×360, square pixels,24fps,
full2s. Text `EDITABLE PATH`, ArialMT32, white, first margin20, mask index1.
Native save/reopen readback: two Linear open cubic Mask Path keys at0 and1s.
Vertices at0:[[80,160],[280,100],[540,160]];
at1:[[80,240],[280,180],[540,240]].
Tangents in:[[0,0],[-80,0],[-60,-60]], out:[[60,-60],[80,0],[0,0]].

`input.json` is an independently specified editable FX control with the same
cubic geometry, source-zero key times and Text controls. It is **not** a pruned
original49 or a claimed faithful import of the native control. The regression
asserts bounded guide lowering, affine input-edit response and clock rejection.

Native reference request `w06-animated-textpath-reference-v1` used managed
`render_aep` at30fps without changing native source FPS. MP4 SHA256
`e8b97b4b7dd734b045728e8bde2862f94ca67741ff6f7570ea5af61d70d6490c`,
640×360,2s,60 frames. Samples0/.5/1/1.5s show nonblank curved text moving down.
Video remains ignored/local; long-term Asset publication/download is incomplete.

Managed native readback request `w06-animated-textpath-readback-v1` attempted
base/green/edited fresh exports, but the baseline Text writer failed Adobe open
with `Error reading the text layer. Skipping the text layer.` READY recovered.
Green/edited native controls and RGB/alpha equality therefore remain unproved.
Native authoring evidence does not establish converter acceptance. Import is
unchanged and has separate Text mask-coordinate/wrapper limitations.

## Native pixel-unit / full-export regression

`native_text_path_fixture_pins_pixel_cubic_keys` consumes this exact native AEP,
pinning layer13, two open cubic keys at0/1s, complete vertex/control triples and
pixel bounds [80,100,540,160] then [80,180,540,240]. These are saved native-file
assertions; the original native getter receipt observed vertices/key count,
closure and path index, not tangent/time/interpolation getters.

`text_path_full_export_keeps_native_pixel_keys_across_canvas_and_input_edits`
freshly exports the explicit FX input and compares its generated Mask keys
against those source records. Nine combinations cover640×360,360×640,1279×513,
unchanged geometry, an affine guide-position edit and a second-key-only+40Y edit.
It also asserts Mask None/index1, the authored Text Path selection value1 and
absence of a separately painted consumed guide. The native selection's generic
scalar envelope and Adobe getter acceptance remain a separate proof gap; an
own-reader selection assertion is not native linkage proof.

RED queue1466 reproduces the wrong first exported bounds
[0.125,0.27777779,0.84375,0.44444445] versus native pixel bounds. GREEN1474 passes
7 selected tests (0 failed,4 existing ignored proof-backlog cases) after only
the TextPath guide normalization divisor becomes identity[1,1]. Ordinary mask
source dimensions, AV normalization, FX coordinates, clocks and native sources
are unchanged. This is not an invented1×1 source/canvas or a bounds clamp.
Generated native Text open/edit/reference/Asset/RGB/alpha proof remains blocked
as recorded above; no known-failed input was replayed and no4807 code was stacked.

See `docs/after-effects-support.md` for direction-specific limitations.
