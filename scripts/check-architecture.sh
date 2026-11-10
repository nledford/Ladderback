#!/usr/bin/env bash
# Enforces the hexagonal dependency rule (technical design §3.2):
#   - `domain` (host and wasm32) and `app` never depend, even indirectly, on
#     runtime, I/O, engine or presentation crates, or on `chrono` (time goes
#     through `jiff`, decision 22), with any feature enabled;
#   - `domain` builds for wasm32-unknown-unknown;
#   - ICU4X's locale data is the daemon's alone: `l10n` and `web` build
#     without it, and `ladderback` builds with it (technical design §8.4);
#   - the inner rings share one banned-call list (clippy.toml);
#   - a release build leaves out the development pseudolocales' `fluent-pseudo`
#     (technical design §8.4): `ladderback` and `web` have it by default, and not
#     when built with the release flags (`--no-default-features`, as a release is);
#   - a release build leaves out the component gallery, the dev-only route
#     `/_gallery` (the `web` feature `gallery`).
# Dev-dependencies are excluded, so tests may use a runtime. `testkit` (test
# support) may be a dev-dependency only.
set -euo pipefail

cd "$(dirname "$0")/.."

# The cargo flags a release build is made with: the Justfile's `release_flags`,
# the one definition (technical design §12.1), which `just architecture` passes
# in. Release packaging uses the same, so these checks look at what ships.
if [[ -z ${RELEASE_FLAGS:-} ]]; then
  echo "error: RELEASE_FLAGS is empty; run this as 'just architecture', which passes the Justfile's release_flags" >&2
  exit 1
fi
read -r -a release_flags <<<"$RELEASE_FLAGS"

banned='^(tokio|sqlx|axum|hyper|reqwest|rustic_core|rustic_backend|fluent-bundle|fluent-langneg|unic-langid|icu_[a-z_]+|rand|chrono|l10n)$'
status=0

check() {
  local package=$1
  shift
  # All features, so an optional banned dependency behind a feature (the
  # `fakes` one, say) is caught. Dev-dependencies stay excluded.
  local tree found
  # Separate from the pipeline below: a failing `cargo tree` (unknown package,
  # no wasm target) must stop the script, while "no match" must not.
  tree=$(cargo tree --package "$package" --all-features --edges normal,build --prefix none --format '{p}' "$@")
  found=$(awk '{print $1}' <<<"$tree" | sort -u | grep -E "$banned" || true)
  if [[ -n $found ]]; then
    echo "error: $package ${*:-(host)} depends on banned crates:" >&2
    sed 's/^/  /' <<<"$found" >&2
    status=1
  else
    echo "ok: $package ${*:-(host)} has no banned dependencies"
  fi
}

check domain
check domain --target wasm32-unknown-unknown
check app

# A browser must never download ICU4X's locale data (technical design §8.4).
# `l10n` has ICU4X behind its `icu` feature, which only the root crate turns
# on, so the packages the web UI is built from have none of the crates that
# format or carry data, and the daemon has them (that catches the root crate
# losing the feature). These packages are checked with their default
# features: `--all-features` would turn `icu` on. Only these crates are
# named: `icu_locale_core`, `icu_provider` and others reach any build that
# has `url`.
icu='^(fixed_decimal|icu_(calendar|datetime|decimal|plurals|time)(_data)?)$'

# The crates a package builds with its default features, one name per line.
# The arguments after the package go to `cargo tree`.
default_crates() {
  local package=$1 tree
  shift
  # Separate from the pipeline, and `|| exit`: the caller runs this in a
  # command substitution, where `set -e` is off, so a failing `cargo tree`
  # would otherwise read as a tree with no ICU4X in it.
  tree=$(cargo tree --package "$package" --edges normal,build --prefix none --format '{p}' "$@") || exit 1
  awk '{print $1}' <<<"$tree" | sort -u
}

check_no_icu() {
  local package=$1 crates found
  shift
  crates=$(default_crates "$package" "$@")
  found=$(grep -E "$icu" <<<"$crates" || true)
  if [[ -n $found ]]; then
    echo "error: $package ${*:-(host)} builds with ICU4X's locale data by default:" >&2
    sed 's/^/  /' <<<"$found" >&2
    status=1
  else
    echo "ok: $package ${*:-(host)} has no ICU4X locale data"
  fi
}

check_no_icu l10n
check_no_icu web --target wasm32-unknown-unknown

daemon=$(default_crates ladderback)
daemon_has_icu=1
for crate in icu_decimal icu_datetime; do
  if ! grep -qx "$crate" <<<"$daemon"; then
    echo "error: ladderback doesn't build with $crate: the daemon lacks l10n's \`icu\` feature" >&2
    daemon_has_icu=0
    status=1
  fi
done
if ((daemon_has_icu)); then
  echo "ok: ladderback builds with ICU4X"
fi

# testkit is test support: a dev-dependency only. `cargo tree --invert` lists
# the crates that depend on it through normal or build edges, which includes
# testkit itself and must include nothing else.
# Separate from the pipeline: a failing `cargo tree` must stop the script.
tree=$(cargo tree --workspace --all-features --edges normal,build --invert testkit --prefix none --format '{p}')
testkit_users=$(awk '{print $1}' <<<"$tree" | sort -u | grep -vx testkit || true)
if [[ -n $testkit_users ]]; then
  echo "error: testkit is a dev-dependency only, but these depend on it:" >&2
  sed 's/^/  /' <<<"$testkit_users" >&2
  status=1
else
  echo "ok: testkit is a dev-dependency only"
fi

# The development pseudolocales stay out of a release build (technical design
# §8.4). `ladderback` and `web` each pass their `pseudo` feature on to `l10n`,
# which brings in `fluent-pseudo` and, with it, a regex engine: about a
# megabyte of the web UI. Both sides are checked, so that a change that blinds
# this check cannot pass it with nothing to find.
pseudolocales() {
  local want=$1 package=$2
  shift 2
  local tree found
  # Separate from the pipeline, as in `check`: a failing `cargo tree` must
  # stop the script, while "no match" must not.
  tree=$(cargo tree --package "$package" --edges normal,build --prefix none --format '{p}' "$@")
  found=$(awk '{print $1}' <<<"$tree" | sort -u | grep -Fx fluent-pseudo || true)
  if [[ $want == has && -z $found ]]; then
    echo "error: $package ${*:-(default features)} lacks fluent-pseudo, so a development build has no pseudolocales" >&2
    status=1
  elif [[ $want == lacks && -n $found ]]; then
    echo "error: $package ${*:-(default features)} depends on fluent-pseudo, which a release build must leave out" >&2
    status=1
  else
    echo "ok: $package ${*:-(default features)} $want fluent-pseudo"
  fi
}

pseudolocales has ladderback
pseudolocales has web --target wasm32-unknown-unknown
pseudolocales lacks ladderback "${release_flags[@]}"
pseudolocales lacks web "${release_flags[@]}" --target wasm32-unknown-unknown

# The component gallery (technical design §4.5) is a dev-only route: the `web`
# feature `gallery` is off by default, and a release build (`release_flags`)
# leaves it out. `just web-test` turns it on for the UI tests. The feature adds
# no crate, so the check reads the feature graph. As for the pseudolocales, both
# sides are checked, so that this cannot pass with nothing to find.
gallery() {
  local want=$1
  shift
  local tree found
  # Separate from the pipeline, as in `check`: a failing `cargo tree` must
  # stop the script, while "no match" must not.
  tree=$(cargo tree --package web --edges features --invert web --prefix none --target wasm32-unknown-unknown "$@")
  found=$(grep -F 'web feature "gallery"' <<<"$tree" || true)
  if [[ $want == has && -z $found ]]; then
    echo "error: web ${*:-(default features)} lacks the gallery, so the UI tests have no /_gallery" >&2
    status=1
  elif [[ $want == lacks && -n $found ]]; then
    echo "error: web ${*:-(default features)} has the gallery, which a release build must leave out" >&2
    status=1
  else
    echo "ok: web ${*:-(default features)} $want the gallery"
  fi
}

gallery has --features gallery
gallery lacks
gallery lacks "${release_flags[@]}"

if ! cmp -s crates/domain/clippy.toml crates/app/clippy.toml; then
  echo "error: crates/domain/clippy.toml and crates/app/clippy.toml differ" >&2
  status=1
fi

# Production code never crashes (technical design §13.5). Clippy reads the
# nearest clippy.toml only, so the root one and the two inner rings' must
# all let tests unwrap, expect and panic; and every production crate root
# must deny the panicking lints. `testkit` is test support, not production.
no_panic_lints=(unwrap_used expect_used panic unimplemented todo unreachable)
no_panic_ok=1
for file in clippy.toml $(find crates -name clippy.toml | sort); do
  for setting in allow-unwrap-in-tests allow-expect-in-tests allow-panic-in-tests; do
    if ! grep -qx "$setting = true" "$file"; then
      echo "error: $file lacks '$setting = true'" >&2
      no_panic_ok=0
    fi
  done
done
roots=$(find crates src -maxdepth 5 \( -path 'src/*' -o -path '*/src/*' \) \( -name lib.rs -o -name main.rs \) -not -path 'crates/testkit/*' | sort)
if [[ -z $roots ]]; then
  echo "error: found no crate roots to check" >&2
  no_panic_ok=0
fi
while IFS= read -r root; do
  [[ -n $root ]] || continue
  for lint in "${no_panic_lints[@]}"; do
    # The lint's name, not a longer one that starts with it, and not in a comment.
    if (($(grep -v '^[[:space:]]*//' "$root" | grep -Ec "clippy::$lint([^_a-z]|\$)") == 0)); then
      echo "error: $root doesn't deny clippy::$lint" >&2
      no_panic_ok=0
    fi
  done
done <<<"$roots"
if ((no_panic_ok)); then
  echo "ok: crate roots deny panicking code; every clippy.toml lets tests use it"
else
  status=1
fi

cargo build --quiet --package domain --target wasm32-unknown-unknown
echo "ok: domain builds for wasm32-unknown-unknown"

exit "$status"
