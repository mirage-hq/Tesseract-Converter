# Manual AEP audio artifact comparator (PR #4458 follow-up)

This is an **offline test-infrastructure milestone**, not native proof. The
[case manifest](aep_audio_test_cases.json) records distinct import and export
contracts, both `UNRUN`. No Adobe application, native render, publication,
download, conversion or production audio scoring was executed for these cases.
Do not use the historical RGB MP4 reference hash as an audio-reference hash.

| Direction / case | Known input and identity | Missing before comparing |
|---|---|---|
| import / `import-audio-gain-keyed-c63-pending-audio` | unchanged `media/import_audio_media_controls.aep`, SHA-256 `36920d6bdc07dbb6292cc305327ace44efbacacb182dc18da3c979439f5473a8`, composition item ID 63; existing editable-key assertion in `export_document::tests::audio::audio_native_keyed_levels_import_and_fresh_export_retain_editable_gain` | Original primary audio media and independently rendered/inspected native audio reference, immutable reference bytes/hash, fresh import of the exact source, FX audio export artifact, matched timing/channel policy and comparison result. Existing native fixture is supplementary, not a newly authored feature oracle. |
| export / `export-edited-audio-pending-native-oracle` | No pinned FX archive, no independently authored expected AEP target | Pin explicit edited `.tsrct` bytes/hash, independent native expected source hash/composition ID, corresponding inspected reference audio/hash, fresh converter export, independent Adobe open/render of that export and comparison result. Own-reader round trips do not fill this gap. |

`make -C opensource/conv aep-audio-test-check` runs 14 offline synthetic tests (identical,
Hold/Linear gain envelopes including wrong key time, slope and level, generic gain
error, timing offset, whole/channel mute, swapped stereo channels, silence
leakage, duration/channel drift, nonfinite/missing input, invalid policy,
delayed/missing MP4 audio PTS, direction and hash rejections) and validates the
pending manifest. This target uses ffprobe/ffmpeg
for decode only; it does not call Adobe or render FX. The pinned initial policy in
the JSON is **48 kHz stereo, four seconds**, decoded duration difference ≤10 ms,
channel-by-channel relative RMS error ≤0.05, worst unaligned 10 ms window RMS
error ≤0.003, and actual RMS ≤0.0005 in windows whose reference RMS is ≤0.0001.
There is **no** gain normalization, channel mixdown, time shift or alignment.
**Zero-origin artifact contract:** both the audio stream and container must report
`start_time == 0`; only raw WAV may omit these fields. Nonzero, missing (for
other formats), nonfinite or invalid presentation origins are rejected before
decoding. Stripping MP4 timestamps to PCM does not make delayed audio equivalent.
The policy is an untested proposed gate for future independently obtained case audio,
not a measured threshold or claim of codec equivalence; any necessary policy
revision requires separate review **before** scoring real cases. Nonfinite samples,
missing/multiple audio streams, missing/changed hashes, mismatched rate/channels,
invalid metadata, excess duration and absent cases fail closed. Inputs are limited
to 30 s, 48 kHz and two channels for bounded memory; decode may consume up to
about 12 MB per artifact, and each invocation selects exactly one case/direction.

To compare after separate, explicitly authorized creation and SHA verification of
native reference and actual artifacts, fill the **case-specific** blank fields in
a reviewed copy of the JSON manifest and run from the enclosing repository root (both paths
must be real files):

```sh
make -C opensource/conv aep-audio-test args='--manifest /path/to/reviewed-cases.json --case import-audio-gain-keyed-c63-pending-audio --direction import --source crates/aftereffects_file/tests/fixtures/media/import_audio_media_controls.aep --reference /path/to/independent-native-audio.wav --actual /path/to/fresh-import-fx-render-audio.wav'
# This legacy comparator does not perform conversion or editable inspection.
# make -C opensource/conv aep-test runs the separate audio_e2e/README.md conversion pipeline.
make -C opensource/conv aep-audio-test args='--manifest /path/to/reviewed-cases.json --case export-edited-audio-pending-native-oracle --direction export --source /path/to/independent-native-expected.aep --fx-input /path/to/edited.tsrct --reference /path/to/independent-native-audio.wav --actual /path/to/fresh-export-adobe-render-audio.wav'
```

The runner checks the native-source hash, expected reference hash and (for
export) edited-FX input hash before decoding. It **does not** create, download,
authenticate, inspect or verify provenance of either audio artifact beyond those
hashes; it cannot establish that `--actual` came from a fresh conversion or
that `--reference` came from Adobe. The caller must document conversion commands,
source/target identities, decoded primary media, independent Adobe settings,
Asset IDs and fresh verified downloads, observed results and critical time windows
outside the manifest. The runner prints separate per-channel metrics and returns
nonzero on failures; a `check` result means only that the test contract is valid.
Never store downloaded reference media in Git. Import/export Adobe acceptance,
editable export inspection and audible/render fidelity remain **unmeasured**.
