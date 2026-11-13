# Decision 84: Formatting the inside of `view!` blocks

**leptosfmt formats the inside of every `view!` block, at a version the Justfile pins, with rustfmt's settings; `just fmt` runs it before `cargo fmt` and `just fmt-check` checks it. A comment about a view's Rust code goes at node level, above the element or block it explains, because leptosfmt drops comments inside that code, and a check stops a run that would drop one** (recorded 2026-11-13, §13.13; #382).

Decided:

1. **The tool.** leptosfmt 0.1.33 (`MIT OR Apache-2.0`), installed by `just setup` and by CI's `Check` job on Linux, never a crate dependency. `leptosfmt.toml` holds rustfmt's width and indentation. `just fmt` and `just fmt-check` refuse any other version.
2. **Order.** leptosfmt, then `cargo fmt`. leptosfmt's output can leave a closure rustfmt still changes (`.map(|path| view! { … })` gains braces); rustfmt after it reaches a state both accept, which `just fmt-check` checks.
3. **Comments.** A comment inside a view's Rust code (an attribute's closure, a `{…}` child) moves to node level, directly above the element or block it explains, where leptosfmt keeps it; one that would follow a one-line text or `{…}` child, which leptosfmt moves onto that child, goes above the parent element. `scripts/leptosfmt-comments.ts` runs leptosfmt on each file's text and fails when the output has fewer `//` than the input, or when a second run changes it, before `just fmt` writes and in `just fmt-check`.

Why:

- **Hand-formatted views carry unchecked layout.** rustfmt leaves macro bodies alone, so each `view!` was laid out by hand, and a diff there mixed layout with the change.
- **It changes layout, not meaning.** On `crates/web` at 5a31aad, a token-by-token comparison of every file before and after a run, with comments kept as tokens: string literals, attributes, their order, `aria-*` values and message IDs identical in all 24 files, no parse failure. The other changes are braces around a closure or match-arm body broken over several lines, their commas, and trailing commas in calls it splits.
- **Except comments in code.** Its README says leptosfmt doesn't support non-doc comments in a view's code blocks: it formats them with a prettyplease fork built on `syn`, which keeps no comments. On that run it dropped 5 comments and moved 4, one to below the statement it explained; the 7 comments between nodes stayed in place. With the comments moved to node level first, a run moved and dropped none.

Alternatives:

- **Keep formatting views by hand** (as before): no tool to install, but the layout inside views stays unchecked.
- **Allow comments in a view's code and guard by a comment-position check**: keeps a comment beside the line it explains, but needs a lexer in the check, and leptosfmt would still move the comment on every run.
- **Wait for comment support upstream**: the README calls it a known limitation the author keeps to stay close to upstream prettyplease, and the last release was 2025-01-30.

Cost: a tool to install and pin, one more step in `just fmt-check`, and a comment about a handler sits above the element rather than inside the closure. leptosfmt's last release predates Leptos 0.8; it parses today's views. A view it can't parse it skips without a word, so that view stays laid out by hand and unchecked; a file that doesn't parse as Rust fails `just fmt` and `just fmt-check`. The guard counts comments, so a comment leptosfmt moves within a file without dropping one passes it; the rule above keeps comments where leptosfmt doesn't move them.
