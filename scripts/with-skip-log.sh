#!/usr/bin/env bash
# Runs a command with STEADING_SKIP_LOG pointing at a fresh file, then lists
# the tests that called `testkit::skip!` during the run, so a skip shows in
# CI logs instead of passing silently (technical design §13.5). The listing
# comes last, also when the command fails, and the command's exit status is
# kept.
#
# Usage: scripts/with-skip-log.sh <command> [args…]
set -uo pipefail

cd "$(dirname "$0")/.."

# Absolute, because test binaries run in their package's directory.
target=${CARGO_TARGET_DIR:-target}
mkdir -p "$target"
target=$(cd "$target" && pwd)
log=$(mktemp "$target/skipped-tests.XXXXXX")
trap 'rm -f "$log"' EXIT
export STEADING_SKIP_LOG=$log

"$@"
status=$?

count=$(wc -l <"$log" | tr -d ' ')
echo
if [[ $count -eq 0 ]]; then
  echo "Skipped tests: none"
else
  echo "Skipped tests ($count):"
  sed 's/^/  /' "$log"
fi
exit "$status"
