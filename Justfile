# Ladderback's workflows, for people and agents alike. CI runs these same
# recipes. `just` lists them; `just verify` runs everything CI checks.

set shell := ["bash", "-euo", "pipefail", "-c"]

root := justfile_directory()

# The crates mutation testing gates (technical design §13.1). Add a crate here
# when it lands, so `mutants` and `mutants-all` both cover it; one added later
# is mutated whole by the `mutants` run of the pull request that adds it.
mutants_packages := "--package domain --package app --package api --package l10n --package config-adapter --package web --package hooks --package interop --package secrets --package spawn-gate --package notify-adapter --package mcp --package releases-adapter --package signing-adapter --package tls-adapter"

# The cargo flags a release build is made with (technical design §12.1): the
# development pseudolocales and the component gallery stay out. `architecture`
# checks a build with them, and release packaging must pass them too: `trunk
# build --release` for the web UI (`web-release`), cargo-dist, which can't read
# this file, so `dist-workspace.toml` says the same and `architecture` checks
# that the two agree, and the image. `just --evaluate release_flags` prints
# them. A feature a release needs joins this string, so the checks and the
# artifacts change together.
release_flags := "--no-default-features"

# On macOS, development installs use this self-signed identity by default so
# Full Disk Access and Photos grants survive an update. Set
# LADDERBACK_SIGN_IDENTITY to a different identity or SHA-1 hash when needed.
macos_sign_identity := env_var_or_default("LADDERBACK_SIGN_IDENTITY", "Ladderback Code Signing")

# The leptosfmt release `fmt` and `fmt-check` expect (technical design §13.13).
# Another release formats differently, so `setup` installs this one, CI too
# (`just --evaluate leptosfmt_version`), and `fmt-check` refuses another.
leptosfmt_version := "0.1.34"

# The mdBook release that builds the user docs site (§16, decision 128).
# Another release renders the pages and their anchors differently, so `setup`
# installs this one, CI too (it reads this line), and `docs-site` refuses
# another. `docs-site` also fails when docs/guide/theme/toc.html.hbs no longer
# matches this release's own template (scripts/check-theme-override.ts).
mdbook_version := "0.5.4"

# List available workflows.
[group('help')]
default:
    @just --list --unsorted

alias t := test
alias v := verify

# Installs the wasm target, the cargo tools (needs cargo-binstall), the UI test
# dependencies and the three test browsers (Chromium, Firefox, WebKit).
[doc('Install the build, test and release tools.')]
[group('setup')]
setup:
    rustup target add wasm32-unknown-unknown
    cargo binstall --no-confirm trunk cargo-deny cargo-mutants cargo-nextest release-plz zizmor cargo-auditable
    cargo binstall --no-confirm --version "$(just _wasm-bindgen-version)" wasm-bindgen-cli
    cargo binstall --no-confirm --version '{{ leptosfmt_version }}' leptosfmt
    cargo binstall --no-confirm --version '{{ mdbook_version }}' mdbook
    scripts/install-rustic.sh
    scripts/install-actionlint.sh
    scripts/install-shellcheck.sh
    scripts/install-dist.sh
    scripts/install-nfpm.sh
    cd ui-tests && npm ci
    cd ui-tests && node_modules/.bin/playwright install chromium firefox webkit

# Format the Rust and TypeScript code: leptosfmt formats the `view!` blocks,
# which rustfmt leaves alone, then rustfmt the rest, then Biome the TypeScript
# tooling (biome.json). leptosfmt goes first because rustfmt settles the code
# around a view it has laid out, never the other way round. The comment check
# runs before either writes: leptosfmt drops a comment inside a view's Rust
# code (§13.13).
[doc('Format the Rust and TypeScript code: leptosfmt, rustfmt, then Biome.')]
[group('quality')]
fmt: _leptosfmt-pinned _ui-deps
    node scripts/leptosfmt-comments.ts leptosfmt.toml crates/web
    leptosfmt --quiet --config-file leptosfmt.toml crates/web
    cargo fmt --all
    ui-tests/node_modules/.bin/biome format --write

# Check Rust formatting without changing files: rustfmt, leptosfmt, and no
# comment that a leptosfmt run would drop. The TypeScript format is checked by
# `lint`.
[doc('Check the Rust formatting without changing files.')]
[group('quality')]
fmt-check: _leptosfmt-pinned
    cargo fmt --all --check
    node scripts/leptosfmt-comments.ts leptosfmt.toml crates/web
    leptosfmt --quiet --check --config-file leptosfmt.toml crates/web

# Fails unless the installed leptosfmt is the pinned release.
[private]
_leptosfmt-pinned:
    @[ "$(leptosfmt --version)" = 'leptosfmt {{ leptosfmt_version }}' ] || { echo "needs leptosfmt {{ leptosfmt_version }}, found $(leptosfmt --version 2>&1 || true); run just setup" >&2; exit 1; }

# Fails unless the installed mdBook is the pinned release.
[private]
_mdbook-pinned:
    @[ "$(mdbook --version)" = 'mdbook v{{ mdbook_version }}' ] || { echo "needs mdbook {{ mdbook_version }}, found $(mdbook --version 2>&1 || true); run just setup" >&2; exit 1; }

# Build the user docs site (docs/guide/, §16 decision 128) into
# docs/guide/book/, then check that every link in its pages resolves:
# mdBook copies a broken link into the page as it is, so the check is what
# fails the build on one (scripts/check-site-links.ts). Then check that the
# site's copy of mdBook's sidebar template still renders mdBook's own page
# with only its title added (scripts/check-theme-override.ts), so a new
# mdBook can't leave it stale. `lint` runs it, so CI does, docs-only pull
# requests included.
[doc('Build the user docs site into docs/guide/book/ and check its links and its copied template.')]
[group('quality')]
docs-site: _mdbook-pinned
    mdbook build docs/guide
    node scripts/check-site-links.ts
    node scripts/check-theme-override.ts

# Build the user docs site, check its links (`docs-site`), then run axe-core
# over every page in Chromium: WCAG 2.2 A and AA, the rules the @ui scenarios
# scan with, in each of mdBook's five themes at a wide and a narrow window, on
# the landing page with its theme list open and with search results showing,
# and on a result's page with its matches marked
# (ui-tests/scripts/docs-site-a11y.ts; §16, decision 128). The docs site
# workflow (.github/workflows/docs-site.yml) runs it on every change to the
# site, before it publishes one. `lint` doesn't: it starts no browser.
[doc('Build the user docs site, check its links, and run axe-core over every page in Chromium.')]
[group('quality')]
docs-site-a11y: docs-site _ui-deps
    node ui-tests/scripts/docs-site-a11y.ts

# Fails unless the shellcheck on the PATH is the release
# scripts/install-shellcheck.sh pins (its `version=` line, the one place the
# version lives, beside the hashes that go with it): another release finds
# other things, and on CI a copy the runner ships could stand ahead of it.
[private]
_shellcheck-pinned:
    @want=$(sed -n 's/^version=//p' scripts/install-shellcheck.sh); have=$({ shellcheck --version 2>/dev/null || true; } | sed -n 's/^version: //p'); [ -n "$want" ] && [ "$have" = "$want" ] || { echo "needs shellcheck $want, found ${have:-none}; run just setup" >&2; exit 1; }

# Clippy on the host crates: the part of `lint` that can differ by OS
# (`cfg(target_os)` code), and so the only part CI runs on macOS too.
[doc('Clippy on the host crates only, the part of `lint` that can differ by OS.')]
[group('quality')]
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Lint: `clippy`, clippy on the web UI (wasm32, its wasm-only tests included),
# the web UI once more with the `gallery` feature (no other build turns it on,
# so without it a warning in a demo of the component gallery goes unseen),
# the host and the web UI (not the gallery, which no release build has) again
# with `release_flags`, so the release build has its dead code and warnings
# caught too, the type check of the
# TypeScript tooling (tsconfig.json; its compiler is a ui-tests
# dev-dependency), Biome's format and lint check of that TypeScript
# (biome.json, also a ui-tests dev-dependency; `biome ci` writes nothing, and
# with `--error-on-warnings` a warning fails like an error, so a rule is on or
# off, never advisory; `just fmt` writes the format), shellcheck on the shell
# scripts (scripts/install-shellcheck.sh pins it; at `--severity=warning`, so
# its warnings and errors fail and its style notes don't), the tooling's own
# tests (`scripts/*.test.ts`, under Node's built-in runner), the journey lint,
# the check that every path AGENTS.md and CLAUDE.md name exists, the check that
# no paragraph in docs/ runs over 150 words, the check that the tracked files
# spell in American English (but for the stored and wire values and the label
# it lists), the user docs site's build and link check (`docs-site`), the
# check of the §16 decision files (good names and headings, a number each, no
# table of them in §16; decision 162) and that §4.5's index matches the
# component contract files, and
# rustdoc on every crate, private items
# included, with warnings as errors, so a doc comment's broken link or
# malformed markup fails. Rustdoc sees only the items built for the host and,
# for `web`, not its wasm32-only ones. CI runs `lint` on Linux alone, so a doc
# comment on macOS-only code is checked only by a local run on a Mac; macOS CI
# runs `clippy` (see `clippy`).
[doc('The static checks: clippy (host, web UI, gallery, release flags), TypeScript, shellcheck, the tooling tests, the docs checks and rustdoc.')]
[group('quality')]
lint: _ui-deps _shellcheck-pinned clippy docs-site
    cargo clippy --package web --all-targets --target wasm32-unknown-unknown -- -D warnings
    cargo clippy --package web --all-targets --features gallery --target wasm32-unknown-unknown -- -D warnings
    cargo clippy --workspace --all-targets {{ release_flags }} -- -D warnings
    cargo clippy --package web --all-targets --target wasm32-unknown-unknown {{ release_flags }} -- -D warnings
    ui-tests/node_modules/.bin/tsc --project tsconfig.json
    ui-tests/node_modules/.bin/biome ci --error-on-warnings
    shellcheck --severity=warning scripts/*.sh
    node --test scripts/*.test.ts
    cd ui-tests && npm run --silent lint
    scripts/check-doc-paths.sh
    node scripts/check-paragraph-length.ts
    node scripts/check-american-spelling.ts
    node scripts/decision-index.ts
    node scripts/component-index.ts
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items

# Print the index of the §16 decisions, one Markdown row per file in
# docs/design/decisions/ in number order, after checking the files as `lint`
# does. The index isn't committed: the files are it (decision 162).
[doc('Print the §16 decision index from docs/design/decisions/.')]
[group('quality')]
decision-index:
    node scripts/decision-index.ts --print

# Write the index of the shared components' accessibility contracts in
# docs/design/components/accessibility.md (§4.5) from the files in
# docs/design/components/accessibility/, one row per file, its heading and its
# component. After adding or renaming a contract, and to resolve a conflict in
# the table: take either side, then run this.
[doc('Write the §4.5 contract index from docs/design/components/accessibility/.')]
[group('quality')]
component-index:
    node scripts/component-index.ts --write

# List, for each translation (or the tags given), the messages that are
# missing, translated but not yet reviewed, or stale: their en-US text has
# changed since they were translated. Each stale one comes with the English
# it was translated from, read from the git history, and the English now
# (technical design §8.4, decision 101). Exits 0 whatever it finds.
[doc('List missing, unreviewed and stale messages of each translation.')]
[group('quality')]
[positional-arguments]
translations *tags:
    node scripts/translations.ts list "$@"

# After a review, record the en-US text the messages `ids` of `tag` were
# translated from (`--all`: every translated message not reviewed yet, never
# a stale one, which is accepted only by its ID), in
# crates/l10n/locales/<tag>/sources.txt, so `just translations` finds them
# current until that English changes.
[doc('Record the English of translated messages accepted after review.')]
[group('quality')]
[positional-arguments]
accept-translations tag *ids:
    node scripts/translations.ts accept "$@"

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

# Check the dependency rule: banned crates in domain and app, the crate graph,
# the owners of foreign models and no async in domain (from `cargo metadata`,
# with Node), domain on wasm32.
[group('quality')]
architecture:
    RELEASE_FLAGS='{{ release_flags }}' scripts/check-architecture.sh

# Checks the dependency tree against deny.toml: RustSec advisories, licenses,
# banned crates and sources (technical design §13.6), for every workspace
# package: `web` is not a dependency of the root package, so without
# `--workspace` its tree would go unchecked. It fetches the advisory database,
# so it needs the network, and a new advisory can fail it without any change
# here: `verify` leaves it out, and CI runs it in its own job, nightly too.
[doc('Check dependencies for advisories, licenses, bans and sources (cargo-deny; needs the network).')]
[group('quality')]
deny:
    cargo deny --locked --all-features --workspace check

# Checks the two npm lockfiles against the registry's advisories (technical
# design §13.6). Every package there is build or test tooling, a dev dependency,
# so `--omit=dev` would audit nothing: all of them count. It fails on a high or
# critical advisory. It needs the network, and a new advisory can fail it
# without any change here: `verify` leaves it out, and CI runs it in the `deny`
# job, nightly too.
[doc('Check the npm lockfiles for high or critical advisories (npm audit; needs the network).')]
[group('quality')]
npm-audit:
    #!/usr/bin/env bash
    # Audit both before failing, so the log names every advisory.
    status=0
    (cd crates/web && npm audit --audit-level=high) || status=1
    (cd ui-tests && npm audit --audit-level=high) || status=1
    exit "$status"

# Runs unit, contract and doc tests with cargo-nextest, then the acceptance and
# @e2e cucumber runners; extra arguments go to nextest (a filterset, a test
# name). Every test process has a scratch home, and the run fails if a test
# wrote there (scripts/with-test-home.sh). Tests that skip themselves
# (`testkit::skip!`) are listed at the end.
# Set NEXTEST_PROFILE=ci for retries and the JUnit report (.config/nextest.toml).
[doc('Run every Rust test suite, then list skipped tests.')]
[group('test')]
[positional-arguments]
test *args:
    scripts/with-skip-log.sh scripts/with-test-home.sh '{{ just_executable() }}' _test-suites "$@"

# The suites `test` runs, under its one skip log. The cucumber runners use
# `harness = false`, which nextest can't run (its default filter in
# .config/nextest.toml leaves them out), so `cargo test` runs them; nextest
# doesn't run doc tests either. Every cargo command says --workspace, even
# for one test target: Cargo unifies features over the packages a command
# selects, so `--package app` or the root package alone builds dependencies
# with other features, and each of those commands rebuilt nearly all of them.
# A suite that fails doesn't stop the next one: the run goes on and fails at
# the end, naming each suite that failed, so one run lists every failure, as
# the nextest `ci` profile's `fail-fast = false` means it to (#997). An
# interrupt (Ctrl-C, a canceled CI job) stops the run at the suite it reaches,
# with the signal's status: the traps catch one that the suite handles itself
# and exits from normally, as nextest does (exit 100), and the status check
# one that ends the suite (status above 128).
[positional-arguments]
[private]
_test-suites *args:
    #!/usr/bin/env bash
    set -uo pipefail
    trap 'exit 130' INT
    trap 'exit 143' TERM
    failed=()
    suite() {
        local name=$1 status
        shift
        "$@"
        status=$?
        if [[ $status -gt 128 ]]; then
            exit "$status"
        elif [[ $status -ne 0 ]]; then
            failed+=("$name (exit $status)")
        fi
    }
    suite 'unit and contract tests (nextest)' cargo nextest run --workspace "$@"
    suite 'doc tests' cargo test --doc --workspace
    suite 'acceptance scenarios' cargo test --workspace --test acceptance
    suite '@e2e scenarios' cargo test --workspace --test e2e
    if [[ ${#failed[@]} -gt 0 ]]; then
        echo
        echo "Failed suites (${#failed[@]} of 4):"
        printf '  %s\n' "${failed[@]}"
        exit 1
    fi

# Arguments go to cucumber-rs as given, so `--name 'a job that runs'` stays
# one pattern, and narrow this runner's scenarios (as for `test-e2e`). The
# tests run in a scratch home, as in `test`.
[doc('Run the acceptance scenarios (fakes and a fake clock) only.')]
[group('test')]
[positional-arguments]
test-acceptance *args:
    scripts/with-skip-log.sh scripts/with-test-home.sh cargo test --workspace --test acceptance -- "$@"

# Run the @e2e scenarios against the real binary only, in a scratch home as in `test`.
[group('test')]
[positional-arguments]
test-e2e *args:
    scripts/with-skip-log.sh scripts/with-test-home.sh cargo test --workspace --test e2e -- "$@"

# A release is built with `release_flags` (`--no-default-features`), which
# leaves out the development pseudolocales (`pseudo`, technical design §8.4).
# `test` builds with the default features, so the tests that exist only
# without them never run there: the release lane of `l10n`'s tests
# (`tests/lanes/release.rs`) and the no-pseudo test in `tests/cli_locale.rs`.
# This runs, built that way, the tests of every package with default features
# (`l10n`, `ladderback`, `mcp` and `web`), so those run, and
# so does any test whose result depends on a feature. Each package gets the
# features the release build itself turns on in it (`l10n`'s `icu`), and none
# of the development ones: `scripts/release-test-packages.ts` reads both from
# `cargo metadata` and `cargo tree`, so a package's first default feature is
# picked up with no edit here, and it fails before any test runs if a
# package's tests (their dev-dependencies too) would turn a workspace crate's
# `pseudo` back on, naming the path. Each package is built and tested
# on its own: in one run Cargo would unify features across them. `web`'s wasm
# tests run in `test-wasm`. The cucumber runners are left out, as in `test`
# (nextest's default filter), and the tests run in a scratch home, as in
# `test`. Arguments go to nextest (never `--package`, which would bring the
# unified build back); with any, a package in which they select no test only
# warns. A failed package doesn't stop the others, and the recipe names each
# one at the end; an interrupt stops the run, as in `test`.
[doc('Run the tests of every package with default features as a release build is made (no default features).')]
[group('test')]
[positional-arguments]
test-release *args:
    scripts/with-skip-log.sh scripts/with-test-home.sh '{{ just_executable() }}' _test-release-packages "$@"

# The packages `test-release` runs, under its one skip log and scratch home.
[positional-arguments]
[private]
_test-release-packages *args:
    #!/usr/bin/env bash
    set -uo pipefail
    trap 'exit 130' INT
    trap 'exit 143' TERM
    plan=$(node scripts/release-test-packages.ts {{ release_flags }}) || exit 1
    failed=()
    while IFS=$'\t' read -r -u 3 name features; do
        echo "test-release: $name${features:+ (features: $features)}"
        cargo nextest run --package "$name" {{ release_flags }} ${features:+--features "$features"} ${1:+--no-tests=warn} "$@"
        status=$?
        if [[ $status -gt 128 ]]; then
            exit "$status"
        elif [[ $status -ne 0 ]]; then
            failed+=("$name (exit $status)")
        fi
    done 3<<<"$plan"
    if [[ ${#failed[@]} -gt 0 ]]; then
        echo "test-release: failed in ${failed[*]}" >&2
        exit 1
    fi

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
# setup, and the daemon listens on a free port, never `serve`'s 9898. Its
# release check asks an address nothing answers at, never GitHub. Arguments
# replace `serve`, and paths are relative to where you run it: `just try
# import-profile demo.toml` imports into the same scratch configuration. A
# keyring password source still uses your real OS keyring (the Keychain on
# macOS, the Secret Service on Linux). Delete the folder to start over.
#
# `just try --demo` uses a second scratch folder, `try-demo`, holding a
# repository with months of history, for screenshots and demos: the first run
# builds the rich history fixture (`tests/fixtures/history/rich.toml`,
# development.md §13.9) into it, as this machine's backups, and writes a
# configuration with its plans, a keep policy and the canary password
# `demo-password` in a file; later runs reuse it. A build that stopped
# part-way is started again. The fixture's failed run is a missing night and
# data for prune to reclaim: the daemon's operation log starts empty, so no
# page lists it as a failed run. Arguments after `--demo` replace `serve` as
# above.
[doc('Run ladderback on scratch folders and a free port (default: serve; --demo: on a demo history).')]
[group('dev')]
[no-cd]
[positional-arguments]
try *args: build
    #!/usr/bin/env bash
    set -euo pipefail
    target="${CARGO_TARGET_DIR:-{{ root }}/target}"
    scratch="$target/try"
    if [ "${1:-}" = --demo ]; then
        shift
        scratch="$target/try-demo"
        mkdir -p "$scratch/config" "$scratch/data"
        # The configuration names its folders by absolute paths, in TOML
        # literal strings, which can't hold a `'`.
        scratch="$(cd "$scratch" && pwd)"
        case "$scratch" in
            *"'"*) echo "try: --demo can't write a configuration for $scratch, whose path holds a '" >&2; exit 1 ;;
        esac
        if [ ! -e "$scratch/repo/config" ]; then
            if [ -e "$scratch/repo" ]; then
                echo "try: $scratch/repo holds no repository; delete $scratch to start over" >&2
                exit 1
            fi
            rm -rf "$scratch/repo.partial" "$scratch/sources"
            # The ID the daemon would choose, chosen first, so the history is
            # this machine's own; an ID kept from an earlier run stays.
            if [ ! -s "$scratch/data/machine-id" ]; then
                echo 0190a1b2-c3d4-7e5f-8a6b-7c8d9e0f1a2b > "$scratch/data/machine-id"
            fi
            echo "try: building the demo history; it takes about a minute" >&2
            plans="$(cargo run --quiet --manifest-path "{{ root }}/Cargo.toml" --package engine --features history-fixture --example history-fixture -- \
                "{{ root }}/tests/fixtures/history/rich.toml" "$scratch/repo.partial" "$scratch/sources" "$(cat "$scratch/data/machine-id")")"
            # The canary the builder sets, in a file: a password written in
            # config.toml would show the daemon's warning on every page. The
            # machine's host name is the one the declaration's snapshots carry.
            printf %s demo-password > "$scratch/password"
            {
                printf "schema = 1\n\n[agent]\nhostname = \"demo-host\"\n\n[repositories.demo]\nrepository = '%s'\npassword-file = '%s'\nmaintenance = \"local\"\n" "$scratch/repo" "$scratch/password"
                while IFS=$'\t' read -r plan folder; do
                    printf "\n[[plans]]\nid = \"%s\"\nrepository = \"demo\"\nsources = ['%s']\n\n[plans.retention]\nkeep-daily = 14\nkeep-weekly = 8\nkeep-monthly = 6\n" "$plan" "$folder"
                done <<< "$plans"
            } > "$scratch/config/config.toml"
            mv "$scratch/repo.partial" "$scratch/repo"
        fi
    fi
    mkdir -p "$scratch/config" "$scratch/data" "$scratch/home"
    if [ $# -eq 0 ]; then set -- serve --listen 127.0.0.1:0; fi
    echo "try: scratch folders in $scratch" >&2
    HOME="$scratch/home" LADDERBACK_CONFIG_DIR="$scratch/config" LADDERBACK_DATA_DIR="$scratch/data" LADDERBACK_CACHE_DIR="$scratch/cache" LADDERBACK_RELEASES_URL="http://127.0.0.1:9/" \
        exec "$target/debug/ladderback" "$@"

# Calls the cucumber-js that `_ui-deps` installs by its path: `npx` (even
# `npm exec --no`) runs an unrelated registry package of that name, or a cached
# copy of it, when the local one is missing, and that package exits 0. Its
# config is TypeScript, which it doesn't look for by itself, so it is named. The
# binary is built here, after `web-test`, so that it embeds the web UI with the
# gallery, not the release one `build` makes. The scenarios run the binary
# cargo just built, wherever its target directory is (`CARGO_TARGET_DIR`, as
# each worktree sets its own; `ui-tests/support/world.ts` asks cargo), unless
# `LADDERBACK_BIN` names another. A first argument that names one of
# Playwright's engines (`chromium`, `firefox`, `webkit`) picks the browser, as
# in `just test-ui webkit --name 'Sign in'`; without one, `LADDERBACK_UI_BROWSER`
# picks it, and Chromium when that is unset. `LADDERBACK_UI_WORKERS` sets how
# many scenarios run at once (3 when unset; 1 runs them one at a time). The rest
# go to cucumber-js. Before the scenarios, `node --test` runs the tests of the
# UI test tooling that need a browser (`ui-tests/support/*.browser.test.ts`) in
# the same engine; a failure there stops the recipe, and so does a pattern that
# matches no file (`failglob`), which `node --test` would read as its own glob
# and pass with no test run. `just lint` runs only `scripts/*.test.ts`, so it
# starts no browser. The run writes `ui-tests/reports/`: the JUnit report, and
# a trace and a screenshot of each failed scenario (`ui-tests/support/world.ts`);
# the recipe empties it first, so what is there is this run's.
[doc('Run the UI tooling browser tests, then the @ui scenarios, in a browser engine (default chromium) against a fresh build with the gallery.')]
[group('test')]
[positional-arguments]
test-ui *args: web-test _ui-deps
    cargo build
    shopt -s failglob && cd ui-tests && rm -rf reports && case "${1:-}" in chromium | firefox | webkit) export LADDERBACK_UI_BROWSER="$1"; shift ;; esac && node --test support/*.browser.test.ts && node_modules/.bin/cucumber-js --config cucumber.ts "$@"

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

# The quick part of what CI checks, run before pushing (AGENTS.md, "Local
# checks, then CI"): formatting, the lints, the dependency rule, then the
# unit, contract and acceptance tests in a scratch home, with the skip list.
# CI runs the rest on clean runners: doc tests, the @e2e runner, the release
# build, wasm, contrast, the @ui suite, mutants. Arguments narrow the nextest
# run (a filterset, a test name); the acceptance scenarios always run, as they
# use fakes and a fake clock. Red-first is separate: `just red-first` on a
# branch whose first commit is its test: commit.
[doc('Run the quick checks before pushing; CI runs the full suites.')]
[group('quality')]
[positional-arguments]
quick-ci *args: fmt-check lint architecture
    scripts/with-skip-log.sh scripts/with-test-home.sh '{{ just_executable() }}' _quick-suites "$@"

# The suites `quick-ci` runs, under its one skip log (see `_test-suites`).
[positional-arguments]
[private]
_quick-suites *args:
    cargo nextest run --workspace "$@"
    cargo test --workspace --test acceptance

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
# it has no pseudolocales and no gallery (§12.1), and with the `wasm-release`
# profile (Cargo.toml), which optimizes it for size. `--release` stays, so trunk
# still runs wasm-opt and the precompression hook (crates/web/Trunk.toml).
[doc('Build the web UI into crates/web/dist as a release embeds it.')]
[group('release')]
web-release:
    cd crates/web && trunk build --release --cargo-profile wasm-release {{ release_flags }}

# Installs the ladderback binary built from this checkout with `cargo install`,
# into Cargo's bin folder (`~/.cargo/bin`, or `bin` under `--root <dir>`). It is
# built as a release archive is (§12.1): with `release_flags`, so it has no
# pseudolocales and no gallery, and with the `dist` profile (the `release`
# profile with thin LTO), so the installed binary runs as the archives' does,
# though without the dependency list cargo-auditable embeds in them. It
# embeds the web UI that `web-release` builds first: `cargo install` from a git
# URL can't build that UI, so this checkout is the way in. The daemon starts its
# own workers, so this one binary is all an installation needs. Arguments go to
# `cargo install`: `just install --root ~/.local` installs into `~/.local/bin`.
# On macOS, `macos_sign_identity` names the code-signing identity used for the
# installed binary, defaulting to the self-signed certificate
# `Ladderback Code Signing`. macOS carries Full Disk Access and Photos grants
# between updates signed by the same identity (decision 71). `_signing-identity`
# refuses a name it can't sign with before anything is built, and the install
# ends by signing the binary (scripts/install-signing.ts). Elsewhere the
# signing helper does nothing.
[doc('Install the ladderback release binary from this checkout (arguments go to cargo install; macOS signs it with the configured self-signed identity).')]
[group('release')]
[positional-arguments]
install *args: _signing-identity web-release
    cargo install --locked --path . --profile dist {{ release_flags }} "$@"
    @LADDERBACK_SIGN_IDENTITY={{ quote(macos_sign_identity) }} node scripts/install-signing.ts sign "$@"

# Refuses, before `install` builds, a signing identity that names no valid
# code-signing identity on macOS.
[private]
_signing-identity:
    @LADDERBACK_SIGN_IDENTITY={{ quote(macos_sign_identity) }} node scripts/install-signing.ts check

# Updates an installed service from this checkout (#519): `install` with the
# arguments, into a staging folder beside the installed binary (its `--root`),
# then reads each `password-keyring` entry once with the new binary
# (`ladderback check-keyring`, #892), so the macOS keychain asks for the new
# build in this terminal rather than at the next scheduled backup; an entry
# it can't read stops it unless `--ignore-keyring-failure`. Then it waits
# until the daemon has no operation running or queued (its operation log,
# read-only, every 5 s, up to `--wait` minutes, default 120), moves the new
# binary into place, restarts the service with it at once (`ladderback
# install-service`) and checks that the service runs a new process of this
# checkout's build. Until then the running daemon's binary stays in place, so
# its keychain reads keep working (#1107). A daemon it can't tell is idle
# isn't restarted unless `--now`, which skips the wait.
# INSTALL_RESTART_ADDRESS names the service's address when it isn't
# 127.0.0.1:9898. Why it reads the log, the wait's race and the rest:
# scripts/install-restart.ts.
[doc('Build from this checkout, then install it and restart the service once it is idle (--now, --wait).')]
[group('release')]
[positional-arguments]
install-restart *args:
    @LADDERBACK_SIGN_IDENTITY={{ quote(macos_sign_identity) }} node scripts/install-restart.ts "$@"

# Builds the release archive for `target` as the Release builds workflow does
# (§12.1): the web UI with the release flags, which the binary embeds, then
# cargo-dist's `dist build` (dist-workspace.toml), which builds the daemon with
# the same flags and writes the archive, its checksum and the unpacked folder
# to `target/distrib` (dist's folder, whatever CARGO_TARGET_DIR says), with
# dist's manifest as `release-build-manifest.json` beside them. The binary's
# dynamic libraries go to the log as a table. Needs dist
# (scripts/install-dist.sh) and cargo-auditable, which dist builds with
# (`just setup` installs it). It publishes nothing.
[doc('Build the release archive for a target with cargo-dist; publishes nothing.')]
[group('release')]
release-build target: web-release
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p target/distrib
    dist build --artifacts=local --target {{ quote(target) }} --print=linkage --output-format=json > target/distrib/release-build-manifest.json

# Builds the .deb and the .rpm of a Linux release binary with nfpm
# (packaging/nfpm.yaml, §16 decision 123): the binary `just release-build`
# built and the archive holds, copied as it is, never rebuilt. `version` is
# what the binary says (`0.5.0`, `0.5.0-rc.1`, or `0.5.0+3f937cf` between
# releases) and goes into the file names as written; nfpm writes a release
# candidate into the package as `0.5.0~rc.1`, so it sorts before the release.
# The packages and a `.sha256` of each go to `target/distrib`, beside the
# archive: `ladderback_<version>_<amd64|arm64>.deb` and
# `ladderback-<version>-1.<x86_64|aarch64>.rpm`. Needs nfpm
# (scripts/install-nfpm.sh). It publishes nothing.
[doc('Build the .deb and .rpm of a Linux release binary with nfpm; publishes nothing.')]
[group('release')]
package target binary version:
    #!/usr/bin/env bash
    set -euo pipefail
    target={{ quote(target) }}
    case $target in
      x86_64-unknown-linux-gnu) deb_arch=amd64 rpm_arch=x86_64 ;;
      aarch64-unknown-linux-gnu) deb_arch=arm64 rpm_arch=aarch64 ;;
      *) echo "error: no packages for target $target: Linux on x86_64 or aarch64 only" >&2; exit 1 ;;
    esac
    version={{ quote(version) }}
    export PACKAGE_VERSION=$version PACKAGE_ARCH=$deb_arch PACKAGE_BINARY
    PACKAGE_BINARY=$(realpath {{ quote(binary) }})
    [[ -f $PACKAGE_BINARY ]] || { echo "error: no binary at $PACKAGE_BINARY" >&2; exit 1; }
    out=target/distrib
    mkdir -p "$out"
    deb="ladderback_${version}_$deb_arch.deb"
    rpm="ladderback-${version}-1.$rpm_arch.rpm"
    nfpm package --config packaging/nfpm.yaml --packager deb --target "$out/$deb"
    nfpm package --config packaging/nfpm.yaml --packager rpm --target "$out/$rpm"
    cd "$out"
    for file in "$deb" "$rpm"; do
      if command -v sha256sum >/dev/null; then sha256sum "$file" > "$file.sha256"; else shasum -a 256 "$file" > "$file.sha256"; fi
    done

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

# Builds the Docker image (Containerfile; §12, §16 decision 126) from Linux
# release archives, `ladderback-<x86_64|aarch64>-unknown-linux-gnu.tar.xz` as
# `just release-build` makes them and a release carries, never from source,
# so the image runs the archive's own binary. Each archive is unpacked into
# target/image under Docker's name for its architecture (amd64, arm64), the
# build context, and the image is built for the platforms of the archives
# given. `version` is what the binary says, for the image's version label.
# The other arguments go to `docker buildx build`: `--tag ladderback:dev
# --load` keeps an image of one platform locally. Needs Docker with buildx.
[doc('Build the Docker image from Linux release archives, never from source.')]
[group('release')]
[positional-arguments]
image version +args:
    #!/usr/bin/env bash
    set -euo pipefail
    version=$1
    shift
    context=target/image
    rm -rf "$context"
    mkdir -p "$context"
    platforms=()
    build=()
    for arg in "$@"; do
      case $arg in
        *.tar.xz)
          case $(basename "$arg" .tar.xz) in
            ladderback-x86_64-unknown-linux-gnu) arch=amd64 ;;
            ladderback-aarch64-unknown-linux-gnu) arch=arm64 ;;
            *) echo "error: $arg is no Linux release archive: ladderback-<x86_64|aarch64>-unknown-linux-gnu.tar.xz" >&2; exit 1 ;;
          esac
          [[ ! -e $context/$arch ]] || { echo "error: a second archive for linux/$arch: $arg" >&2; exit 1; }
          mkdir "$context/$arch"
          tar -xJf "$arg" -C "$context/$arch" --strip-components=1
          platforms+=("linux/$arch")
          ;;
        *) build+=("$arg") ;;
      esac
    done
    (( ${#platforms[@]} > 0 )) || { echo "error: no release archive given" >&2; exit 1; }
    list=$(IFS=,; echo "${platforms[*]}")
    docker buildx build --file Containerfile --platform "$list" --build-arg "VERSION=$version" ${build[@]+"${build[@]}"} "$context"

# Runs scripts/smoke-image.ts on a Docker image `just image` built: its
# version and labels, its user, the refusal to listen beyond loopback
# without `--allowed-host`, then the daemon in a container with sign-in on,
# backing up a read-only mounted folder into a repository on a volume, and a
# new container on the same volumes keeping the data; last, that the install
# guide's Compose file parses (`docker compose config`). It creates its own
# containers and volumes and removes them. `--help` lists the options.
[doc('Smoke-test a Docker image: version, user, sign-in, one backup, data kept, the Compose file.')]
[group('release')]
[positional-arguments]
smoke-image image *args:
    node scripts/smoke-image.ts "$@"

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
        "completed cancelled") echo "error: the Release run was canceled (a push to main can take its place in the queue); run this again" >&2; exit 1 ;;
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
