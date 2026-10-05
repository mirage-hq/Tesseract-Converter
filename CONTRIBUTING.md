# Contributing

## Source of truth

The source of truth is [`bungeeapp/jerboa`](https://github.com/bungeeapp/jerboa),
under `opensource/conv/`. Develop converter code, documentation and tests there,
and open issues and pull requests in Jerboa. Standalone or public distribution
repositories are downstream mirrors, not upstream development sources. Do not
use a repository redirect to choose a different development repository. Sync or
publication of a downstream mirror requires a separate explicit request and review;
the publication restrictions below still apply.

Open an issue in Jerboa to discuss substantial changes before implementing them.
Submit focused pull requests against Jerboa's `main` with a description of the behavior,
compatibility impact and verification performed. Please keep the standalone
workspace buildable: do not introduce dependencies on another repository or
require native editor installations for the Rust unit tests.

## Tooling and publication boundary

The eight-package Rust workspace builds independently. Default builds, checks
and tests require FFmpeg 7 development libraries for read-only media inspection.
`make build-ffmpeg` remains a compatibility alias for the default CLI build,
which includes both explicit transcode backends. Install `ffmpeg@7` and
`pkg-config` with Homebrew, or supply `FFMPEG_PKG_CONFIG_PATH` / `FFMPEG_DIR`
for your installation. CI builds the same pinned runtime as binary releases. Optional native-editor, Asset API and FX rendering helpers require
separately authorized services or external tools; they are not prerequisites of
the Rust unit tests or offline helper tests. Install the one external Python
test dependency in a repository-local environment:

```sh
python3 -m venv .venv
. .venv/bin/activate                    # Windows: .venv\Scripts\activate
python -m pip install pytest==9.0.2
make test-offline                       # also requires Node
```

Do not use internal credentials or invoke native editors, Asset publication,
FX rendering/scoring, or network media retrieval for standalone verification. Those
maintainer workflows are opt-in, host-specific operations and require separate
approval. Mocked/offline test success is not evidence that an integration ran.

**Public distribution remains on hold for the newly moved integration tooling.**
Do not sync these helpers to a public repository or publicly release a revision
containing them until the separate publication review is complete. MIT licensing,
the export inventory and green tests do not clear that review. Checked-in
fixtures and references need no Asset API retrieval.
See the [publication notice](README.md#conversion-test-tooling).

## Build and verify

Install the Rust toolchain in `rust-toolchain.toml`, then run:

```sh
make check
make fmt
make clippy
make test
```

For CLI changes, also run `make build` and `target/debug/tsrct-conv --help`.
CI checks pull requests. Automated checks do not establish native-render fidelity.

## Conversion changes and fixtures

Add a focused test and a minimal source fixture for every conversion bug fix.
For feature work, identify the source and target formats, and verify both import
and export independently where supported. Never describe a synthetic round trip
or a parsed native file as an independent native-render comparison. The
[fixture index](tests/README.md) and [AE support ledger](docs/after-effects-support.md)
record existing proof, approximation and omission boundaries. Do not lower a
comparison threshold or replace an immutable native-render reference to make a test pass.
Only include fixtures, images or media you are authorized to distribute; do not
commit credentials, customer content or generated reference MP4s. Review new
native fixtures and provenance for embedded host paths and personal metadata,
and record source provenance and hashes. Existing native fixtures also require
this audit before the repository becomes public.

Update the [format support documentation](docs/formats/README.md) when supported
features, losses or limitations change. Keep
`scripts/conversion-export-files.json` in sync with added or removed files. See
[RELEASING.md](RELEASING.md) for automatic and manual binary releases. The
workspace packages are not published to crates.io.

## Licensing

Project code is licensed under [MIT](LICENSE). Third-party code and fixture
media retain their own rights and notices; the project license does not relicense
them. Distributed FFmpeg libraries retain their LGPL notices and matching source
and rebuild instructions. Confirm redistribution rights before adding any third-party material.

By submitting a contribution for inclusion, you agree to provide your original
contribution under the project's MIT license. Submit only work you are entitled
to license this way, including any required employer or client authorization.
Keep third-party copyright and license notices; identify adapted code, its exact
upstream revision and any local modifications in the pull request. A source URL
or a file hash alone is not redistribution permission. Do not claim authorship
of third-party work or change its license to MIT.

For each fixture family, record its author/source, creation recipe where
available, exact bytes/hash, applicable license or permission evidence, and
whether personal metadata has been reviewed. Preserve immutable native sources
and references: sanitization or replacement requires a separately identified
revision and an explicit account of which tests and native-editor evidence remain valid.
If rights or evidence are unknown, report that limitation before adding the file;
do not mark it cleared because a test passes.

## Review and community

Maintainers review changes before merging. Contributors should identify changes
to parser limits, persisted formats, dependencies, bundled materials and public
APIs, and report exactly which checks ran, failed or remain unrun. Do not silently
remove coverage or weaken a reference threshold to make a contribution pass.

Follow the [code of conduct](CODE_OF_CONDUCT.md). Report vulnerabilities through
the private process in [SECURITY.md](SECURITY.md), not a public issue or pull
request containing an exploit or sensitive project file.
