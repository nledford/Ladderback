# AGENTS.md

Guidance for coding agents working on Steading, a self-hosted backup manager for [rustic](https://rustic.cli.rs). It is built on `rustic_core` in Rust, with a Leptos (CSR) web UI.

## Status

Milestone M0 (skeleton) is in place: the workspace, the worker protocol with a fake operation, the embedded hello UI and every CI check. Next is M1a ([docs/TECHNICAL-DESIGN.md §14](docs/TECHNICAL-DESIGN.md)).

## Commands

Every workflow is a `just` recipe, and CI runs the same recipes. `just` lists them.

| Command | Does |
|---|---|
| `just setup` | Installs the wasm target, trunk, cargo-mutants and release-plz (through cargo-binstall), the UI test dependencies and Chromium |
| `just verify` | Everything CI checks except mutants and red-first: `fmt-check`, `lint`, `architecture`, `test`, `test-ui` |
| `just fmt` | Formats Rust code |
| `just lint` | Clippy with `-D warnings` on the host crates and on `web` for wasm32, plus the journey lint (no pointer actions in `ui-tests/steps`) |
| `just architecture` | Banned crates in `domain` and `app`, the `domain` wasm32 build, identical banned-call lists (`crates/{domain,app}/clippy.toml`) |
| `just test [args]` | Unit, contract, acceptance and `@e2e` tests (`cargo test --workspace`) |
| `just test-acceptance [args]` / `just test-e2e [args]` | One cucumber-rs runner; pass `--name <regex>` to pick scenarios |
| `just test-ui [args]` | Builds the web UI and the binary, then runs the `@ui` scenarios |
| `just mutants [base]` | `cargo mutants` on `domain` and `app` code changed since `base` (default `origin/main`) |
| `just red-first [base]` | Checks that the branch's first commit is a `test:` commit whose new tests fail at run time (§13.1) |
| `just serve` | Builds everything and runs `steading serve` on `127.0.0.1:9898` |
| `just release-preview` | Shows the version bump and changelog entry the next release PR would make (§12.1) |

The cucumber runners use `harness = false`, so `cargo nextest` can't run them; use `cargo test` for those. Don't hand-edit the version or `CHANGELOG.md`: the release PR does both (§12.1).

## Read first

| File | Owns |
|---|---|
| [PRODUCT.md](PRODUCT.md) | Users, purpose, voice ("calm and exact"), boundaries, accessibility floor |
| [docs/TECHNICAL-DESIGN.md](docs/TECHNICAL-DESIGN.md) | Invariants (§0), domain model (§3.1), architecture (§3.2), workspace layout (§8.3), localization (§8.4), accessibility (§4.5), versioning and releases (§12.1), development method (§13), milestones (§14), decisions (§16) |

Each fact lives in one file. Link to it rather than copying it. The design document is the source of truth: when work changes a decision, update the document, including §16, in the same change.

## Invariants

§0 of the technical design lists safety invariants I1–I7 and accessibility invariants A1–A6. They are not negotiable. A change that weakens one doesn't merge, and every change that touches one adds or updates the test that proves it. Above all:

- Exactly one maintenance owner per repository (I1). Prune never deletes instantly (I2).
- Resolved secrets never appear in argv, environments, logs, templates, sync messages or the UI (I4).
- Forget deletes only snapshots a plan owns or the user explicitly selected (I6).

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
- The core returns typed errors and codes, never user-facing sentences. Text is produced through `l10n` (§8.4).

**Test first (TDD).**

- No production code without a failing test that requires it: red, green, refactor. A bug fix starts with a test that reproduces the bug.
- In a pull request, the first commit is a `test:` commit that fails at run time, with `todo!()` stubs so it compiles. Implementation commits follow. CI's red-first check verifies this (§13.1).
- Mutation testing (`cargo mutants`) gates changed code in `domain`, `app`, `adapters/config` and `adapters/interop`. A surviving mutant means a behaviour no test asserts.
- There is no coverage gate. Test behaviour, boundaries, invariants and errors. Don't test wiring, mock echoes, source text or incidental defaults.
- Doubles replace ports only. Domain behaviour is never mocked. A new port gets a contract suite that runs against both its fake and its real adapter.

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

## Conventions

- **Accessibility:** every UI change meets WCAG 2.2 AA through the shared component set (§4.5). axe-core must be clean and the journey must work by keyboard alone. New components need contract tests.
- **Localization:** no hard-coded user-facing strings. Use message IDs from `l10n`, Fluent terms for glossary words, and complete sentences. Code never concatenates fragments (§8.4).
- **Docs:** keep [PRODUCT.md](PRODUCT.md) and the technical design current with the code. Record new decisions in §16.
- **Commits:** atomic, in logical groups. Each commit is one coherent change for one reason: it builds, passes its tests (except the deliberately failing `test:` commit, §13.1), and can be reviewed and reverted on its own.
  - Split unrelated work, formatting, refactors and dependency updates into separate commits. Stage hunks selectively when one file holds changes for different commits.
  - Never stage secrets, local config or generated artifacts.
  - Use Conventional Commits (`feat:`, `fix:`, `test:`, `docs:`, `refactor:`, `chore:`) with an imperative subject and a body explaining why.
  - The atomic commits are for review; the pull request is squash-merged at the end.
- **Pull requests:** one coherent feature per pull request, off `main`. Pull requests are squash-merged. Don't push, or open, edit or close a pull request, without the maintainer's approval.
  - The squash commit's subject is the pull request title, and the changelog is built from it, so the title is a Conventional Commit written for users. Mark a change that breaks the public contract in §12.1 with `!` (`feat!: …`) and say in the body how to upgrade.
- **Screenshots and fixtures:** use neutral demo data and scratch repositories, never real backups, hostnames or paths.
- **License:** MIT OR Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
