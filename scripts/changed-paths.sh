#!/usr/bin/env bash
# Sorts a pull request's changed paths into the lanes CI runs (development.md
# §13.8), with plain git, so the jobs that read the answer can skip work.
# `ci.yml`'s `changes` job runs this and appends its output to
# `$GITHUB_OUTPUT`.
#
#   scripts/changed-paths.sh BASE HEAD
#
# BASE and HEAD are commits; the paths are the pull request's (`BASE...HEAD`).
# Output on stdout, one line per lane for `$GITHUB_OUTPUT`:
#
#   docs-only=true|false   every path is docs, which skip the code suites
#   service=true|false     a path can change what the `service` jobs check
#   ui=true|false          a path can change what Firefox and WebKit see
#
# Anything that isn't clearly docs counts as code, and anything that isn't
# clearly outside the service jobs' reach counts as inside it: the jobs run
# unless this says "true" (`docs-only`) or "false" (`service`). The `ui`
# lane is the other way round: false unless a path names the UI, because
# Chromium's required job covers the rest. A failure prints nothing, and the
# jobs then run as for code. A `.feature` file is read from the checked-out
# tree, which is what the jobs run on.
set -euo pipefail

cd "$(dirname "$0")/.."

if (($# != 2)); then
  echo "usage: $0 BASE HEAD" >&2
  exit 2
fi
base=$1
head=$2

changed=$(mktemp)
trap 'rm -f "$changed"' EXIT
git diff -z --name-only --no-renames "$base...$head" > "$changed"

docs_only=true
service=false
ui=false
count=0
while IFS= read -r -d '' file; do
  count=$((count + 1))
  case "$file" in
    # Markdown under crates/ can be include_str!'d or doc-tested.
    crates/*) docs_only=false ;;
    # The guides' config.toml and keyring examples are tested
    # (crates/adapters/config/tests/guide.rs).
    docs/guide/*) docs_only=false ;;
    docs/* | LICENSE* | .github/ISSUE_TEMPLATE/* | *.md) ;;
    *) docs_only=false ;;
  esac
  # What the `service` jobs run (#995): `install-service` against the real
  # service managers. The first two arms name what they exercise; the third
  # names what they can't reach (the web UI, the UI tests and their
  # scenarios, the TypeScript tooling, docs) and is the only way to stay
  # false. Every other path, a new one too, runs the jobs.
  case "$file" in
    crates/adapters/platform/* | scripts/service-contract.sh | src/service.rs | src/main.rs) service=true ;;
    .github/workflows/* | .github/actions/* | Justfile | Cargo.lock | Cargo.toml | */Cargo.toml) service=true ;;
    crates/web/* | ui-tests/* | features/ui/* | scripts/*.ts | docs/* | LICENSE* | .github/ISSUE_TEMPLATE/* | *.md) ;;
    *) service=true ;;
  esac
  # What the Firefox and WebKit journeys (`ui-engines`) can see that
  # Chromium's can't: what the page is made of, how it is served and styled,
  # and what the browser formats (the l10n crate's `Intl` calls differ by
  # engine), signs in with and downloads. Only a path named here turns them
  # on; every other path leaves them off. A `.feature` outside features/ui/
  # counts only when it holds an `@ui` scenario, in the checked-out tree (a
  # deleted file has none).
  case "$file" in
    crates/web/* | ui-tests/* | features/ui/*) ui=true ;;
    crates/l10n/*) ui=true ;;
    crates/adapters/http/src/lib.rs | crates/adapters/http/src/response_headers.rs) ui=true ;;
    crates/adapters/http/src/sign_in.rs | crates/adapters/http/src/downloads.rs | crates/adapters/http/src/request_checks.rs) ui=true ;;
    rust-toolchain.toml | scripts/web-packages.sh | scripts/install-browser.sh) ui=true ;;
    .github/workflows/ci.yml | .github/actions/browser/* | .github/actions/npm-ci/*) ui=true ;;
    features/*.feature) if grep -Eq '@ui([[:space:]]|$)' "$file" 2> /dev/null; then ui=true; fi ;;
  esac
done < "$changed"
# A change with no files isn't a docs change, and runs the jobs.
if ((count == 0)); then docs_only=false service=true ui=true; fi

echo "docs-only=$docs_only"
echo "service=$service"
echo "ui=$ui"
