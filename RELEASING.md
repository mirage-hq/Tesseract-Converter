# Binary releases

The **Release** GitHub Actions workflow builds and tests `tsrct-conv`
for macOS Apple Silicon and Intel, Windows x86_64, and Linux x86_64. All four
release builds enable `ffmpeg-library` and bundle a pinned, controlled LGPL
FFmpeg 7.1.5 runtime. The default `library` backend does not require a system
FFmpeg installation. Keep the extracted directory intact; copying only the
executable loses its runtime dependencies. A separate
checkout-free macOS job signs both Mac binaries and their FFmpeg dylibs with
the Mirage Apple Developer certificate (hardened runtime and timestamp), and
submits each signed ZIP to Apple notarization. The workflow checks all four
final ZIPs and their `.zip.sha256` files, then creates a GitHub Release at
`v<version>` with those eight files, titled `Tesseract-Converter v<version>`.
Each version is immutable: no tag or asset is overwritten. A new release run
publishes a Release, not just workflow artifacts. Unchanged versions and completed
same-source reruns skip publication. Nothing is published to crates.io, and
repository visibility is unchanged.

This workflow runs from the **standalone repository root** at
`mirage-hq/Tesseract-Converter`, after synchronizing `opensource/conv/` from
Tesseract. It does not run from Tesseract's nested `.github/` directory. Configure
the standalone repository's `release` environment and signing secrets before
publishing.

## Publication hold for relocated tooling

The newly included reference-rendering integration helpers are
pending a separate publication review. **Do not sync them to a public repository
or publicly release a revision containing them before that review is complete.** The
MIT license and ZIP checks are not publication approval. Checked-in
fixtures and references need no Asset API retrieval, and this hold does not
introduce a separate tooling directory; see the [scope notice](README.md#conversion-test-tooling).
The procedure below is available for authorized non-public testing, or after
public-distribution clearance, not permission to bypass this hold. The release
workflow does **not** upload to the CDN. Public CDN distribution is a separate,
manual local operation after the review of third-party notices and redistribution
rights; an opaque URL is not access control.

## Run a release

1. **Automatic:** increase `[package].version` in
   `apps/tesseract-conv/Cargo.toml` and push/merge to `main`. The workflow compares
   it with the version before the entire push (not merely the previous commit).
   An unchanged or decreased version skips the release. The root `Cargo.toml`
   is a virtual workspace, not the CLI version source. Versions must be numeric
   `major.minor.patch`.

   **Manual:** choose **Run workflow** on `main`. No version or SHA input is
   needed: the workflow pins the selected revision and reads its Cargo version.
   Manual dispatch can release a version without a new bump, provided its tag
   has not already been used. Missing/non-ancestor push baselines fail closed.
   A complete same-source release is a no-op; partial or conflicting tags/releases
   fail for manual investigation. Do not delete or overwrite a published release
   to reuse its version—bump the version instead.
2. The workflow builds pinned shared FFmpeg and the library-enabled CLI on all
   four platforms, packages and extracts each ZIP, then runs default-backend
   conversion from another directory with an empty PATH and no loader overrides.
   Smoke cases exercise PCM encoding (including sample equality), compatible
   copying, ProRes alpha encoding, and platform H.264 encoding/policy. Rust tests
   remain in the separate CI workflow. Mac builds first upload **unsigned**
   artifacts; the isolated signer accepts only their matching source provenance
   and checksums, signs all allowlisted dylibs before the CLI,
   repackages it and regenerates its checksums, and requires Apple
   `notarytool` status `Accepted` before uploading the final Mac artifacts.
   The `sign-macos` job uses the standalone repository's `release` environment to access
   its six `APPLE_SIGNING_*` and `APPLE_NOTARY_*` secrets and `APPLE_TEAM_ID`,
   the Team ID every signed file must carry; environment secrets
   are not available to jobs without that environment. Missing credentials,
   a signing/Team ID mismatch, or rejected notarization blocks the Release.
   No signing key material is checked into source or downloaded from artifacts.
   The release job verifies the complete final set before creating the tag and
   Release. Build/sign/verification failures do not publish anything; a network
   failure during publication may leave a partial release requiring investigation.
   Existing tags/releases cannot be overwritten.
   Linux binaries target Ubuntu 22.04 / glibc 2.35. Apple accepts the signed
   ZIP online; ZIPs cannot be stapled, so first-launch assessment may require
   network access. Older Mac Releases predate this signer and must not be
   represented as Developer ID signed/notarized.
3. Install from the published release with
   [`sh install.sh`](README.md#macos--linux) on macOS/Linux;
   no token or GitHub CLI is required. For Windows, download the ZIP and
   `.sha256` from the [Releases page](https://github.com/mirage-hq/Tesseract-Converter/releases).
   Archives are named
   `Tesseract-Converter-<version>-darwin-arm64.zip`, `...-darwin-x86_64.zip`,
   `...-windows-x86_64.zip`, and `...-linux-x86_64.zip`. The containing directory
   uses the same name without `.zip`; the executable is still `tsrct-conv`.
   Hashes are kept in verification metadata, not filenames. Verify `.zip.sha256` with
   `shasum -a 256 -c` on macOS, `sha256sum -c` on Linux, or compare against
   `Get-FileHash -Algorithm SHA256` on Windows.

For a separately approved **manual local** CDN upload, download the exact
release assets with `gh release download <tag> --repo mirage-hq/Tesseract-Converter --dir
release-artifacts`, then from this source checkout run:

```sh
python3 scripts/release_archive.py publish-cdn --version <version> \
  --source-sha <full-40-character-sha> --output release-artifacts \
  --skill-output conv-skill/tsrct-conv
```

This requires local `gcloud` write credentials and reviewed
`THIRD_PARTY_NOTICES.md` in the matching source checkout. It verifies the eight
files before uploading them to an immutable random prefix determined by
`scripts/release_archive.py`, then writes a
**local** `conv-skill/tsrct-conv/SKILL.md` with the actual CDN URLs and ZIP
SHA-256 digests. The workflow neither uploads CDN files nor attaches this skill:
its URLs cannot be known until the later manual upload. A partial upload may
leave publicly accessible objects; investigate before retrying. No `latest`
alias is updated.

Each ZIP contains the CLI, README, build provenance, the project [MIT license](LICENSE)
and generated Rust dependency notices. It also contains FFmpeg shared libraries (`lib/`
on macOS/Linux, DLLs beside the executable on Windows), `native/manifest.json`
with file hashes, the exact FFmpeg source archive and rebuild instructions,
configuration provenance, and matching native license texts. Windows also includes
pinned zlib source/license evidence and any required Visual C++ redistributable
DLLs with their notice. Archive verification rejects missing or additional runtime
files, changed hashes, unapproved configurations and mismatched source/license
material. macOS signing regenerates both native file hashes and the ZIP checksum.

The native H.264 encoder remains VideoToolbox on macOS and Media Foundation on
Windows. VideoToolbox prefers hardware but permits Apple's software encoder when
hardware encoding is unavailable; it does not force software on hardware-capable
hosts or switch to an external FFmpeg process. Both native encoders use the numeric
H.264 High profile value, since Media Foundation does not accept the `high` alias.
**Linux has no approved bundled H.264 encoder:** compatible copy/remux,
PCM and ProRes work, but new H.264 encoding reports an explicit policy error.
An explicitly selected external FFmpeg with an appropriate encoder is separate;
the release does not silently switch backends or add GPL/nonfree codecs.

The native build/relocation helpers adapt the existing Tesseract public CLI's
controlled dependency policy without depending on another checkout. Adding
`--features ffmpeg-library` alone is not a distributable build. Before publishing,
the four-platform build, relocated smoke and macOS signature/notarization gates
must actually pass; offline tests do not prove those results.

Each newly built archive includes a release-time generated `THIRD_PARTY_NOTICES.md`.
Install `cargo-about` 0.9.2 and run `make generate-third-party-notices` from
the standalone repository root; this uses the locked CLI manifest with `ffmpeg-library` and
writes `target/THIRD_PARTY_NOTICES.md`. The release workflow runs this after
building and passes the result via `release_archive.py build --notices`.
`about.toml` chooses accepted SPDX alternatives and `about.hbs` formats the
notices. No vendored Rust license tree or checked-in generated notice is required.
Run `make test-release-archive` for offline packaging regressions. The generated
notice is not legal approval: review dependency licenses, native FFmpeg obligations,
fixture/media rights and existing Releases before public distribution. Changing
repository visibility also exposes existing assets. Public runs require the project
`LICENSE`; a successful private test does not establish public-distribution
clearance. Older archives may predate the project license and need separate review.
Native fixture permissions and the tooling publication hold remain separate release
blockers.

All eight workspace packages have `publish = false`;
the workflow has no crates.io upload step or registry token.
