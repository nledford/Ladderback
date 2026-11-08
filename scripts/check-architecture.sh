#!/usr/bin/env bash
# Enforces the hexagonal dependency rule (technical design §3.2):
#   - `domain` (host and wasm32) and `app` never depend, even indirectly, on
#     runtime, I/O, engine or presentation crates, or on `chrono` (time goes
#     through `jiff`, decision 22), with any feature enabled;
#   - `domain` builds for wasm32-unknown-unknown;
#   - the inner rings share one banned-call list (clippy.toml).
# Dev-dependencies are excluded, so tests may use a runtime. `testkit` (test
# support) may be a dev-dependency only.
set -euo pipefail

cd "$(dirname "$0")/.."

banned='^(tokio|sqlx|axum|hyper|reqwest|rustic_core|rustic_backend|fluent-bundle|fluent-langneg|unic-langid|icu_[a-z_]+|rand|chrono)$'
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

# testkit is test support: a dev-dependency only. `cargo tree --invert` lists
# the crates that depend on it through normal or build edges, which includes
# testkit itself and must include nothing else.
testkit_users=$(cargo tree --workspace --all-features --edges normal,build --invert testkit --prefix none --format '{p}' | awk '{print $1}' | sort -u | grep -vx testkit || true)
if [[ -n $testkit_users ]]; then
  echo "error: testkit is a dev-dependency only, but these depend on it:" >&2
  sed 's/^/  /' <<<"$testkit_users" >&2
  status=1
else
  echo "ok: testkit is a dev-dependency only"
fi

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
for file in clippy.toml crates/domain/clippy.toml crates/app/clippy.toml; do
  for setting in allow-unwrap-in-tests allow-expect-in-tests allow-panic-in-tests; do
    if ! grep -qx "$setting = true" "$file"; then
      echo "error: $file lacks '$setting = true'" >&2
      no_panic_ok=0
    fi
  done
done
while IFS= read -r root; do
  for lint in "${no_panic_lints[@]}"; do
    # The lint's name, not a longer one that starts with it.
    if ! grep -Eq "clippy::$lint([^_a-z]|\$)" "$root"; then
      echo "error: $root doesn't deny clippy::$lint" >&2
      no_panic_ok=0
    fi
  done
done < <(find crates src -maxdepth 5 -path '*/src/*' \( -name lib.rs -o -name main.rs \) -not -path 'crates/testkit/*' | sort)
if ((no_panic_ok)); then
  echo "ok: crate roots deny panicking code; every clippy.toml lets tests use it"
else
  status=1
fi

cargo build --quiet --package domain --target wasm32-unknown-unknown
echo "ok: domain builds for wasm32-unknown-unknown"

exit "$status"
