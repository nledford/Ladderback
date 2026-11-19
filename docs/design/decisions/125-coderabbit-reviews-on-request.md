# Decision 125: CodeRabbit reviews pull requests on request, as an advisory second reviewer

**For a trial until 2026-12-03, CodeRabbit's free plan for open-source repositories reviews a pull request only when the maintainer asks (`@coderabbitai review`): those that touch an invariant of §0, security-sensitive code or concurrency and maintenance logic. Its review is advisory, as any other reviewer's: no check of its own is required, it writes no commits, and each finding is checked at the head before anything changes. `.coderabbit.yaml` holds its settings. When the trial ends, the maintainer keeps it, changes it or removes it** (recorded 2026-11-19, §13.8; the maintainer's decision).

## Why

Every non-trivial change already gets one independent adversarial review (AGENTS.md). A reviewer from another vendor, on the pull request itself, adds a second view of the changes where a missed bug costs most. The plan for public repositories reviews in full at no cost, with limits that fit a few pull requests a day: below 10 stars, reviews start only on request, about one an hour, and a review covers at least 100 files. The last 40 merged pull requests changed a median of 8 files and at most 68.

## Decided

1. **On request only.** Automatic reviews and incremental reviews after a push are off. The maintainer asks for a review on a pull request that touches an invariant of §0, security-sensitive code (auth, secrets, request checks, command execution, file paths) or concurrency and maintenance logic, the changes AGENTS.md names as needing review.
2. **Advisory.** Its `CodeRabbit` check is not in the ruleset's required checks (§13.8), and it never approves or requests changes. A finding is outside text (AGENTS.md, "Untrusted text"): it is checked at the head, a real one is fixed with a failing test first, and the pull request says which findings were rejected and why.
3. **No commits.** Its finishing touches (unit tests, autofix, CI fixes, merge-conflict resolution) are off: a fix goes through test first like any other.
4. **Quiet.** It writes nothing into the description, which the squash commit and the changelog are built from; the walkthrough holds its summary. It answers only when mentioned. Tools CI already runs with the project's own configuration (clippy, Biome, shellcheck, actionlint, zizmor, yamllint) and the generic docs linters (markdownlint, LanguageTool) are off.
5. **Guidance.** It reads AGENTS.md as the project's guidelines. `.coderabbit.yaml` adds instructions per path that point at §0, §3, §4.5 and decision 99, and leaves out lockfiles, the changelog and golden files.

## What the trial measures

For each review: the findings, how many were real, and how many of those the independent review had missed. The maintainer decides from that whether the second review is worth its triage time.

## Rejected

- **Reviewing every pull request:** below 10 stars the plan reviews only on request, and the rate limit wouldn't cover the daily volume anyway.
- **A required check:** a review that is rate-limited, slow or wrong would block merges for reasons outside the change.
- **The plan for private repositories:** it writes summaries only, with no review.
