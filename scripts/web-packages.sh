#!/bin/sh
# Installs the web UI's npm packages (Tailwind's CLI) for trunk's pre-build
# hook (crates/web/Trunk.toml), which runs it in crates/web: `npm ci` when
# node_modules wasn't installed from this package-lock.json, so a lockfile a
# pull or a branch switch changed is installed before the build uses it. With
# the lockfile unchanged it does nothing and needs no network.
#
# The record of what is installed is a copy of the lockfile `npm ci` installed,
# written into node_modules once it succeeded; the two are compared by content,
# not by date. It is removed before an install, and `npm ci` deletes
# node_modules first, so an install that fails or stops part way leaves no
# record and the next build installs again. node_modules from before the record
# existed has none either and is installed once more.
#
# Two builds at once in one checkout aren't supported: they would install into
# the same node_modules, as they write the same tw-out.css and dist/.
#
# POSIX sh, for Linux and macOS; Windows isn't a build platform.
set -eu

record=node_modules/.ladderback-package-lock.json

if ! command -v npm >/dev/null 2>&1; then
  echo "error: npm is not installed: building the web UI needs Node.js and npm, to run Tailwind's CLI (crates/web/package.json)" >&2
  exit 1
fi

if cmp -s package-lock.json "$record"; then
  exit 0
fi

rm -f "$record"
npm ci --no-audit --no-fund
cp package-lock.json "$record"
# Writing the record changed node_modules after npm wrote its own record of
# the tree, which npm then distrusts as older than the folder, reading every
# package from disk instead. Bring it back up to date; -c creates nothing.
touch -c node_modules/.package-lock.json
