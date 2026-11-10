#!/usr/bin/env bash
# Checks that every repository path the agent guidance names exists, so an
# agent following AGENTS.md or CLAUDE.md never opens a file that moved or was
# never there. Arguments are the files to check (default: AGENTS.md CLAUDE.md).
#
# A path is a relative Markdown link target, an import line (`@AGENTS.md`), or
# a `code span` that contains a slash and whose first segment is an entry at
# the repository root (`crates/…`, `scripts/…`, `.cargo/…`). A span whose
# first segment is a directory under `crates/` (`adapters/engine`) is a
# shortened path, which an agent can't open, and fails too. Other spans, such
# as `origin/main`, `/api/v1` or `~/.cache/trunk`, aren't repository paths.
# Spans with a placeholder or a pattern (`<slug>`, `{domain,app}`, `*`, `…`)
# are skipped: they name a family of paths, not one. Paths resolve from the
# repository root, where both documents live.
set -euo pipefail

cd "$(dirname "$0")/.."

if (($# == 0)); then
  set -- AGENTS.md CLAUDE.md
fi

status=0
for doc in "$@"; do
  if [[ ! -f $doc ]]; then
    echo "error: $doc doesn't exist" >&2
    status=1
    continue
  fi
  # `|| true`: a document with no candidates is fine.
  links=$(grep -oE '\]\([^)#[:space:]]+' "$doc" | sed 's/^](//' | grep -vE '^[a-z]+:' || true)
  imports=$(grep -E '^@[^[:space:]]+$' "$doc" | sed 's/^@//' || true)
  spans=$(grep -oE '`[^`[:space:]]*/[^`[:space:]]*`' "$doc" | tr -d '`' | grep -vE '[<>{}*…]' || true)
  candidates=$(
    {
      if [[ -n $links ]]; then printf '%s\n' "$links"; fi
      if [[ -n $imports ]]; then printf '%s\n' "$imports"; fi
      while IFS= read -r span; do
        first=${span%%/*}
        if [[ -z $span || $first == . || $first == .. || -z $first ]]; then
          continue
        elif [[ -e $first ]]; then
          printf '%s\n' "$span"
        elif [[ -d crates/$first ]]; then
          # Spans hold no spaces, so this tag can't be a path.
          printf 'shortened %s\n' "$span"
        fi
      done <<<"$spans"
    } | sort -u
  )
  while IFS= read -r path; do
    [[ -n $path ]] || continue
    if [[ $path == "shortened "* ]]; then
      echo "error: $doc names ${path#shortened }; write the full path, crates/${path#shortened }" >&2
      status=1
    elif [[ ! -e $path ]]; then
      echo "error: $doc names $path, which doesn't exist" >&2
      status=1
    fi
  done <<<"$candidates"
done

if ((status == 0)); then
  echo "ok: every repository path in $* exists"
fi
exit "$status"
