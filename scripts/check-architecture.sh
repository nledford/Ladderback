#!/usr/bin/env bash
# Enforces the hexagonal dependency rule (technical design §3.2):
#   - `domain` (host and wasm32) and `app` never depend, even indirectly, on
#     runtime, I/O, engine or presentation crates, or on `chrono` (time goes
#     through `jiff`, decision 22), with any feature enabled;
#   - `domain` builds for wasm32-unknown-unknown;
#   - ICU4X's locale data is the daemon's alone: `l10n` and `web` build
#     without it, and `ladderback` builds with it (technical design §8.4);
#   - the inner rings share one banned-call list (clippy.toml), and every
#     clippy.toml sets the same lint thresholds and test exemptions;
#   - a release build leaves out the development pseudolocales' `fluent-pseudo`
#     (technical design §8.4): `ladderback` and `web` have it by default, and not
#     when built with the release flags (`--no-default-features`, as a release is);
#   - a release build leaves out the component gallery, the dev-only route
#     `/_gallery` (the `web` feature `gallery`).
#   - a build of `ladderback` leaves out the history fixture builder (the
#     `engine` feature `history-fixture`, development.md §13.9), which only
#     its tests and `just try --demo` turn on.
#   - a release build leaves out the hub (§16, decision 122): `ladderback` and
#     `web` have the `hub` feature by default, and with the release flags no
#     crate has it, nor a crate only the hub uses.
#   - cargo-dist (`dist-workspace.toml`) builds the release archives with the
#     release flags these checks use (technical design §12.1).
#   - the build has one version of `dbus-secret-service`, the error type
#     `secrets` downcasts `keyring`'s Secret Service failures to.
#   - the workspace crates depend on each other only as the crate graph
#     allows, only its owners depend on each foreign model, and `domain` has
#     no `async` (scripts/check-crate-graph.ts, from `cargo metadata`).
# Dev-dependencies are excluded, so tests may use a runtime (the crate graph
# alone checks them too). `testkit` (test support) may be a dev-dependency
# only.
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

# The crate graph, the owners of foreign models and no `async` in `domain`
# (technical design §3.2): scripts/check-crate-graph.ts reads them from
# `cargo metadata`, which needs no build. Separate from the script: a failing
# `cargo metadata` must stop this one.
metadata=$(cargo metadata --no-deps --format-version 1)
node scripts/check-crate-graph.ts <<<"$metadata" || status=1

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

# On Linux `keyring` boxes a Secret Service failure as a
# `dbus_secret_service::Error`, and `secrets` downcasts it to tell a keyring
# with no service to ask from one that failed (security §10.2). With two
# versions of `dbus-secret-service` in the build, `keyring` could box the
# other's type, and the downcast would stop matching without a word: every
# missing service would read as `secret-unreadable`. So the build has exactly
# one. `Cargo.lock` lists the packages of every target, so this holds on any
# host. The lock file is an argument, so the check can be tried on a copy.
one_version() {
  local crate=$1 lock=$2 versions count
  versions=$(awk -v name="name = \"$crate\"" '$0 == name { getline; gsub(/^version = "|"$/, ""); print }' "$lock")
  count=$(grep -c . <<<"$versions" || true)
  if ((count != 1)); then
    echo "error: $lock has $count versions of $crate (${versions//$'\n'/, }), not one: keyring's boxed error type must be the one secrets downcasts to" >&2
    return 1
  fi
  echo "ok: one version of $crate ($versions)"
}

one_version dbus-secret-service Cargo.lock || status=1

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

# The history fixture builder (development.md §13.9) writes repositories from a
# declared history with a canary password: test support, which no build of
# the daemon has. The `engine` feature `history-fixture` is on only through a
# dev-dependency (the root crate's and engine's own tests) and for
# `just try --demo`'s example. Without dev edges the tree resolves features
# as a build does. As for the pseudolocales, both sides are checked, so that
# this cannot pass with nothing to find.
history_fixture() {
  local want=$1 edges=$2
  shift 2
  local tree found
  # Separate from the pipeline, as in `check`: a failing `cargo tree` must
  # stop the script, while "no match" must not.
  tree=$(cargo tree --package ladderback --edges "$edges" --prefix none --format '{p}|{f}' "$@")
  found=$(awk -F'|' '$1 ~ /^engine v/ { n = split($2, features, ","); for (i = 1; i <= n; i++) if (features[i] == "history-fixture") print $1 }' <<<"$tree")
  if [[ $want == has && -z $found ]]; then
    echo "error: ladderback's tests (edges $edges ${*}) lack the history fixture builder, so the scenarios that use it can't build" >&2
    status=1
  elif [[ $want == lacks && -n $found ]]; then
    echo "error: ladderback (edges $edges ${*:-default features}) has the history fixture builder, which only tests may have" >&2
    status=1
  else
    echo "ok: ladderback (edges $edges ${*:-default features}) $want the history fixture builder"
  fi
}

history_fixture has normal,build,dev
history_fixture lacks normal,build
history_fixture lacks normal,build "${release_flags[@]}"

# The hub stays out of a release until M8 (§16, decision 122, item 6). The
# tree lists each package with the features it is built with, so each
# workspace crate (a path package) whose `hub` is on is found;
# dev-dependencies are in it on purpose, so no test support turns the hub on
# in `ladderback`'s test build either (`just test-release` checks the test
# build of every package it runs the same way, `pseudo` too, but not the
# hub-only crates below or the wasm32 build). A release build must also lack every crate
# only the hub uses, which a plain dependency added by mistake would bring in:
# the QR crates, and the built-in TLS's `tls-adapter` with its `rcgen`
# (`rustls` and `tokio-rustls` are in every release already, for the
# daemon's own HTTPS clients). As for the pseudolocales, both sides are
# checked, so that a change that blinds this check cannot pass it with
# nothing to find.
hub_only_crates='^(qrcode|rqrr|rcgen|tls-adapter)$'
hub() {
  local want=$1 package=$2
  shift 2
  local tree found crates
  # Separate from the pipeline, as in `check`: a failing `cargo tree` must
  # stop the script, while "no match" must not.
  tree=$(cargo tree --package "$package" --edges normal,build,dev --prefix none --format '{p}|{f}' "$@")
  found=$(awk -F'|' '$1 ~ / \(\// { n = split($2, features, ","); for (i = 1; i <= n; i++) if (features[i] == "hub") print $1 }' <<<"$tree" | awk '{print $1}' | sort -u)
  crates=$(awk '{print $1}' <<<"$tree" | sort -u | grep -E "$hub_only_crates" || true)
  if [[ $want == has && -z $found ]]; then
    echo "error: $package ${*:-(default features)} lacks the hub feature, so the development build has no hub" >&2
    status=1
  elif [[ $want == lacks && -n $found ]]; then
    echo "error: $package ${*:-(default features)} has the hub feature of $(tr '\n' ' ' <<<"$found")which a release build must leave out" >&2
    status=1
  elif [[ $want == lacks && -n $crates ]]; then
    echo "error: $package ${*:-(default features)} depends on $(tr '\n' ' ' <<<"$crates")which only the hub uses and a release build must leave out" >&2
    status=1
  else
    echo "ok: $package ${*:-(default features)} $want the hub"
  fi
}

hub has ladderback
hub has web --target wasm32-unknown-unknown
hub lacks ladderback "${release_flags[@]}"
hub lacks web "${release_flags[@]}" --target wasm32-unknown-unknown

# The release archives are built by cargo-dist (dist-workspace.toml, technical
# design §12.1), which can't read the Justfile: its `[dist]` build keys must
# say what `release_flags` says, or an archive would ship what the checks above
# didn't look at. `precise-builds = true` makes dist build the `ladderback`
# package alone, so its features are the ones checked here. A `features` list
# is read from one line.
dist_flags() {
  local section value
  section=$(awk '/^\[/ { in_dist = ($0 == "[dist]") } in_dist' dist-workspace.toml)
  value=$(sed -nE 's/^default-features[[:space:]]*=[[:space:]]*([a-z]+).*/\1/p' <<<"$section")
  if [[ $value == false ]]; then echo --no-default-features; fi
  value=$(sed -nE 's/^all-features[[:space:]]*=[[:space:]]*([a-z]+).*/\1/p' <<<"$section")
  if [[ $value == true ]]; then echo --all-features; fi
  value=$(sed -nE 's/^features[[:space:]]*=[[:space:]]*\[(.*)\].*/\1/p' <<<"$section" | tr -d ' "')
  if [[ -n $value ]]; then echo "--features=$value"; fi
  value=$(sed -nE 's/^precise-builds[[:space:]]*=[[:space:]]*([a-z]+).*/\1/p' <<<"$section")
  if [[ $value != true ]]; then echo "(precise-builds is not true)"; fi
}
# `release_flags` in the same form: one flag per line, `--features=a,b`.
justfile_flags() {
  local i=0 flag
  while ((i < ${#release_flags[@]})); do
    flag=${release_flags[i]}
    if [[ $flag == --features ]]; then
      i=$((i + 1))
      flag="--features=${release_flags[i]:-}"
    fi
    echo "$flag"
    i=$((i + 1))
  done
}
dist=$(dist_flags | sort)
justfile=$(justfile_flags | sort)
if [[ $dist != "$justfile" ]]; then
  echo "error: dist-workspace.toml builds with [$(tr '\n' ' ' <<<"$dist")], the Justfile's release_flags are [$(tr '\n' ' ' <<<"$justfile")]" >&2
  status=1
else
  echo "ok: dist-workspace.toml builds with the release flags ($RELEASE_FLAGS)"
fi

# dist also reads those keys per package, from `[package.metadata.dist]` in
# Cargo.toml, over the workspace's: there they would go unchecked, so they
# stay out.
package_keys=$(awk '/^\[/ { in_dist = ($0 == "[package.metadata.dist]") } in_dist' Cargo.toml |
  grep -E '^(default-features|all-features|features|precise-builds)[[:space:]]*=' || true)
if [[ -n $package_keys ]]; then
  echo "error: Cargo.toml's [package.metadata.dist] sets $(tr '\n' ' ' <<<"$package_keys"); build keys belong in dist-workspace.toml's [dist], which is checked against release_flags" >&2
  status=1
fi

if ! cmp -s crates/domain/clippy.toml crates/app/clippy.toml; then
  echo "error: crates/domain/clippy.toml and crates/app/clippy.toml differ" >&2
  status=1
fi

# The banned-call list lives in those two clippy.toml files. Where a
# .clippy.toml sits beside one, clippy reads it instead and ignores the list.
for ring in domain app; do
  if [[ -e crates/$ring/.clippy.toml ]]; then
    echo "error: crates/$ring/.clippy.toml takes the place of crates/$ring/clippy.toml, whose banned-call list clippy would then ignore" >&2
    status=1
  fi
done

# Production code never crashes (technical design §13.5). Clippy reads the
# nearest clippy.toml (or .clippy.toml) only, so the root one and every
# crate's must all let tests unwrap, expect and panic; and every production
# crate root must deny the panicking lints. `testkit` is test support, not
# production.
no_panic_lints=(unwrap_used expect_used panic unimplemented todo unreachable)
no_panic_ok=1
for file in clippy.toml $(find . -maxdepth 1 -name .clippy.toml) $(find crates \( -name clippy.toml -o -name .clippy.toml \) | sort); do
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

# The lints Cargo.toml adds beyond `all` (technical design §13.5) take their
# thresholds and their test exemption from the nearest clippy.toml (or
# .clippy.toml), so every one must carry the same lines; none sets the
# cognitive-complexity threshold (or its old name, cyclomatic), so all keep
# clippy's default.
lint_settings=('excessive-nesting-threshold = 6' 'allow-indexing-slicing-in-tests = true')
lint_settings_ok=1
for file in clippy.toml $(find . -maxdepth 1 -name .clippy.toml) $(find crates \( -name clippy.toml -o -name .clippy.toml \) | sort); do
  for setting in "${lint_settings[@]}"; do
    if ! grep -qx "$setting" "$file"; then
      echo "error: $file lacks '$setting'" >&2
      lint_settings_ok=0
    fi
  done
  if grep -Eq '^[[:space:]]*(cognitive|cyclomatic)-complexity-threshold' "$file"; then
    echo "error: $file sets the cognitive-complexity threshold; every clippy.toml keeps clippy's default" >&2
    lint_settings_ok=0
  fi
done
# `allow_attributes` sees only outer attributes: an inner `#![allow(…)]` or
# `#![cfg_attr(…, allow(…))]` would pass clippy and go stale unnoticed.
inner_allows=$(git grep -nE '^[[:space:]]*#!\[[[:space:]]*(allow|cfg_attr\(.*[^_a-z]allow)[[:space:]]*\(' -- '*.rs' || true)
if [[ -n $inner_allows ]]; then
  echo "error: an inner allow attribute; write #![expect(…, reason = \"…\")]:" >&2
  echo "$inner_allows" >&2
  lint_settings_ok=0
fi
if ((lint_settings_ok)); then
  echo "ok: every clippy.toml sets the same nesting threshold and test exemption; no inner allow"
else
  status=1
fi

cargo build --quiet --package domain --target wasm32-unknown-unknown
echo "ok: domain builds for wasm32-unknown-unknown"

exit "$status"
