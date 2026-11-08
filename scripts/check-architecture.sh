#!/usr/bin/env bash
# Enforces the hexagonal dependency rule (technical design §3.2):
#   - `domain` (host and wasm32) and `app` never depend, even indirectly, on
#     runtime, I/O, engine or presentation crates, with any feature enabled;
#   - `domain` builds for wasm32-unknown-unknown;
#   - the inner rings share one banned-call list (clippy.toml).
# Dev-dependencies are excluded, so tests may use a runtime. `testkit` (test
# support) may be a dev-dependency only.
set -euo pipefail

cd "$(dirname "$0")/.."

banned='^(tokio|sqlx|axum|hyper|reqwest|rustic_core|rustic_backend|fluent-bundle|fluent-langneg|unic-langid|icu_[a-z_]+|rand)$'
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

cargo build --quiet --package domain --target wasm32-unknown-unknown
echo "ok: domain builds for wasm32-unknown-unknown"

exit "$status"
