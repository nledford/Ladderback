## 13. Development method

Ladderback is built behaviour first and test first. The two together are how the invariants in §0 stay true as the code grows.

### 13.1 Test first, always

**Rule:** no production code without a failing test that requires it.

Every change follows red, green, refactor:

1. write the smallest failing test that names the behaviour;
2. make it pass with the smallest correct change;
3. clean up with the tests green.

A bug fix starts with a test that reproduces the bug.

**Exempt:** documentation, formatting, generated code, dependency bumps with no behaviour change, and spikes. Spikes live on `spike/*` branches and are never merged; their findings are rebuilt test-first.

**How CI makes skipping it visible.** No tool can prove a test was written before the code. These checks make skipping test-first visible and costly:

1. **Red-first check**, for pull requests labelled `behaviour`.
   - **What it runs.** The pull request's first commit is a `test:` commit that adds the new tests and scenarios, with `todo!()` stubs for any new API so that it compiles. CI checks out that commit and runs exactly the tests it added, by name (`cargo test -- --exact`, and the new scenarios by name in both the Rust and the `@ui` runner).
   - **Each one must fail at run time.** A compile error doesn't count, because it would pass vacuously. A test that already passes specifies nothing new.
   - **Why each scenario is red.** Under each red scenario the log prints `failed at:`, its first failing step and the error's message on one line, cut at 200 characters, so a reviewer can tell the expected assertion from a page that never loaded or a server error. A report the script can't read prints `no failing step in the runner's output`; the scenario still counts as red.
   - **An undefined step.** A step with no definition is red in the Rust runners and shows as such; in the `@ui` runner it is reported as undefined, which is not a failure, so the check says `GREEN`.
   - **Its own workflow.** It runs in its own workflow, `.github/workflows/red-first.yml`, so adding or removing a label re-runs it alone and not the rest of CI. In MVP mode (§13.8) it doesn't run on pull requests, and the required check reports as skipped.
   - **Build reuse.** The base tree's dependencies come from the latest build of `main` that CI's build cache holds, as compiler outputs in mbx's store (since #1076 CI leaves the script's `RED_FIRST_SEED` unset; **pending measurement**, and `RED_FIRST_SEED=target` goes back if copying the restored `target/` is faster), and the `test:` commit's tree starts from a copy of the base tree's. So neither builds the dependencies again, only what changed since that build of `main` and the workspace crates. `just red-first <base>` runs the same script locally, where the base tree's build stays in `target/red-first` from one run to the next.
   - **Wasm tests.** `#[wasm_bindgen_test]` tests count. When the `test:` commit adds one, the script builds both trees for `wasm32-unknown-unknown`, lists each package's wasm tests, and runs the new ones by name with `wasm-bindgen-test-runner` (under Node, as `just test-wasm` does). Each must fail at run time, like a host test, and the results show in the same `red`/`GREEN` lines, marked `(wasm32)`.
   - **What wasm costs.** A pull request that adds no wasm test pays nothing. The workflow installs Node, the `wasm32` target and `wasm-bindgen-cli` at the version `Cargo.lock` pins, as `just setup` does.
   - **The UI build.** The `@ui` scenarios run against a UI built with the tree's `just web-test` recipe when its Justfile defines one, and with `trunk build --release` when it doesn't, so a tree can build the UI with the features its scenarios need.
   - **Every engine for `@ui`.** Each new `@ui` scenario runs in Chromium, Firefox and WebKit (`LADDERBACK_UI_BROWSER`, §13.2; [decision 97](decisions/97-red-first-in-every-engine.md)), so a bug only one engine shows can be red. Chromium stays primary: a scenario that fails there is red, and the required `UI journeys` job proves it passes at the head.
   - **Red outside Chromium only.** No required job runs Firefox or WebKit, so a scenario that fails only in them counts as red only if it also passes in that engine at the pull request's head. The script then moves the `test:` commit's tree to the head, builds its UI and binary again, and runs it there. Otherwise a scenario Chromium finds nothing wrong with, failing in Firefox for a reason the branch never fixes, would pass for red. The log shows each engine under the scenario's `red` or `GREEN` line (`firefox: red, failed at: …`, `firefox at the head: passes`).
   - **What the engines cost.** The workflow installs all three engines from the caches `.github/actions/browser` keeps for that set, which ci.yml's `Browser cache for the red-first check` job saves on each push to `main` (a pull request can restore only `main`'s caches), so it takes seconds per engine. Each new `@ui` scenario starts the runner three times; the head build happens only for a scenario red outside Chromium alone.
   - **Older trees, the rules, and macOS.** A tree whose `world.ts` predates `LADDERBACK_UI_BROWSER` runs in Chromium alone, and the log says so. The verdict's rules live in `scripts/red-first-ui.ts`, tested by `node --test` in `just lint`. WebKit on macOS moves focus to buttons and links with Option+Tab, not Tab, unlike Linux CI, so a local run's WebKit lines can differ from CI's.
2. **Mutation testing.** CI writes the pull request's diff against the base to a file and runs `cargo mutants --in-diff <file>` on the gated crates. A mutant that survives in changed code means behaviour no test asserts, and blocks the merge.
   - **The gated crates.** The Justfile's `mutants_packages` lists them:
     - `domain`;
     - `app`;
     - `api` (the wire contract, including each code's arguments);
     - `l10n` (the code-to-message mapping and the formatters);
     - `web` (the announcer's timing and the page titles; its DOM glue runs only in a browser and is excluded, covered by the `@ui` scenarios);
     - `adapters/config`;
     - `adapters/hooks`;
     - `adapters/interop`;
     - `adapters/secrets` (where I4 is kept, so a surviving mutant there is a security gap);
     - `adapters/notify` (what a notification sends, and that nothing it reports holds a secret, I4);
     - `adapters/mcp` (what each MCP tool answers, and the protocol around it);
     - `adapters/releases` (what the release check sends, and how it reads an answer it can't trust, §10.7);
     - `adapters/signing` (Ed25519 for the hub and its agents: what it signs and what it accepts, §16, decision 120);
     - `adapters/tls` (the hub's built-in TLS and the agent's pinned client: which certificate an agent accepts, and the session's binding, §16, decision 120);
     - `spawn-gate` (one child starts at a time, decision 58).
   - **A crate that joins the gate is tested whole once.** When a pull request adds a crate to `mutants_packages`, `just mutants` compares the list with the one at the merge base and runs `cargo mutants` over the whole of each crate the branch added, not only its changed lines, because a crate whose code merged before it joined has no diff to mutate (as `hooks` had).
   - **Exceptions** are marked `#[mutants::skip]` (from the `mutants` crate) or listed in `.cargo/mutants.toml` `exclude_re` or `exclude_globs`, each with a reason.
   - **Shards sized to the change, two mutants at a time.** The pull request's mutants are split round-robin over one to four jobs, `Mutation testing (shard k of n)`, each running `just mutants` with `--jobs 2 --shard k/n` on its four-core runner within a 60-minute limit; one runner outran that limit on a large change (#443, 371 mutants). Each prints every outcome as it comes (`--caught --unviable`) and uploads its `mutants.out` on a failure or a timeout.
   - **How many shards.** A job waits for a runner far longer than a small change's mutants take to test, so the shards are few. The `Changed paths` job estimates the mutants from the diff alone, in seconds and with no toolchain (`scripts/estimate-mutant-shards.sh`): one shard per 590 lines added or deleted in the source of the gated crates, rounded up, at most four. It leaves out what cargo-mutants skips (test modules, the `exclude_globs` of `.cargo/mutants.toml`). A change with no such lines gets one shard. A change to `mutants_packages` or to the mutants recipes gets four, since it can change what is mutated; so does one that removes or edits a line of `.cargo/mutants.toml`, but not one that only adds a comment or a list entry, which can only take mutants away. If the estimate fails, n is four.
   - **Where 590 comes from.** Of the last 460 merged pull requests (those with fewer than 1,000 mutants), 186 changed gated source. Their mutants per changed line had a median of 0.05 and a 90th percentile of 0.12, and 97 in 100 were at most 0.17: a shard of 100 mutants is 590 lines. A shard tests a mutant in 14 to 22 seconds (the slowest are `web`'s) after a baseline build of one to two minutes, so 100 take 25 to 40 of the 60 minutes. Of those 186, five had more than 100 mutants and were sized by this rule; they got two to four shards, and none a shard of more than 93 mutants. Recompute the ratio when the gate or its exclusions change a lot.
   - **The required check.** The check the ruleset requires, `Mutation testing (changed code)`, is a job that passes only when every shard passed, whether there is one or four.
   - **In MVP mode** (§13.8) the shards and the required check are skipped on pull requests; the nightly run still runs every mutant.
   - **Nightly.** A full run happens nightly.
3. **Squash merge.** The check runs on the pull request before merge, and pull requests are squash-merged, so the failing `test:` commit never lands on `main` and `git bisect` stays clean.

There is deliberately **no coverage percentage gate**. Coverage counts executed lines, not asserted behaviour; mutation testing measures the latter.

### 13.2 Behaviour first (BDD)

Every user-visible behaviour starts as a Given/When/Then scenario in `features/<context>/*.feature`, written in the ubiquitous language (§3.1) before any test or code for it. Scenarios describe outcomes, never endpoints, tables or crates. Tests of the test tooling itself are not scenarios but `node --test` files: `scripts/*.test.ts`, which `just lint` runs and which need no browser, and, beside the UI test tooling, `ui-tests/support/*.browser.test.ts`, which need one and which `just test-ui <engine>` runs in that engine before the scenarios.

- **Runner:** the `cucumber` crate (cucumber-rs). `crates/app/tests/acceptance.rs` (`harness = false`) runs every scenario tagged neither `@e2e` nor `@ui`. Because it lives in `app`, `cargo mutants` runs it for every `app` mutant.
- **Default level:** steps drive `app` use cases with the in-memory fakes and the fake clock, so scenarios about sleep, deadlines, leases and catch-up run in milliseconds and deterministically.
- **`@e2e` scenarios** run from the root `tests/e2e.rs` against the real `ladderback` binary with scratch repositories. Rules enforced in adapters (request checks in `http`, the deadline watchdog in the worker) are specified this way.
  - **Blocking steps.** cucumber-rs polls every scenario of the runner from one task, so a step that blocks for long (`rustic_core`, waiting for a daemon to start, thousands of file writes) runs through `blocking(…)` in `tests/e2e.rs`, which waits on a blocking thread.
  - **Four at once.** The runner runs 4 scenarios at once, because each backup's worker uses every core.
  - **The stall check.** The run fails when the runner's thread was unavailable for more than 8 s, before the operation log's 10 s wait for a connection (#92) can run out.
  - **Raw HTTP.** A step that speaks raw HTTP to the daemon opens its connection with `testkit::net::connect`, never `TcpStream::connect`, which clippy bans: the runner starts `tokio::process` children, so tokio handles `SIGCHLD`. On macOS a signal that interrupts `TcpStream::connect` makes it return a stream that is not connected yet, whose write fails with "Socket is not connected" and whose read waits for its timeout and fails with `WouldBlock` (#170).
- **A time limit on each scenario.** Both Rust runners watch their scenarios with `testkit::scenario_watch` from a thread of its own.
  - A scenario that runs longer than its runner's limit (60 s for the acceptance runner, 120 s for the `@e2e` one) stops the run with exit status 1 and its name and line. So a step that waits for something that never comes fails within minutes instead of holding CI until it cancels the job (#246).
  - When the run ends, each runner names its slowest scenario, the figure the limit is chosen from. Raising a limit to fit a slow scenario is weakening a check.
- **How a hang shows.** When the runner's stdout isn't a terminal, as in CI or `just test > log 2>&1`, the scenario watch also writes a line on stderr as each scenario begins (`e2e runner: began "<scenario>" (<feature file>:<line>)`) and ends (`… ended "<scenario>" (…) after 3.2s`).
  - A run that hangs where the limit can't see it, after a scenario's `after` hook (its world being dropped) or at the runner's own end, then still shows in the log which scenarios ran last and whether any began and never ended (#638).
  - In a terminal the lines stay off, since cucumber's writer redraws its last lines there.
  - A hang that outlasts every limit fails CI's `just test` step at its own limit (§13.8), which keeps the step's log; the last `Running` or `Doc-tests` line cargo printed there names the suite that held it.
- **No process outlives its scenario.** A step that starts `ladderback serve`, or any process that runs until something stops it and inherits the runner's stdout or stderr, starts it as a `testkit::children::Owned`. The pipe holder in `tests/steps/operations.rs` needn't: its output is piped, and it lets go when the runner's end of its stdin closes.
  - The scenario's end drops it, whether the scenario passed, failed or panicked, and the scenario watch ends every one still running before it stops a run at its limit, since `std::process::exit` runs no destructor.
  - Ending one stops the process, kills the process group each of its children leads (the workers and hooks of a daemon still running each lead one), then kills and reaps it.
  - Why: a daemon left running would hold the runner's output pipe, and whatever waits for that pipe to close, `just red-first` among them, would wait for its own timeout (#499).
- **`@ui` scenarios** drive the browser. They share `features/` with the rest, but run under `@cucumber/cucumber` (Node) with Playwright, from M0. Both Rust runners skip them. Their steps find elements only by role and accessible name (`getByRole`), so a control a screen reader can't identify can't be tested either (A2).
- **The `@ui` scenarios run three at a time.** `LADDERBACK_UI_WORKERS` (`ui-tests/cucumber.ts`) sets how many, 3 when unset or empty; 1 runs them one at a time, and anything but a whole number from 1 to 16 stops the run with an error.
  - Each worker is a thread with its own browser and its own shared daemon, and a scenario that starts a daemon of its own gives it its own scratch folder and port. So workers share only what they read: the binary, the built UI and the fixture repository.
  - A scenario must not depend on another having run before it, on the same worker or at all.
  - The red-first check runs one scenario, or an outline's few rows, so it sets 1 rather than start a browser and a daemon per worker for them (§13.1).
- **The `@ui` journeys run in three engines:** Playwright's Chromium, Firefox (Gecko: Firefox, Zen, LibreWolf, Tor Browser) and WebKit (Safari's engine, the closest stand-in for Safari that runs unattended, though not Safari itself), so a layout, focus-order or ARIA difference in one of them fails a journey (A1–A6 hold in every browser a person uses).
  - `LADDERBACK_UI_BROWSER` (`chromium`, `firefox` or `webkit`; `ui-tests/support/world.ts`) picks the engine, Chromium when unset; `just test-ui webkit` sets it.
  - CI runs each in its own Linux job, from `.github/actions/browser`, which installs the engines it is given and their system packages, and each job names one; only the Chromium job, `UI journeys`, is a required check (§13.8), and Firefox and WebKit run on a pull request only when it touches the UI (§13.8). The red-first check runs a new scenario in all three (§13.1).
  - A step whose browser behaviour differs by engine branches on `engine` from `world.ts`, with its reason, and never skips (grep `engine` in `ui-tests/` for the branches); a difference in the product itself is a `fix:` after a `test:` scenario that fails in that engine (decision 97).
- **A failed `@ui` scenario leaves a trace and a screenshot.** Each scenario's browser context records a Playwright trace (`ui-tests/support/world.ts`). A scenario that passes throws it away; one that fails keeps it, with a screenshot of the page, in `ui-tests/reports/failures/<engine>/`, at the step that failed, and its report names both files.
  - `just test-ui` empties `ui-tests/reports/` first and writes cucumber-js's JUnit report there too (`ui-tests/cucumber.ts`). Open a trace with `node_modules/.bin/playwright show-trace <file>` in `ui-tests` (not `npx`, for the reason AGENTS.md gives), or at trace.playwright.dev.
  - When a UI job fails, CI uploads the folder as an artifact named after the engine and the run attempt, kept 14 days, so a failure seen only on CI, such as a page crash, can be read from the run (#998).
  - A page that crashed has no screenshot; its trace still shows each step up to the crash.
- **Running the `@ui` journeys on Fedora.** `just setup` installs the tools and Playwright's three browsers, but not the system libraries a browser needs to run the `@ui` journeys (`just test-ui`). On Fedora, work in a [toolbox](https://containertoolbx.org/) or [distrobox](https://distrobox.it/) container: it is the only place `dnf` works on an atomic host (Silverblue, Bazzite), and it keeps the host clean on Workstation and Server too.

  ```sh
  toolbox create ladderback && toolbox enter ladderback
  sudo dnf install nss atk at-spi2-core libdrm libxkbcommon libXcomposite libXdamage libXfixes libXrandr mesa-libgbm alsa-lib
  ```

  Outside a container, Workstation and Server take the same list with `sudo dnf install`; an atomic host can layer it with `rpm-ostree install`, which needs a reboot. This list is what headless Chromium needs to launch on Fedora 45; Firefox and WebKit may need more, and Playwright names their packages for Ubuntu and Debian only.
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

Each ring of the architecture is tested at its own level:

| Ring | Tests | Tools | Examples |
|---|---|---|---|
| `domain` | Unit and property tests | `proptest` | `ForgetSelection` partitions, `Deadline`, lease transitions, `NextRun` across DST, `WakeDetector`, `Redactor` |
| `app` | Use-case tests and BDD scenarios with fakes | cucumber-rs, fake clock | Catch-up, retries, overlap, claims, prune guard waiting, deadline stop decision, restore policy |
| Adapters | **Port contract tests** (§3.2), plus integration tests against real infrastructure and property tests | sqlx on SQLite (also on btrfs in CI), scratch rustic repos (local + one OpenDAL backend), rustic CLI golden tests, golden config files per schema version, `proptest` for the config file's round trip, WebSocket over loopback | Store contract and `try_start`, kill-at-random-points test, import list replacement, export round trip, config migrations, config written and read back, outbox replay |
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
- docs are updated, and `fmt` and `clippy` are clean, which includes the robustness rules (§13.5);
- paragraphs and list items in `docs/` run to at most 150 words, which `just lint` checks (`scripts/check-paragraph-length.ts`).

### 13.5 Robustness rules

These rules keep the daemon running and the workspace safe to work in. Rules 1 to 4 are §16, decision 21; rules 6 and 7 cite their own. The daemon runs unattended and owns the operation queue: a panic in it silently stops backups, and input it didn't expect is the likeliest cause of one.

1. **Never crash in non-test code.** No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or `unimplemented!` outside tests.
   - Return typed errors through `Result` and `?`; use `let … else`, `ok_or` and `unwrap_or…` only where the fallback can't hide corruption.
   - The one exception is a provably infallible case, written `#[expect(clippy::expect_used, reason = "why it can't fail")]` with `.expect("…")`.
   - A poisoned mutex is `lock().unwrap_or_else(PoisonError::into_inner)` or an error, and a thread join is a `Result`.
   - **Enforcement.** Every production crate root (each `lib.rs`, `src/main.rs` and the `web` crate root; crate-level `//!` docs come first) carries `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`. The deny is not in `[workspace.lints]`, which would also reach integration tests and the cucumber runners.
   - **In tests.** Clippy lets tests use `unwrap`, `expect` and `panic!` (`allow-unwrap-in-tests`, `allow-expect-in-tests`, `allow-panic-in-tests`); `todo!`, `unimplemented!` and `unreachable!` stay denied in tests too.
   - **`clippy.toml`.** Clippy reads the nearest `clippy.toml` only, so the root file covers the crates without one and `crates/{domain,app}/clippy.toml` repeat its settings: this rule's three `allow-*-in-tests` and rule 7's two; `just architecture` checks that every `clippy.toml` and every crate root agree.
   - **Printing.** `src/main.rs` also denies `clippy::print_stdout` and `clippy::print_stderr`, because `print!` and `eprint!` panic when nobody reads what they print: a command writes through `output` and takes the failure (§4.5).
   - **Where it doesn't hold.** `testkit` is test support and has no deny. A red `test:` commit's `todo!()` stubs (§13.1) are the one place the lint doesn't hold, and the next commit removes them.
2. **Input-derived numbers are hostile.** Bytes from outside the process (the worker's stdout lines, the `JobSpec` on its stdin, `/api/v1` and other HTTP requests, `config.toml`, imported rustic profiles, later hub sync messages) never size an allocation unbounded and are never indexed unchecked.
   - Use `get()`, not `[i]` or `[a..b]`, on input-derived offsets.
   - Use checked or saturating arithmetic on sizes and counts.
   - Cap line and message lengths; the worker protocol's cap is in §4.1.
   - Slice strings only at char boundaries.
   - HTTP bodies are not read into memory by the M0 UI server, and axum's `DefaultBodyLimit` (2 MiB) bounds every extractor that does; a handler that needs more states its limit.
   - Clippy checks the indexing and the slicing (rule 7).
3. **Visible test skips.** A test that can't run here, such as `chmod 000` under root or a backend tool that isn't installed, calls `testkit::skip!(reason)`, never a silent early return or a bare `eprintln!`.
   - **The skip log.** The macro prints `SKIPPED <module path>: <reason>` to stderr and appends it to the file named by `LADDERBACK_SKIP_LOG`. `just test`, `just test-acceptance` and `just test-e2e` run through `scripts/with-skip-log.sh`, which points that variable at a fresh file in the target directory (`$CARGO_TARGET_DIR`, default `target/`) and lists its lines after the run, also when tests fail and without changing the exit status.
   - **`just test`.** It runs `cargo nextest` (which captures a passing test's stderr, so the log file is what makes a skip visible), the doc tests and both cucumber runners under that one log. nextest can't run the cucumber runners (`harness = false`), so `.config/nextest.toml` filters them out and `just test` runs them with `cargo test`. A failing suite doesn't stop the ones after it: all four run, and the recipe then fails, naming each suite that failed, so one CI run reports every failure. Only an interrupt (Ctrl-C, a cancelled run) stops it early, at the suite it reaches.
   - **`.config/nextest.toml`.** The same file terminates a test that hangs for 120 s. Its `ci` profile retries a failing test twice, reports a test that passed on retry as flaky, runs every test despite failures and writes `target/nextest/ci/junit.xml`, which CI uploads.
   - **In CI.** CI runs `just test` with `NEXTEST_PROFILE=ci`, so skips show in its log.
   - **`testkit`** is a dev-dependency only (`just architecture` checks it).
4. **Parallel agents.** Each agent works in its own git worktree under `.worktrees/<slug>`, which git, cargo-mutants and Docker builds ignore, with its own `CARGO_TARGET_DIR`, its own ports (never `just serve`'s 9898) and its own scratch folder.
   - **Shared paths.** It never runs `rm -rf` on a shared path, deletes its target directory when done, and keeps every `Cargo.toml` valid at all times: one broken manifest breaks every build in the workspace.
   - **cargo-mutants.** `just mutants` and `just mutants-all` unset `CARGO_TARGET_DIR` and `CARGO_BUILD_TARGET_DIR` for cargo-mutants, so each of its jobs builds in its own copy of the tree. The config-file keys `build.target-dir` and `build.build-dir` are out of scope, because the repo sets neither.
   - **Why mutants need their own folder.** Cargo judges a crate up to date by file times and by paths relative to the workspace, not by where the copy is. In one shared target directory, a job can find the crate another job just built up to date and test that job's mutant instead of its own, so a mutant is counted caught, missed or timed out wrongly. Even with one job, the last mutant's build stays in that folder, and the worktree's next `cargo test` finds it up to date and tests the mutant.
   - **The daemons the tests start.** The same goes for them: a test that starts `ladderback serve` gives it a data and a config folder of its own with `testkit::daemon::isolate` (`LADDERBACK_DATA_DIR`, `LADDERBACK_CONFIG_DIR`), never the platform's, which a developer's own daemon uses. A shared operation log means racing migrations, `NewerSchema` on a branch older than the real database, and test writes in the real log.
   - **A scratch home for every test binary.** `just test`, `just test-release`, `just test-acceptance` and `just test-e2e` run through `scripts/with-test-home.sh`, which has Cargo and nextest start every test binary (the cucumber runners and the doc tests too) through a target runner, `scripts/test-home-runner.sh`. The runner points `HOME` and the XDG data, config, cache and state folders into one scratch home and removes the `LADDERBACK_*_DIR` overrides.
   - **What the scratch home checks.** Where the platform keeps the config, that home holds a `config.toml` of a newer schema, so a daemon that reads it refuses to start. After the suites, the recipe fails when anything else is left there, naming the paths, and when the runner never ran (a build for another target).
   - **Cargo's own `HOME`.** Cargo itself keeps the real `HOME`: changed for Cargo or the toolchain's wrappers, it rebuilds everything and writes to that folder. A raw `cargo test` or `cargo nextest run` gets none of this.
   - **What it can't see.** The check can't see a file a test removes again, a write by a daemon that outlives the run, or a test that expects the daemon to refuse to start and gets that refusal from the planted config. The `@ui` journeys start the daemon from Node in scratch folders of their own.

5. **How many agents at once.** Whoever starts agents starts with 3 and keeps to these limits:
   - **Agents that edit code:** 2 or 3. Each has its own worktree (item 4), and every extra branch still costs a rebase and a review.
   - **Agents running heavy local work** (builds, test suites, mutants, Docker): 2 to 4, by what the machine can take. Here that's nearly every agent that edits code, because each one builds and tests in its own target directory.
   - **Read-only research** (searching code, reading docs, auditing separate modules): 5 to 8, and never more than 10.

   Each agent is asked for a short, structured report. Raise the number only for independent, read-only work, while the starter is waiting on its agents rather than the other way around. Lower it when edits conflict, work is duplicated, or requests start failing with HTTP 429.

   The limits are low for these reasons:
   - **Rate limits:** every agent draws on one requests- and tokens-per-minute budget, and past it, agents spend their time retrying.
   - **Cost:** each agent starts without the starter's context and rebuilds it by reading the same files again.
   - **The starter's context:** every report lands in the starter's context window, and many long reports leave it worse at combining them.
   - **Few truly independent pieces:** real work rarely splits into more than about 5, and the time saved past that goes on reconciling results.
   - **Review:** checking each agent's work is what limits throughput.
   - **One machine:** parallel builds compete for CPU, memory and disk, and slow one another. Under that load, timing-sensitive checks fail for reasons outside the change, such as an `@e2e` scenario losing its connection (`NotConnected`) or a mutant timing out.

6. **Unsafe code.** The workspace forbids it (`unsafe_code = "forbid"`, root `Cargo.toml`), and no source has any. Anywhere but the one place below, unsafe code is refused.
   - **The one place.** The design allows it only in a single module of `crates/adapters/platform`, for a macOS reader of the Wi-Fi network's name, and only if the commands that reader runs instead stop working (§16, decision 80, point 11; none exists, since the commands do). Then:
     - **Its lints.** That crate writes out its lint tables in full, because a crate can't take `lints.workspace = true` and override a lint, with `unsafe_code = "deny"`, `clippy::undocumented_unsafe_blocks` and `clippy::multiple_unsafe_ops_per_block` denied, and `#![expect(unsafe_code, reason = "…")]` on that one module.
     - **Its proof.** Every `unsafe` block has a `// SAFETY:` comment naming the invariant that makes it sound, and every `unsafe fn` it calls is listed in the module's documentation with the upstream line that states its contract.
     - **Its surface and review.** The surface is one safe function, and the review checks every comment against the platform's documentation.
7. **The lint set.** Beyond clippy's default `all`, `[workspace.lints.clippy]` in the root `Cargo.toml` turns on six lints, and `just lint` fails on any warning (§16, decision 105). Their settings, `excessive-nesting-threshold = 6` and `allow-indexing-slicing-in-tests = true`, are in every `clippy.toml`, and `just architecture` checks that the files agree. Each lint is there for one reason:
   - **`cognitive_complexity`**, at clippy's default of 25: no function harder to follow than that lands unnoticed. A function over it is split, not exempted. Clippy has no cyclomatic-complexity lint.
   - **`excessive_nesting`**, at 6 blocks: it catches the deep nesting the cognitive score misses. Early `continue`s, `let … else` and helpers flatten it.
   - **`allow_attributes` and `allow_attributes_without_reason`**: every exemption is `#[expect(…, reason = "…")]`, whose reason says why the case is safe. An expectation that no longer fires is a warning itself, so a stale exemption gets deleted. One that fires in some builds only (a feature, a target, a test build) sits under the `cfg_attr` of those builds. `allow_attributes` sees outer attributes only, so `just architecture` refuses an inner `#![allow(…)]`.
   - **`indexing_slicing` and `string_slice`**: rule 2's `get()` and char-boundary slicing, checked. Tests may index (`allow-indexing-slicing-in-tests`), but not slice strings unchecked, and a contract suite or a step file is not a test to clippy: they use `get()`, iterators or patterns. Where an index is clearer, an `#[expect]` gives the proof in its reason.

   Left out: `too_many_lines`, which mostly flags exhaustive `match`es over closed enums; `missing_docs`, a decision of its own; and `arithmetic_side_effects`, which flags all arithmetic, not only input-derived numbers.

8. **Decisions.** How a §16 decision is filed and numbered:
   - **Decision files.** A §16 decision is a file, `docs/design/decisions/NN-<slug>.md`, whose row in the index in §16 `just decision-index` writes from its number and heading (`just lint` fails when the index doesn't match the files, or two files share a number).
   - **Numbers.** A decision gets its number when its branch rebases before merging, the next after the highest on `main`. Branches open at the same time can't see each other's decisions, so a number taken earlier can clash, and the branch that merges second renumbers its file and every reference to it. A conflict in the index is resolved by taking either side and running `just decision-index`.

### 13.6 Dependency checks

**`cargo deny`, with the policy in `deny.toml` at the repository root** (§16, decision 43).

`just deny` runs `cargo deny --locked --all-features --workspace check` (`--workspace` because `web` is not a dependency of the root package) against the lockfile for the host targets (Linux and macOS, both architectures) and `wasm32-unknown-unknown`, and checks four things:

- **Advisories.** RustSec vulnerabilities, unmaintained and yanked crates fail. An advisory that can't be fixed with `cargo update -p <crate>` inside the requirements of `Cargo.toml` and of the crates above it is listed under `ignore` with its ID, why it doesn't reach Ladderback, and what ends the ignore; there is no entry without a reason.
- **Licenses.** An explicit allow list of permissive licenses, compatible with MIT OR Apache-2.0, built from what the tree uses.
  - An `OR` expression passes on any one allowed term, so `self_cell`, which offers `Apache-2.0 OR GPL-2.0-only`, is used under Apache-2.0 and GPL stays off the list.
  - The one dependency that has only a copyleft license, `option-ext` (MPL-2.0, file-level copyleft, which we never modify), is allowed by name as an `exception`, so a second MPL-2.0 crate fails.
  - A license not on the list is the maintainer's decision, not an edit made to turn the check green.
- **Bans.** Wildcard requirements are denied; duplicate versions only warn, because the tree has some we don't control. `chrono` is denied except as a dependency of `suppaftp`, `rmcp` or `schemars`, which can't drop it (decision 22), so a Ladderback crate that starts to use it fails.
- **Sources.** crates.io only, plus the one git repository, `rustic-rs/rustic_core`, that the pinned engine crates come from (decision 7).

**In CI.** The `Dependencies (cargo-deny)` job runs `just deny` on pull requests that touch more than docs, and every night.

- The RustSec database changes without any code change, so a new advisory first turns the nightly run red, before a pull request does; an open pull request's next run shows it too.
- A scheduled run notifies no one, so a failed nightly run opens an issue titled "Nightly dependency check failed", or comments on the open one.
- It is not a required check yet; the maintainer may add it to the ruleset once it has proved stable.
- `just verify` leaves it out because it needs the network.

### 13.7 Changes that touch several places

Each list is one change, unless a step says it lands later (§13.7.6, step 3): leave out a step and something breaks later, often far from the edit.

#### 13.7.1 An error or warning code

A code isn't done until a person can read it in their own language (§8.4). Three steps, in the same change:

1. Add the variant to `ErrorCode` or `WarningCode` in `crates/api/src/lib.rs`, with its arm in that enum's `args()`: the arguments the sender attaches (`&[]` for none). Mark an argument `optional` when the sender may leave it out. It must be text, and every text of the message, in every locale, reads it only through a select that has a `[none]` arm. Never put a secret in them (I4).
2. Add one line to that enum's list in `crates/l10n/src/code.rs`. The lookup is a `match` without a wildcard, so a variant with no line doesn't compile.
3. Write the message in `crates/l10n/locales/en-US/common.ftl`, named `error-<code>` or `warning-<code>` after the code's name on the wire (`InvalidJobSpec` is `error-invalid-job-spec`). It reads the variables `args()` lists and no others, with a translator comment above it when it reads any, and may have a `.with-detail` attribute that shows `{ $detail }`, the sender's own text. Voice follows PRODUCT.md: say what happened and what to do next.

`just test` fails for a code with no message, a message with no code, a message that reads a variable its code doesn't declare, and a catalog message that no product code names.

A warning the operation log keeps is a domain `WarningKind` too: its code goes in `WarningKind::code`, its row in the store's `WARNING_KINDS` (`crates/adapters/store/src/codec.rs`, §9) and its literal code in the codec's test match, which doesn't compile until it has one.

#### 13.7.2 A port

A port is four pieces:

1. The trait in `crates/app/src/ports/`, in the ubiquitous language, returning typed errors.
2. The in-memory fake in `crates/app/src/fakes/`, behind the `fakes` feature.
3. The contract suite in `crates/app/src/contract/`, run against the fake from `crates/app/tests/contracts.rs` and against the real adapter from that adapter's tests (`crates/adapters/store/tests/contract.rs` is one).
4. The real adapter in its own crate under `crates/adapters/`, depending on no other adapter, and its wiring in the composition root (`src/main.rs` and its `composition` module).

A hub port (decision 122) differs in two steps. A real adapter in a crate a release links, and its run of the contract suite, sit behind that crate's `hub` feature, on by default, which the root's `hub` feature turns on (`platform/hub` for `Identity`, `store/hub` for `AgentRegistry`); `just architecture` fails a release build in which any workspace crate has its `hub`, so a new one needs no line there. A migration a hub adapter's tables need is not gated: it ships in every build (§9). An adapter in a crate of its own, as `Signing`'s is, needs no feature until the root depends on it, then as an optional dependency that `hub` turns on. Its wiring in the composition root lands with the first use case that calls it, as `Signing`'s and `Identity`'s do in #567's later slices.

#### 13.7.3 A gated crate

Add it to `mutants_packages` in the Justfile and to the list in §13.1, in the change that creates the crate. A crate added later has its whole code mutated by that pull request's `just mutants` run (§13.1), so its surviving mutants are fixed in the same change.

#### 13.7.4 A package's first `wasm_bindgen_test`

Add `wasm-bindgen-test` to the package's dev-dependencies (`wasm-bindgen-test.workspace = true`). `just test-wasm` and `just red-first` both find the package by that dependency, from `cargo metadata` (`scripts/wasm-test-packages.ts`), so there is no list to edit.

#### 13.7.5 A shared UI component

A shared component is four pieces:

1. The component in a file of its own, `crates/web/src/components/<component>.rs`, re-exported from `components.rs`, with any decision it makes in a child module, `components/<component>/rules.rs`, tested on the host.
   - It is built from native elements first (§4.5), takes its tone, size and placement from the shared vocabulary (`crates/web/src/variants.rs`), its sizes drawn at the shared scale's lengths (`--size`).
   - It has its row in §4.5's table naming any tone it leaves out, its default tone where it isn't `Primary`, and any size it draws at other lengths, with the reason.
2. Its entry in the gallery (`crates/web/src/gallery.rs`), with every state it can show: a `<section>` whose `id` is its anchor, the demo's English heading in lower case with hyphens for spaces, then `-demo` (`text-field-demo`), and whose heading is a `DemoHeading`, so that the address `/_gallery#<id>` puts focus on it.
3. A row in each Scenario Outline of `features/ui/components/gallery.feature`: "The address of the "<demo>" demo puts focus on its heading", for its anchor; no axe-core violations in its demo states; and its structure pinned by an ARIA snapshot, whose literal goes in `ui-tests/steps/components.ts`.
4. Contract scenarios for its behaviour in a feature file of its own, `features/ui/components/<component>.feature`, with steps that find it by role and name and drive it by keyboard alone. Each scenario starts at its own demo, `When I open the "<demo>" demo`, rather than walking from the top of the gallery, so a demo added above it never lengthens its walk. The one exception is a pointer's own behaviour, such as a tooltip's hover (1.4.13): its steps move the pointer through `ui-tests/support/pointer.ts`, which the maintainer approved and which never presses.

#### 13.7.6 A workspace crate

A new workspace crate takes these steps, the third once a release holds it:

1. Its path in `members` and `default-members` in the root `Cargo.toml` (`default-members` leaves out `crates/web`, which builds for wasm32).
2. A `[[package]]` entry in `release-plz.toml` whose `git_tag_name` no tag matches, in the change that creates the crate. release-plz reads each package from the tree of its last `vX.Y.Z` tag, so a crate missing there stops the release PR with `cannot find package "<name>" in workspace` (§12.1).
3. Once a release's tag holds the crate, a pull request after it removes that entry: with it, the crate would look unreleased, and its own `feat` commits would bump only the patch version.
4. Its row in the crate graph in `scripts/check-crate-graph.ts` (an adapter takes the adapters' row) and its edges in §3.2, which change together; `just architecture` fails for a workspace crate with no row. If it owns a foreign model, its entry in that file's `OWNERS` too.
5. If its code is gated, §13.7.3.

### 13.8 Pull requests and merging

`AGENTS.md` carries the short form of the rules every pull request follows. This section is the full text for stacking, titles, hotfixes, break-glass, the required check names, the `Check` jobs' test step limit, CI's pinned Node and runner images, the `release` label, the jobs on the real service managers, and the Firefox and WebKit journeys.

**Stacking.** A pull request may stack on another that is still open when it builds on that work (a component that needs another component, the next slice of a feature) and waiting for the merge would hold it up. Pieces that can land independently still go off `main` separately.

Keep the chain short: at most three pull requests, and two where possible. Every merge underneath means a rebase above, and a squash merge rewrites the base's commits, so a long chain turns each merge into rebases of everything above it. Don't start another layer on a chain that is already three deep: wait for the bottom one to merge.

- **Naming:** say "stacked on #N" in the title and the first line of the description of each pull request above the bottom one. When its base merges and it targets `main`, take the words out of the title, since the title becomes the changelog line.
- **Bases:** open each layer against the branch below it (GitHub's stacked pull requests, `gh stack`, or a plain base branch), so its diff shows only its own commits.
- **Checks:** each layer passes every check on its own. A layer that changes behaviour starts with its own failing `test:` commit (`just red-first <branch below>`).
- **Merging:** bottom-up. After a squash merge below, rebase the next layer onto `main` with `git rebase --onto origin/main <old base branch>`, which drops the base's original commits, then retarget it to `main`.

**Title and squash commit.** The squash commit's subject is the pull request title, and the changelog is built from it, so the title is a Conventional Commit written for users: a type release-plz knows, no trailing full stop and no `(#N)`. Mark a change that breaks the public contract in §12.1 with `!` (`feat!: …`) and say in the body how to upgrade. `just check-pr-title "<title>"` runs CI's check.

**Hotfixes go through pull requests too.** Nobody pushes to `main` directly; the ruleset allows no direct push. A bad merge is first undone with a revert pull request, then fixed properly. A hotfix is an ordinary pull request with every required check.

**Up to date only where it matters.** The ruleset requires a pull request's required checks to have passed on its head, but no longer requires its branch to contain `main` (maintainer, 2026-11-18): a run waited longer for runners than it ran, and updating every branch after each merge ran CI again for nothing. Two pull requests that each pass alone can still break `main` together, as #905 with #909 and #914 with #922 did, and both pairs changed a file in common. So before merging a branch behind `main`, compare the files it changes with those `main` has changed since their merge base. When they share one, or the comparison fails, update the branch (`gh pr update-branch <n>` or a rebase) and let its checks run again; when they share none, merge it as it stands. `main`'s own run after each merge is the safety net.

**Break-glass.** Only the maintainer may merge a pull request whose required checks fail, through the ruleset's pull-request-only bypass, and only when a check can't pass for a reason outside the change: a GitHub Actions outage, a broken action or toolchain release, or a yanked crate. Each use gets an issue that says why, and fixing that check comes next. Agents never use the bypass.

**Required check names.** The "Protect main" ruleset requires checks by job name:

- `Check (ubuntu-latest)`
- `Check (macos-latest)`
- `UI journeys (Chromium, axe-core, keyboard)`
- `Release-build tests (no default features)`
- `Mutation testing (changed code)`
- `Red-first check`
- `Pull request title`

Renaming one of those jobs blocks every merge, so don't, unless the maintainer updates the ruleset at the same time.

**MVP mode.** Until every milestone except M7 is implemented, the maintainer may set the repository variable `CI_MODE` to `mvp`, and a pull request then runs a reduced set of jobs ([decision 124](decisions/124-mvp-ci-mode.md) lists it, and what is lost). It changes nothing on pushes to `main`, the nightly run and manual runs, which run every job.

- **Required checks still report.** A job the mode skips is skipped by its own `if`, which GitHub counts as passing, so each required name above still reports. `Check (macos-latest)` is a leg of the `Check` matrix and can't be skipped as a job: in MVP mode it stays, runs on a Linux runner and runs no step.
- **Where it is read.** The `mvp` output of the `Changed paths` job in ci.yml, and `vars.CI_MODE` in red-first.yml's `if`. A job added to either keeps to the same rule: skipped by its own `if`, never renamed.

**The `Check` jobs' test step.** The `just test` step has a `timeout-minutes` of its own (30), below the job's 45: a run that hangs fails the step, whose log survives, where GitHub discards the log of a job it cancels at its limit (#638).

- **Where 30 comes from.** The figure is about twice the slowest of 37 passing runs of each leg in November 2026: macOS 7.5 to 15.7 minutes, median 11.1, the 5 that started without a build cache 11.4 to 14.0; Linux 5.2 to 8.3, median 6.3.
- **Its margin.** The step starts at most 4.6 minutes into the job, so it ends 10 minutes before the job's limit.
- **Retuning.** Retune it from the same measurement when the suite grows; how the log names the hang is in §13.2.

**The UI jobs' test step.** The `just test-ui` step of `UI journeys` and of both engine jobs has a `timeout-minutes` of 35, ten below the job's 45 and well above the 13 to 20 minutes a passing run takes: a hang fails the step, and the steps that run after a failure still run, where GitHub cancels a job at its limit and skips them (#1016). The step starts up to about 4 minutes into the job, so 35 leaves about 6 minutes for the steps after it.

**Pinned Node and runner images.** CI doesn't take whatever `lts/*`, `ubuntu-latest` and `macos-latest` point at on the day it runs.

- **Node.** Node's major version is in `ui-tests/.nvmrc`, which every `setup-node` step reads.
- **Runner images.** Every job names its runner image: `ubuntu-26.04` and `macos-26`, and for the Linux release builds `ubuntu-22.04` and `ubuntu-22.04-arm`, chosen for their glibc (§12.1).
- **Bumps.** Each is bumped by a pull request of its own, as the Rust toolchain is (decision 78), so what a new Node or image breaks shows up there and is fixed there, not in an unrelated change. Dependabot bumps neither.
- **Check names stay.** The `Check` jobs keep their required names, `Check (ubuntu-latest)` and `Check (macos-latest)`: the matrix gives each leg a `name`, which the ruleset requires, apart from its `os`, the pinned image, so bumping an image renames no check.

**The `release` label and release builds.** The `Release builds` workflow (`.github/workflows/release-builds.yml`, §12.1, §16 decision 54) builds the release archives with cargo-dist, and the Linux `.deb` and `.rpm` packages from their binaries with nfpm (§16 decision 123), and smoke-tests them every night and on demand. The Release workflow calls it for each release, whose Linux archives and packages it publishes (§12.1).

- **On labelled pull requests.** It also runs on each push to a pull request labelled `release`. release-plz puts the label on every release PR (`release-plz.toml`), so a release's binaries are built before it is tagged.
- **Where the label belongs.** On any pull request that could break a release build but not a development one: a dependency with C code or a system library, the release profile, `release_flags` or `dist-workspace.toml`, the embedded web UI's build, or the workflow itself.
- **Not required.** It is not a required check, and its job names (`Release archive (<target>)`, `Report a failed nightly release build`) match none of the required ones above; a job added to it keeps to that.
- **Nightly failures.** A failed nightly run opens an issue titled "Nightly release build failed", or comments on the open one.

**The real service managers.** The `Service managers (launchd)` and `Service managers (systemd)` jobs (`.github/workflows/ci.yml`) run `install-service` against the real managers, on pull requests whose paths can change what they check (below) except in MVP mode (§13.8), on pushes to `main` and on manual runs.

- **When a pull request runs them.** The `changes` job's `service` output is true for `crates/adapters/platform/`, `scripts/service-contract.sh`, the install-service code in `src/` (`src/service.rs`, `src/main.rs`), `.github/workflows/` and `.github/actions/`, the `Justfile`, `Cargo.lock` and every `Cargo.toml` (the build unifies features over all crates, `web` included), and for any path it doesn't know. It is false only when every changed path is in `crates/web/`, `ui-tests/`, `features/ui/`, `docs/`, a TypeScript file of `scripts/`, a license file, an issue template or a Markdown file. A failure of the job leaves the output empty, so the jobs run. This saves a macOS runner on a pull request that can't reach them (#995).
- **Why.** The tests use stand-ins for `launchctl` and `systemctl` (decision 64), which take the commands at their word, so #542's race (`bootout` returns before the program has ended) showed only on a real Mac.
- **`contract`.** Each job runs `scripts/service-contract.sh` twice. `contract` runs the adapter's contract against the real manager (`the_real_manager_meets_the_contract`, with `LADDERBACK_TEST_REAL_SERVICE_MANAGER=1`), on a service of a name of its own.
- **`install`.** `install` runs the real binary: `install-service`, which says how to linger only when the real `loginctl` doesn't say the user lingers (#546), the daemon answering on `127.0.0.1:9898`, `install-service` again over the running daemon, which must have ended by the time it returns while a new one answers, then `remove-service`, after which nothing of it runs.
- **The session.** On macOS the runner user's GUI session serves the agent; on Linux the job turns on lingering for the runner user (`loginctl enable-linger`), which starts its systemd user manager.
- **No skips.** Both steps fail, never skip, when no manager answers: a skip would hide what they exist to catch.
- **`install` needs a throwaway machine.** `install` installs the real service under its real name, and the daemon keeps its data in the user's real folders. So it also needs `LADDERBACK_TEST_INSTALL_SERVICE=1`, which says the machine is a throwaway one such as a CI runner, and it refuses to start where a Ladderback service is installed or anything listens on its port; `contract` may run anywhere.
- **Not required.** They are not required checks; the maintainer may add them to the ruleset once they have a clean record.

**The Firefox and WebKit journeys.** The `UI journeys (Firefox, axe-core, keyboard)` and `UI journeys (WebKit, axe-core, keyboard)` jobs (`ui-engines` in `.github/workflows/ci.yml`) run on every push to `main`, every manual run and every pull request whose paths can change what they see, and the required Chromium job runs on every pull request that touches code (in MVP mode only on one whose paths can change what the engines see, by the same `ui` output). They are not required checks, and the nightly run doesn't start them, as it doesn't start Chromium's.

- **When a pull request runs them.** The `changes` job's `ui` output is true for `crates/web/`, `ui-tests/`, `features/ui/`, any other `.feature` file that holds an `@ui` scenario, `crates/l10n/` (the catalogs, and the formatting that calls the browser's `Intl`, whose output differs by engine), the files of `crates/adapters/http` that serve the page, set its headers, set the session cookie, send downloads and check the origin (`src/lib.rs`, `response_headers.rs`, `sign_in.rs`, `downloads.rs`, `request_checks.rs`), `rust-toolchain.toml`, the two scripts that build and install what the journeys run (`scripts/web-packages.sh`, `scripts/install-browser.sh`), `.github/workflows/ci.yml`, `.github/actions/browser/` and `.github/actions/npm-ci/`. Every other path leaves it false, the reverse of `service`: a change outside the list can't reach what only Firefox or WebKit would show, and Chromium's job still runs it. A failure of the `changes` job leaves the output empty, so the jobs run.
- **What a skipped pull request shows.** The matrix can't expand when its job's condition is false, so the checks list has one skipped `UI journeys (${{ matrix.name }}, axe-core, keyboard)` and no result per engine. Neither is a required check.
- **What isn't on the list.** `Cargo.lock`, `Cargo.toml` and the `Justfile` change in about a quarter of the commits on `main` (138 of 507 on 2026-11-18), so a dependency bump that moves `leptos` or `web-sys` runs Chromium alone on its pull request and all three engines on `main` after the merge. Adding them would put the two engine jobs back on a quarter of all pull requests.

### 13.9 Dependencies, license and fixtures

What may come into the repository, under which license, and what test data may hold:

- **Dependencies:** ask the maintainer before adding an external dependency (a crates.io crate, an npm package, a GitHub Action). Its license must be compatible with MIT OR Apache-2.0. A new workspace crate is not a dependency in this sense.
- **License:** MIT OR Apache-2.0 ([LICENSE-MIT](../../LICENSE-MIT), [LICENSE-APACHE](../../LICENSE-APACHE)). Ladderback is inspired by Backrest and never derived from it: Backrest is GPL-3.0, so never copy, port or paraphrase its code ([design](README.md)).
- **Screenshots and fixtures:** use neutral demo data and scratch repositories, never real backups, hostnames or paths. A secret in a test is a canary value, never a real repository password or key.

### 13.10 Keeping AGENTS.md useful

`AGENTS.md` is loaded whole into every agent and reviewer before any work, so every line costs tokens on every task.

What only some tasks need goes in the design, behind a one-line pointer that says when to read it (§13.7 to §13.9 are such sections), and a fact stays in one place. Keep the file under 32 KiB, the size past which some agents silently cut project instructions.

`just lint` runs `scripts/check-doc-paths.sh`, which fails when a path the file names doesn't exist. The rules for what a line must earn, and for adding and removing lines, are at the end of `AGENTS.md`.

### 13.11 Build profiles

Build settings live in the root `Cargo.toml`'s profiles, so a local build, CI and every recipe build the same way. The release builds' `dist` profile is in §16, decision 54.

- **`dev`**, which `cargo build`, `cargo test`, nextest and the development and test recipes use, gives Ladderback's crates line tables only (`debug = "line-tables-only"`) and dependencies no debug info (`[profile.dev.package."*"]`).
  - A backtrace still names the file and line of each Ladderback frame, and a dependency's frames keep their function names; full debug info would only make the target folder bigger and every link slower.
  - Build scripts get none either (`[profile.dev.build-override]`). So debug info never makes a crate built for a build script or proc macro differ from the same crate built for the code, nor change between commands that ask for different sets of units, which would rebuild it.
- **The crates that dominate test time** (scrypt, the ciphers, `zstd-sys`) are built with `opt-level = 3` in `dev` too: every test that opens a repository derives a key and encrypts and compresses every blob.
- **`debugging`** is `dev` with full debug info for Ladderback's crates and every dependency, for a debugger: `cargo build --profile debugging` puts the binary in `target/debugging/`, and `cargo nextest run --cargo-profile debugging` runs tests from it. Its own folder means switching to it and back rebuilds neither.
- **CI** sets `CARGO_INCREMENTAL=0`: its builds are cached and thrown away, so rustc's incremental session files would only grow every cache entry.
  - **The build dir's layout.** CI no longer sets `__CARGO_TEMPORARY_BUILD_DIR_NEW_LAYOUT_OPT_OUT=1`, which kept Cargo 1.100's build dir in the old layout (`target/<profile>/.fingerprint` and `deps`) that the `target` payload of the mbx build cache could save (#598). The `objects` payload (next item) doesn't depend on that layout, and Cargo means to remove the variable. **Pending measurement** (#1076).
  - **The build cache.** Since #1076 the Rust builds' cache entries are mbx's `objects` payload, the compiler outputs a job's builds used, and not the Cargo `target` tree; **pending measurement**: the draft pull request's runs read its wall time and entry sizes against `main`'s, and the `target` payload comes back, with the manifest hash in its key, if `objects` loses. The cache's keys, what saves each entry (only a push to `main`, and only the jobs that pass `save: "true"` to `setup-rust`) and when it goes, and the 10 GB budget it keeps to are in `ci.yml`'s header comment (#727, #1076).
  - **One setup path.** Every job in `ci.yml` that runs Rust, and the red-first check, sets up through `.github/actions/setup-rust`: the pinned toolchain, the build cache (whose steps may fail, so an outage only makes the build slower) and the tools the job names. Both UI jobs install npm's packages through `.github/actions/npm-ci` and restore trunk's tools the same way, and only a push to `main` saves a cache. A change to the setup is made once there (#996).
  - **The red-first job** also sets `CARGO_PROFILE_DEV_DEBUG=line-tables-only`: it builds the pull request's first commit and its merge base, whose `Cargo.toml` can predate these profiles and would otherwise build everything with full debug info. On a tree that has them, the variable changes nothing.

### 13.12 The Rust toolchain

`rust-toolchain.toml` pins an exact Rust version, with rustfmt, Clippy and the `wasm32-unknown-unknown` target (§16, decision 78). rustup installs it on the first build in the checkout, and every CI job, the release builds and the release workflow use the version that file names, so a contributor's build, CI and a release use the same compiler. The workspace's `rust-version`, the MSRV, is the same version, so every CI run proves the floor.

**Bumping it.** A new Rust stable release, a point release included, is adopted by its own pull request, titled `chore: update the Rust toolchain to 1.x.y`, within about two weeks of the release.

- **The bump commit.** One commit changes the `channel` line in `rust-toolchain.toml` and `rust-version` in the root `Cargo.toml` to the new version, and nothing else.
- **Fixes.** Each fix the new compiler needs (a test that depends on the standard library's output, a new Clippy lint or one the raised MSRV switches on, a change in rustfmt's output) is a commit of its own that says why.
- **What it fixes.** That pull request's CI shows what the release breaks, and only it has to fix it: new Clippy lints arrive through it, never through a pull request about something else.
- **A fix a user would notice** is the exception, because the pull request is squash-merged under its `chore:` title and the changelog leaves `chore:` out. When the fix builds with both compilers, it lands first, in a `fix:` pull request of its own with its failing test (§13.1), as #407 did for 1.100.0; when it needs the new one, the bump pull request takes the `fix:` title.
- **No `cargo update`.** The bump doesn't run `cargo update`: refreshing the dependencies is a change of its own.
- **What follows stable.** Only the tools installed with `cargo +stable install` (trunk and just, in the [install guide](../guide/install.md)) follow stable.

### 13.13 Formatting

rustfmt formats the Rust code and leaves a macro's body alone, so [leptosfmt](https://github.com/bram209/leptosfmt) formats the inside of every `view!` block in `crates/web` (§16, decision 84).

`just fmt` runs leptosfmt, then `cargo fmt`: rustfmt settles the code around a view leptosfmt has laid out, and the two then agree. `just fmt-check`, and so `just verify` and CI, checks both. `leptosfmt.toml` holds rustfmt's settings (width 100, four spaces); the workspace has no `rustfmt.toml`, so rustfmt runs with its edition 2024 defaults, and a change to one file is made to the other.

- **One version.** The Justfile's `leptosfmt_version` pins the release. `just setup` installs it with cargo-binstall, CI's `Check` job installs the same on Linux through install-action, and `just fmt` and `just fmt-check` stop when another version is installed, since each release lays out views differently. A bump is its own pull request, whose reformat is a commit of its own.
- **Comments go between nodes.** leptosfmt formats the Rust code inside a view (an attribute's closure such as `on:click=move |_| { … }`, a `{…}` child) with a fork of prettyplease, which keeps no non-doc comments: a comment there is dropped or moved to another statement.
  - A comment between a view's nodes is kept where it is, except one just after a one-line text or `{…}` child: one run joins it to that child's line and the next puts it above the child, on the wrong node.
  - So a comment about a handler, or a block, goes directly above the element or block it explains, at node level, and above the parent element when it would follow such a child.
  - `scripts/leptosfmt-comments.ts` runs before `just fmt` writes anything, and in `just fmt-check`: it fails on any file in which a leptosfmt run would leave fewer `//` than there are, or whose layout a second run changes.
- **What it changes.** Besides whitespace and line breaks, leptosfmt puts braces around a closure or match-arm body it breaks over several lines, with the commas that go with them, and a trailing comma after the last argument of a call it splits. String literals, attributes and their order are left as they are.
