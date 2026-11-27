# Decision 171: A guide-only lane, and a label that skips a pull request's mutants

**A pull request whose every path is docs, with at least one under `docs/guide/`, runs what a docs-only pull request runs and the guide's example tests (`crates/adapters/config/tests/guide.rs`) alone; everything else skips as for docs-only, and every required check still reports. A pull request labeled `no-behavior-change` skips only its mutation testing, which reports as skipped; every test still runs, and the nightly and `main` runs are unaffected. Beside `behaviour` the label fails the pull request's mutation check. A label counts as of the pull request's latest CI run: adding or removing it takes effect at the next push or re-run** (recorded 2026-11-26, §13.1, §13.8; the maintainer's decision).

**Amended 2026-11-27 (#1188):** the guide lane runs every test target named `guide`, with the same command: beside `crates/adapters/config/tests/guide.rs`, `crates/adapters/mcp/tests/guide.rs` checks the guide's table of MCP tools against the server's `tools/list`.

## Why

- **The guide.** The guide counted as code, because `guide.rs` parses its `config.toml` examples and checks its keyring entries. So a wording fix in the guide ran every Rust suite on Linux and macOS, the UI journeys, the mutants, the wasm tests and the release-build tests, for one test file that reads Markdown.
- **The label.** A rename or a spelling change touches many lines of the gated crates while changing no behavior. The pull request's mutation run then re-mutated those lines, whole crates' worth, for hours, while the nightly full run tests every mutant anyway, a day after the merge at the latest. The author knows when a change can't change behavior; the label says so, and the review checks it.

## Decided

1. **The guide lane.** `scripts/changed-paths.sh` (which ci.yml's `Changed paths` job runs, with node tests that `just lint` runs) counts `docs/guide/` as docs and says `guide=true` when a path is under it. On a docs-only pull request with `guide=true`, `Check (ubuntu-latest)` sets up Rust and runs `cargo nextest run --workspace --test guide`: only that test target, with `--workspace` so the build's features match every other build and the restored cache. No job's `if` changes: every job that skips for docs-only skips here too. A guide change beside code runs `guide.rs` in `just test`, as before.
2. **What the guide lane leaves to code.** A guide page that the web UI's source (`crates/web/src`) or a UI scenario (`features/ui`) names counts as code, since a UI journey checks the heading such a link leads to (`pairing_dialog.feature`), and so do the site's theme scripts (`docs/guide/theme/*.js`), which Biome checks in `just lint`.
3. **The label skips only the pull request's mutants.** `Changed paths` passes the script the pull request's labels; with `no-behavior-change` it says `mutants=false`, as for a docs-only pull request, and the shards and `Mutation testing (changed code)` are skipped. No other job reads the label. The nightly run (`Mutation testing (full, shard k of 32)`) reads no label. Labels match whatever their case, as red-first.yml's do.
4. **The clash fails.** Beside `behaviour` (a change to behavior, whose mutants must be tested) the script refuses, and `Changed paths` fails with "The labels 'no-behavior-change' and 'behaviour' contradict each other". `Mutation testing (changed code)` then runs and fails, so the merge is blocked until one label goes and CI runs again.
5. **A label counts from the next run.** ci.yml doesn't run on `labeled` or `unlabeled`, so adding or removing the label takes effect at the next push, or at a re-run of the latest run (`gh run rerun <run-id>`, the whole run, so `Changed paths` reads the labels again). `Changed paths` reads them from the pull request when it runs (`pulls/<n>`, with `pull-requests: read`), not from the event, so a re-run sees them as they are. Every check on a commit comes from one run that read one set of labels: no skipped check stands for a run that hasn't reported yet.
6. **A failed `Changed paths` fails closed for the mutants.** It runs no shard, and `Mutation testing (changed code)` fails, naming it; the other jobs run as for code. Before, it ran four shards; a failure now can't skip the mutants either.

## Considered and rejected

- **Re-running on a label change.** Adding `labeled` and `unlabeled` to ci.yml re-runs the whole suite, macOS included, on every label change (`coderabbit-pending`, `dependencies`, and `behaviour` as a pull request opens). Moving the mutation jobs to a workflow of their own that a label change re-runs avoids that, but then the earlier run's skipped required check stands as the latest of its name until the new run's shards end, so a pull request whose label was just removed could merge without its mutants; closing that needs a required job that polls the run's jobs. A label that counts from the next run is the smallest safe design.
- **A separate job per guide lane, or a new condition on every job.** The `docs-only` output already skips what a guide change can't reach; one output (`guide`) and two step conditions in `Check` add the guide's tests without touching any job's `if`.

## Limits

- **A label counts from the next run.** Adding the label to a pull request whose mutants are running doesn't stop them, and removing it doesn't start them, until a push or `gh run rerun`. The label's description says so.
- **The label is the author's word.** Nothing checks that a change labeled `no-behavior-change` changes no behavior; the review does, and the nightly run finds a surviving mutant a day after the merge at the latest.
- **The guide lane is Linux alone.** `guide.rs` reads text and has no platform code; macOS's leg skips as for docs-only.
- **A link the UI builds at run time.** Point 2 finds a guide page by its path in the web UI's source or a UI scenario; a link that names the page any other way would leave a guide-only change to it docs-only.
