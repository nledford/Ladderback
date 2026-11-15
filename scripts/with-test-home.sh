#!/usr/bin/env bash
# Runs a command (a test suite) with every test process in a scratch home, and
# fails when the run left anything there. A daemon a test starts without
# folders of its own uses the platform's (`~/Library/Application
# Support/ladderback` on macOS, the XDG folders on Linux): the developer's real
# operation log, its migrations and its schema version, shared with every
# other daemon on the machine (technical design §13.5, decision 59).
#
# Cargo and nextest start each test binary through a target runner,
# scripts/test-home-runner.sh, which points `HOME` and the XDG data, config,
# cache and state folders into the scratch home and drops the
# `LADDERBACK_*_DIR` overrides. Only the test processes see the scratch home:
# changing `HOME` for Cargo itself, or for the toolchain's wrappers, rebuilds
# everything and writes to that folder. Where the platform keeps the config,
# the scratch home holds a `config.toml` from a newer schema: a daemon that
# reads the platform's config, which leaves no file behind, refuses to start
# and fails its test. After the command, anything else in the scratch home,
# or a planted file changed or gone, fails the run, after the command's own
# output, and the scratch home is kept for a look. So does a run in which the
# runner never ran (a build for another target, which takes no host runner).
# The command's exit status is kept when it failed.
#
# Usage: scripts/with-test-home.sh <command> [args…]
set -uo pipefail

cd "$(dirname "$0")/.." || exit 1
root=$PWD

# Outside the checkout, whose path may hold a space, which a runner named in
# Cargo's environment variable can't: Cargo splits its value on spaces.
scratch=$(mktemp -d "${TMPDIR:-/tmp}/ladderback-test-home.XXXXXX") || exit 1
keep=
cleanup() {
  [[ -z $keep ]] || return
  chmod -R u+rwx "$scratch" 2>/dev/null
  rm -rf "$scratch"
}
trap cleanup EXIT
# The runner makes `ran` beside the home, never in it.
home=$scratch/home
ln -s "$root/scripts/test-home-runner.sh" "$scratch/runner" || exit 1

# Where the daemon would read its `config.toml` if no override named a
# folder: the Mac's application-support folder, and the XDG config folder.
# Both on either platform, so a daemon that picked the other one shows too.
planted=(
  "Library/Application Support/ladderback/config.toml"
  ".config/ladderback/config.toml"
)
printf 'schema = 999999\n' >"$scratch/config.toml" || exit 1
for file in "${planted[@]}"; do
  mkdir -p "$home/$(dirname "$file")" || exit 1
  cp "$scratch/config.toml" "$home/$file" || exit 1
done

host=$("${RUSTC:-rustc}" -vV | sed -n 's/^host: //p')
if [[ -z $host ]]; then
  echo "with-test-home: rustc -vV named no host target" >&2
  exit 1
fi
runner_var=CARGO_TARGET_$(tr 'a-z.-' 'A-Z__' <<<"$host")_RUNNER
if [[ -n ${!runner_var:-} ]]; then
  echo "with-test-home: $runner_var is set already; the scratch home needs the runner" >&2
  exit 1
fi
export "$runner_var=$scratch/runner"
export LADDERBACK_TEST_HOME=$home

"$@"
status=$?

if [[ $status -eq 0 && ! -e $scratch/ran ]]; then
  echo "error: no test ran in the scratch home: Cargo never started $runner_var's runner." >&2
  echo "A build for another target than $host (--target, CARGO_BUILD_TARGET) takes no host runner." >&2
  exit 1
fi

# The planted files and the folders above them are not a test's doing.
expected=()
for file in "${planted[@]}"; do
  path=$file
  while [[ $path != . ]]; do
    expected+=("$path")
    path=$(dirname "$path")
  done
done
left=$(cd "$home" && find . -mindepth 1 | sed 's|^\./||' | grep -vxF -f <(printf '%s\n' "${expected[@]}") | sort)
for file in "${planted[@]}"; do
  if [[ -L $home/$file ]] || ! cmp -s "$scratch/config.toml" "$home/$file"; then
    left+=${left:+$'\n'}"$file (planted; changed or removed)"
  fi
done

if [[ -n $left ]]; then
  keep=1
  {
    echo
    echo "error: tests wrote to the platform's folders, which a developer's real daemon uses."
    echo "Every test process had $home as HOME, with XDG_DATA_HOME,"
    echo "XDG_CONFIG_HOME, XDG_CACHE_HOME and XDG_STATE_HOME inside it, and the run left there:"
    sed "s|^|  $home/|" <<<"$left"
    echo "Every daemon a test starts needs folders of its own: testkit::daemon::isolate, or"
    echo "LADDERBACK_DATA_DIR, LADDERBACK_CONFIG_DIR and LADDERBACK_CACHE_DIR set to a scratch"
    echo "folder of the test's own; a tool such as rustic needs --no-cache or a cache folder there."
    echo "The scratch home is kept; remove $scratch when done."
  } >&2
  [[ $status -ne 0 ]] || status=1
fi
exit "$status"
