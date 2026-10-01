# Native audio conversion cases

`audio_cases.aep` was independently authored by Adobe After Effects 26.5x89
using `author_audio_cases.jsx`, not the converter's writer. Native save and
scripted control readback succeeded. `native-readback.json` records observed
controls; `provenance.json` pins source/media hashes and links supporting
compositions to cases. `cases.json` pins all 28 root composition IDs (320×180,
6 seconds, **24fps**) and 25 explicit edited FX documents under `fx/`.

There are **19 import and 25 export case-direction selections**. This is an
input/contract inventory, not 44 passing conversions. All 28 independent native references were rendered at 30fps and are committed
under `tests/references/aep/audio/`. Repository-relative paths are in
`cases.json`; the historical publication evidence remains in
`reference_provenance.json`. Ordinary tests load the committed files directly,
without Asset API access, URL downloads, or media hash checks. Adobe acceptance
of generated exports remains **UNRUN**.

The Adobe-free `wave-static` smoke ran in both directions but failed the duration
contract: generated AAC is 5.952 seconds versus the six-second reference.
The CPU export structural panel passed 22/25 cases; outstanding differences are
`all-zero-one-key` (terminal interpolation), `linear-gain` (Bezier versus Linear),
and `empty-only-group` (retained Null versus expected omission). Thresholds and
pinned expectations were not weakened. Other conversion comparisons remain
unmeasured. These are not fidelity passes.

## Commands

Run these commands from the **enclosing repository root**. The native fixtures and
Rust assertions and audio E2E scripts live in the standalone conv workspace.
The `tsrct` renderer is an external dependency; the standalone converter alone
cannot execute the full audio/render comparison.

```sh
make -C opensource/conv aep-audio-test-check  # offline comparator/orchestration tests
python3 opensource/conv/scripts/aep-test.py list  # verify pins and list real cases
make aep-audio-e2e-build  # root-owned integration tools
# Ordinary tests need no Adobe installation/license; use a NEW work directory:
make -C opensource/conv aep-test args="--case wave-static --direction both --tsrct $PWD/target/debug/tsrct --work tmp/audio-wave-static-run1"
```

The final command really converts, inspects, renders and compares; it is not a
mock. It reads committed repository references without Adobe, network access, or
company credentials. Missing references fail explicitly. Reference preparation
is separate maintainer-only tooling
(`opensource/conv/scripts/aep_audio_reference.py` in the enclosing repository),
never imported or invoked by ordinary tests. Retired `--allow-adobe` and
`--create-reference` flags are rejected. No CI policy is changed.

- **Import:** scratch-only source relinking → fresh AEP→TSRCT import →
  feature-specific editable assertions → FX audio render → pinned reference.
- **Export:** package and render the explicit edited FX → fresh FX→AEP export →
  own-reader native structural assertions → fresh reimport → FX render.
  Compare against both direct FX playback and the pinned independent reference.
  This is own-reader/roundtrip evidence, **not Adobe acceptance or an Adobe render
  of our export**. Obtain approval before any broader comparison run.
- Results, stage failures, diagnostics, hashes and logs persist under the fresh
  work directory. A failed case/direction does not silently pass or prevent
  other requested directions from being attempted. Missing tools/pins, conversion
  failures and timeouts are execution failures, not low audio scores.

## Coverage and limitations

| Cases | Contract |
|---|---|
| wave-static, trim-offset | source identity, .5 gain, source offset .5s; sourceRange duration is not implicit playback speed |
| native-muted, hidden-layer, hidden-group, static-zero | audio switch/visibility and exact static mute; import currently projects native mute to zero gain, not preserved inactive gain controls |
| constant-zero-zero-base, constant-zero-nonzero-base | Constant(0) overrides the scalar base and disables native audio |
| all-zero-one-key, all-zero-two-keys, zero-base-unmute | all-zero tracks mute; nonzero authored keys override a zero base |
| hold-gain, linear-gain, bezier-gain | key counts, times, interpolation and gain; continuous gain→dB conversion is explicitly approximate |
| affine-playback | positive affine two-key playback and gain-key rebasing through trim/stretch |
| eof-tail | audible prefix preserved, removed editable silent tail diagnosed |
| mov-audio-only | real audio/video source with video disabled and audio enabled |
| audio-group, group-affine-playback, empty-nested-group, fractional-duration | audio-only precomps, affine parent clock, empty siblings, whole-frame storage without extending occurrence timing |
| source-switch | independent native occurrences, source offsets and original-owner gain keys on every occurrence |
| nonlinear-remap | native movie Time Remap; editable audio playback without gain keys |
| conflicting-gain-clock, invalid-gain-hull, empty-only-group | diagnosed unsupported target/empty hierarchy and surviving sibling; not fidelity of omitted content |
| stereo-levels, expression | import-only quieter-channel approximation and unsupported expression diagnostics, preserving siblings |

FX-only controls have export-only contracts; native-muted/stereo-levels/expression
have import-only contracts. The manifest is authoritative. Source-switch and
zero-base/override identity require the explicit FX documents; identical native
mute controls do not by themselves prove those different FX inputs. Unsupported
and approximate cases may fail fidelity scoring intentionally; never weaken the
policy to hide a mismatch. Audio tests do not establish RGB or alpha fidelity.

## Primary media (not reference renders)

`sound.wav` and `other.wav` are deterministic four-second, stereo 48kHz, 16-bit
PCM with distinct channel/second/source frequencies and clock ticks.
`movie.mov` is an **eight-second** blue 320×180/24fps AVC1/AAC primary source.
Its qt-brand, AAC-v0, constant-packet-duration, no-edit-list container exercises
the existing supported MOV profile. AAC priming remains in this primary signal;
it is not a sample-identical replacement for the WAV. Both directions of the
MOV/Time Remap cases use the same pinned movie bytes. Source hashes and byte
counts are in `provenance.json`.

Generate media only into a new directory:
`python3 opensource/conv/scripts/aep_audio_fixture.py --dir <new-dir>` from the enclosing repository's
root, or `python3 scripts/aep_audio_fixture.py --dir <new-dir>` from standalone conv.
Do not overwrite these immutable native inputs. Author a new revision when a
case changes. The published 30fps references preserve full six-second
composition duration/canvas; source FPS/bytes stay unchanged. The 28 semantic reference MP4s under `tests/references/aep/audio/` are the
committed test oracles; primary media remains in this fixture directory.

The comparator never normalizes gain, shifts time, mixes channels or trims to
make a policy pass. It rejects nonzero presentation origins. Only AAC decoder
padding (at most one packet) beyond the stream's own declared presentation end
is discarded; synthetic unit coverage checks this independently.
