# Steading's workflows, for people and agents alike. CI runs these same
# recipes. `just` lists them; `just verify` runs everything CI checks.

set shell := ["bash", "-euo", "pipefail", "-c"]

root := justfile_directory()

# The crates mutation testing gates (technical design §13.1). Add a crate here
# when it lands, so `mutants` and `mutants-all` both cover it.
mutants_packages := "--package domain --package app"

# List available workflows.
[group('help')]
default:
    @just --list --unsorted

alias t := test
alias v := verify

# Installs the wasm target, the cargo tools (needs cargo-binstall), the UI test
# dependencies and the test browser.
[doc('Install the build, test and release tools.')]
[group('setup')]
setup:
    rustup target add wasm32-unknown-unknown
    cargo binstall --no-confirm trunk cargo-mutants cargo-nextest release-plz
    cd ui-tests && npm ci
    cd ui-tests && node_modules/.bin/playwright install chromium

# Format Rust code.
[group('quality')]
fmt:
    cargo fmt --all

# Check formatting without changing files.
[group('quality')]
fmt-check:
    cargo fmt --all --check

# Lint: clippy on the host crates and on the web UI (wasm32), then the journey lint.
[group('quality')]
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --package web --target wasm32-unknown-unknown -- -D warnings
    cd ui-tests && npm run --silent lint

# Check the dependency rule: banned crates in domain and app, domain on wasm32.
[group('quality')]
architecture:
    scripts/check-architecture.sh

# Runs unit, contract and doc tests with cargo-nextest, then the acceptance and
# @e2e cucumber runners; extra arguments go to nextest (a filterset, a test
# name). Tests that skip themselves (`testkit::skip!`) are listed at the end.
# Set NEXTEST_PROFILE=ci for retries and the JUnit report (.config/nextest.toml).
[doc('Run every Rust test suite, then list skipped tests.')]
[group('test')]
[positional-arguments]
test *args:
    scripts/with-skip-log.sh '{{ just_executable() }}' _test-suites "$@"

# The suites `test` runs, under its one skip log. The cucumber runners use
# `harness = false`, which nextest can't run (its default filter in
# .config/nextest.toml leaves them out), so `cargo test` runs them; nextest
# doesn't run doc tests either. Every cargo command says --workspace, even
# for one test target: Cargo unifies features over the packages a command
# selects, so `--package app` or the root package alone builds dependencies
# with other features, and each of those commands rebuilt nearly all of them.
[private]
[positional-arguments]
_test-suites *args:
    cargo nextest run --workspace "$@"
    cargo test --doc --workspace
    cargo test --workspace --test acceptance
    cargo test --workspace --test e2e

# Arguments go to cucumber-rs as given, so `--name 'a job that runs'` stays
# one pattern, and narrow this runner's scenarios (as for `test-e2e`).
[doc('Run the acceptance scenarios (fakes and a fake clock) only.')]
[group('test')]
[positional-arguments]
test-acceptance *args:
    scripts/with-skip-log.sh cargo test --workspace --test acceptance -- "$@"

# Run the @e2e scenarios against the real binary only.
[group('test')]
[positional-arguments]
test-e2e *args:
    scripts/with-skip-log.sh cargo test --workspace --test e2e -- "$@"

# Build the web UI into crates/web/dist, where the binary embeds it from.
[group('build')]
web:
    cd crates/web && trunk build --release

# Build the steading binary with the current web UI.
[group('build')]
build: web
    cargo build

# Run the agent locally on the default address.
[group('dev')]
serve: build
    cargo run -- serve

# Calls the cucumber-js that `just setup` installs by its path: `npx` (even
# `npm exec --no`) runs an unrelated registry package of that name, or a cached
# copy of it, when the local one is missing, and that package exits 0.
[doc('Run the @ui scenarios in Chromium against a fresh build.')]
[group('test')]
[positional-arguments]
test-ui *args: build
    cd ui-tests && node_modules/.bin/cucumber-js "$@"

# Mutation-test the gated crates' code changed since `base`.
[group('test')]
mutants base='origin/main':
    #!/usr/bin/env bash
    set -euo pipefail
    diff=$(mktemp)
    trap 'rm -f "$diff"' EXIT
    git diff '{{ base }}...HEAD' > "$diff"
    cargo mutants --in-diff "$diff" {{ mutants_packages }}

# Mutation-test all of the gated crates (slow; CI runs it nightly).
[group('test')]
mutants-all:
    cargo mutants {{ mutants_packages }}

# Check that the branch's first commit is a failing test: commit (§13.1).
[group('test')]
red-first base='origin/main':
    node scripts/red-first.mjs '{{ base }}'

# Run every check CI runs, except mutation testing and the red-first check.
[group('quality')]
verify: fmt-check lint architecture test test-ui

# Shows the version bump and CHANGELOG.md entry that the release PR would
# contain for the committed history (§12.1). Changes nothing.
[doc('Preview the next release.')]
[group('release')]
release-preview:
    #!/usr/bin/env bash
    set -euo pipefail
    scratch=$(mktemp -d)
    trap 'rm -rf "$scratch"' EXIT
    git clone --quiet --no-local '{{ root }}' "$scratch/steading"
    cd "$scratch/steading"
    release-plz update
    git --no-pager diff -- Cargo.toml CHANGELOG.md
