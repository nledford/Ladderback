# Decision 105: Clippy lints for complexity, nesting and indexing

**Beyond clippy's default `all`, `[workspace.lints.clippy]` turns on six lints, and `just lint` fails on any warning: `cognitive_complexity` (clippy's default threshold, 25), `excessive_nesting` (6 blocks), `allow_attributes`, `allow_attributes_without_reason`, `indexing_slicing` (tests exempt) and `string_slice`. An exemption is `#[expect(…, reason = "…")]`, never `#[allow]`. The settings are in every `clippy.toml`, and `just architecture` checks that the files agree and that no inner `#![allow(…)]` is left, which clippy doesn't see** (recorded 2026-11-15, §13.5 rule 7; decided by the maintainer on #730).

## Why

- **Rules in words, checked.** AGENTS.md and §13.5 rule 2 ask for `get()` rather than `[i]` or `[a..b]`, and string slices only at char boundaries; until now nothing checked either. `indexing_slicing` and `string_slice` do, outside tests.
- **Tests may index.** In a test, an index out of range is a failure with a message, which is what the test wants (`allow-indexing-slicing-in-tests`). Clippy has no such exemption for `string_slice`, and to clippy a contract suite, a cucumber step file or other test support outside `#[test]` functions and `cfg(test)` modules isn't a test: those use `get()`, iterators or patterns too.
- **Complexity that can't land unnoticed.** Three functions were over 25 (`RunOperation::drive` 27, the `rich` contract suite 27, the golden test of the profile writer 33), and `ChooseLocation::try_each` and a test of the domain's secrets nested past six blocks. Each was split or flattened, none exempted. `excessive_nesting` catches deep blocks that the cognitive score misses. Clippy has no cyclomatic-complexity lint.
- **`#[expect]` over `#[allow]`.** An expectation that no longer fires is a warning of its own, so a stale exemption can't stay, and its reason has to prove the case. `allow_attributes` sees outer attributes only, so `just architecture` refuses an inner `#![allow(…)]`. An exemption needed in some builds only is written under the `cfg_attr` of those builds: in the `web` crate, the shared components without the `gallery` feature, and the toast without it and outside its tests.
- **Every `clippy.toml`.** Clippy reads the nearest one only, so the root file and those of `domain` and `app` each carry `excessive-nesting-threshold` and `allow-indexing-slicing-in-tests`, as they already did the test exemptions of rule 1.

## The counts

On the host build before the change, clippy flagged 48 `allow` attributes (24 in production code, 24 in tests), 40 `indexing_slicing` hits, 16 `string_slice`, 6 `excessive_nesting` and 3 `cognitive_complexity`. After it, none, on the host, on wasm32 with and without the gallery, and in the release build (`--no-default-features`).

The tree held 68 `allow` attributes, inner ones and those of other builds included. The sweep turned each into an `expect`, and 41 of the lints they named no longer fired: 38 attributes were deleted and one lost a lint, which leaves 30. One deleted attribute, the `dead_code` of the interop tests' shared helpers, moved to the four test files where it still fires.

Two exemptions of the new lints remain, both `string_slice` in the configuration editor's `above` and `below` (`crates/adapters/config/src/edit.rs`), where the offset is 0 or the end of a line `split_inclusive` gave.

## Left out

- `too_many_lines`: it mostly flags exhaustive `match`es over closed enums, which are long because the enum is.
- `missing_docs`: 1,186 hits, a decision of its own.
- `arithmetic_side_effects`: it flags all arithmetic, not only arithmetic on input-derived numbers, which rule 2 already covers.
