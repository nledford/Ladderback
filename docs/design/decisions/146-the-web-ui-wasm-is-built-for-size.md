# Decision 146: The release web UI's wasm is built for size, with a profile of its own

**`just web-release`, and with it `just install`, `just release-build` and the release archives and image built from it, builds the web UI's wasm with the `wasm-release` profile: the `release` profile with `opt-level = "z"`, `lto = "fat"` and `codegen-units = 1`, passed to trunk as `--cargo-profile wasm-release` beside `--release`. The daemon keeps the `release` and `dist` profiles, and `just web` and `just web-test` keep `release`** (#1428, part of #1371; §8.1, §13.11).

## Why

Cargo's release defaults (`opt-level = 3`, no LTO, 16 codegen units) suit the daemon, which runs where it is installed. The wasm is downloaded and compiled by a browser after every update, and speed-first optimization inlines and unrolls Leptos's generic view code; without LTO across codegen units, the linker can't merge or drop what crosses crate boundaries, and `wasm-opt -Oz` afterwards recovers only part of it. Measured in #1428 and again on this change, the size profile takes 44 % off the wasm and 28 % off its Brotli copy, and the page loads sooner (§8.1).

## Decided

1. **A profile of its own.** `[profile.wasm-release]` in the root `Cargo.toml` inherits `release`, so a later change to `release` reaches both, and nothing else selects it: the host binary's profiles are unchanged.
2. **`z`, fat LTO, one codegen unit**, chosen by measurement over `s` and over each setting alone: LTO saves almost nothing at `opt-level = 3`, and on top of `z` takes off another 23 % of the raw wasm. `s` with the same LTO is 81 KB larger in Brotli and costs no script time; `z`'s extra cost is about 1 ms a change of page (§8.1).
3. **`--release` stays.** Trunk keeps running wasm-opt for a release build, and sets `TRUNK_PROFILE` to `release` whatever Cargo profile it is given, so the precompression hook (§10.1) still writes the Brotli and gzip copies the binary embeds.
4. **Only the release builds.** `just web` and `just web-test` rebuild the UI on every change to it, and fat LTO over one codegen unit redoes the whole program's optimization each time; they keep `release`. The `@ui` scenarios therefore run against a build that differs from the shipped one in optimization, as it already did in its features (the gallery and the pseudolocales), not in what the pages do.

## What it costs

- **Slower release builds of the UI.** Fat LTO over one codegen unit can't run in parallel: from an empty target folder, #1428 measured 200 s against 155 s.
- **Slower script.** A change of page spends about 1 ms longer in script at full speed and 6 ms with the CPU slowed 4×: 96 ms against 84 ms over 10 changes, 481 ms against 422 ms slowed (§8.1). The wall-clock time of a page change didn't move past its noise; a page whose own work grows can measure it again against `s`.
- **The `@ui` scenarios don't run the build a release ships.** They run `release`; 13 table, Operations and snapshot scenarios passed against a `wasm-release` build of the gallery when this was decided.
