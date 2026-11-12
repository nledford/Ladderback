# Decision 77: Which characters Go prints

**Whether Go's `strconv.Quote` writes a character as it is comes from a table `domain` carries, `GO_PRINT`: the characters Go's `unicode.IsPrint` takes, generated from the Unicode version of the Go that rclone is built with (Unicode 17, Go 1.27) by `scripts/gen-go-print-table.py`. It no longer asks Rust's `Debug`** (recorded 2026-11-12, §10.2, I4; #406). It replaces the reading of `Debug` that the Go-quoted form started with (#331).

Decided:

1. **Go's rule, Go's data.** `unicode.IsPrint` takes a letter, mark, number, punctuation or symbol (general categories L, M, N, P and S) and the space, by the categories of the Unicode version its `unicode` package was generated from (`unicode.Version`). The table holds exactly those characters of that version's `UnicodeData.txt`, as sorted, merged ranges of code points; a surrogate, a private-use, a format or an unassigned code point is not in it. `go_prints` looks a character up by binary search. `"` and `\` are in it, and `go_quoted` escapes them before it asks.
2. **Generated, not written.** The table is a `const` in `crates/domain/src/secrets/go_print.rs`, with no I/O and no dependency, and its header names the Unicode version, the file's URL and the command that wrote it. The script reads a given `UnicodeData.txt` or downloads the version's from unicode.org.
3. **Pinned to Go's own answer.** The every-character test compares the count of characters Go escapes and a hash of every character's quoted form with what Go 1.27.3 wrote; the table passed it unchanged, on Rust 1.99.1 and 1.100.0. When rclone moves to a Go on a newer Unicode version, the table is regenerated for it and the test pinned to that Go's answer.

Why:

- **`Debug` is not Go and changes with the toolchain.** Rust 1.100 escapes every grapheme extender, a combining mark after a letter too, and prints U+0558, which Unicode 17 hasn't assigned, so under it a secret holding such a character was not masked as rclone quotes it. Corrections for `Debug`'s escapes (`DEBUG_ESCAPES_GO_PRINTS`) would have to follow every toolchain; Go's data follows only rclone's Go.
- **A table over a crate.** A Unicode crate brings its own version, which need not be Go's, and a dependency to `domain`; 741 ranges are about 6 KiB in the binary.

Left as it was: a character one Unicode version has assigned and another not is still quoted differently by an rclone built with a Go on another version, and a secret holding one is not masked in that line; that residual risk is §10.2's.
