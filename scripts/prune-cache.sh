#!/usr/bin/env bash
# Keeps one Actions cache entry per family on the default branch: for each
# family (an extended regular expression over cache keys) named on the command
# line, it deletes every entry of main but the newest. ci.yml's `cache-prune`
# job runs it after the jobs that save those entries, so an entry is deleted
# only after its successor exists: a family never drops below one entry, and a
# run whose save failed deletes nothing of what the pull requests restore.
#
# Needs `gh` and `jq`, both on GitHub's hosted runners, and a token that holds
# `actions: write` in GH_TOKEN. GITHUB_REPOSITORY names the repository.
# PRUNE_REF (default refs/heads/main) is the ref whose entries are pruned.
#
# Families must not overlap: an entry that two of them match would be counted
# in both, and the older of two entries could stay. Anchor each expression at
# the key's start, and end its stem with the separator that follows it.
set -euo pipefail

if [ $# -eq 0 ]; then
    echo "prune-cache: name at least one family, an extended regular expression over cache keys" >&2
    exit 2
fi
repo="${GITHUB_REPOSITORY:?prune-cache: GITHUB_REPOSITORY is not set}"
ref="${PRUNE_REF:-refs/heads/main}"

# One JSON document per page; -s gathers them. The `ref` filter makes the
# server list only the entries main's own runs saved, so a pull request's
# entry is never counted, nor deleted here.
caches=$(gh api --paginate "repos/$repo/actions/caches?ref=$ref&per_page=100" | jq -s '[.[].actions_caches[]]')

failed=0
unclaimed="$caches"
for family in "$@"; do
    matching=$(jq -c --arg re "$family" '[.[] | select(.key | test($re))] | sort_by(.created_at, .id) | reverse' <<< "$caches")
    unclaimed=$(jq -c --arg re "$family" '[.[] | select(.key | test($re) | not)]' <<< "$unclaimed")
    count=$(jq 'length' <<< "$matching")
    if [ "$count" -le 1 ]; then
        echo "prune-cache: $family: $count on $ref, nothing to delete"
        continue
    fi
    echo "prune-cache: $family: $count on $ref, keeping $(jq -r '.[0].key' <<< "$matching")"
    while read -r id key; do
        if gh api --method DELETE "repos/$repo/actions/caches/$id" > /dev/null; then
            echo "prune-cache:   deleted $key"
        else
            echo "::warning title=Cache prune::could not delete the cache entry $key (ID $id)"
            failed=1
        fi
    done < <(jq -r '.[1:][] | "\(.id) \(.key)"' <<< "$matching")
done

# An entry no family claims is one nothing prunes: a cache a later change added
# without a family here. Say so while it is one entry, not when the limit is hit.
if [ "$(jq 'length' <<< "$unclaimed")" -gt 0 ]; then
    echo "::warning title=Cache prune::these entries on $ref belong to no family, so nothing prunes them: $(jq -r 'map(.key) | join(", ")' <<< "$unclaimed")"
fi
exit "$failed"
