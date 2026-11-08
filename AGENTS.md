# AGENTS.md

Guidance for coding agents working on Steading, a self-hosted backup manager for [rustic](https://rustic.cli.rs). It is built on `rustic_core` in Rust, with a Leptos (CSR) web UI.

## Status

Milestones M0 (skeleton) and M1a (engine) are in place: the workspace and CI checks, the embedded hello UI, and `steading worker` running identify, backup, prune and check through `rustic_core`, with cancellation and the deadline watchdog.

M1b (single-machine MVP, [docs/design/roadmap.md §14](docs/design/roadmap.md#14-milestones)) is in progress. In place so far:
- secret sources, `${…}` substitution and redaction (§10.2);
- the operation log and history in SQLite;
- request checks (§10.1);
- the `l10n` core with the `en-US` catalog and error codes rendered through it (§8.4).

The rest of the M1b row is still to come; follow-up work is tracked as GitHub issues.

## Commands

Every workflow is a `just` recipe, and CI runs the same recipes. `just` lists them.

| Command | Does |
|---|---|
| `just setup` | Installs the wasm target, trunk, cargo-mutants, cargo-nextest and release-plz (through cargo-binstall), the UI test dependencies and Chromium |
| `just verify` | Everything CI checks except mutants and red-first: `fmt-check`, `lint`, `architecture`, `test`, `test-ui` |
| `just fmt` | Formats Rust code |
| `just lint` | Clippy with `-D warnings` on the host crates and on `web` for wasm32, plus the journey lint (no pointer actions in `ui-tests/steps`) |
| `just architecture` | Banned crates in `domain` and `app`, the `domain` wasm32 build, identical banned-call lists (`crates/{domain,app}/clippy.toml`), the no-panic settings and crate-root lints, `testkit` only as a dev-dependency |
| `just test [args]` | Unit and contract tests (`cargo nextest`; `args` go to it, e.g. a test name), doc tests, then the acceptance and `@e2e` runners, then the list of skipped tests. `NEXTEST_PROFILE=ci` adds retries and a JUnit report |
| `just test-acceptance [args]` / `just test-e2e [args]` | One cucumber-rs runner; `--name <regex>` or `--tags <expr>` narrows its scenarios |
| `just test-ui [args]` | Builds the web UI and the binary, installs the UI test dependencies if they are missing or stale, then runs the `@ui` scenarios |
| `just mutants [base] [args]` | `cargo mutants` on code changed since `base` (default `origin/main`) in the gated crates, which the Justfile's `mutants_packages` lists; further `args` go to `cargo mutants` (e.g. `--jobs 3`) |
| `just red-first [base]` | Checks that the branch's first commit is a `test:` commit whose new tests fail at run time (§13.1) |
| `just serve` | Builds everything and runs `steading serve` on `127.0.0.1:9898` |
| `just release-preview` | Shows the version bump and changelog entry the next release PR would make (§12.1) |

The cucumber runners use `harness = false`, so `cargo nextest` can't run them; `just test` excludes them from nextest and runs them with `cargo test`, as do `just test-acceptance` and `just test-e2e`. Don't hand-edit the version or `CHANGELOG.md`: the release PR does both (§12.1).

## Read first

| File | Owns |
|---|---|
| [PRODUCT.md](PRODUCT.md) | Users, purpose, voice ("calm and exact"), boundaries, accessibility floor |
| [docs/design/](docs/design/README.md) | Invariants (§0), domain model (§3.1), architecture (§3.2), workspace layout (§8.3), localization (§8.4), accessibility (§4.5), versioning and releases (§12.1), development method (§13), milestones (§14), decisions (§16) |

Each fact lives in one file. Link to it rather than copying it. The design document is the source of truth: when work changes a decision, update the document, including §16, in the same change.

## Invariants

§0 of the technical design lists safety invariants I1–I7 and accessibility invariants A1–A6. They are not negotiable. A change that weakens one doesn't merge, and every change that touches one adds or updates the test that proves it. Above all:

- Exactly one maintenance owner per repository (I1). Prune never deletes instantly (I2).
- Resolved secrets never appear in argv, environments, logs, templates, sync messages or the UI (I4).
- Forget deletes only snapshots a plan owns or the user explicitly selected (I6).

## Never crash

The daemon runs unattended and owns the operation queue, so a panic silently stops backups. Rules and reasons: §13.5.

- No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or `unimplemented!` outside tests. Return typed errors through `Result` and `?`. The one exception is a provably infallible case: `#[allow(clippy::expect_used, reason = "why it can't fail")]` and `.expect("…")`. Every production crate root denies these lints, so clippy fails the build. `todo!()` stubs live only in the red `test:` commit.
- Mutex poisoning is `lock().unwrap_or_else(PoisonError::into_inner)` or an error; a thread join is a `Result`.
- Input-derived numbers are hostile: bytes from outside the process (worker output, `JobSpec`, HTTP requests, `config.toml`, imported profiles, sync messages) never size an allocation unbounded and never index unchecked. Use `get()`, not `[i]` or `[a..b]`; checked or saturating arithmetic; caps on line and message lengths (§4.1); slice strings only at char boundaries.

## Adding an error or warning code

A code isn't done until a person can read it in their own language (§8.4). Three steps, in the same change:

1. Add the variant to `ErrorCode` or `WarningCode` in `crates/api/src/lib.rs`, with its arm in that enum's `args()`: the arguments the sender attaches (`&[]` for none). Mark an argument `optional` when the sender may leave it out. It must be text, and every text of the message, in every locale, reads it only through a select that has a `[none]` arm. Never put a secret in them (I4).
2. Add one line to that enum's list in `crates/l10n/src/code.rs`. The lookup is a `match` without a wildcard, so a variant with no line doesn't compile.
3. Write the message in `crates/l10n/locales/en-US/common.ftl`, named `error-<code>` or `warning-<code>` after the code's name on the wire (`InvalidJobSpec` is `error-invalid-job-spec`). It reads the variables `args()` lists and no others, with a translator comment above it when it reads any, and may have a `.with-detail` attribute that shows `{ $detail }`, the sender's own text. Voice follows PRODUCT.md: say what happened and what to do next.

`just test` fails for a code with no message, a message with no code, a message that reads a variable its code doesn't declare, and a catalog message that no product code names.

## How to work: BDD, DDD and TDD

Follow all three whenever possible. Deviating needs a stated reason in the pull request.

**Behaviour first (BDD).**

- Every user-visible behaviour starts as a Given/When/Then scenario in `features/<bounded-context>/*.feature`, before any test or code for it.
- Scenarios describe outcomes in the ubiquitous language, never endpoints, tables or crates.
- Tags pick the runner (§13.2):
  - untagged: use cases with fakes and a fake clock;
  - `@e2e`: the real binary;
  - `@ui`: the browser, through Playwright.
- `@ui` steps find elements only by role and accessible name.

**Domain first (DDD).**

- Use the ubiquitous language from §3.1 exactly, in code, tests, UI copy and docs. A new domain term goes into the glossary first.
- Respect the bounded contexts and aggregates in §3.1. Invariants live on the aggregate or domain service that owns them, not in adapters or handlers.
- Keep the hexagonal dependency rule (§3.2):
  - `domain` has no I/O, no async, and no ambient time or randomness.
  - `app` depends only on `domain`; time and randomness come from the `Clock` and `Random` ports.
  - Adapters never depend on each other. Only `src/main.rs` wires them together.
- Foreign models stay behind their anti-corruption layers: `rustic_core` types only in `adapters/engine`, rustic profile TOML only in `adapters/interop`.
- Secret sources (command, keyring, file, environment) are read only in `adapters/secrets`, behind the `SecretResolver` port; `adapters/engine` hands workers a scrubbed environment (§10.2, §4.1).
- The core returns typed errors and codes, never user-facing sentences. Text is produced through `l10n` (§8.4).

**Test first (TDD).**

- No production code without a failing test that requires it: red, green, refactor. A bug fix starts with a test that reproduces the bug.
- A pull request that changes behaviour carries the `behaviour` label, and its first commit is a `test:` commit that fails at run time, with `todo!()` stubs so it compiles. Implementation commits follow. CI's red-first check runs only on pull requests with that label; §13.1 lists the changes exempt from test first.
- Mutation testing (`cargo mutants`) gates changed code in the crates §13.1 names; the Justfile's `mutants_packages` lists the ones that exist so far, and a new gated crate joins it when it lands. A surviving mutant means a behaviour no test asserts.
- There is no coverage gate. Test behaviour, boundaries, invariants and errors. Don't test wiring, mock echoes, source text or incidental defaults.
- Doubles replace ports only. Domain behaviour is never mocked. A new port gets a contract suite that runs against both its fake and its real adapter.
- A test that can't run here (running as root makes `chmod 000` ineffective, a backend tool isn't installed) calls `testkit::skip!(reason)`, never a silent early return or a bare `eprintln!`. `just test` lists the skips at its end, so CI shows them. `testkit` is a dev-dependency only.

## Independent adversarial review

After implementing a non-trivial change, get one independent adversarial review from a reviewer subagent that has a fresh context. Don't review your own work. If your tool has no subagents, use a new session that sees only the diff and the relevant design sections.

- **Review needed:**
  - product code;
  - anything touching an invariant in §0;
  - security-sensitive code (auth, secrets, request checks, command execution, file paths);
  - concurrency or maintenance logic (leases, prune guard, deadlines);
  - changes to design decisions or the architecture.
- **No review needed:** mechanical work, such as renames, link fixes, formatting and wording-only edits.
- **What to give the reviewer:** the diff; the design sections and invariants it touches; the acceptance criteria; and an instruction to find what is wrong, not to confirm it works. Ask for evidence (file and line, a failing input, a counterexample) and a severity for each finding.
- **Afterwards:** fix every real finding, with a failing test first where it is a bug. Say in the pull request which findings were rejected and why. One review per change; re-review only when the fixes change the design.

## Parallel agents

Several agents may work on the repository at once (§13.5). Each one:

- works in its own git worktree under `.worktrees/<slug>` (git ignores the folder): `git worktree add -b <branch> .worktrees/<slug> origin/main` from the main checkout. It uses its own `CARGO_TARGET_DIR` and deletes that target directory when done;
- edits files and runs commands only inside that worktree, never in the main checkout. If a tool resolves relative paths against the main checkout, pass absolute paths or the `.worktrees/<slug>/` prefix. A container that runs tests mounts the worktree, not the main checkout, which holds every other worktree;
- passes `--jobs` to `just mutants` when several agents share the machine;
- uses its own ports, never `just serve`'s 9898, and its own scratch folder;
- never runs `rm -rf` on a shared path;
- keeps every `Cargo.toml` valid at all times: one broken manifest breaks every build in the workspace.

## Task tracking

The session that steers the work (the main session, which starts the other agents) keeps a task queue in a Markdown file outside the repository: `~/.local/state/steading-scratch/queue.md`.

- **Sections:** Now, Next, Waiting and Done.
- **Items:** one item is one maintainer decision or one agent run, and every open item says `running` or `blocked-on: <what>`.
- **When to read it:** at the start of every session and after every context compaction, before acting.
- **Keeping it in sync with the to-do list:** the session's to-do list mirrors the queue's Now and Next items, with the same wording and order. Update both in the same step whenever a task starts, finishes, is added, is reordered or gets blocked. Waiting and Done items live only in the queue.
- **Product follow-ups are GitHub issues,** not queue items. The queue links them as `#N`, and the pull request that fixes one says `Closes #N`.
- **Other agents don't edit the queue.** They report what they finished and what they deferred, and the main session updates it.

## Conventions

- **Accessibility:** every UI change meets WCAG 2.2 AA through the shared component set (§4.5). axe-core must be clean and the journey must work by keyboard alone. New components need contract tests.
- **Localization:** no hard-coded user-facing strings. Use message IDs from `l10n`, Fluent terms for glossary words, and complete sentences. Code never concatenates fragments (§8.4).
- **Time:** time goes through `jiff`, via the `Clock` port in the core. Never add `chrono` to a Steading crate: it reaches the build only transitively, through `rustic_backend`'s OpenDAL FTP service (`suppaftp`), which we can't turn off. `just architecture` bans it from `domain` and `app` (§16, decision 22).
- **CI job names:** the "Protect main" ruleset requires checks by job name (`Check (ubuntu-latest)`, `Check (macos-latest)`, `UI journeys (Chromium, axe-core, keyboard)`, `Mutation testing (changed code)`, `Red-first check`). Renaming one of those jobs blocks every merge, so don't, unless the maintainer updates the ruleset at the same time.
- **Docs:** keep [PRODUCT.md](PRODUCT.md) and the technical design current with the code. Record new decisions in §16.
- **Commits:** atomic, in logical groups. Each commit is one coherent change for one reason: it builds, passes its tests (except the deliberately failing `test:` commit, §13.1), and can be reviewed and reverted on its own.
  - Split unrelated work, formatting, refactors and dependency updates into separate commits. Stage hunks selectively when one file holds changes for different commits.
  - Never stage secrets, local config or generated artifacts.
  - Use Conventional Commits (`feat:`, `fix:`, `test:`, `docs:`, `refactor:`, `chore:`) with an imperative subject and a body explaining why.
  - The atomic commits are for review; the pull request is squash-merged at the end.
- **Pull requests:** one coherent feature per pull request, off `main`. Pull requests are squash-merged. Don't push, or open, edit or close a pull request, without the maintainer's approval.
  - Large work may be split into a stack of dependent pull requests with GitHub's stacked pull requests (`gh stack`). Each layer is one reviewable concern and passes every check on its own. A layer that changes behaviour starts with its own failing `test:` commit; run `just red-first <branch below>` for it. A stack merges bottom-up with `gh stack merge`.
  - The squash commit's subject is the pull request title, and the changelog is built from it, so the title is a Conventional Commit written for users. Mark a change that breaks the public contract in §12.1 with `!` (`feat!: …`) and say in the body how to upgrade.
  - **Hotfixes go through pull requests too.** Nobody pushes to `main` directly; the ruleset allows no direct push. A bad merge is first undone with a revert pull request, then fixed properly. A hotfix is an ordinary pull request with every required check.
  - **Break-glass:** only the maintainer may merge a pull request whose required checks fail, through the ruleset's pull-request-only bypass, and only when a check can't pass for a reason outside the change: a GitHub Actions outage, a broken action or toolchain release, or a yanked crate. Each use gets an issue that says why, and fixing that check comes next. Agents never use the bypass.
- **Screenshots and fixtures:** use neutral demo data and scratch repositories, never real backups, hostnames or paths.
- **License:** MIT OR Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
