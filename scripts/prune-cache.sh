#!/usr/bin/env bash
# Keeps the newest Actions cache entry of each family on the default branch and
# deletes the rest: `prune-cache.sh [--keep N] FAMILY...`, a family being an
# extended regular expression over cache keys. ci.yml's `cache-prune` job runs
# it after the jobs that save those entries, so an entry is deleted only after
# its successor exists: a family never drops below its newest entry, and a run
# whose save failed deletes nothing that pull requests restore.
#
# `--keep N` sets how many entries the families after it keep (1 by default,
# until the next `--keep`). Two entries are right where two versions of
# something are in use at once, as the two runner images GitHub rolls out side
# by side, each restoring the package set of its own.
#
# Needs `gh` and `jq`, both on GitHub's hosted runners, and a token that holds
# `actions: write` in GH_TOKEN. GITHUB_REPOSITORY names the repository.
# PRUNE_REF (default refs/heads/main) is the ref whose entries are pruned.
#
# Families must not overlap: an entry that two of them match makes the script
# exit with status 2 before it deletes anything. Anchor each expression at the
# key's start, and end its stem with the separator that follows it.
set -euo pipefail

repo="${GITHUB_REPOSITORY:?prune-cache: GITHUB_REPOSITORY is not set}"
ref="${PRUNE_REF:-refs/heads/main}"

if [ $# -eq 0 ]; then
    echo "prune-cache: name at least one family, an extended regular expression over cache keys" >&2
    exit 2
fi

# The families and how many entries each keeps, read before anything is listed
# or deleted, so a bad argument deletes nothing.
families=()
keeps=()
keep=1
while [ $# -gt 0 ]; do
    if [ "$1" = "--keep" ]; then
        case "${2:-}" in
            '' | *[!0-9]* | 0)
                echo "prune-cache: --keep takes a number of entries, 1 or more" >&2
                exit 2
                ;;
        esac
        keep=$2
        shift 2
        continue
    fi
    families+=("$1")
    keeps+=("$keep")
    shift
done
if [ ${#families[@]} -eq 0 ]; then
    echo "prune-cache: --keep needs a family after it" >&2
    exit 2
fi

# One JSON document per page; -s gathers them. The `ref` filter makes the
# server list only the entries main's own runs saved, so a pull request's
# entry is never counted, nor deleted here.
caches=$(gh api --paginate "repos/$repo/actions/caches?ref=$ref&per_page=100" | jq -s '[.[].actions_caches[]]')

# An entry two families match would be pruned as part of both, and could be
# deleted as the older one of a family it is the newest of. Refuse, before any
# delete, so that a pattern that grew too wide fails the run instead.
overlapping=$(jq -r --argjson res "$(printf '%s\n' "${families[@]}" | jq -R . | jq -s .)" \
    '.[] | .key as $key | select([$res[] | select(. as $re | $key | test($re))] | length > 1) | .key' <<< "$caches")
if [ -n "$overlapping" ]; then
    echo "prune-cache: these keys match more than one family, so nothing was deleted:" >&2
    echo "$overlapping" >&2
    exit 2
fi

failed=0
unclaimed="$caches"
for i in "${!families[@]}"; do
    family=${families[$i]}
    keep=${keeps[$i]}
    matching=$(jq -c --arg re "$family" '[.[] | select(.key | test($re))] | sort_by(.created_at, .id) | reverse' <<< "$caches")
    unclaimed=$(jq -c --arg re "$family" '[.[] | select(.key | test($re) | not)]' <<< "$unclaimed")
    count=$(jq 'length' <<< "$matching")
    if [ "$count" -le "$keep" ]; then
        echo "prune-cache: $family: $count on $ref, keeping $keep, nothing to delete"
        continue
    fi
    echo "prune-cache: $family: $count on $ref, keeping the newest $keep, $(jq -r '.[0].key' <<< "$matching") first"
    while read -r id key; do
        if gh api --method DELETE "repos/$repo/actions/caches/$id" > /dev/null; then
            echo "prune-cache:   deleted $key"
        else
            echo "::warning title=Cache prune::could not delete the cache entry $key (ID $id)"
            failed=1
        fi
    done < <(jq -r --argjson keep "$keep" '.[$keep:][] | "\(.id) \(.key)"' <<< "$matching")
done

# An entry no family claims is one nothing prunes: a cache a later change added
# without a family here. Say so while it is one entry, not when the limit is hit.
if [ "$(jq 'length' <<< "$unclaimed")" -gt 0 ]; then
    echo "::warning title=Cache prune::these entries on $ref belong to no family, so nothing prunes them: $(jq -r 'map(.key) | join(", ")' <<< "$unclaimed")"
fi
exit "$failed"
