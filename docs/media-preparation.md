# Explicit media preparation

Adobe import never starts FFmpeg or silently removes a used video because its
container/codec is unsupported. Inspect and preparation are separate operations:

```sh
tsrct-conv inspect source.aep --composition 4 --json
tsrct-conv transcode input.mov --output prepared.mov \
  --backend external-ffmpeg-command --ffmpeg-path /path/to/ffmpeg \
  --ffprobe-path /path/to/ffprobe
# Separately assemble a source-bound map if these bytes will replace project media.
tsrct-conv convert source.aep --to tesseract --composition 4 \
  --media-map prepared/media-map.json --output converted
```

For Premiere use `.prproj` and `--sequence <native-ID>`. Names never select a
scene. Inspect without a selection lists every target. `transcode` takes one
media file and writes one media file: it never reads a project, selects a target,
inspects unrelated assets, or creates a media map. Call it separately for each
file that needs conversion; retry only a failed invocation. A map is bound to
one target, so select that target when
inspecting with a map in a multi-target project.

## Hybrid export destination preparation

Selected AE picture scopes can prepare otherwise unsupported whole video sources
with the library backend and recheck the result through native AE admission.
Exact H.264 and ProRes-alpha clocks remux into video-only MOV without edit lists;
eligible opaque sources use the existing H.264 profile. Original assets and FX
clocks stay unchanged. Already-admitted sources, including supported MP4, need
no preparation. Audio/timecode, unsupported colour/timing/topology and unverified
alpha precision retain the complete native Premiere picture scope.

Enabled unmapped effects withhold video preparation for the selected scope.
All original assets still undergo archive lookup, kind/integrity checks and
ordinary interpretation. Existing still/audio preparation is unchanged. A source
policy rejection retains native fallback; I/O, cancellation, malformed output and
backend failures abort publication. Preparation does not prove native RGB, alpha,
audio, relocation or edit propagation. See the [linked-picture limits](after-effects-support.md#linked-picture-destination-media-and-source-clocks).

## Inspection is admission, not render proof

Default `inspect` emits schema version 2: the original scene inventory plus
`media_preflight`, `media_admission` and `video_admission`. Use
`inspect --metadata-only` for the original version-1 inventory without opening
media.
Statuses distinguish `supported`, `requires_transcode`, `missing`, `unreadable`,
`invalid_media`, and `unassessed`. Human output labels `missing` as
`PATH UNRESOLVED`: the native resolver could not select a local file, not a claim
that no copy exists on disk. For AE, after missing authored and native alias
locations, the resolver tries the exact adjacent `(Footage)` path reconstructed
from native project folders and the authored basename. It does not search other
folders or accept conflicting identities or escaped symlinks. JSON retains
`missing` for compatibility and gives the attempted-path reason; resolved files
carry `original`/`selected` paths. Found incompatible video is
`requires_transcode` with a remediation, not `missing`.
A completed inspection exits zero even when
media is blocked. Project parse/selection errors exit nonzero. Video readiness
is not full decoding, runtime decoder availability, effect support, or visual
fidelity. Audio admission is reported separately through the media records and
aggregate `media_admission`. These new checks concern audio/video only. Native
still-image descriptors and their image-specific import errors do not participate
in this admission gate; ordinary image import behavior is unchanged.

Native format libraries own path resolution and target reachability. Nested,
disabled and off-range source references are inspected before FX omissions;
unrelated panel/target assets do not block a selected target. Supported Dynamic
Links use the linked AEP's native identity and resolver. An unresolved link is
unassessed, not proof that every referenced video is ready. Existing missing-file,
still-image and effect approximation policies are not made universally fatal by
this video policy.

SWF is not one video codec. The classifier examines native tags in FWS/CWS data:
a very restricted single opaque full-stage video, placed once without transforms
or scripts and with one video frame per timeline frame, is an extraction
*candidate*. The backend must still decode and validate it. Other display-list,
vector, script, audio-stream or unassessed compression semantics require external
rendering or further assessment. FFmpeg is not a Flash renderer; ProRes output
cannot manufacture the missing rendered frames. Malformed SWFs are not candidates.

## Two backends, no implicit fallback

- `library` (default) calls **FFmpeg 7 shared libraries directly in the
  `tsrct-conv` process**, without another executable or JSON IPC. This requires
  building the CLI with the `ffmpeg-library` feature. A build without that feature
  reports an explicit error when selected. It never switches to the external
  backend implicitly.
- `external-ffmpeg-command` uses host executables. Supply `--ffmpeg-path` and
  `--ffprobe-path`, or let the operating system resolve their names on PATH.
  Commands use argv, not a shell. Arbitrary FFmpeg argument passthrough is not
  accepted.

Build the CLI with both backends:

```sh
FFMPEG_PKG_CONFIG_PATH=/path/to/ffmpeg-7/lib/pkgconfig make build-ffmpeg
make check-media-library FFMPEG_PKG_CONFIG_PATH=/path/to/ffmpeg-7/lib/pkgconfig
```

Plain `make build` also requires FFmpeg 7 development libraries: the default CLI
features enable read-only native inspection and both transcode backends.

Headers and matching shared libraries must be installed for that build and
available to the CLI's platform loader at execution. An incompatible FFmpeg ABI,
absent decoder or unavailable selected encoder is an error, not permission to
switch backends. Merely having Tesseract installed does **not** prove this CLI
can load its FFmpeg bundle. These development targets do not bundle libraries.
The [release workflow](../RELEASING.md) builds with `ffmpeg-library`, ships a
controlled relocatable FFmpeg runtime, and requires extracted-archive conversion
smokes plus macOS signing/notarization. Keep all extracted files together; do not
copy just the executable. No global loader configuration is modified.

Tesseract's existing LGPL FFmpeg bundle has platform-specific encoders (notably
no bundled H.264 encoder on Linux). The library backend uses the platform H.264
encoder; the external profile can use an explicitly installed software encoder.
FFmpeg retains its own license and build-dependent LGPL/GPL obligations; this
MIT repository does not relicense FFmpeg. Release ZIPs include the controlled
LGPL libraries, exact pinned sources, configuration/rebuild records and license
texts; Rust dependency notices are generated with cargo-about at release time (see
[RELEASING.md](../RELEASING.md)). Public-distribution review remains required. An external build
with libx264 is not represented as the existing LGPL-only bundled build.

## Preservation policy

A call works on a whole media source, not a clip trim or baked timeline. A
compatible source is copied unchanged when its container already matches, or
remuxed where supported; it is not recompressed merely to normalize GOPs.
H.264 copy/remux requires decoded progressive 8-bit 4:2:0 (`yuv420p` or
full-range `yuvj420p`). Higher chroma/depth, including 10-bit 4:2:2, uses the
existing lossy H.264 8-bit 4:2:0 encoding profile in both general and AE
preparation. Encoded output is re-probed for that pixel layout before publication;
this does not relax downstream native admission or any source safety gate.
Video outputs use `.mp4` or `.mov`; standalone audio outputs use `.wav`.
Encoding uses a fixed quality-first profile: opaque SDR H.264, or ProRes 4444
in `.mov` for verified 8-bit alpha sources. Higher or
unknown alpha precision is blocked when re-encoding would be necessary; remuxing
retains the existing coded precision. Alpha is handled independently of color
scaling to avoid FFmpeg 7's packed-RGB alpha expansion bias. These profiles are
not advertised as lossless RGB encodings. Premiere import admits ProRes 4444
`ap4h` through its existing native decoder and retains original packets, including
coded alpha precision. WebCodecs playback is unavailable and native RGB/alpha
fidelity remains unverified. QuickTime Animation (`rle ` / QTRLE) still needs
explicit whole-source preparation and a source-bound `--media-map`; no import
silently encodes media. Premiere imports can combine `--media-relink` with
`--media-map`: authenticate the original UID/authored path and relocated file
first, then apply the separately hashed prepared replacement. The map's original
is the real canonical local source, not the saved alias or a guessed basename.
Both sidecars retain project/target binding, original/prepared hashes, path
containment and pre-publication freshness checks. Prepared files are never
relocation identities. Standalone audio is prepared as PCM WAVE
without silently reducing sample rate, channel layout or sample precision.
AIFF/WAVE's implicit mono/stereo order is normalized when their demuxer omits a
layout label; unknown multichannel layouts are not inferred.

New H.264 encoding starts with an independently decodable frame and uses a
seek-friendly maximum one-second GOP with no B frames. Remuxing does not rewrite
an existing GOP. Source frame cadence/count, dimensions, time origins, color
metadata, alpha presence and audio layout/timing are checked again after output.
Unknown/unsupported preservation cases (including VFR, HDR/wide gamut, interlace,
unsupported display transforms, ICC-managed sources, non-square pixels,
nonzero starts and unsupported multi-stream layouts)
are rejected rather than silently tone-mapped, resampled, downmixed or dropped.
Explicit RGB identity-matrix signaling is not re-encoded as a mislabeled YUV
stream. Asserted matrix/range settings configure native pixel conversion as
well as output tags. Unspecified color enums retain FFmpeg's defaults; that is
not evidence of a match to an independently color-managed Adobe render.
General single-file preparation preserves unit quarter-turn display matrices,
including zero or canonical coded-bounds translation. It retains coded dimensions
and pixels without autorotation, and verifies the complete output matrix, not
just its angle. Mirrors, scale, skew, perspective and arbitrary translations are
rejected. Automatic AE destination preparation still requires identity orientation.

One recognized MOV `tmcd` timecode track is copied with its packet timing and
metadata, including when it follows other data tracks. Explicit preparation also
accepts MOV/MP4 camera data tagged `rtmd` or `mebx`. Byte-for-byte compatible copies
retain these tracks without a loss warning. Remuxing or encoding **omits these
camera tracks**: FFmpeg cannot write their valid sample entries. The result's
`warnings` (also shown in human output) name each omitted track and any timecode
label it carried. An `rtmd` label is not converted into a fabricated `tmcd` track.
Original files are unchanged. Picture/sound selection and all existing A/V
validation remain unchanged; this does not relax rotation, VFR, source-profile,
colour or timing gates. Unknown data, subtitle, attachment and extra video/audio
tracks remain rejected. Automatic AE destination preparation does not acquire
this metadata-loss policy.

Progress is emitted to stderr about every five seconds, starting before input
hashing/probing. Short operations normally finish before the first heartbeat.
Callbacks update the latest measurement without printing a line per frame;
preparation, copying, and stalls still emit elapsed-time heartbeats. By default
these are human-readable lines with media time, percentage and ETA when measured.
`--json` emits NDJSON events (`schemaVersion: 1`, `type`, `command`, `phase`,
`elapsedSeconds`, `completed`, `total`, `unit`, `ratePerSecond`, `etaSeconds`).
The original source/processed_seconds/total_seconds/speed/eta_seconds/status keys
remain on measured transcode events. Unknown quantities are null, including ETA
before measurement, after ten seconds without advancement, and during final
validation/publication. Runtime failures emit an `error` event with message and
cause chain in JSON mode (text otherwise), with nonzero exit status and never a
success line. Help and argument parsing errors remain normal CLI text. Stdout
contains only the final human result or the existing `--json` result on success. Ctrl-C kills and waits for the external
backend's owned child. The library backend cooperatively checks cancellation
between probe/decode/encode iterations; an in-flight FFmpeg call must return
before cancellation completes. Partial owned staging is cleaned up in both cases.
The input and any existing output are never deleted or overwritten. A failed
invocation cannot delete the result of another successful invocation.

## Single-file publication and separate map identity

The command validates the staged media and checks source freshness before atomic
no-clobber publication of the requested **file**. It creates no map, report
sidecar or project bundle. `--json` reports the input/output paths and hashes,
backend, operation, encoder and measured media facts on stdout.

Map assembly is a separate project-aware caller responsibility. A caller can use
the returned file hashes to create a map, then inspect/import the original project
with `--media-map`. This is not required to transcode a file.

The strict version-1 map shape is:

```json
{
  "version": 1,
  "source": {
    "format": "after-effects",
    "sha256": "<64 lowercase hex digits for the original project>",
    "target": "4"
  },
  "replacements": [{
    "original": "/resolved/absolute/source.mov",
    "original_sha256": "<original media SHA-256>",
    "replacement": "media/0000.mov",
    "replacement_sha256": "<prepared media SHA-256>"
  }]
}
```

Original identity uses canonical full paths and hashes, never basename matching.
Replacement paths must remain inside the map directory, including through
symlinks. Duplicate originals, changed projects/media/replacements, wrong targets,
path escapes and unsupported map versions fail. Import resolves the original
using its normal relocation rules **before** applying a map. The selected bytes
still undergo ordinary admission. Linked media retain their original owning-project identities in
inspection; the map is bound to the selected outer input project and whole source
files, not a render of a particular linked placement.

## Bounded local verification

`make test-media-transcode-smoke args='--converter ... --ffmpeg ... --ffprobe ... --work-dir ...'` generates four sources (at most eight
seconds each) and exercises both backends. It checks blocked inspection,
individual-file conversion, caller-assembled maps, mapped inspection, Check/Write
imports, packaged replacement hashes,
and unchanged original project/media hashes. Results remain in the requested
fresh work directory; generated media is not committed or uploaded.

The single-file CLI workflow passed all eight cases on macOS with FFmpeg 7.1.5:

| Generated source | External command | Native library |
| --- | --- | --- |
| AE QTRLE + AAC, 320×180, 24 fps, 192 frames | RGB MAE 1.46/255 | RGB MAE 1.84/255 |
| Premiere QTRLE, 1920×1080, 30 fps, 30 frames | RGB MAE 0.84/255 | RGB MAE 1.01/255 |
| AE alpha ramp, all 256 codes, 192 frames | 11,059,200 alpha bytes equal | 11,059,200 alpha bytes equal |
| AE 24-bit AIFF, 48 kHz stereo, 8 seconds | 3,072,000 decoded S32 bytes equal | 3,072,000 decoded S32 bytes equal |

The AE video case also preserves all 1,536,000 decoded embedded AAC audio bytes
on each backend. The alpha case also preserves its MOV timecode label and packet.
Each imported editable Video/Audio layer is checked against the packaged
replacement, rather than merely checking for a nonempty archive.

The RGB check is an explicit generated-source smoke bound of MAE ≤6/255, not an
Adobe reference threshold. Alpha equality is the decoded alpha-plane readback,
not proof of every player's subsequent RGBA conversion or compositing. Native
H.264 uses VideoToolbox here; Linux/Windows encoder and deployment parity remains
unverified. The compatible-copy, cancellation/cleanup, no-clobber publication, exact decoded
frame count, matrix/ICC rejection, Essential override reachability, and omitted
Premiere placement regressions have separate CPU tests.

The focused `crates/media_transcode/tests/rotation_smoke.py` accepts the same
`--converter`, `--ffmpeg`, `--ffprobe` and fresh `--work-dir` arguments. It generates
two one-second corner-pattern sources and checks copy, remux and re-encode on both
backends: exact display matrices, unchanged coded dimensions, and coded/displayed
first-frame corner orientation. Re-encoded RGB uses a bounded smoke comparison,
not lossless equality or Adobe fidelity proof.

## Reported QTRLE files: single-file CLI verification

The three local Flibbertigibbet QTRLE sources were each converted to ProRes 4444
MOV through `tsrct-conv transcode`, using both backends (six successful outputs).
Direct FFmpeg was used only for independent decode/readback measurements, not as
a substitute for the CLI conversion. The originals retained their SHA-256 hashes.

| Source | Frames at 30 fps | Canvas | Exact alpha bytes per backend | RGB PSNR, external / library |
| --- | ---: | --- | ---: | --- |
| `Flibbertigibbet_Type.mov` | 136 | 2160×2160 | 634,521,600 | 78.18 / 80.18 dB |
| `!.mov` | 238 | 1000×1000 | 238,000,000 | 74.90 / 82.69 dB |
| `?.mov` | 238 | 1000×1000 | 238,000,000 | 71.93 / 77.88 dB |

All-frame alpha hashes, decoded frame counts, dimensions, frame rates, starts,
durations, and timecode labels/packet payloads matched the inputs. RGB PSNR is a
measurement, not a claim of lossless RGB or an Adobe-render acceptance threshold.

An initial library run exposed missing `avg_frame_rate` on the copied timecode
track. The fix has a regression test. Retrying the failed invocation retained the
already verified external-backend output, rather than restarting a project batch.
The generated smoke panel also covers this timecode path.

Reinspection of composition 4 reports 26 audio/video sources and zero unassessed
image-selector errors: 22 SWFs require external rendering, three original QTRLE
sources remain transcode candidates, and one WAV is supported. The unchanged
project still references its original files. These results do **not** establish
successful whole-project import, SWF rendering, or independent Adobe fidelity.
Generated/local media and readback artifacts were not committed or uploaded.

## Evidence boundary

The map/import regressions and media-backend tests are software contract evidence.
Synthetic SWF tags and mutated `stsd` entries are explicitly not independent Adobe
renders or real codec-decoding proof. Actual generated-codec smoke runs, when
recorded, establish only their measured frame/audio/alpha checks and admission.
No new Adobe readback, independent native render comparison, Asset publication,
or general alpha/color/audio fidelity claim is made by this feature. AE/Premiere
**import** admission/preparation changes; FX-to-Adobe **export** is unchanged and
has no new proof here. See the [AE limitation ledger](after-effects-support.md)
and [Premiere format notes](formats/premiere.md).
