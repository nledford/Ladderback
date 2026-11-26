# Decision 172: Three shards per browser

**Each browser's journeys (Chromium, Firefox, WebKit) run in three shards, not two; the required `UI journeys (Chromium, axe-core, keyboard)` aggregate keeps its name. Firefox, WebKit, the release-build tests, the pull request's mutants and macOS's `Check` stay on pull requests as they are** (recorded 2026-11-26, §13.8; the maintainer's decision).

## Why

- **The journeys set the wall time.** On the full pull request runs of #1647 and #1657, a run took 26 to 30 minutes, and the browser shards set it: Firefox's took 18 to 29 minutes, WebKit's 17 to 20 and Chromium's 13 to 22, against 11 to 13 for Linux's `Check` and 19 for macOS's. A third shard cuts each to about two thirds.
- **What stays, and why.** While decision 124's MVP mode kept these jobs off pull requests (2026-11-19 to 26), `main`'s push runs failed 26 times in WebKit, 6 times in the release-build tests and twice in Firefox: each a failure a pull request would have caught. So Firefox, WebKit, the release-build tests and the mutants stay on every pull request they ran on.

## Decided

1. **Three shards.** `ui-shard` runs `just test-ui --shard k/3` for k in 1 to 3, and `ui-engines` three per engine, six jobs. The aggregate `ui` needs every Chromium shard. Shard 1 alone saves the caches on `main`. The shard names (`UI journeys (Chromium 1/3, axe-core, keyboard)` and the like) aren't required checks.
2. **Nothing else narrows.** Every job runs on the pull requests it ran on before.

## Considered and rejected

- **Firefox and WebKit on the UI's paths alone, or the release-build tests and mutants off pull requests.** The MVP-mode failures above say what that costs.
- **macOS's `Check` only when a path macOS can see changes.** GitHub runs at most 5 macOS jobs at once, and the leg took 19 minutes, so a lane that skipped it was drafted: a crate counted when any of its files named an OS at the base or the head, and every path outside a crate but docs and `ui-tests/` counted. Run over the 80 commits on `main` before 801dba411, it skipped the leg for 13, nearly all docs-only, which already skip its steps. Narrowing it to a hand-kept list of macOS paths would save more but miss a crate that gains macOS code until someone updates the list. The saving didn't pay for the risk.

## Limits

- **The job count.** A full pull request run now starts up to 9 browser jobs, three more than before, within GitHub's 20 at once.
