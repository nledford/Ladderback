# Ladderback's workflows, for people and agents alike. CI runs these same
# recipes. `just` lists them; `just verify` runs everything CI checks.

set shell := ["bash", "-euo", "pipefail", "-c"]

root := justfile_directory()

# The crates mutation testing gates (technical design §13.1). Add a crate here
# when it lands, so `mutants` and `mutants-all` both cover it.
mutants_packages := "--package domain --package app --package api --package l10n --package config-adapter --package web --package hooks --package interop"

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
    cargo binstall --no-confirm --version "$(just _wasm-bindgen-version)" wasm-bindgen-cli
    scripts/install-rustic.sh
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

# Lint: clippy on the host crates and on the web UI (wasm32), the journey lint,
# then the check that every path AGENTS.md and CLAUDE.md name exists.
[group('quality')]
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --package web --target wasm32-unknown-unknown -- -D warnings
    cd ui-tests && npm run --silent lint
    scripts/check-doc-paths.sh

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
[positional-arguments]
[private]
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

# A release is built with `--no-default-features`, which leaves out the
# development pseudolocales (`pseudo`, technical design §8.4). `test` builds
# with the default features, so the tests that exist only without them never
# run there: the release lane of `l10n`'s tests (`tests/lanes/release.rs`) and
# the no-pseudo test in `tests/cli_locale.rs`. This runs the tests of `l10n`
# and `ladderback` built that way, so those run, and so does any test whose
# result depends on the feature. The packages are named because only these two
# have tests that read the feature; `web`'s wasm tests run in `test-wasm`. The
# cucumber runners are left out, as in `test` (nextest's default filter). One
# test, `isolation`, builds and runs the daemon's other test binaries itself,
# with the default features. Arguments go to nextest.
[doc('Run the l10n and daemon tests as a release build is made (no default features).')]
[group('test')]
[positional-arguments]
test-release *args:
    scripts/with-skip-log.sh cargo nextest run --package l10n --package ladderback --no-default-features "$@"

# The runner is `wasm-bindgen-test-runner` from wasm-bindgen-cli, whose version
# must equal the `wasm-bindgen` in Cargo.lock; `just setup` installs it. It is
# Cargo's runner for wasm32 in this recipe only, so no other command changes.
# The packages are the ones with wasm-bindgen tests (`scripts/red-first.mjs`
# finds them by their `wasm-bindgen-test` dependency): add a package here when
# it gets its first. Selecting both unifies their features, and `--all-features`
# turns on those that only matter on wasm32 (`l10n`'s `browser-intl`). Zero
# tests is a pass.
[doc("Run the wasm-bindgen tests under Node (the browser lanes).")]
[group('test')]
[positional-arguments]
test-wasm *args:
    #!/usr/bin/env bash
    set -euo pipefail
    want=$(just _wasm-bindgen-version)
    have=$(wasm-bindgen-test-runner --version 2>/dev/null | awk '{print $2}' || true)
    if [[ $have != "$want" ]]; then
        echo "error: wasm-bindgen-test-runner is ${have:-not installed}, Cargo.lock has wasm-bindgen $want; run: cargo binstall --no-confirm --version $want wasm-bindgen-cli" >&2
        exit 1
    fi
    export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
    # Node reads this as the viewer's zone: not UTC, so a host that read no zone
    # as the viewer's would fail the tests that expect UTC.
    export TZ=America/New_York
    # With CARGO_TERM_QUIET set, cargo passes `--quiet` on to the test binary,
    # and wasm-bindgen-test-runner refuses it.
    unset CARGO_TERM_QUIET
    cargo test --package l10n --package web --lib --bins --tests --all-features --target wasm32-unknown-unknown --no-fail-fast "$@"


# The color tokens in style.css must meet WCAG 2.2 AA in every pair the UI
# uses (§4.5): 4.5:1 for text, 3:1 for UI parts like the focus ring.
[doc('Check the style.css token pairs for WCAG 2.2 AA contrast.')]
[group('test')]
test-contrast:
    node scripts/check-contrast.mjs crates/web/style.css

# The version of wasm-bindgen that Cargo.lock pins.
[private]
_wasm-bindgen-version:
    @grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p'

# Build the web UI into crates/web/dist: a release binary embeds it, a debug one reads it as it runs.
[group('build')]
web:
    cd crates/web && trunk build --release

# The UI tests drive the component gallery (`/_gallery`), which a release build
# leaves out (`just architecture` checks that), so they run against this one.
[doc('Build the web UI with the gallery into crates/web/dist, for the @ui scenarios.')]
[group('build')]
web-test:
    cd crates/web && trunk build --release --features gallery

# Build the ladderback binary with the current web UI.
[group('build')]
build: web
    cargo build

# Run the agent locally on the default address.
[group('dev')]
serve: build
    cargo run -- serve

# Calls the cucumber-js that `_ui-deps` installs by its path: `npx` (even
# `npm exec --no`) runs an unrelated registry package of that name, or a cached
# copy of it, when the local one is missing, and that package exits 0. The
# binary is built here, after `web-test`, so that it embeds the web UI with the
# gallery, not the release one `build` makes. The scenarios run the binary
# cargo just built, wherever its target directory is (`CARGO_TARGET_DIR`, as
# each worktree sets its own), unless `LADDERBACK_BIN` names another.
[doc('Run the @ui scenarios in Chromium against a fresh build with the gallery.')]
[group('test')]
[positional-arguments]
test-ui *args: web-test _ui-deps
    cargo build
    target="$(cargo metadata --format-version 1 --no-deps | node -e 'let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => process.stdout.write(JSON.parse(s).target_directory))')" && cd ui-tests && LADDERBACK_BIN="${LADDERBACK_BIN:-$target/debug/ladderback}" node_modules/.bin/cucumber-js "$@"

# Installs the UI test dependencies when they are missing or older than the
# lockfile, as in a new worktree: node_modules isn't tracked.
[private]
_ui-deps:
    cd ui-tests && if [ ! -d node_modules ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then npm ci; fi

# Further arguments go to cargo mutants, e.g. `just mutants origin/main --jobs 3`
# when several agents share the machine.
[doc("Mutation-test the gated crates' code changed since `base`.")]
[group('test')]
[positional-arguments]
mutants base='origin/main' *args:
    #!/usr/bin/env bash
    set -euo pipefail
    shift
    diff=$(mktemp)
    trap 'rm -f "$diff"' EXIT
    git diff '{{ base }}...HEAD' > "$diff"
    cargo mutants --in-diff "$diff" {{ mutants_packages }} "$@"

# Further arguments go to cargo mutants, as for `mutants`.
[doc('Mutation-test all of the gated crates (slow; CI runs it nightly).')]
[group('test')]
[positional-arguments]
mutants-all *args:
    cargo mutants {{ mutants_packages }} "$@"

# Check that the branch's first commit is a failing test: commit (§13.1).
[group('test')]
red-first base='origin/main':
    node scripts/red-first.mjs '{{ base }}'

# Run every check CI runs, except mutation testing and the red-first check.
[group('quality')]
verify: fmt-check lint architecture test test-contrast test-release test-wasm test-ui

# Shows the version bump and CHANGELOG.md entry that the release PR would
# contain for the committed history (§12.1). Changes nothing.
[doc('Preview the next release.')]
[group('release')]
release-preview:
    #!/usr/bin/env bash
    set -euo pipefail
    scratch=$(mktemp -d)
    trap 'rm -rf "$scratch"' EXIT
    git clone --quiet --no-local '{{ root }}' "$scratch/ladderback"
    cd "$scratch/ladderback"
    release-plz update
    git --no-pager diff -- Cargo.toml CHANGELOG.md
