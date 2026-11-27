#!/usr/bin/env bash
# Sorts a pull request's changed paths into the lanes CI runs (development.md
# §13.8), with plain git, so the jobs that read the answer can skip work.
# `ci.yml`'s `changes` job runs this with the pull request's labels and
# appends the output to `$GITHUB_OUTPUT`.
#
#   scripts/changed-paths.sh BASE HEAD [LABEL…]
#
# BASE and HEAD are commits; the paths are the pull request's (`BASE...HEAD`).
# Output on stdout, one line per lane for `$GITHUB_OUTPUT`:
#
#   docs-only=true|false   every path is docs, which skip the code suites
#   guide=true|false       a path is in the user guide, docs/guide/
#   service=true|false     a path can change what the `service` jobs check
#   ui=true|false          a path can change what Firefox and WebKit see
#   mutants=true|false     the pull request's mutation testing runs
#
# Anything that isn't clearly docs counts as code, and anything that isn't
# clearly outside the service jobs' reach counts as inside it: the jobs run
# unless this says "true" (`docs-only`) or "false" (`service`, `mutants`).
# The `ui` lane is the other way round: false unless a path names the UI,
# because Chromium's required job covers the rest. The user guide is docs,
# but its config.toml and keyring examples, and its list of MCP tools, are
# tested (the `guide` test targets, crates/adapters/config/tests/guide.rs and
# crates/adapters/mcp/tests/guide.rs): a docs-only change that touches it
# runs those tests, and a change to code runs them with the rest. A guide page
# the web UI links to counts as code, since a UI journey checks the heading
# the link leads to (features/ui/pairing_dialog.feature), and so do the
# site's theme scripts, which Biome checks in `just lint`. A failure prints
# nothing, and the jobs then run as for code. A `.feature` file, and the web
# UI's links, are read from the checked-out tree, which is what the jobs run
# on.
#
# The labels (decision 171): `no-behavior-change` turns the pull request's
# mutation testing off, as a docs-only change does. Beside `behaviour` it is a
# contradiction: the script prints nothing, says so on stderr and fails, and
# so does the `changes` job, which fails the required mutation check.
set -euo pipefail

cd "$(dirname "$0")/.."

if (($# < 2)); then
  echo "usage: $0 BASE HEAD [LABEL…]" >&2
  exit 2
fi
base=$1
head=$2
shift 2

no_behavior_change=false
red_first=false
# Case-insensitive, as red-first.yml's `contains()` is.
for label in "$@"; do
  case "$(printf '%s' "$label" | tr '[:upper:]' '[:lower:]')" in
    no-behavior-change) no_behavior_change=true ;;
    'behaviour') red_first=true ;;
  esac
done
if [[ $no_behavior_change == true && $red_first == true ]]; then
  echo "The labels 'no-behavior-change' and 'behaviour' contradict each other: a pull request that changes behavior has its mutants tested. Remove one of them." >&2
  exit 1
fi

changed=$(mktemp)
trap 'rm -f "$changed"' EXIT
git diff -z --name-only --no-renames "$base...$head" > "$changed"

docs_only=true
guide=false
service=false
ui=false
count=0
while IFS= read -r -d '' file; do
  count=$((count + 1))
  case "$file" in
    # Markdown under crates/ can be include_str!'d or doc-tested.
    crates/*) docs_only=false ;;
    docs/guide/theme/*.js) docs_only=false ;;
    docs/guide/*)
      guide=true
      if grep -rqF -- "$file" crates/web/src features/ui 2> /dev/null; then docs_only=false; fi
      ;;
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
mutants=true
if [[ $docs_only == true || $no_behavior_change == true ]]; then mutants=false; fi

echo "docs-only=$docs_only"
echo "guide=$guide"
echo "service=$service"
echo "ui=$ui"
echo "mutants=$mutants"
