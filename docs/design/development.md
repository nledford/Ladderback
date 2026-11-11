## 13. Development method

Ladderback is built behaviour first and test first. The two together are how the invariants in §0 stay true as the code grows.

### 13.1 Test first, always

**Rule:** no production code without a failing test that requires it. Every change follows red, green, refactor: write the smallest failing test that names the behaviour, make it pass with the smallest correct change, then clean up with the tests green. A bug fix starts with a test that reproduces the bug.

**Exempt:** documentation, formatting, generated code, dependency bumps with no behaviour change, and spikes. Spikes live on `spike/*` branches and are never merged; their findings are rebuilt test-first.

**How CI makes skipping it visible.** No tool can prove a test was written before the code. These checks make skipping test-first visible and costly:

1. **Red-first check**, for pull requests labelled `behaviour`. The pull request's first commit is a `test:` commit that adds the new tests and scenarios, with `todo!()` stubs for any new API so that it compiles. CI checks out that commit and runs exactly the tests it added, by name (`cargo test -- --exact`, and the new scenarios by name in both the Rust and the `@ui` runner); each one must **fail at run time**. A compile error doesn't count, because it would pass vacuously. A test that already passes specifies nothing new.
   - **Why each scenario is red.** Under each red scenario the log prints `failed at:`, its first failing step and the error's message on one line, cut at 200 characters, so a reviewer can tell the expected assertion from a page that never loaded or a server error. A report the script can't read prints `no failing step in the runner's output`; the scenario still counts as red. (A step with no definition is red in the Rust runners and shows as such; in the `@ui` runner it is reported as undefined, which is not a failure, so the check says `GREEN`.)
   - **Workflow and build reuse.** It runs in its own workflow, `.github/workflows/red-first.yml`, so adding or removing a label re-runs it alone and not the rest of CI. The base tree's build starts from a copy of the latest build of `main` that CI's build cache holds (the script's `RED_FIRST_SEED`), and the `test:` commit's tree from a copy of the base tree's, so neither builds the dependencies again, only what changed since that build of `main` and the workspace crates. `just red-first <base>` runs the same script locally, where the base tree's build stays in `target/red-first` from one run to the next.
   - **Wasm tests.** `#[wasm_bindgen_test]` tests count. When the `test:` commit adds one, the script builds both trees for `wasm32-unknown-unknown`, lists each package's wasm tests, and runs the new ones by name with `wasm-bindgen-test-runner` (under Node, as `just test-wasm` does). Each must fail at run time, like a host test, and the results show in the same `red`/`GREEN` lines, marked `(wasm32)`. A pull request that adds no wasm test pays nothing. The workflow installs Node, the `wasm32` target and `wasm-bindgen-cli` at the version `Cargo.lock` pins, as `just setup` does.
   - **The UI build.** The `@ui` scenarios run against a UI built with the tree's `just web-test` recipe when its Justfile defines one, and with `trunk build --release` when it doesn't, so a tree can build the UI with the features its scenarios need.
2. **Mutation testing.** CI writes the pull request's diff against the base to a file and runs `cargo mutants --in-diff <file>` on the gated crates: `domain`, `app`, `api` (the wire contract, including each code's arguments), `l10n` (the code-to-message mapping and the formatters), `web` (the announcer's timing and the page titles; its DOM glue runs only in a browser and is excluded, covered by the `@ui` scenarios), `adapters/config`, `adapters/hooks`, `adapters/interop` and `adapters/secrets` (where I4 is kept, so a surviving mutant there is a security gap). The Justfile's `mutants_packages` lists the ones that exist so far. A mutant that survives in changed code means behaviour no test asserts, and blocks the merge. **A crate that joins the gate is tested whole once:** when a pull request adds a crate to `mutants_packages`, `just mutants` compares the list with the one at the merge base and runs `cargo mutants` over the whole of each crate the branch added, not only its changed lines, because a crate whose code merged before it joined has no diff to mutate (as `hooks` had). Exceptions are marked `#[mutants::skip]` (from the `mutants` crate) or listed in `.cargo/mutants.toml` `exclude_re` or `exclude_globs`, each with a reason. A full run happens nightly.
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
| `app` | Use-case tests and BDD scenarios with fakes | cucumber-rs, fake clock | Catch-up, retries, overlap, claims, prune guard waiting, deadline stop decision, restore policy |
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
   - **Enforcement.** Every production crate root (each `lib.rs`, `src/main.rs` and the `web` crate root; crate-level `//!` docs come first) carries `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`, and clippy lets tests use `unwrap`, `expect` and `panic!` (`allow-unwrap-in-tests`, `allow-expect-in-tests`, `allow-panic-in-tests`); `todo!`, `unimplemented!` and `unreachable!` stay denied in tests too. It is not `[workspace.lints]`, which would also reach integration tests and the cucumber runners. Clippy reads the nearest `clippy.toml` only, so the root file covers the crates without one and `crates/{domain,app}/clippy.toml` repeat the three settings; `just architecture` checks that every `clippy.toml` and every crate root agree. `src/main.rs` also denies `clippy::print_stdout` and `clippy::print_stderr`, because `print!` and `eprint!` panic when nobody reads what they print: a command writes through `output` and takes the failure (§4.5). `testkit` is test support and has no deny. A red `test:` commit's `todo!()` stubs (§13.1) are the one place the lint doesn't hold, and the next commit removes them.
2. **Input-derived numbers are hostile.** Bytes from outside the process (the worker's stdout lines, the `JobSpec` on its stdin, `/api/v1` and other HTTP requests, `config.toml`, imported rustic profiles, later hub sync messages) never size an allocation unbounded and are never indexed unchecked. Use `get()`, not `[i]` or `[a..b]`, on input-derived offsets; checked or saturating arithmetic on sizes and counts; caps on line and message lengths; and slice strings only at char boundaries. The worker protocol's cap is in §4.1. HTTP bodies are not read into memory by the M0 UI server, and axum's `DefaultBodyLimit` (2 MiB) bounds every extractor that does; a handler that needs more states its limit.
3. **Visible test skips.** A test that can't run here, such as `chmod 000` under root or a backend tool that isn't installed, calls `testkit::skip!(reason)`, never a silent early return or a bare `eprintln!`. The macro prints `SKIPPED <module path>: <reason>` to stderr and appends it to the file named by `LADDERBACK_SKIP_LOG`. `just test`, `just test-acceptance` and `just test-e2e` run through `scripts/with-skip-log.sh`, which points that variable at a fresh file in the target directory (`$CARGO_TARGET_DIR`, default `target/`) and lists its lines after the run, also when tests fail and without changing the exit status. `just test` runs `cargo nextest` (which captures a passing test's stderr, so the log file is what makes a skip visible), the doc tests and both cucumber runners under that one log; nextest can't run the cucumber runners (`harness = false`), so `.config/nextest.toml` filters them out and `just test` runs them with `cargo test`. The same file terminates a test that hangs for 120 s; its `ci` profile retries a failing test twice, reports a test that passed on retry as flaky, runs every test despite failures and writes `target/nextest/ci/junit.xml`, which CI uploads. CI runs `just test` with `NEXTEST_PROFILE=ci`, so skips show in its log. `testkit` is a dev-dependency only (`just architecture` checks it).
4. **Parallel agents.** Each agent works in its own git worktree under `.worktrees/<slug>`, which git, cargo-mutants and Docker builds ignore, with its own `CARGO_TARGET_DIR`, its own ports (never `just serve`'s 9898) and its own scratch folder. It never runs `rm -rf` on a shared path, deletes its target directory when done, and keeps every `Cargo.toml` valid at all times: one broken manifest breaks every build in the workspace. `just mutants` and `just mutants-all` unset `CARGO_TARGET_DIR` and `CARGO_BUILD_TARGET_DIR` for cargo-mutants, so each of its jobs builds in its own copy of the tree; the config-file keys `build.target-dir` and `build.build-dir` are out of scope, because the repo sets neither. Cargo judges a crate up to date by file times and by paths relative to the workspace, not by where the copy is: in one shared target directory, a job can find the crate another job just built up to date and test that job's mutant instead of its own, so a mutant is counted caught, missed or timed out wrongly. Even with one job, the last mutant's build stays in that folder, and the worktree's next `cargo test` finds it up to date and tests the mutant. A §16 decision is a file, `docs/design/decisions/NN-<slug>.md`, and a row in the index in §16. It gets its number when its branch rebases before merging, the next after the highest on `main`: branches open at the same time can't see each other's decisions, so a number taken earlier can clash, and the branch that merges second renumbers its file, its row and every reference to it. The same goes for the daemons the tests start: a test that starts `ladderback serve` gives it a data and a config folder of its own with `testkit::daemon::isolate` (`LADDERBACK_DATA_DIR`, `LADDERBACK_CONFIG_DIR`), never the platform's, which a developer's own daemon uses (a shared operation log means racing migrations, `NewerSchema` on a branch older than the real database, and test writes in the real log). `tests/isolation.rs` runs each test binary that starts the daemon with `HOME` and the XDG folders pointing at a scratch folder and fails when the run leaves anything there; a new test target that starts the daemon joins its list. The `@ui` journeys start the daemon from Node in scratch folders of their own.

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

### 13.6 Dependency checks

**`cargo deny`, with the policy in `deny.toml` at the repository root** (§16, decision 43). `just deny` runs `cargo deny --locked --all-features --workspace check` (`--workspace` because `web` is not a dependency of the root package) against the lockfile for the host targets (Linux and macOS, both architectures) and `wasm32-unknown-unknown`, and checks four things:

- **Advisories.** RustSec vulnerabilities, unmaintained and yanked crates fail. An advisory that can't be fixed with `cargo update -p <crate>` inside the requirements of `Cargo.toml` and of the crates above it is listed under `ignore` with its ID, why it doesn't reach Ladderback, and what ends the ignore; there is no entry without a reason.
- **Licences.** An explicit allow list of permissive licences, compatible with MIT OR Apache-2.0, built from what the tree uses. An `OR` expression passes on any one allowed term, so `self_cell`, which offers `Apache-2.0 OR GPL-2.0-only`, is used under Apache-2.0 and GPL stays off the list. The one dependency that has only a copyleft licence, `option-ext` (MPL-2.0, file-level copyleft, which we never modify), is allowed by name as an `exception`, so a second MPL-2.0 crate fails. A licence not on the list is the maintainer's decision, not an edit made to turn the check green.
- **Bans.** Wildcard requirements are denied; duplicate versions only warn, because the tree has some we don't control. `chrono` is denied except as the dependency of `suppaftp` (decision 22), so a Ladderback crate that starts to use it fails.
- **Sources.** crates.io only, plus the one git repository, `rustic-rs/rustic_core`, that the pinned engine crates come from (decision 7).

**In CI.** The `Dependencies (cargo-deny)` job runs `just deny` on pull requests that touch more than docs, and every night: the RustSec database changes without any code change, so a new advisory first turns the nightly run red, before a pull request does; an open pull request's next run shows it too. A scheduled run notifies no one, so a failed nightly run opens an issue titled "Nightly dependency check failed", or comments on the open one. It is not a required check yet; the maintainer may add it to the ruleset once it has proved stable. `just verify` leaves it out because it needs the network.

### 13.7 Changes that touch several places

Each list is one change: leave out a step and something breaks later, often far from the edit.

#### 13.7.1 An error or warning code

A code isn't done until a person can read it in their own language (§8.4). Three steps, in the same change:

1. Add the variant to `ErrorCode` or `WarningCode` in `crates/api/src/lib.rs`, with its arm in that enum's `args()`: the arguments the sender attaches (`&[]` for none). Mark an argument `optional` when the sender may leave it out. It must be text, and every text of the message, in every locale, reads it only through a select that has a `[none]` arm. Never put a secret in them (I4).
2. Add one line to that enum's list in `crates/l10n/src/code.rs`. The lookup is a `match` without a wildcard, so a variant with no line doesn't compile.
3. Write the message in `crates/l10n/locales/en-US/common.ftl`, named `error-<code>` or `warning-<code>` after the code's name on the wire (`InvalidJobSpec` is `error-invalid-job-spec`). It reads the variables `args()` lists and no others, with a translator comment above it when it reads any, and may have a `.with-detail` attribute that shows `{ $detail }`, the sender's own text. Voice follows PRODUCT.md: say what happened and what to do next.

`just test` fails for a code with no message, a message with no code, a message that reads a variable its code doesn't declare, and a catalog message that no product code names.

#### 13.7.2 A port

1. The trait in `crates/app/src/ports/`, in the ubiquitous language, returning typed errors.
2. The in-memory fake in `crates/app/src/fakes/`, behind the `fakes` feature.
3. The contract suite in `crates/app/src/contract/`, run against the fake from `crates/app/tests/contracts.rs` and against the real adapter from that adapter's tests (`crates/adapters/store/tests/contract.rs` is one).
4. The real adapter in its own crate under `crates/adapters/`, depending on no other adapter, and its wiring in the composition root (`src/main.rs` and its `composition` module).

#### 13.7.3 A gated crate

Add it to `mutants_packages` in the Justfile and to the list in §13.1, in the change that creates the crate. A crate added later has its whole code mutated by that pull request's `just mutants` run (§13.1), so its surviving mutants are fixed in the same change.

#### 13.7.4 A package's first `wasm_bindgen_test`

Add `wasm-bindgen-test` to the package's dev-dependencies (`wasm-bindgen-test.workspace = true`). `just test-wasm` and `just red-first` both find the package by that dependency, from `cargo metadata` (`scripts/wasm-test-packages.ts`), so there is no list to edit.

#### 13.7.5 A shared UI component

1. The component in `crates/web/src/components.rs`, from native elements first (§4.5).
2. Its entry in the gallery (`crates/web/src/gallery.rs`), with every state it can show.
3. A row in both Scenario Outlines of `features/ui/components.feature`: no axe-core violations in its demo states, and its structure pinned by an ARIA snapshot, whose literal goes in `ui-tests/steps/components.ts`.
4. Contract scenarios for its behaviour in the same file, with steps that find it by role and name and drive it by keyboard alone.

### 13.8 Pull requests and merging

`AGENTS.md` carries the short form of the rules every pull request follows. This section is the full text for stacking, titles, hotfixes, break-glass, the required check names and the `release` label.

**Stacking.** Don't stack pull requests unless it's truly unavoidable, meaning a change that can't land in independent pieces off `main`. Then keep the stack to two pull requests, say "stacked on #N" in both titles and the first line of both descriptions, and use GitHub's stacked pull requests (`gh stack`). Each layer passes every check on its own; a layer that changes behaviour starts with its own failing `test:` commit (`just red-first <branch below>`). A stack merges bottom-up with `gh stack merge`.

**Title and squash commit.** The squash commit's subject is the pull request title, and the changelog is built from it, so the title is a Conventional Commit written for users: a type release-plz knows, no trailing full stop and no `(#N)`. Mark a change that breaks the public contract in §12.1 with `!` (`feat!: …`) and say in the body how to upgrade. `just check-pr-title "<title>"` runs CI's check.

**Hotfixes go through pull requests too.** Nobody pushes to `main` directly; the ruleset allows no direct push. A bad merge is first undone with a revert pull request, then fixed properly. A hotfix is an ordinary pull request with every required check.

**Break-glass.** Only the maintainer may merge a pull request whose required checks fail, through the ruleset's pull-request-only bypass, and only when a check can't pass for a reason outside the change: a GitHub Actions outage, a broken action or toolchain release, or a yanked crate. Each use gets an issue that says why, and fixing that check comes next. Agents never use the bypass.

**Required check names.** The "Protect main" ruleset requires checks by job name (`Check (ubuntu-latest)`, `Check (macos-latest)`, `UI journeys (Chromium, axe-core, keyboard)`, `Release-build tests (no default features)`, `Mutation testing (changed code)`, `Red-first check`, `Pull request title`). Renaming one of those jobs blocks every merge, so don't, unless the maintainer updates the ruleset at the same time.

**The `release` label and release builds.** The `Release builds` workflow (`.github/workflows/release-builds.yml`, §12.1, §16 decision 54) builds the release archives with cargo-dist and smoke-tests them every night and on demand. It also runs on each push to a pull request labelled `release`: release-plz puts the label on every release PR (`release-plz.toml`), so a release's binaries are built before it is tagged, and the label belongs on any pull request that could break a release build but not a development one: a dependency with C code or a system library, the release profile, `release_flags` or `dist-workspace.toml`, the embedded web UI's build, or the workflow itself. It is not a required check, and its job names (`Release archive (<target>)`, `Report a failed nightly release build`) match none of the required ones above; a job added to it keeps to that. A failed nightly run opens an issue titled "Nightly release build failed", or comments on the open one.

### 13.9 Dependencies, licence and fixtures

- **Dependencies:** ask the maintainer before adding an external dependency (a crates.io crate, an npm package, a GitHub Action). Its license must be compatible with MIT OR Apache-2.0. A new workspace crate is not a dependency in this sense.
- **License:** MIT OR Apache-2.0 ([LICENSE-MIT](../../LICENSE-MIT), [LICENSE-APACHE](../../LICENSE-APACHE)). Ladderback is inspired by Backrest and never derived from it: Backrest is GPL-3.0, so never copy, port or paraphrase its code ([design](README.md)).
- **Screenshots and fixtures:** use neutral demo data and scratch repositories, never real backups, hostnames or paths. A secret in a test is a canary value, never a real repository password or key.

### 13.10 Keeping AGENTS.md useful

`AGENTS.md` is loaded whole into every agent and reviewer before any work, so every line costs tokens on every task. What only some tasks need goes in the design, behind a one-line pointer that says when to read it (§13.7 to §13.9 are such sections), and a fact stays in one place. Keep the file under 32 KiB, the size past which some agents silently cut project instructions. `just lint` runs `scripts/check-doc-paths.sh`, which fails when a path the file names doesn't exist. The rules for what a line must earn, and for adding and removing lines, are at the end of `AGENTS.md`.
