# Steading's workflows, for people and agents alike. CI runs these same
# recipes. `just` lists them; `just verify` runs everything CI checks.

set shell := ["bash", "-euo", "pipefail", "-c"]

root := justfile_directory()

# List available workflows.
[group('help')]
default:
    @just --list --unsorted

alias t := test
alias v := verify

# Install the wasm target, the cargo tools (needs cargo-binstall), the UI test
# dependencies and the test browser.
[group('setup')]
setup:
    rustup target add wasm32-unknown-unknown
    cargo binstall --no-confirm trunk cargo-mutants cargo-nextest release-plz
    cd ui-tests && npm ci
    cd ui-tests && npx playwright install chromium

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

# Run unit, contract and doc tests with cargo-nextest, then the acceptance and
# @e2e cucumber runners; extra arguments go to nextest (a filterset, a test
# name). Tests that skip themselves (`testkit::skip!`) are listed at the end.
# Set NEXTEST_PROFILE=ci for retries and the JUnit report (.config/nextest.toml).
[group('test')]
[positional-arguments]
test *args='':
    scripts/with-skip-log.sh '{{ just_executable() }}' _test-suites "$@"

# The suites `test` runs, under its one skip log. The cucumber runners use
# `harness = false`, which nextest can't run (its default filter in
# .config/nextest.toml leaves them out), so `cargo test` runs them; nextest
# doesn't run doc tests either.
[private]
[positional-arguments]
_test-suites *args='':
    cargo nextest run --workspace "$@"
    cargo test --doc --workspace
    cargo test --package app --test acceptance
    cargo test --test e2e

# Run the acceptance scenarios (fakes and a fake clock) only.
[group('test')]
test-acceptance *args='':
    scripts/with-skip-log.sh cargo test --package app --test acceptance -- {{ args }}

# Run the @e2e scenarios against the real binary only.
[group('test')]
test-e2e *args='':
    scripts/with-skip-log.sh cargo test --test e2e -- {{ args }}

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

# Run the @ui scenarios in Chromium against a fresh build.
[group('test')]
test-ui *args='': build
    cd ui-tests && npx cucumber-js {{ args }}

# Mutation-test domain and app code changed since `base`.
[group('test')]
mutants base='origin/main':
    #!/usr/bin/env bash
    set -euo pipefail
    diff=$(mktemp)
    trap 'rm -f "$diff"' EXIT
    git diff '{{ base }}...HEAD' > "$diff"
    cargo mutants --in-diff "$diff" --package domain --package app

# Mutation-test all of domain and app (slow; CI runs it nightly).
[group('test')]
mutants-all:
    cargo mutants --package domain --package app

# Check that the branch's first commit is a failing test: commit (§13.1).
[group('test')]
red-first base='origin/main':
    node scripts/red-first.mjs '{{ base }}'

# Run every check CI runs, except mutation testing and the red-first check.
[group('quality')]
verify: fmt-check lint architecture test test-ui

# Preview the next release: the version bump and CHANGELOG.md entry that the
# release PR would contain for the committed history (§12.1). Changes nothing.
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
