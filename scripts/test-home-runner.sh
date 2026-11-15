#!/bin/sh
# Cargo's target runner while scripts/with-test-home.sh runs a command: Cargo
# and nextest start every test binary through it, the cucumber runners and
# the doc tests too. It points the platform's folders into the scratch home
# that script made, drops the overrides that name Ladderback's own folders
# before the platform's, then runs the test binary as given.
#
# Usage (by Cargo): scripts/test-home-runner.sh <test binary> [args…]
home=${LADDERBACK_TEST_HOME:?scripts/test-home-runner.sh runs under scripts/with-test-home.sh}
export HOME="$home"
export XDG_DATA_HOME="$home/.local/share"
export XDG_CONFIG_HOME="$home/.config"
export XDG_CACHE_HOME="$home/.cache"
export XDG_STATE_HOME="$home/.local/state"
# A caller that has these set would hide a daemon that ignores them.
unset LADDERBACK_CONFIG_DIR LADDERBACK_DATA_DIR LADDERBACK_CACHE_DIR
# Beside the home, never in it: the wrapper's proof that a test ran here.
: >"${home%/*}/ran"
exec "$@"
