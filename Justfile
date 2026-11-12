# Ladderback's workflows, for people and agents alike. CI runs these same
# recipes. `just` lists them; `just verify` runs everything CI checks.

set shell := ["bash", "-euo", "pipefail", "-c"]

root := justfile_directory()

# The crates mutation testing gates (technical design §13.1). Add a crate here
# when it lands, so `mutants` and `mutants-all` both cover it; one added later
# is mutated whole by the `mutants` run of the pull request that adds it.
mutants_packages := "--package domain --package app --package api --package l10n --package config-adapter --package web --package hooks --package interop --package secrets --package spawn-gate"

# The cargo flags a release build is made with (technical design §12.1): the
# development pseudolocales and the component gallery stay out. `architecture`
# checks a build with them, and release packaging must pass them too: `trunk
# build --release` for the web UI (`web-release`), cargo-dist, which can't read
# this file, so `dist-workspace.toml` says the same and `architecture` checks
# that the two agree, and the image. `just --evaluate release_flags` prints
# them. A feature a release needs joins this string, so the checks and the
# artifacts change together.
release_flags := "--no-default-features"

# The container engine `msrv` runs: $DOCKER when set, else `docker` when it is
# installed, else `podman`.
container := env("DOCKER", `command -v docker >/dev/null && echo docker || echo podman`)

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
    cargo binstall --no-confirm trunk cargo-deny cargo-mutants cargo-nextest release-plz zizmor
    cargo binstall --no-confirm --version "$(just _wasm-bindgen-version)" wasm-bindgen-cli
    scripts/install-rustic.sh
    scripts/install-actionlint.sh
    scripts/install-dist.sh
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

# Clippy on the host crates: the part of `lint` that can differ by OS
# (`cfg(target_os)` code), and so the only part CI runs on macOS too.
[doc('Clippy on the host crates only, the part of `lint` that can differ by OS.')]
[group('quality')]
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Lint: `clippy`, clippy on the web UI (wasm32), the type check of the
# TypeScript tooling (tsconfig.json; its compiler is a ui-tests
# dev-dependency), the journey lint, then the check that every path AGENTS.md
# and CLAUDE.md name exists.
[group('quality')]
lint: _ui-deps clippy
    cargo clippy --package web --target wasm32-unknown-unknown -- -D warnings
    ui-tests/node_modules/.bin/tsc --project tsconfig.json
    cd ui-tests && npm run --silent lint
    scripts/check-doc-paths.sh

# Lint the GitHub Actions workflows and the repository's own actions:
# actionlint for the workflows' syntax, expressions and shell steps, zizmor for
# security (template injection, permissions, unpinned actions) in both.
# actionlint reads workflows only. zizmor runs its online audits
# (known-vulnerable actions, impostor commits) only when GH_TOKEN is set, as CI
# sets it; without it, offline.
[doc('Lint the GitHub Actions workflows and actions with actionlint and zizmor.')]
[group('quality')]
lint-workflows:
    actionlint
    zizmor --no-progress .github/workflows .github/actions

# Check the dependency rule: banned crates in domain and app, domain on wasm32.
[group('quality')]
architecture:
    RELEASE_FLAGS='{{ release_flags }}' scripts/check-architecture.sh

# Checks the dependency tree against deny.toml: RustSec advisories, licences,
# banned crates and sources (technical design §13.6), for every workspace
# package: `web` is not a dependency of the root package, so without
# `--workspace` its tree would go unchecked. It fetches the advisory database,
# so it needs the network, and a new advisory can fail it without any change
# here: `verify` leaves it out, and CI runs it in its own job, nightly too.
[doc('Check dependencies for advisories, licences, bans and sources (cargo-deny; needs the network).')]
[group('quality')]
deny:
    cargo deny --locked --all-features --workspace check

# Builds the workspace with the oldest compiler it claims to support (technical
# design §8.2, decision 47), which CI doesn't: CI builds with stable. It runs in
# Docker, in the official image of the `rust-version` Cargo.toml declares, on
# Linux because macOS 27 can't load the proc-macro libraries 1.94 builds.
# RUSTUP_TOOLCHAIN names the image's own toolchain: rust-toolchain.toml would
# otherwise swap in stable, and the check would prove nothing. The target folder
# and the Cargo home are named volumes, so a second run is quick; `docker volume
# rm ladderback-msrv-target ladderback-msrv-cargo` frees them. It uses `docker`
# when present and `podman` otherwise (Fedora); `DOCKER=podman just msrv` picks
# one. The image is named in full because podman won't guess a registry.
[doc('Build the workspace, wasm32 included, with the MSRV in a container (decision 47).')]
[group('quality')]
msrv:
    #!/usr/bin/env bash
    set -euo pipefail
    version=$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml)
    if [ -z "$version" ]; then echo 'msrv: Cargo.toml declares no rust-version' >&2; exit 1; fi
    {{ container }} run --rm \
        -v '{{ root }}':/src:ro -v ladderback-msrv-target:/target -v ladderback-msrv-cargo:/cargo \
        -e CARGO_TARGET_DIR=/target -e CARGO_HOME=/cargo -e CARGO_PROFILE_DEV_DEBUG=0 -w /src \
        "docker.io/library/rust:$version" bash -euo pipefail -c '
            export RUSTUP_TOOLCHAIN="$(cd / && rustup default | cut -d " " -f 1)"
            rustup target add wasm32-unknown-unknown
            rustc --version
            cargo check --locked --workspace --all-targets
            cargo check --locked --package web --target wasm32-unknown-unknown'

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

# A release is built with `release_flags` (`--no-default-features`), which
# leaves out the development pseudolocales (`pseudo`, technical design §8.4).
# `test` builds with the default features, so the tests that exist only
# without them never run there: the release lane of `l10n`'s tests
# (`tests/lanes/release.rs`) and the no-pseudo test in `tests/cli_locale.rs`.
# This runs the tests of `l10n`
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
    scripts/with-skip-log.sh cargo nextest run --package l10n --package ladderback {{ release_flags }} "$@"

# The runner is `wasm-bindgen-test-runner` from wasm-bindgen-cli, whose version
# must equal the `wasm-bindgen` in Cargo.lock; `just setup` installs it. It is
# Cargo's runner for wasm32 in this recipe only, so no other command changes.
# The packages are the ones with wasm-bindgen tests: those with a
# `wasm-bindgen-test` dependency, which `scripts/wasm-test-packages.ts` finds
# from `cargo metadata`, the same way `scripts/red-first.ts` does, so a
# package's first wasm test is picked up by both with no edit here. Selecting
# several unifies their features, and `--all-features` turns on those that only
# matter on wasm32 (`l10n`'s `browser-intl`). Zero tests is a pass.
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
    names=$(cargo metadata --no-deps --format-version 1 | node scripts/wasm-test-packages.ts)
    if [[ -z $names ]]; then
        echo "no package depends on wasm-bindgen-test: no wasm tests to run"
        exit 0
    fi
    packages=()
    for name in $names; do
        packages+=(--package "$name")
    done
    export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
    # Node reads this as the viewer's zone: not UTC, so a host that read no zone
    # as the viewer's would fail the tests that expect UTC.
    export TZ=America/New_York
    # With CARGO_TERM_QUIET set, cargo passes `--quiet` on to the test binary,
    # and wasm-bindgen-test-runner refuses it.
    unset CARGO_TERM_QUIET
    cargo test "${packages[@]}" --lib --bins --tests --all-features --target wasm32-unknown-unknown --no-fail-fast "$@"


# The color tokens in style.css must meet WCAG 2.2 AA in every pair the UI
# uses (§4.5): 4.5:1 for text, 3:1 for UI parts like the focus ring.
[doc('Check the style.css token pairs for WCAG 2.2 AA contrast.')]
[group('test')]
test-contrast:
    node scripts/check-contrast.ts crates/web/style.css

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

# Runs the daemon locally on the default address. The package is named because
# the default members hold a second binary (the engine's `engine-worker`), and
# a bare `cargo run` refuses to choose; the daemon runs its own workers.
[doc('Run the daemon locally on the default address.')]
[group('dev')]
serve: build
    cargo run --package ladderback -- serve

# Runs the ladderback binary on folders of its own under the target directory:
# the configuration, the data and HOME (where rustic keeps its profiles and
# cache) are all scratch, so nothing reaches your real Ladderback or rustic
# setup, and the daemon listens on a free port, never `serve`'s 9898. Arguments
# replace `serve`, and paths are relative to where you run it: `just try
# import-profile demo.toml` imports into the same scratch configuration. A
# keyring password source still uses your real OS keyring (the Keychain on
# macOS, the Secret Service on Linux). Delete the folder to start over.
[doc('Run ladderback on scratch folders and a free port (default: serve).')]
[group('dev')]
[no-cd]
[positional-arguments]
try *args: build
    #!/usr/bin/env bash
    set -euo pipefail
    target="${CARGO_TARGET_DIR:-{{ root }}/target}"
    scratch="$target/try"
    mkdir -p "$scratch/config" "$scratch/data" "$scratch/home"
    if [ $# -eq 0 ]; then set -- serve --listen 127.0.0.1:0; fi
    echo "try: scratch folders in $scratch" >&2
    HOME="$scratch/home" LADDERBACK_CONFIG_DIR="$scratch/config" LADDERBACK_DATA_DIR="$scratch/data" LADDERBACK_CACHE_DIR="$scratch/cache" \
        exec "$target/debug/ladderback" "$@"

# Calls the cucumber-js that `_ui-deps` installs by its path: `npx` (even
# `npm exec --no`) runs an unrelated registry package of that name, or a cached
# copy of it, when the local one is missing, and that package exits 0. Its
# config is TypeScript, which it doesn't look for by itself, so it is named. The
# binary is built here, after `web-test`, so that it embeds the web UI with the
# gallery, not the release one `build` makes. The scenarios run the binary
# cargo just built, wherever its target directory is (`CARGO_TARGET_DIR`, as
# each worktree sets its own; `ui-tests/support/world.ts` asks cargo), unless
# `LADDERBACK_BIN` names another.
[doc('Run the @ui scenarios in Chromium against a fresh build with the gallery.')]
[group('test')]
[positional-arguments]
test-ui *args: web-test _ui-deps
    cargo build
    cd ui-tests && node_modules/.bin/cucumber-js --config cucumber.ts "$@"

# Installs the UI test dependencies when they are missing or older than the
# lockfile, as in a new worktree: node_modules isn't tracked.
[private]
_ui-deps:
    cd ui-tests && if [ ! -d node_modules ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then npm ci; fi

# A crate this branch adds to `mutants_packages` is mutated whole, not only its
# changed lines (§13.1): its code may have merged before it joined the gate,
# and then no diff reaches it. "Added" compares the list with the one at the
# merge base, the commit the diff starts from; if that Justfile has no list
# this recipe can read, every gated crate counts as added, so a change of its
# format costs a longer run, never an untested crate. The other gated crates
# are mutated in the diff only.
# Further arguments go to cargo mutants, e.g. `just mutants origin/main --jobs 3`
# when several agents share the machine.
# cargo mutants runs with CARGO_TARGET_DIR and CARGO_BUILD_TARGET_DIR unset, so
# each job builds in its own copy of the tree, not in one folder shared with the
# other jobs and with the worktree (technical design §13.5, item 4).
[doc("Mutation-test the gated crates' code changed since `base`, and the whole of any crate the branch adds to the gate.")]
[group('test')]
[positional-arguments]
mutants base='origin/main' *args:
    #!/usr/bin/env bash
    set -euo pipefail
    shift
    diff=$(mktemp)
    trap 'rm -f "$diff"' EXIT
    git diff '{{ base }}...HEAD' > "$diff"
    # The crate names in a `--package a --package b` list, space-separated.
    names() { local word; for word in $1; do [ "$word" = --package ] || printf ' %s ' "$word"; done; }
    at_base=$(names "$(git show "$(git merge-base '{{ base }}' HEAD):Justfile" | sed -n 's/^mutants_packages := "\(.*\)"$/\1/p')")
    joined='' in_diff=''
    for package in $(names '{{ mutants_packages }}'); do
        case "$at_base" in
            *" $package "*) in_diff="$in_diff --package $package" ;;
            *) joined="$joined --package $package" ;;
        esac
    done
    if [ -n "$joined" ]; then
        echo "Joining the gate, so mutated whole:$joined"
        env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET_DIR cargo mutants $joined "$@"
    fi
    if [ -n "$in_diff" ]; then
        env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET_DIR cargo mutants --in-diff "$diff" $in_diff "$@"
    fi

# Further arguments go to cargo mutants, as for `mutants`, which says why
# CARGO_TARGET_DIR and CARGO_BUILD_TARGET_DIR are unset.
[doc('Mutation-test all of the gated crates (slow; CI runs it nightly).')]
[group('test')]
[positional-arguments]
mutants-all *args:
    env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET_DIR cargo mutants {{ mutants_packages }} "$@"

# Check that the branch's first commit is a failing test: commit (§13.1).
[group('test')]
red-first base='origin/main':
    node scripts/red-first.ts '{{ base }}'

# Check a pull request title (or a commit subject) as CI's "Pull request
# title" check does: a Conventional Commit with a type release-plz knows.
[doc('Check a pull request title against Conventional Commits, as CI does.')]
[group('quality')]
check-pr-title title:
    node scripts/check-pr-title.ts {{ quote(title) }}

# `deny` also checks what CI checks, but it needs the network, so it is left
# out, as are mutation testing and the red-first check.
[doc('Run every check CI runs, except mutation testing, red-first and deny (needs the network).')]
[group('quality')]
verify: fmt-check lint lint-workflows architecture test test-contrast test-release test-wasm test-ui

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

# The web UI a release embeds: built with `release_flags`, as the daemon is, so
# it has no pseudolocales and no gallery (§12.1).
[doc('Build the web UI into crates/web/dist as a release embeds it.')]
[group('release')]
web-release:
    cd crates/web && trunk build --release {{ release_flags }}

# Builds the release archive for `target` as the Release builds workflow does
# (§12.1): the web UI with the release flags, which the binary embeds, then
# cargo-dist's `dist build` (dist-workspace.toml), which builds the daemon with
# the same flags and writes the archive, its checksum and the unpacked folder
# to `target/distrib` (dist's folder, whatever CARGO_TARGET_DIR says), with
# dist's manifest as `release-build-manifest.json` beside them. The binary's
# dynamic libraries go to the log as a table. Needs dist
# (scripts/install-dist.sh). It publishes nothing.
[doc('Build the release archive for a target with cargo-dist; publishes nothing.')]
[group('release')]
release-build target: web-release
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p target/distrib
    dist build --artifacts=local --target {{ quote(target) }} --print=linkage --output-format=json > target/distrib/release-build-manifest.json

# Runs scripts/smoke-release.ts on a release binary: its version, no
# pseudolocales and no gallery, its dynamic libraries and glibc floor, then
# the daemon serving its UI and running one backup in scratch folders, on this
# machine or, with `--container <name>`, in a clean Linux container (Docker).
# `--help` lists the options.
[doc('Smoke-test a release binary: version, flags, linkage, serve and one backup.')]
[group('release')]
[positional-arguments]
smoke-release binary *args:
    node scripts/smoke-release.ts "$@"

# Runs the Release workflow, which opens or updates the draft release PR, and
# waits for it (§12.1). Then, on that PR's branch: with a `version` (a version
# the commits can't imply: the first release, a release candidate, 1.0.0) it
# runs `release-plz set-version <version>`; in a terminal it opens CHANGELOG.md
# in $EDITOR for the notes; if either changed anything, it commits, pushes to
# the branch and, with a version, retitles the PR. A later run of the workflow,
# this recipe's included, closes a release PR that has such a commit and opens
# a new one from the commits alone, so a rerun needs the same `version` again
# and the notes redone. If the push fails, the edited notes are saved and their
# path printed. Needs `gh` (signed in, allowed to run workflows) and
# `release-plz`. Nothing tests it automatically: it acts on the real repository.
[confirm('Run the Release workflow and push to the release PR branch?')]
[doc('Open or update the release PR; optionally set its version and edit its notes.')]
[group('release')]
release-pr version='':
    #!/usr/bin/env bash
    set -euo pipefail
    root={{ quote(root) }}
    version={{ quote(version) }}
    part='(0|[1-9][0-9]*)'
    if [ -n "$version" ] && ! [[ "$version" =~ ^$part\.$part\.$part(-rc\.$part)?$ ]]; then
        echo "error: version '$version' is not X.Y.Z or X.Y.Z-rc.N (no leading zeros)" >&2
        exit 2
    fi
    for tool in gh release-plz; do
        command -v "$tool" >/dev/null || { echo "error: $tool is not installed (just setup)" >&2; exit 2; }
    done
    # The run this dispatch starts: from the URL gh prints, else the newest
    # dispatch run newer than every run before it (run IDs only grow).
    before=$(gh run list --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId // 0')
    out=$(gh workflow run release.yml --ref main 2>&1)
    echo "$out"
    run=$(grep -oE 'actions/runs/[0-9]+' <<<"$out" | grep -oE '[0-9]+$' | head -n 1 || true)
    for _ in $(seq 1 30); do
        [ -n "$run" ] && break
        sleep 2
        run=$(gh run list --workflow release.yml --event workflow_dispatch --limit 5 \
            --json databaseId -q "[.[] | select(.databaseId > $before)][0].databaseId // empty")
    done
    [ -n "$run" ] || { echo "error: the Release run didn't appear within a minute" >&2; exit 1; }
    # Poll until the run completes: right after a dispatch the API can answer
    # with an error, and an unfinished run has no conclusion yet. 45 minutes
    # covers the job's 30-minute limit and a wait in the queue.
    state=''
    for _ in $(seq 1 270); do
        state=$(gh run view "$run" --json status,conclusion -q '"\(.status) \(.conclusion)"' 2>/dev/null || true)
        [[ $state == "completed "* ]] && break
        sleep 10
    done
    case $state in
        "completed success") ;;
        "completed cancelled") echo "error: the Release run was cancelled (a push to main can take its place in the queue); run this again" >&2; exit 1 ;;
        "completed "*) echo "error: the Release run failed: gh run view $run --log-failed" >&2; exit 1 ;;
        *) echo "error: the Release run hasn't finished after 45 minutes: gh run view $run" >&2; exit 1 ;;
    esac
    # The release PR as release-plz finds it: open, into main, from a
    # `release-plz-` branch. A plain listing, not search (`--label`), which can
    # lag behind the PR the run just opened or closed.
    pr=''
    for _ in $(seq 1 6); do
        pr=$(gh pr list --state open --base main --json number,headRefName \
            -q '[.[] | select(.headRefName | startswith("release-plz-"))] | max_by(.number) | .number // empty')
        [ -n "$pr" ] && break
        sleep 5
    done
    if [ -z "$pr" ]; then
        echo "No open release PR after the run: nothing since the last tag calls for a release (feat, fix, perf, revert or a breaking change)."
        exit 0
    fi
    branch=$(gh pr view "$pr" --json headRefName -q .headRefName)
    git -C "$root" fetch --quiet origin "$branch"
    head=$(git -C "$root" rev-parse FETCH_HEAD)
    scratch=$(mktemp -d)
    keep="${TMPDIR:-/tmp}/ladderback-release-notes-$pr.md"
    notes_edited=0
    cleanup() {
        status=$?
        if [ "$status" -ne 0 ] && [ "$notes_edited" = 1 ] && cp "$scratch/release/CHANGELOG.md" "$keep" 2>/dev/null; then
            echo "Your edited notes are saved in $keep" >&2
        fi
        git -C "$root" worktree remove --force "$scratch/release" 2>/dev/null || true
        rm -rf "$scratch"
    }
    trap cleanup EXIT
    git -C "$root" worktree add --quiet --detach "$scratch/release" "$head"
    cd "$scratch/release"
    changed=()
    if [ -n "$version" ]; then
        release-plz set-version "$version"
        changed+=("set the version to $version")
    fi
    if [ -t 0 ] && [ -t 1 ]; then
        before_edit=$(git hash-object CHANGELOG.md)
        ${EDITOR:-vi} CHANGELOG.md
        if [ "$(git hash-object CHANGELOG.md)" != "$before_edit" ]; then
            notes_edited=1
            changed+=("edit the release notes")
        fi
    fi
    if [ "${#changed[@]}" -eq 0 ]; then
        echo "Release PR unchanged: $(gh pr view "$pr" --json url -q .url)"
        exit 0
    fi
    git add --update
    message="chore: ${changed[0]}"
    [ "${#changed[@]}" -eq 2 ] && message="$message and ${changed[1]}"
    git commit --quiet -m "$message"
    git push --quiet --force-with-lease="$branch:$head" origin "HEAD:refs/heads/$branch"
    if [ -n "$version" ]; then
        gh pr edit "$pr" --title "chore: release v$version" >/dev/null
    fi
    echo "Release PR updated: $(gh pr view "$pr" --json url -q .url)"
