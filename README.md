# Tesseract Converter

Tesseract-Converter is a Rust CLI for converting editable project files.
It is designed to work alongside [Tesseract](https://github.com/mirage-hq/Tesseract),
which provides ChatGPT/Codex plugins and agent skills for creating and editing
`.tsrct` projects. The converter imports Adobe projects into editable `.tsrct`
files and exports supported edits back to Adobe formats.

The executable is named `tsrct-conv`. It uses editable FX documents packaged
as `.tsrct` files for interchange. Here, **FX** names the editable format and
its rendered output; the existing CLI option `--to tesseract` selects FX output.

## Supported formats

Choose the conversion direction by the file you have and the file you need.

| Adobe format | Adobe → TSRCT | TSRCT → Adobe |
| --- | --- | --- |
| [Premiere Pro (`.prproj`)](docs/formats/premiere.md) | Partial | Partial |
| [After Effects (`.aep`)](docs/formats/after-effects.md) | Partial | Experimental |

**Some features may not convert exactly.** Unsupported features may be
approximated or left out. Keep your original projects and review the converted
result and any warnings. See [supported features and limitations](docs/formats/README.md).

## Version and FX schema

| tsrct-conv version | fxSchemaVersion in this source revision |
| --- | --- |
| 0.1.0 | 67 |

`fxSchemaVersion` is the FX schema inventory revision, not the `.tsrct` container
format version or a compatibility gate. New `.tsrct` files record it in
`metadata.json`; older files may omit it. The CLI `--help` and each
Release description report the revision built from their exact source commit.
New releases use immutable `v<version>` tags; `build.json` inside each ZIP records
the exact source commit. Older test releases could reuse a Cargo version, so
check their recorded schema revision rather than assuming identical builds.
Update this table when changing the converter version or FX schema revision.

## Install

### macOS / Linux

```sh
curl -fsSLo install.sh https://raw.githubusercontent.com/mirage-hq/Tesseract-Converter/main/install.sh && sh install.sh
```

Windows: download the ZIP from [Releases](https://github.com/mirage-hq/Tesseract-Converter/releases).

### Build from source (developers)

With [Rust](https://www.rust-lang.org/tools/install) 1.92.0 or newer, from this
repository's root:

```sh
cargo install --locked --path apps/tesseract-conv
```

The default **source build** requires FFmpeg 7 headers/shared libraries for
read-only media inspection and both explicit transcode backends. Install them
first (for example, `brew install ffmpeg@7 pkg-config` on macOS), and configure
`PKG_CONFIG_PATH` (or `FFMPEG_DIR` on Windows). `make` detects the standard
Homebrew FFmpeg 7 paths. Local linkage is not a substitute for the relocatable
release bundle; binary installation does not require system FFmpeg.

## Usage

Use the converter alongside the [Tesseract skill](https://github.com/mirage-hq/Tesseract)
when moving motion-design projects between Tesseract and Adobe. The converter
creates project files; playback, editing and video export happen in Tesseract or
Adobe, not in `tsrct-conv`.

| Direction | Agent workflow |
| --- | --- |
| AEP / PRPROJ → TSRCT | `inspect` → prepare incompatible media with `transcode` if needed → `convert` → verify in Tesseract |
| TSRCT → AEP | `convert` → open the generated AEP in After Effects |
| TSRCT → PRPROJ | `convert` → open the generated Premiere project, with linked AEPs when needed |

Choose **one** direction. Examples below are alternatives, not a script to run
in full. Replace paths and `<…>` placeholders with actual values. Preserve the
source project and media. A `convert --output` destination is a **new directory**,
not a filename; its parent must already exist. Do not overwrite an earlier result.

### AEP / PRPROJ → TSRCT

#### 1. Inspect and select the target

First list targets without probing every target's media:

```sh
tsrct-conv inspect input.aep --metadata-only --json
# Or, for Premiere:
tsrct-conv inspect input.prproj --metadata-only --json
```

This metadata-only inventory does **not** assess media readiness. Select the
intended **composition ID** for AEP or **sequence GUID** for PRPROJ; names are not
selectors. If the intended target is ambiguous, ask the user rather than choosing
the first one. Then inspect that target's media without `--metadata-only`:

```sh
tsrct-conv inspect input.aep --composition '<composition-id>' --json
# Or:
tsrct-conv inspect input.prproj --sequence '<sequence-guid>' --json
```

**An inspection can exit successfully while reporting blocked media.** Check
`media_admission` (`ready` or `blocked`), `media_preflight[].media` and
`media_preflight[].unassessed`. Do not use `video_admission` alone: audio can still
be blocked. This assessment covers reached audio/video, not still images, effects
or render fidelity. Read each media status and remediation, not just the exit code:

- `supported`: no preparation is needed for that media file.
- `requires_transcode`: `remediation: transcode_candidate` permits an explicit
  preparation attempt, not a guarantee. `external_render_required` (for example,
  unsupported Flash content) needs separate rendering; FFmpeg is not a Flash
  renderer. `unknown` requires investigation rather than blind transcoding.
- `missing` (`PATH UNRESOLVED` in human output), `unreadable`, `invalid_media` or
  `unassessed`: resolve the path/problem or report the blocker. Transcoding cannot
  recover a missing file or guarantee support for an unassessed source.

#### 2. Prepare incompatible media only when needed

Skip this step when the selected target's media is already compatible.
`transcode` processes **one media file per invocation**, not an Adobe project:

```sh
mkdir -p prepared/media
# Example video replacement; choose the output type from the remediation:
tsrct-conv transcode /absolute/path/to/source.mov --output prepared/media/source.mov --json
# Example standalone audio replacement:
tsrct-conv transcode /absolute/path/to/source.aiff --output prepared/media/source.wav --json
```

The default backend is `library` (FFmpeg 7 shared libraries). To explicitly use
host executables, add `--backend external-ffmpeg-command`, optionally with
`--ffmpeg-path` and `--ffprobe-path`. There is no automatic backend fallback.

Video output must be `.mp4` or `.mov`; standalone audio must be `.wav`. Alpha
video requires `.mov`, with bounded ProRes 4444 encoding support. A successful
transcode is **not** a target-compatibility guarantee: notably, PRPROJ → TSRCT
does not admit ProRes. Reinspect the prepared result through the media map.
The command may copy or remux already-compatible media rather than re-encode it.

Progress, percentage and ETA are reported when measurable. A failed transcode
must be resolved or reported; do not silently drop the asset, change its intended
semantics, or pretend that every unsupported source can be transcoded.

**Creating replacement files alone does not connect them to the project.** The
agent must separately create `prepared/media-map.json` using the exact
[media-map schema](docs/media-preparation.md#single-file-publication-and-separate-map-identity).
Bind it to the original project's SHA-256, format (`after-effects` or `premiere`)
and selected target ID. Each entry records the canonical original media path,
original hash, replacement path and replacement hash. Replacement paths are
relative to the map's directory and must stay inside it; for the video example,
the replacement path is `media/source.mov`. Use actual lowercase SHA-256 hashes,
never placeholders. In the transcode JSON result, `input` is the canonical
original path, `input_sha256` supplies `original_sha256`, and `output_sha256`
supplies `replacement_sha256`. Compute the original Adobe project's hash
separately for `source.sha256`; `source.target` is a string. `transcode` does
**not** create this map.

Keep the original media available: native path resolution and original-file hash
verification still happen before a replacement is accepted. A map cannot repair
an unresolved original by pointing only at its replacement.

Reinspect the same target with the map before converting:

```sh
tsrct-conv inspect input.aep --composition '<composition-id>' --media-map prepared/media-map.json --json
# Or:
tsrct-conv inspect input.prproj --sequence '<sequence-guid>' --media-map prepared/media-map.json --json
```

Require `media_admission: ready` for a media-ready handoff. If it remains blocked,
report the remaining issues; do not assume a successful transcode resolved them.
See [media preparation](docs/media-preparation.md) for preservation restrictions,
backend requirements and cases that require separate assessment or external rendering.

#### 3. Convert the selected target

When no media replacements are needed:

```sh
tsrct-conv convert input.aep --to tesseract --composition '<composition-id>' --output converted-ae
# Or:
tsrct-conv convert input.prproj --to tesseract --sequence '<sequence-guid>' --output converted-premiere
```

When replacements were prepared, pass the same map used for reinspection:

```sh
tsrct-conv convert input.aep --to tesseract --composition '<composition-id>' --media-map prepared/media-map.json --output converted-ae
# Or:
tsrct-conv convert input.prproj --to tesseract --sequence '<sequence-guid>' --media-map prepared/media-map.json --output converted-premiere
```

`convert` does not transcode automatically. The result is `project.tsrct` inside
the chosen output directory, with its required media packaged in the archive.

#### 4. Open and verify in Tesseract

Use the Tesseract skill to open the resulting `project.tsrct`, check playback,
and verify the intended video export. Review conversion warnings for omitted or
approximated features. Successful conversion and compatible media do **not** prove
successful playback/export or visual equality with Adobe. If Tesseract verification
was not performed, report it as unverified rather than claiming the project works.

### TSRCT → AEP

Run `convert` directly. There is no separate `inspect` or `transcode` preparation
step for this direction:

```sh
tsrct-conv convert edited.tsrct --to after-effects --output exported-ae
```

The default output rate is **24fps**. Use `--fps 30`, for example, to request
30fps explicitly; the rate is not inferred from the `.tsrct` input.

The package contains **one `project.aep`**, plus required external assets under
`media/` when needed. Open `exported-ae/project.aep` in After Effects and keep the
whole output directory together so its media references remain valid. Review
warnings and check the result in Adobe; native AEP generation is experimental
and does not guarantee full fidelity.

### TSRCT → PRPROJ

Run `convert` directly; no separate `inspect` or `transcode` step is required:

```sh
tsrct-conv convert edited.tsrct --to premiere --output exported-premiere
```

The default output rate is **30fps**. `--fps` accepts `23.976`, `24`, `25`, `29.97`,
`30` or `59.94` for native Premiere output. If linked AEPs are needed, only **24,
25 or 30fps** are accepted; fractional-rate requests fail rather than silently
changing clocks. `--fps` is not accepted for AEP/PRPROJ → TSRCT.

The package contains **one `project.prproj`** and its required media. When the
native Premiere route cannot adequately represent picture content, it may generate
**one or more linked AEP files** automatically—no additional flag is needed.
Each AEP covers a dependency-connected picture scope, not necessarily one layer;
supported picture dependencies may move with it. Unreplaced picture and native
audio remain in Premiere:

```text
exported-premiere/
  project.prproj
  media/
    ...native media...
    ae-0001/
      compositions.aep
      ...AEP-local assets...
    ae-0002/
      compositions.aep
      ...AEP-local assets...
```

Native-only projects do not need these AEP folders. Not every unsupported feature
can be represented by the AEP fallback; omissions and approximations remain in
the diagnostics. Open `project.prproj` with both Premiere Pro and After Effects
installed when linked AEPs are present. Keep the whole package together, including
each AEP's local assets. Verify the picture, sound and intended edits in Adobe.
The converter marks these packages `HYBRID-EXPERIMENTAL`: Adobe acceptance,
render/alpha/audio fidelity, edit propagation and relocation remain unmeasured.

### Agent automation and progress

- `inspect --json` returns a machine-readable inventory. `convert --json` and
  `transcode --json` return one final result on stdout, with NDJSON progress,
  diagnostics and runtime errors on stderr. Preserve both streams; do not treat a
  progress event as a successful result. Help/argument errors and `inspect` errors
  remain text, even with `--json`.
- Human-readable progress is enabled by default, about every five seconds. Short
  commands may finish before a heartbeat. Conversion percentage and ETA describe
  the **current measured phase**, not the entire command. Reading, finalization
  or stalled measurements may have no ETA; that alone is not a failure. JSON
  conversion progress includes `percentage` and `etaScope: "phase"`; transcode
  progress uses `completed`/`total` in seconds, with nullable `etaSeconds`.
  Do not assume transcode events contain a `percentage` field.
- Add `--check` to `convert` only when you want validation without publishing the
  output. It performs the conversion work, not just a quick header check, and does
  not create a usable final project. It is optional, not a required extra step.
- On a nonzero exit, report the error and do not claim a completed conversion.
  On success, report the project path, warnings and any playback/Adobe checks that
  remain unverified. Inspect the returned artifact list when using `--json`.

Run `tsrct-conv --help` or `tsrct-conv <command> --help` for options.
See the [CLI reference](apps/tesseract-conv/README.md) for frame-rate options,
linked-project constraints and the complete conversion contract.

## Workspace packages

The `media_transcode` crate provides read-only libavformat container inspection
and explicit media preparation with external commands or in-process FFmpeg.
Inspection does not decode/re-encode media or launch a subprocess. The default
CLI enables native FFmpeg; `make build-ffmpeg` remains a compatibility alias.
Library consumers may disable default features to build without native linkage,
but media inspection then returns an explicit unavailable-backend error. It also includes `fx_keyframe_bake`, which provides the
shared existing scalar fitter, the Boa execution core, and deterministic
conversion identities used when evaluated FX animation is baked into editable
keyframes. Host adapters remain responsible for document graphs, property types,
and owner-local clocks.

Boa's VM loop, recursion, and stack limits are defensive execution bounds, not
a wall-clock or heap sandbox. No format adapter claims fidelity without
independent validation; each adapter must document its own conversion behavior.

## Conversion test tooling

For After Effects, start with the [feature × direction ledger](docs/after-effects-support.md).
The Rust workspace and offline helper tests are the standalone development surface.
The 38 strict Premiere cases and AE Adobe reference media are checked into
`tests/references/`; the former 85-case `structural_only` Premiere diagnostic
corpus is no longer registered or distributed as test coverage. Create an
isolated Python environment for the pytest-dependent tests; do not install
these test dependencies into the system Python:

```sh
python3 -m venv .venv
. .venv/bin/activate                    # Windows: .venv\Scripts\activate
python -m pip install pytest==9.0.2
make test-offline                       # mocked Python/JS tests; also requires Node
make check-conversion-fixtures          # fixture metadata and mocked orchestration
```

Pytest is the only external Python dependency used by the offline tests; their
other imports are from the standard library or this repository. These commands do
not invoke native editors, build an FX renderer, render media, or establish
conversion fidelity.
Checked-in fixtures and references need no Asset API; no active test or local
reference authoring command downloads from or uploads to the Asset service.

Internal native-editor workflow guides live in the enclosing repository. FX
rendering/scoring and native-editor execution remain separate maintainer workflows
requiring external tools, suitable hosts, and explicit authorization. For
example, `make aep-score-list` only lists registrations, not passing results.
The [support ledger](docs/after-effects-support.md) distinguishes implementation
from proof; removed journals and duplicated historical reports are not current
guidance. No visual gate is claimed to have run for this checkout.

From the enclosing repository's root, standalone commands may be prefixed with
`make -C opensource/conv`. Relative arguments are resolved from this directory;
use absolute paths when in doubt. The core `check`, `test`, `clippy`, and `fmt`
targets do not depend on the enclosing checkout. Optional rendering and scoring
targets live only in the enclosing repository's root `Makefile`; they are not
part of standalone builds or offline tests.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development and testing.

## License

[MIT](LICENSE). Third-party code and media retain their own licenses and rights. Release ZIPs contain Rust dependency notices generated at build time with cargo-about (see [RELEASING.md](RELEASING.md)); native FFmpeg notices and sources are packaged separately. Generated notices are not legal approval.
