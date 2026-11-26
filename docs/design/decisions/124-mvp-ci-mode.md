# Decision 124: How CI runs on pull requests until the MVP

**Until the MVP milestone (§14) is done, a pull request runs a minimal CI when the repository variable `CI_MODE` is `mvp`: the pull request title check and `Check (ubuntu-latest)` always, the Chromium UI journeys when its paths reach the UI, the workflow lint and the dependency check only when what they read changed. Pushes to `main`, the nightly run and manual runs always run everything, and the maintainer's merge train, which pauses merging when a required job fails on `main`, is the safety net. Setting `CI_MODE` to anything else, or deleting it, restores today's full run. The ruleset doesn't change** (recorded 2026-11-18, §13.8; the maintainer's decision).

**Amended 2026-11-22 (the maintainer, #1097):** MVP mode ends when the milestone "MVP: Ready for real-world testing" is done, not when every milestone except M7 (Japanese) is implemented, as first decided. The goal is now an MVP that works and can start real-world testing before a proper `1.0.0`; M4, M5 and M8 to M10 follow it, before `1.0.0`, with full CI on pull requests again (§14). Turning full CI back on (#1097) is one of that milestone's issues.

**Superseded 2026-11-26 (#1097):** MVP mode is gone, not just switched off. ci.yml's `mvp` output, the `workflows` and `deps` outputs that only it read, the `CI_MODE` reads in ci.yml and red-first.yml, and every job condition and step guard that narrowed a pull request's run are deleted, so each job runs on pull requests as it did before this decision, `Red-first check` included, whatever the variable says. The maintainer deletes the variable after the merge. The rest of this record is kept as it was decided.

## Why

A pull request's CI takes a median of 83 minutes, and most of it is waiting for runners: macOS has the smallest pool. A pull request has also passed local checks before it opens: the quick lane of AGENTS.md, and the maintainer's own run of `just quick-ci`, red-first and the Chromium UI journeys. So the full run mostly repeats work that has already passed, at the price of the queue.

The maintainer accepts the cost: a breakage that only macOS, the release build, Firefox or WebKit shows is found after the merge, not before it.

## Decided

1. **One variable switches it.** `CI_MODE` is a repository variable. Only the value `mvp` (compared as an Actions expression compares strings, without regard to letter case) is MVP mode; unset, `full` or any other value is full mode, as before this decision.
2. **It applies to pull requests alone.** The decision is made in one place, the `mvp` output of ci.yml's `Changed paths` job, which runs on pull requests only. A push to `main`, the nightly run and a manual run skip that job, so they never see `mvp` and run every job. `red-first.yml` runs on pull requests only and reads the same variable.
3. **What a pull request runs in MVP mode.**
   - Always: `Changed paths`, `Pull request title` and `Check (ubuntu-latest)`.
   - Only when its paths reach the UI (the `ui` output of `Changed paths`, the rule that already starts the Firefox and WebKit journeys in full mode): `UI journeys (Chromium, axe-core, keyboard)`. Otherwise it is skipped, which counts as passing; `Check (ubuntu-latest)` still runs the unit, acceptance and `@e2e` suites.
   - Only when a workflow or action file, `.github/actionlint.yaml`, `scripts/install-actionlint.sh` or `scripts/install-shellcheck.sh` changed: `Workflow lint (actionlint, zizmor)`.
   - Only when `Cargo.lock`, a `Cargo.toml`, `deny.toml`, an npm `package.json` or `package-lock.json` below the root, or `ui-tests/.nvmrc` changed: `Dependencies (cargo-deny)`.
   - A docs-only pull request still skips what it skips today.
4. **What it skips.** `Check (macos-latest)`, `Release-build tests (no default features)`, the four mutation shards and `Mutation testing (changed code)`, `Red-first check`, the Firefox and WebKit journeys, `Service managers (launchd)` and `(systemd)`, and `Wasm tests (Node)`.
5. **A skipped required check still reports.** GitHub counts a check whose job a job-level `if` skipped as passing, so the ruleset's required names (§13.8) all still report and no merge is blocked. The matrix job `check` can't be skipped as a whole, since its legs' checks would never be created, so in MVP mode its macOS leg stays, moves to a Linux runner and runs no step: it reports `Check (macos-latest)` as passing within seconds, without waiting for a macOS runner. `Mutation testing (changed code)`, which would fail on its skipped shards' result, is skipped itself.
6. **A failure of `Changed paths` runs everything.** Its outputs are empty then, and every condition above reads empty as full mode.
7. **A change to CI itself runs in full mode** (added 2026-11-22, the maintainer's decision). When a pull request changes a file under `.github/workflows/`, `.github/actions/` or `scripts/`, or the `Justfile`, `Changed paths` sets `mvp` to false whatever the variable says, so the jobs the change may alter run before the merge. #1295 showed the gap: it changed the release-build tests, in the `Justfile` and a script only that job runs, which MVP mode skipped on its pull request, and they first failed on `main`. `Red-first check` still follows the variable alone, since red-first.yml reads it directly; a pull request that changes CI is rarely a `behaviour` one.

## What is lost, and how it is covered

- **macOS-only, release-build-only, Firefox- and WebKit-only breakage,** what the service managers and the wasm tests find, and what a change outside the UI's paths breaks in the Chromium journeys, shows after the merge. It is covered by `main`'s run after each merge, which runs all of those jobs, and by the merge train's pause on a required job that fails on `main`.
- **Red-first** has run in the maintainer's local checks before a pull request opens, which AGENTS.md's quick lane asks for on a `behaviour` branch; in MVP mode CI doesn't run it at all.
- **The mutants** run nowhere before the merge: not on the pull request, and not locally (the maintainer's decision, 2026-11-19), so a change can merge with behavior no test asserts. The nightly full run tests every mutant, a day after the merge at the latest, and a surviving one is fixed then. It runs in 32 shards, at most 12 at once, `Mutation testing (full, shard k of 32)`: one runner ran out of its 180 minutes every night, and a timeout ended the job as canceled, which reported nothing. A shard's step now has a shorter limit than its job, so running out of time is a failure, and a failed shard of a scheduled run opens an issue, or comments on the open one; a manual run's failure is for whoever started it (2026-11-22).
- **The risk the maintainer accepts:** a break lands on `main`, is found by its run, and costs a fix or revert pull request.

## Exit condition

MVP mode is turned off when the milestone "MVP: Ready for real-world testing" is done (§14; amended 2026-11-22, the maintainer's decision, #1097: it was when every milestone except M7 had been implemented). The maintainer then sets `CI_MODE` to `full` or deletes it. No file changes: the jobs and conditions stay, and with the variable gone they behave as before.

## Not decided here

The ruleset, the required check names and the jobs' contents. This decision changes which jobs run on a pull request, nothing else about them.

## Rejected

- **Skipping the macOS leg with a job-level `if`:** the matrix's per-leg check would never be created, and the merge would wait for it forever.
- **A second job named `Check (macos-latest)`:** in full mode two checks would carry the name.
- **Dropping the required checks from the ruleset for the period:** a ruleset change for what a variable does, and the names would stop protecting `main` after the exit.
