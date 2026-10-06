# `premiere_isolated_nest_audio_outer_keys_26_5`

A Premiere-native nest audio item with Level keys. The case is
`structural_only`: no converter render has been compared with its reference,
and nothing here is a score.

## Source

- `feature_nest_audio_outer_keys_26_5.prproj`: 15,645 bytes, SHA-256
  `76777fbd64aabc3dbf1447a1fea617cde43b71ef8ba9fba19524881bd3c8b7d0`. A new
  project saved once by Premiere Pro 26.5.1 (project version 45) through its
  scripting API; the Premiere UI was not inspected. Its bytes are unchanged,
  including the authoring workspace's absolute media paths beside the
  package-relative `./<name>` paths that conversion resolves.
- `feature_timecoded_source.mp4`: the existing fixture (279,125 bytes,
  `4256ae026cb923ee0498374a1def4dd8c0e51078099a9f415726935e198ac0fe`).
- `nest_tone_stereo_8s.wav`: 1,536,044 bytes,
  `83cea43ac62f001ee5b99916315edad05d3b9ecfa5a5fc537f1ed452a60855ca`, generated
  for this case: 8 s of 48 kHz stereo, phase-continuous sines, left 0.25 at
  400 + 40k Hz and right 0.125 at 1300 + 40k Hz, where k = floor(t / 0.5 s).

Sequence "Nest outer keys 26.5" (`f47a5a31-cde1-470d-b4b9-c9791fd203e2`, 30 fps,
1920x1080) plays "Nest inner 26.5" (`e7b3e816-661a-4943-a234-c5de54750bac`:
the video over 0-10 s and the tone over 0-8 s at a static -6 dB) as a nest.
Video item 91 plays it at 1-8 s from In 1.5 s. The linked audio item 92 plays
it at 1-7 s from In 1.5 s, and its clip keeps Out 8.5 s: the item was
shortened to End 7 s through its track item's end, and whether a trim in the
UI writes the same Out is unverified. Item 92's Volume (Mute 173, Level 174)
has six Level keys on its In/Out clock, each saved with its outgoing
interpolation: 0 dB at 2 s and -12 dB at 3 s Linear (mode 0), -12 dB at
3.5 s and silence at 4 s Hold (mode 4), and +6 dB at 5 s and -6 dB at 6 s
Linear. The Level falls from 0 dB to -12 dB over 2-3 s and stays there (a
Linear segment between equal Levels) until the Hold key at 3.5 s, which
keeps it at -12 dB until the step to silence at 4 s; silence then holds
until the step to +6 dB at 5 s, and the Level falls to -6 dB at 6 s.

## Reference

AME 2026 build 85 rendered the sequence with "01 - Match Source - High
bitrate.epr" (preset SHA-256 `68cf2ab2a0a62193c8d9f4a0b4501c0bd4be72258a13c1e0437539cf35aa4597`):
H.264 1920x1080 at 30 fps, 240 frames (8 s), AAC LC 48 kHz stereo. The render
is long-term Asset `eSCZMZ4ckQRGClY36oXP_vid` (1,168,056 bytes, SHA-256
`0c75947caf4bb8f9861189b4f61f0d7314a2f25123a797d22efd629d4868e4f9`), freshly
downloaded and verified after publication. It is not in Git.

## Source facts measured from the reference

These were measured from the reference render; the measurement data and logs
are not in this repository. They describe Premiere's output, not the
converter's. After one lag of 1024 samples (21.333 ms, AME's unsignalled AAC
priming), per-channel gains in 10 ms windows against the unit-gain source
placed by the saved Start, End and In show:

- The keys play on the item's In/Out clock, at outer time Start + key - In:
  0.0057 dB rms error over 1000 windows, against 8.93 dB for the clip's own
  clock and 3.43 dB for the outer sequence's.
- Linear segments follow the clip fader law, linear in u = g^0.4475
  (2 - g^-0.4475 above 0 dB): 0.0057 dB rms, against 0.743 dB for linear gain
  and 0.309 dB for linear dB.
- The inner -6 dB multiplies the item's Level: -6 dB before the first key,
  -18 dB over the -12 dB segment and its Hold, and -12 dB after the last key.
  The first key's Level holds before it and the last key's after it.
- A Hold steps at the next key, within the 1 ms window: to silence at outer
  3.5 s and to +6 dB at 4.5 s.
- The sound starts at 1 s and stops at End, 7 s, not at Start + (Out - In)
  = 8 s, though the inner tone plays on; it is digital silence before 1 s,
  over the silent Hold and after 7 s. Left and right take equal gains.
- The picture shows the inner timecode frame-exact from source 1.5 s and
  continues after the sound ends.

## Conversion coverage

`adobe_nest_audio_item_level_keys_play_its_sound_until_its_end`
(`crates/premiere_file/tests/conversion/audio.rs`) imports the unchanged
project: one audio layer of the tone at 1-7 s from source 1.5 s at -6 dB,
whose keys lie at layer 0.5, 1.5, 2, 2.5, 3.5 and 4.5 s (-6, -18 and -18 dB,
silence, 0 and -12 dB; linear, fitted, linear, Hold, Hold and fitted, each FX
key easing the segment that ends at it), and a group of the inner video over
1-8 s from source 1.5 s, without sound.
`a_nest_audio_item_plays_its_sequence_from_in_until_its_end` converts edits of
it. These assert editable structure only.

## Not measured or not covered

- The converter's render of this case has not been compared with the
  reference: render fidelity is unmeasured, and there is no score.
- This source has no mono or remapped channels, other speed, reverse, keyed
  inner sound, track fader, mute or Clip Gain on the item, Bezier keys,
  second item or occurrence, or hidden or disabled picture. Premiere's own
  playback was not measured.
