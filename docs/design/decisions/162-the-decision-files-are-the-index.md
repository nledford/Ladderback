# Decision 162: The decision files are the index, and §16 keeps no table of them

**§16 of the roadmap holds no table of the decisions. Each decision is a file in `docs/design/decisions/`, named `NN-<slug>.md` with `# Decision NN: <question>` as its heading, and the files are the index. `just decision-index` prints the table to standard output, and `just lint` checks the files: each name, heading and number, no two files with one number, no conflict marker, and no row linking a decision in §16 again (`scripts/decision-index.ts`).**

## Why

The table was generated from the files by `just decision-index` and committed. Every pull request that recorded a decision added its row at the table's end, so any two such pull requests open together conflicted at the same line: in the 7 days to 2026-11-25, 43 of the roadmap's 44 commits were index rows. The conflict resolved the same way every time (take either side, run the recipe), but it still stopped GitHub's update of the branch, and with it the merge train, until someone merged by hand. Several agents record decisions on branches open at the same time (§13.5), so this cost came back with nearly every merge.

The files already held everything the table did: the number in the name and the heading, and the question in the heading. Removing the committed copy removes the shared line; resolving its conflicts by tooling would have kept both the conflicts and a tool to maintain.

## Decided

1. **No table in §16.** §16 says where the decisions are, how to list them, and when the early ones were recorded.
2. **`just decision-index` prints the table** in number order, a row per decision, a superseded one marked so, as the committed table was.
3. **`just lint` keeps checking the files,** as it did: a bad name or heading, a conflict marker and a number two files share each fail it. The committed table also made two branches that took one number conflict, which kept the second from merging; without it their paths differ, so §13.8 now counts a branch that adds a decision file as sharing a file with `main` whenever `main` has added one since their merge base. That branch is updated, `just lint` sees both files, and the one that merges second renumbers (§13.5, rule 8). The merge train follows the same rule.
4. **`just lint` also refuses, in `roadmap.md`,** a conflict marker anywhere, a §16 heading it can't find (so the check can't pass by not running), and any table line in §16 that links a decision, so a branch opened before this change, which still writes the table, can't bring it back in a merge.

## What it costs

- **No one-page list of the decisions on GitHub.** The folder lists the files by name, sorted as text (`100-…` before `11-…`); `just decision-index` prints the list in number order.
- **A branch opened before this change** conflicts once in §16 when it is updated from `main`: delete the table on its side.
