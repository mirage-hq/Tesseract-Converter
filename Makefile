.DEFAULT_GOAL := help
# Native inspection uses FFmpeg 7. Explicit caller paths win over Homebrew defaults.
PKG_CONFIG_PATH := $(or $(FFMPEG_PKG_CONFIG_PATH),$(PKG_CONFIG_PATH),$(firstword $(wildcard /opt/homebrew/opt/ffmpeg@7/lib/pkgconfig /usr/local/opt/ffmpeg@7/lib/pkgconfig)))
export PKG_CONFIG_PATH
.PHONY: help build check test clippy fmt build-conversion check-conversion test-conversion \
	check-conversion-fixtures check-aep-support-ledger \
	test-tesseract-file test-fx-keyframe-bake test-aftereffects-file test-aftereffects-script-bake test-aftereffects-feature-proof aep-contract-test \
	adobe-test-cpu adobe-test-offline aep-read-empty-probe aep-write-empty-probe \
	aep-audio-fixture-build aep-audio-test-check aep-audio-test aep-test \
	aep-score-offline-test aep-score-list test-offline \
	test-converter-cli test-fx-conv test-release-archive generate-third-party-notices

help: ## Show converter-owned commands (run from this directory)
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z_-]+:.*## / {printf "  %-36s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build the standalone converter
	cargo build --locked -p tsrct-conv

.PHONY: build-release
build-release: ## Build the library-enabled release converter (requires FFmpeg 7)
	cargo build --locked --release -p tsrct-conv --features ffmpeg-library

check: ## Check the workspace (requires FFmpeg 7 development libraries)
	cargo check --locked --workspace --all-targets

test: ## Run offline Rust tests
	cargo test --locked --workspace

clippy: ## Lint the workspace (requires FFmpeg 7 development libraries)
	cargo clippy --locked --workspace --all-targets -- -D warnings

fmt: ## Check Rust formatting
	cargo fmt --all -- --check

.PHONY: fmt-write build-ffmpeg check-media-library test-media-library test-media-transcode test-media-transcode-smoke
fmt-write: ## Apply Rust formatting
	cargo fmt --all

# Compatibility aliases: native inspection is enabled in the default CLI build.
FFMPEG_PKG_CONFIG_PATH ?= $(PKG_CONFIG_PATH)
build-ffmpeg: ## Build tsrct-conv with both transcode backends (requires FFmpeg 7)
	PKG_CONFIG_PATH="$(FFMPEG_PKG_CONFIG_PATH)" cargo build --locked -p tsrct-conv --features ffmpeg-library

check-media-library: ## Check and lint tsrct-conv with in-process FFmpeg support
	PKG_CONFIG_PATH="$(FFMPEG_PKG_CONFIG_PATH)" cargo check --locked -p tsrct-conv --features ffmpeg-library --all-targets
	PKG_CONFIG_PATH="$(FFMPEG_PKG_CONFIG_PATH)" cargo clippy --locked -p tsrct-conv --features ffmpeg-library --all-targets -- -D warnings

test-media-library: ## Test both the preparation library and CLI with FFmpeg 7
	PKG_CONFIG_PATH="$(FFMPEG_PKG_CONFIG_PATH)" cargo test --locked -p media_transcode -p tsrct-conv --features ffmpeg-library $(filter)

test-media-transcode: ## Run preparation policy/process tests without FFmpeg or Adobe
	cargo test --locked -p media_transcode $(filter)

test-media-transcode-smoke: ## Explicit generated-media smoke (4 sources x 2 backends; pass tool paths and fresh work-dir via args)
	python3 scripts/test-media-transcode-smoke.py $(args)

check-conversion-fixtures: ## Validate pinned fixtures and offline staging/scoring tests
	python3 scripts/conversion-fixtures.py check
	python3 -m pytest -q scripts/test-conversion-fixtures.py scripts/test-conversion-ame.py scripts/test-prproj-scores.py

test-converter-cli: ## Run converter CLI unit and smoke tests (no Adobe)
	cargo test --locked -p tsrct-conv $(filter)

test-fx-conv: ## Run neutral conversion contract tests (optional filter=name)
	cargo test --locked -p fx_conv $(filter)

test-offline: ## Run Python/JS helper regressions without Adobe/network/GPU
	python3 -m pytest -q scripts/test*.py
	node scripts/test-aep-paint-bindings.mjs
	node scripts/test-aep-parametric-animator.mjs
	node scripts/test-aep-path-animator.mjs
	python3 scripts/conversion-ci-classify.py --self-test

.PHONY: premiere-audio-test-check
premiere-audio-test-check: ## Test Premiere audio artifact comparison (offline, no Adobe)
	python3 scripts/test_premiere_audio_test.py
	python3 scripts/test-aep-audio-test.py

aep-audio-test-check: ## Run offline audio comparison/orchestration tests
	python3 scripts/test-aep-audio-test.py
	python3 scripts/test-aep-audio-e2e.py
	python3 -m pytest -q scripts/test-aep-audio-adobe.py
	python3 scripts/aep-test.py check

build-conversion: build
check-conversion: check clippy
test-conversion: check-conversion-fixtures check-aep-support-ledger test

test-release-archive: ## Test bundles, release routing and installation offline
	python3 scripts/test_native_bundle.py
	python3 scripts/test_release_archive.py
	python3 scripts/test_release_workflow.py
	python3 scripts/test_release_plan.py
	python3 scripts/test_install.py
	sh -n install.sh

.PHONY: build-release-ffmpeg test-release-runtime
build-release-ffmpeg: ## Build the pinned LGPL FFmpeg runtime for a release (prefix required)
	python3 scripts/ffmpeg.py "$(prefix)"

test-release-runtime: ## Exercise a relocated bundle's default library backend (bundle/platform/ffmpeg required)
	python3 scripts/test_release_runtime.py --bundle "$(bundle)" --platform "$(platform)" --ffmpeg "$(ffmpeg)"

generate-third-party-notices: ## Generate locked release dependency notices with cargo-about 0.9.2
	cargo about generate --locked --fail -m apps/tesseract-conv/Cargo.toml --features ffmpeg-library -c about.toml -o target/THIRD_PARTY_NOTICES.md about.hbs

.PHONY: test-premiere-file clippy-premiere-file
test-premiere-file: ## Run Premiere CPU tests (optional filter=name, cargo_args=--no-default-features, test_args=--ignored)
	cargo test --locked -p premiere_file $(cargo_args) $(filter) -- $(test_args)

clippy-premiere-file: ## Lint Premiere library and CPU tests
	cargo clippy --locked -p premiere_file $(cargo_args) --all-targets -- -D warnings

test-tesseract-file: ## Run document roundtrip and archive validation tests (optional filter=name)
	cargo test --locked -p tesseract_file $(filter)

test-fx-keyframe-bake: ## Run shared FX keyframe baking tests
	cargo test --locked -p fx_keyframe_bake

check-aep-support-ledger: ## Require a support-ledger entry for every AE diagnostic
	python3 scripts/test-aep-support-ledger.py

test-aftereffects-file: check-aep-support-ledger ## Run native After Effects CPU tests (optional filter=name)
	cargo test --locked -p aftereffects_file $(filter)

test-aftereffects-script-bake: ## Run After Effects script-baking tests
	cargo test --locked -p aftereffects_file --lib script_bake

test-aftereffects-feature-proof: ## Run native assertions including the explicitly ignored proof backlog
	cargo test --locked -p aftereffects_file $(filter) -- --include-ignored

aep-contract-test: ## Run positive conversion gap contracts (CPU-only; known failures remain failures)
	cargo test --locked -p aftereffects_file --lib coverage_contract_ -- --include-ignored

adobe-test-cpu: ## CPU assertion stage; optional filter=exact_symbol; includes ignored proof cases
	cargo test --locked -p aftereffects_file --lib $(filter) -- $(if $(filter),--exact,) --test-threads=1 --include-ignored

adobe-test-offline: ## Test runner isolation without Adobe/network/GPU
	python3 -m unittest discover -s scripts -p 'test_adobe_*.py'

aep-read-empty-probe: ## Development-only empty AEP metadata probe (input=...)
	cargo run --locked -p aftereffects_file --example read_empty -- "$(input)"

aep-write-empty-probe: ## Development-only empty AEP writer probe (out=... width=... height=... frames=... name=...)
	cargo run --locked -p aftereffects_file --example write_empty -- "$(out)" "$(width)" "$(height)" "$(frames)" "$(name)"

aep-audio-fixture-build: ## Build explicit CPU-only audio fixture preparer
	cargo build --locked -p aftereffects_file --example audio_e2e

aep-audio-test: ## Compare supplied audio artifacts (args='...')
	python3 scripts/aep-test.py compare $(args)

aep-test: ## Adobe-free audio conversion/reference comparison (args='...')
	python3 scripts/aep-test.py $(args)

aep-score-offline-test: ## Test scoring/readback without Adobe/network/GPU
	python3 scripts/test_aep_test.py
	python3 -m pytest -q scripts/test_aep_effects_readback.py

aep-score-list: ## List registered targets; registration is not a passing result
	python3 scripts/aep_test.py list
