#!/usr/bin/env bash
# Estimates, from the diff alone, how many shards the pull request's mutation
# run needs (technical design §13.1), so the CI job that tells the shards
# apart needs no toolchain and no runner of its own: `ci.yml`'s `changes` job
# runs this and appends its output to `$GITHUB_OUTPUT`.
#
#   scripts/estimate-mutant-shards.sh BASE HEAD
#
# BASE and HEAD are commits; the diff is the pull request's (`BASE...HEAD`).
# Output on stdout, two lines for `$GITHUB_OUTPUT`, and one line on stderr
# that says why:
#
#   mutant-shards=[0,1]
#   mutant-shard-count=2
#
# What it counts: the lines added and deleted in the Rust source of the crates
# `just mutants` gates (the `mutants_packages` list in the Justfile, read at
# HEAD), less what cargo-mutants leaves alone: test modules (`tests.rs`, a
# `tests/` folder) and the `exclude_globs` of .cargo/mutants.toml. The
# `exclude_re` patterns drop single mutants and aren't read, so the estimate
# can only run high. A change that touches no such file has no mutants and
# gets one shard.
#
# A change to `mutants_packages` (a crate that joins the gate is mutated
# whole) or to the `mutants` and `mutants-all` recipes can change what is
# mutated, not just how much, so it gets the most shards. So does a change to
# .cargo/mutants.toml that removes or edits a line, which can bring mutants
# back; one that only adds a bare entry to a list, or a comment, can only
# take mutants away, and is sized by its lines like any other.
#
# The gate (the Justfile's list, the manifests, the exclusions) is read from
# the checked-out tree, HEAD, which is what the shards run on; the diff is
# the pull request's own.
#
# The ratio. Of the last 460 merged pull requests (those with fewer than
# 1,000 mutants), 186 changed lines of gated source. Their mutants per
# changed line had a median of 0.05 and a 90th percentile of 0.12, and 97 in
# 100 were at most 0.17. A shard of 100 mutants is then 590 lines (100 /
# 0.17): one shard per 590 lines, rounded up, at most four. The five of them
# that had more than 100 mutants and were sized by this rule got two to four
# shards, and none a shard of more than 93 mutants. development.md §13.1
# gives the rule; recompute the ratio if the gate or the exclusions change a
# lot.
set -euo pipefail

cd "$(dirname "$0")/.."

if (($# != 2)); then
  echo "usage: $0 BASE HEAD" >&2
  exit 2
fi
base=$1
head=$2

lines_per_shard=590
max_shards=4

# One line per changed gated file is read below; a path with a quote or a
# newline in it would break the loop, and the repository has none.
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.quotepath GIT_CONFIG_VALUE_0=false

merge_base=$(git merge-base "$base" "$head")

emit() {
  local shards=$1 why=$2 list=0 k
  for ((k = 1; k < shards; k++)); do list="$list,$k"; done
  echo "$why, so $shards shard(s)" >&2
  echo "mutant-shards=[$list]"
  echo "mutant-shard-count=$shards"
}

# What decides what is mutated: the gate's list and the two recipes.
mutation_setup() {
  git show "$1:Justfile" 2> /dev/null | awk '
    /^mutants_packages :=/ { print }
    /^mutants(-all)? / { show = 1; print; next }
    /^[^[:space:]]/ { show = 0 }
    show { print }' || true
}

if [[ $(mutation_setup "$merge_base") != "$(mutation_setup "$head")" ]]; then
  emit "$max_shards" "the Justfile's mutation setup changed"
  exit 0
fi
# What mutants.toml's diff can do: a removed or edited line (an exclusion
# dropped, a timeout changed) can bring mutants back; an added comment or an
# added bare entry of a list can only take them away.
toml_diff=$(git diff -U0 "$merge_base" "$head" -- .cargo/mutants.toml | tail -n +5 || true)
if printf '%s\n' "$toml_diff" | grep -v -E '^@@|^\+[[:space:]]*(#.*)?$|^\+[[:space:]]*("[^"]*"|'"'"'[^'"'"']*'"'"'),?[[:space:]]*(#.*)?$' | grep -q .; then
  emit "$max_shards" ".cargo/mutants.toml changed"
  exit 0
fi

# The gated crates' directories: the package names of the list, matched to
# the `name` of each crate's manifest.
packages=" $(mutation_setup HEAD | sed -n 's/^mutants_packages := "\(.*\)"$/\1/p' | sed 's/--package//g' | xargs) "
dirs=()
while IFS= read -r manifest; do
  name=$(git show "HEAD:$manifest" | sed -n 's/^name = "\(.*\)"$/\1/p' | head -n 1)
  case "$packages" in
    *" $name "*) dirs+=("$(dirname "$manifest")") ;;
  esac
done < <(git ls-tree -r --name-only HEAD -- crates | grep '/Cargo\.toml$' || true)
# A list that can't be read, or a name that is no crate under crates/ (a
# package at the root, a typo): say so by the largest estimate.
# shellcheck disable=SC2086 # the names are words
set -- $packages
if ((${#dirs[@]} == 0 || ${#dirs[@]} != $#)); then
  emit "$max_shards" "the Justfile's gated packages don't match the crates' manifests"
  exit 0
fi

# Regexes: a gated file, and an excluded one (a glob's `**` crosses folders,
# its `*` doesn't).
gated="^($(IFS='|'; echo "${dirs[*]}"))/src/.*\\.rs\$"
globs=$(git show "HEAD:.cargo/mutants.toml" 2> /dev/null | awk '
  /^exclude_globs = \[/ { in_list = 1; next }
  in_list && /^\]/ { exit }
  in_list { print }' | sed -n 's/^[[:space:]]*"\(.*\)",\{0,1\}$/\1/p' \
  | sed 's/[][(){}.+?^$|\\]/\\&/g; s/\*\*/#/g; s/\*/[^\/]*/g; s/#/.*/g' | paste -sd '|' -) || true
excluded="^(${globs:-^$})\$"
tests='(^|/)tests\.rs$|/tests/'

lines=0
while IFS=$'\t' read -r added deleted path; do
  [[ $added == - ]] && continue # a binary file
  [[ $path =~ $gated ]] || continue
  [[ $path =~ $excluded ]] && continue
  [[ $path =~ $tests ]] && continue
  lines=$((lines + added + deleted))
done < <(git diff --numstat --no-renames "$base...$head" -- crates)

shards=$(((lines + lines_per_shard - 1) / lines_per_shard))
((shards < 1)) && shards=1
((shards > max_shards)) && shards=$max_shards
emit "$shards" "$lines changed lines in the gated crates' source"
