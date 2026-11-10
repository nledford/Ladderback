## 13. Development method

Ladderback is built behaviour first and test first. The two together are how the invariants in §0 stay true as the code grows.

### 13.1 Test first, always

**Rule:** no production code without a failing test that requires it. Every change follows red, green, refactor: write the smallest failing test that names the behaviour, make it pass with the smallest correct change, then clean up with the tests green. A bug fix starts with a test that reproduces the bug.

**Exempt:** documentation, formatting, generated code, dependency bumps with no behaviour change, and spikes. Spikes live on `spike/*` branches and are never merged; their findings are rebuilt test-first.

**How CI makes skipping it visible.** No tool can prove a test was written before the code. These checks make skipping test-first visible and costly:

1. **Red-first check**, for pull requests labelled `behaviour`. The pull request's first commit is a `test:` commit that adds the new tests and scenarios, with `todo!()` stubs for any new API so that it compiles. CI checks out that commit and runs exactly the tests it added, by name (`cargo test -- --exact`, and the new scenarios by name in both the Rust and the `@ui` runner); each one must **fail at run time**. A compile error doesn't count, because it would pass vacuously. A test that already passes specifies nothing new.
   - **Workflow and cache.** It runs in its own workflow, `.github/workflows/red-first.yml`, so adding or removing a label re-runs it alone and not the rest of CI. The base tree's build is kept in the Actions cache, keyed on the OS, the toolchain, `Cargo.lock` and the merge base, so a later run of the same pull request doesn't rebuild the dependencies, only the workspace crates. The first run of a pull request is cold, because pull requests can't read each other's caches. `just red-first <base>` runs the same script locally.
   - **Wasm tests.** `#[wasm_bindgen_test]` tests count. When the `test:` commit adds one, the script builds both trees for `wasm32-unknown-unknown`, lists each package's wasm tests, and runs the new ones by name with `wasm-bindgen-test-runner` (under Node, as `just test-wasm` does). Each must fail at run time, like a host test, and the results show in the same `red`/`GREEN` lines, marked `(wasm32)`. A pull request that adds no wasm test pays nothing. The workflow installs Node, the `wasm32` target and `wasm-bindgen-cli` at the version `Cargo.lock` pins, as `just setup` does.
   - **The UI build.** The `@ui` scenarios run against a UI built with the tree's `just web-test` recipe when its Justfile defines one, and with `trunk build --release` when it doesn't, so a tree can build the UI with the features its scenarios need.
2. **Mutation testing.** CI writes the pull request's diff against the base to a file and runs `cargo mutants --in-diff <file>` on the gated crates: `domain`, `app`, `api` (the wire contract, including each code's arguments), `l10n` (the code-to-message mapping and the formatters), `web` (the announcer's timing and the page titles; its DOM glue runs only in a browser and is excluded, covered by the `@ui` scenarios), `adapters/config` and `adapters/interop`. The Justfile's `mutants_packages` lists the ones that exist so far. A mutant that survives in changed code means behaviour no test asserts, and blocks the merge. Exceptions are marked `#[mutants::skip]` (from the `mutants` crate) or listed in `.cargo/mutants.toml` `exclude_re` or `exclude_globs`, each with a reason. A full run happens nightly.
3. **Squash merge.** The check runs on the pull request before merge, and pull requests are squash-merged, so the failing `test:` commit never lands on `main` and `git bisect` stays clean.

There is deliberately **no coverage percentage gate**. Coverage counts executed lines, not asserted behaviour; mutation testing measures the latter.

### 13.2 Behaviour first (BDD)

Every user-visible behaviour starts as a Given/When/Then scenario in `features/<context>/*.feature`, written in the ubiquitous language (§3.1) before any test or code for it. Scenarios describe outcomes, never endpoints, tables or crates.

- **Runner:** the `cucumber` crate (cucumber-rs). `crates/app/tests/acceptance.rs` (`harness = false`) runs every scenario tagged neither `@e2e` nor `@ui`. Because it lives in `app`, `cargo mutants` runs it for every `app` mutant.
- **Default level:** steps drive `app` use cases with the in-memory fakes and the fake clock, so scenarios about sleep, deadlines, leases and catch-up run in milliseconds and deterministically.
- **`@e2e` scenarios** run from the root `tests/e2e.rs` against the real `ladderback` binary with scratch repositories. Rules enforced in adapters (request checks in `http`, the deadline watchdog in the worker) are specified this way. cucumber-rs polls every scenario of the runner from one task, so a step that blocks for long (`rustic_core`, waiting for a daemon to start, thousands of file writes) runs through `blocking(…)` in `tests/e2e.rs`, which waits on a blocking thread; the runner runs 4 scenarios at once, because each backup's worker uses every core; and the run fails when the runner's thread was unavailable for more than 8 s, before the operation log's 10 s wait for a connection (#92) can run out.
- **`@ui` scenarios** drive the browser. They share `features/` with the rest, but run under `@cucumber/cucumber` (Node) with Playwright, from M0. Both Rust runners skip them. Their steps find elements only by role and accessible name (`getByRole`), so a control a screen reader can't identify can't be tested either (A2).
- **A milestone is done when its feature files pass** (§14).

Examples, one per core rule:

```gherkin
Feature: Catch-up after sleep
  Scenario: Missed hourly runs collapse into one
    Given the plan "home" runs every hour
    And the laptop slept from 09:00 to 13:30
    When the laptop wakes and the network is ready
    Then exactly one backup of "home" is queued
    And its trigger is "catch-up"

Feature: Forget only touches managed snapshots
  Scenario: Untagged history survives a forget
    Given the repository holds 40 untagged snapshots from an old cron job
    And it holds 10 snapshots managed by the plan "home"
    And the plan "home" keeps the last 3 snapshots
    When the maintenance owner runs forget
    Then 7 snapshots managed by "home" are deleted
    And no untagged snapshot is deleted
    And the repository page lists 1 unmanaged group

Feature: One maintenance owner
  Scenario: A second claim is rejected
    Given the agent "server1" owns maintenance for repository "nas"
    When the agent "laptop" asks to own maintenance for "nas"
    Then the claim is rejected
    And both machines show the rejected claim

Feature: Backup deadline
  Scenario: A backup that slept past its deadline writes no snapshot
    Given keep-delete for repository "nas" is 6 days and its prune margin is 2 hours
    And a backup of "home" started reading the index on Monday at 09:00
    When the laptop sleeps until the following Monday at 09:00
    Then the backup ends as failed with reason "deadline passed"
    And no snapshot from that backup exists

Feature: Restores from the hub
  Scenario: An agent that hasn't opted in refuses a hub restore
    Given the agent "laptop" does not allow restores from the hub
    When the hub requests a restore of a snapshot to "laptop"
    Then the restore is refused
    And no files are written on "laptop"
```

### 13.3 Test levels by ring

| Ring | Tests | Tools | Examples |
|---|---|---|---|
| `domain` | Unit and property tests | `proptest` | `ForgetSelection` partitions, `Deadline`, lease transitions, `NextRun` across DST, `WakeDetector`, `Redactor` |
| `app` | Use-case tests and BDD scenarios with fakes | cucumber-rs, fake clock | Catch-up, retries, overlap, claims, prune guard waiting, deadline kill decision, restore policy |
| Adapters | **Port contract tests** (§3.2), plus integration tests against real infrastructure | sqlx on SQLite (also on btrfs in CI), scratch rustic repos (local + one OpenDAL backend), rustic CLI golden tests, golden config files per schema version, WebSocket over loopback | Store contract and `try_start`, kill-at-random-points test, import list replacement, export round trip, config migrations, outbox replay |
| `http`, `web` | Request handling, view models and components | axum `oneshot` requests; Leptos view-model unit tests; `wasm-bindgen-test` for component structure; Playwright against the component gallery for component behavior; announcer tests (§4.5) | Host and Origin checks, auth, step-up confirmation; dashboard state from `UiEvent`s; dialog focus return; announcer merging |
| `l10n` | Catalog and formatting lanes (§8.4) | `fluent-bundle`, `fluent-pseudo`, `wasm-bindgen-test` (`just test-wasm`) | Every message ID and error code formats in `en-US` across its selector branches; `NUMBER` drives plural selectors; `NUMBER`/`DATETIME` in `de` and `ja` with both formatters |
| System | `@e2e` scenarios on the real binary; `@ui` journeys with axe-core and keyboard-only runs (§4.5) | cucumber-rs; `@cucumber/cucumber` with Playwright | First backup end to end; deadline with a real worker; hub pairing; restore into a new folder; each journey by keyboard alone |

Domain behaviour is never mocked. Doubles replace ports only: slow, external or nondeterministic boundaries.

### 13.4 Definition of done

A change is done when:

- its scenarios exist and pass;
- the red-first check passed;
- no mutants survive in its changed code in a gated crate (§13.1);
- any new port has a contract test run against both fake and real adapter;
- every invariant in §0 it touches has a test;
- for UI changes: axe-core is clean, the journey works by keyboard alone, and new components have contract tests; the milestone's screen-reader pass covers them (§4.5);
- docs are updated, and `fmt` and `clippy` are clean, which includes the robustness rules (§13.5).

### 13.5 Robustness rules

Maintainer decision, 2026-11-08 (§16, decision 21). The daemon runs unattended and owns the operation queue: a panic in it silently stops backups, and input it didn't expect is the likeliest cause of one.

1. **Never crash in non-test code.** No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or `unimplemented!` outside tests. Return typed errors through `Result` and `?`; use `let … else`, `ok_or` and `unwrap_or…` only where the fallback can't hide corruption. The one exception is a provably infallible case, written `#[allow(clippy::expect_used, reason = "why it can't fail")]` with `.expect("…")`. A poisoned mutex is `lock().unwrap_or_else(PoisonError::into_inner)` or an error, and a thread join is a `Result`.
   - **Enforcement.** Every production crate root (each `lib.rs`, `src/main.rs` and the `web` crate root; crate-level `//!` docs come first) carries `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`, and clippy lets tests use `unwrap`, `expect` and `panic!` (`allow-unwrap-in-tests`, `allow-expect-in-tests`, `allow-panic-in-tests`); `todo!`, `unimplemented!` and `unreachable!` stay denied in tests too. It is not `[workspace.lints]`, which would also reach integration tests and the cucumber runners. Clippy reads the nearest `clippy.toml` only, so the root file covers the crates without one and `crates/{domain,app}/clippy.toml` repeat the three settings; `just architecture` checks that every `clippy.toml` and every crate root agree. `testkit` is test support and has no deny. A red `test:` commit's `todo!()` stubs (§13.1) are the one place the lint doesn't hold, and the next commit removes them.
2. **Input-derived numbers are hostile.** Bytes from outside the process (the worker's stdout lines, the `JobSpec` on its stdin, `/api/v1` and other HTTP requests, `config.toml`, imported rustic profiles, later hub sync messages) never size an allocation unbounded and are never indexed unchecked. Use `get()`, not `[i]` or `[a..b]`, on input-derived offsets; checked or saturating arithmetic on sizes and counts; caps on line and message lengths; and slice strings only at char boundaries. The worker protocol's cap is in §4.1. HTTP bodies are not read into memory by the M0 UI server, and axum's `DefaultBodyLimit` (2 MiB) bounds every extractor that does; a handler that needs more states its limit.
3. **Visible test skips.** A test that can't run here, such as `chmod 000` under root or a backend tool that isn't installed, calls `testkit::skip!(reason)`, never a silent early return or a bare `eprintln!`. The macro prints `SKIPPED <module path>: <reason>` to stderr and appends it to the file named by `LADDERBACK_SKIP_LOG`. `just test`, `just test-acceptance` and `just test-e2e` run through `scripts/with-skip-log.sh`, which points that variable at a fresh file in the target directory (`$CARGO_TARGET_DIR`, default `target/`) and lists its lines after the run, also when tests fail and without changing the exit status. `just test` runs `cargo nextest` (which captures a passing test's stderr, so the log file is what makes a skip visible), the doc tests and both cucumber runners under that one log; nextest can't run the cucumber runners (`harness = false`), so `.config/nextest.toml` filters them out and `just test` runs them with `cargo test`. The same file terminates a test that hangs for 120 s; its `ci` profile retries a failing test twice, reports a test that passed on retry as flaky, runs every test despite failures and writes `target/nextest/ci/junit.xml`, which CI uploads. CI runs `just test` with `NEXTEST_PROFILE=ci`, so skips show in its log. `testkit` is a dev-dependency only (`just architecture` checks it).
4. **Parallel agents.** Each agent works in its own git worktree under `.worktrees/<slug>`, which git, cargo-mutants and Docker builds ignore, with its own `CARGO_TARGET_DIR`, its own ports (never `just serve`'s 9898) and its own scratch folder. It never runs `rm -rf` on a shared path, deletes its target directory when done, and keeps every `Cargo.toml` valid at all times: one broken manifest breaks every build in the workspace. A §16 decision row gets its number when its branch rebases before merging, the next after the highest on `main`: branches open at the same time can't see each other's rows, so a number taken earlier can clash, and the branch that merges second renumbers its row and every reference to it. The same goes for the daemons the tests start: a test that starts `ladderback serve` gives it a data and a config folder of its own with `testkit::daemon::isolate` (`LADDERBACK_DATA_DIR`, `LADDERBACK_CONFIG_DIR`), never the platform's, which a developer's own daemon uses (a shared operation log means racing migrations, `NewerSchema` on a branch older than the real database, and test writes in the real log). `tests/isolation.rs` runs each test binary that starts the daemon with `HOME` and the XDG folders pointing at a scratch folder and fails when the run leaves anything there; a new test target that starts the daemon joins its list. The `@ui` journeys start the daemon from Node in scratch folders of their own.
