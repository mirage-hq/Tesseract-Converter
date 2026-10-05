# Native OUTLINE Source Text keys

`native.xml` is a minimal offline derivation of a human-authored Premiere 2026
save, source SHA-256
`e74d088116570ddb7178b127129036755be2f8e553dea80f98aa59b601585b76`.
Sequence `graphic_outline`: `a6eb3c4b-2c5b-4cef-b0fd-68b013bfb73d`.
Occurrence `VideoClipTrackItem:515`, Text component `1127`, Source Text parameter
`1264`. The source and its payloads were not modified. Extraction removes other
sequences, audio/data track groups, the backdrop video, sequence UI metadata and
its backdrop link, and retains the selected graphic's reference closure.
Whitespace-only lines left by extraction have their indentation removed.
Fixture SHA-256: `b008a36e6d7d00111cca9f8c4e5c8255731c767dd4f2c4067050e07439edf009`.

The generator In is `914455911965987` ticks; the placement is
`0..1268174448143` ticks. The three held Source Text keys are:

| Source ticks | Rounded local ms | Text | Enabled stroke width |
| --- | ---: | --- | ---: |
| 914455911965987 | 0 | OUTLINE | 5 |
| 915102766046919 | 2547 | OUTLINE | 25 |
| 915366614422036 | 3585 | OUTLINE | 25 |

All three omit stroke-color style slot 4; the third also omits run-marker slot
24. The saved sequence and generator frame period is `8511237907` ticks.

The tests supplement this fixture in memory with explicit white stroke paint,
a 30 fps rate and frame-aligned 5-second placement End/source Out: these are
**authored test inputs, not native facts**.
