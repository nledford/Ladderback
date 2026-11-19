# Decision 124: How CI runs on pull requests until the MVP

**Until every milestone except M7 (Japanese) is implemented, a pull request runs a minimal CI when the repository variable `CI_MODE` is `mvp`: the pull request title check and `Check (ubuntu-latest)` always, the Chromium UI journeys when its paths reach the UI, the workflow lint and the dependency check only when what they read changed. Pushes to `main`, the nightly run and manual runs always run everything, and the maintainer's merge train, which pauses merging when a required job fails on `main`, is the safety net. Setting `CI_MODE` to anything else, or deleting it, restores today's full run. The ruleset doesn't change** (recorded 2026-11-18, §13.8; the maintainer's decision).

## Why

A pull request's CI takes a median of 83 minutes, and most of it is waiting for runners: macOS has the smallest pool. A pull request has also passed local checks before it opens: the quick lane of AGENTS.md, and the maintainer's own run of `just quick-ci`, red-first, the mutants and the UI journeys. So the full run mostly repeats work that has already passed, at the price of the queue.

The maintainer accepts the cost: a breakage that only macOS, the release build, Firefox or WebKit shows is found after the merge, not before it.

## Decided

1. **One variable switches it.** `CI_MODE` is a repository variable. Only the value `mvp` (compared as an Actions expression compares strings, without regard to letter case) is MVP mode; unset, `full` or any other value is full mode, as before this decision.
2. **It applies to pull requests alone.** The decision is made in one place, the `mvp` output of ci.yml's `Changed paths` job, which runs on pull requests only. A push to `main`, the nightly run and a manual run skip that job, so they never see `mvp` and run every job. `red-first.yml` runs on pull requests only and reads the same variable.
3. **What a pull request runs in MVP mode.**
   - Always: `Changed paths`, `Pull request title` and `Check (ubuntu-latest)`.
   - Only when its paths reach the UI (the `ui` output of `Changed paths`, the rule that already starts the Firefox and WebKit journeys in full mode): `UI journeys (Chromium, axe-core, keyboard)`. Otherwise it is skipped, which counts as passing; `Check (ubuntu-latest)` still runs the unit, acceptance and `@e2e` suites.
   - Only when a workflow or action file, `.github/actionlint.yaml` or `scripts/install-actionlint.sh` changed: `Workflow lint (actionlint, zizmor)`.
   - Only when `Cargo.lock`, a `Cargo.toml` or `deny.toml` changed: `Dependencies (cargo-deny)`.
   - A docs-only pull request still skips what it skips today.
4. **What it skips.** `Check (macos-latest)`, `Release-build tests (no default features)`, the four mutation shards and `Mutation testing (changed code)`, `Red-first check`, the Firefox and WebKit journeys, `Service managers (launchd)` and `(systemd)`, and `Wasm tests (Node)`.
5. **A skipped required check still reports.** GitHub counts a check whose job a job-level `if` skipped as passing, so the ruleset's required names (§13.8) all still report and no merge is blocked. The matrix job `check` can't be skipped as a whole, since its legs' checks would never be created, so in MVP mode its macOS leg stays, moves to a Linux runner and runs no step: it reports `Check (macos-latest)` as passing within seconds, without waiting for a macOS runner. `Mutation testing (changed code)`, which would fail on its skipped shards' result, is skipped itself.
6. **A failure of `Changed paths` runs everything.** Its outputs are empty then, and every condition above reads empty as full mode.

## What is lost, and how it is covered

- **macOS-only, release-build-only, Firefox- and WebKit-only breakage,** what the service managers and the wasm tests find, and what a change outside the UI's paths breaks in the Chromium journeys, shows after the merge. It is covered by `main`'s run after each merge, which runs all of those jobs, and by the merge train's pause on a required job that fails on `main`.
- **Red-first and the mutants** have run in the maintainer's local checks before a pull request opens, which AGENTS.md's quick lane doesn't require of every author (it asks for red-first on a `behaviour` branch, not for the mutants). In MVP mode CI doesn't run red-first at all, and runs the mutants only in the nightly full run, a day after the merge at the latest.
- **The risk the maintainer accepts:** a break lands on `main`, is found by its run, and costs a fix or revert pull request.

## Exit condition

MVP mode is turned off when every milestone except M7 has been implemented (§14; M10 "Polish" is the last of them). The maintainer then sets `CI_MODE` to `full` or deletes it. No file changes: the jobs and conditions stay, and with the variable gone they behave as before.

## Not decided here

The ruleset, the required check names and the jobs' contents. This decision changes which jobs run on a pull request, nothing else about them.

## Rejected

- **Skipping the macOS leg with a job-level `if`:** the matrix's per-leg check would never be created, and the merge would wait for it forever.
- **A second job named `Check (macos-latest)`:** in full mode two checks would carry the name.
- **Dropping the required checks from the ruleset for the period:** a ruleset change for what a variable does, and the names would stop protecting `main` after the exit.
