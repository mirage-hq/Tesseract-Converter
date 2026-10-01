# `feature_motion_anchor_scale_width_probe.prproj`

Native probe `premiere_motion_anchor_scale_width_probe_20260930` for keyed
intrinsic Motion Anchor Point and Scale Width. It is a structural derivative
of `feature_motion_static_transform_strict.prproj` (SHA-256
`a3291bc0c24e0d1356dcb1b5461d6324502c87aa9a3da8af79740eec8619c39f`), not a
Premiere save: only the Motion parameter records 147, 148, 149, 151 and 152
changed. Their key strings take the Linear forms that Premiere saved for
Position in `feature_motion_opacity_26_5_strict.prproj` (14-field point keys)
and for Scale in `feature_motion_scale_linear_strict.prproj` (8-field scalar
keys). Every other record, identity and reference is the parent's.

- 7,718 bytes, SHA-256
  `429745911a526d1d99f008f09c68b05853a63d2a8ddf0c100dc6e48b9690f809`; decoded
  XML SHA-256 `b31933ffaa6f40c6340694d2e66c57d627adba7d62caa86fd94beb8e76cabc0f`.
- Sequence `c8acf9c1-34b2-4086-9f55-d528950a7059` (it keeps its parent's
  name), 1920 x 1080 at 30 fps: one 2 s clip of the unchanged
  `feature_timecoded_source.mp4` (SHA-256
  `4256ae026cb923ee0498374a1def4dd8c0e51078099a9f415726935e198ac0fe`) from
  source 0 s. Its only media path is that package-relative `RelativePath`.
- Motion: Position 0.5:0.5, Scale 50 with Uniform Scale off, Rotation 0;
  Anchor Point keys 0.25:0.25 at 0 s and 0.5:0.5 at 0.8 s, and Scale Width keys
  50 at 1 s and 100 at 1.8 s, all Linear and without spatial tangents.

AME 26.5.2.2 rendered the sequence once (job
`be41e5f3d80144a2bf41254eca106a4b`): H.264 1920 x 1080 at 30 fps, 60 frames,
257,537 bytes, SHA-256
`f88570a7764a341dce4857bab71b7e00ecec52f392c69be18cbb542af25f200b`. It is the
checked-in reference of the strict case
`premiere_motion_anchor_scale_width_probe_20260930` (the workspace's
`tests/references/premiere/videos/premiere_motion_anchor_scale_width_probe_20260930.mp4`;
see its `tests/README.md`), not an Asset. An independent measurement of
all 60 frames fits Position plus each axis's Scale times the source pixel less
the anchor (the Anchor Point times the source size) within 0.625 px on the
picture's edges and 0.8125 px on timecode landmarks, and its 14 critical
frames were inspected. The render also carries an AAC stereo track, which was
not assessed. The Premiere UI was not inspected.

`native_probe_anchor_point_and_scale_width_keys_import_as_editable_axis_tracks`
(`tests/conversion/roundtrip.rs`) pins these bytes and asserts the editable
import. A fresh conversion of this project passes that case's score gate
(picture only). A generated export of a separate explicit FX edit passed a
bounded Adobe export gate, recorded with that case in the workspace's
`tests/README.md`; its render is not an import reference.
