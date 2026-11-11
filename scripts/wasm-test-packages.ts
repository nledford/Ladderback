#!/usr/bin/env node
// The packages that have wasm-bindgen tests: those with a `wasm-bindgen-test`
// dependency. `just test-wasm` and `scripts/red-first.mjs` both ask here, so
// the `Wasm tests (Node)` job and the red-first check can't disagree about
// which packages to run, and a package's first wasm test needs no list to add
// it to (#60).
//
// Usage: cargo metadata --no-deps --format-version 1 | node scripts/wasm-test-packages.mjs
// Prints one package name per line, in the order `cargo metadata` lists them,
// and nothing when no package has a wasm-bindgen test.

import { readFileSync, realpathSync } from "node:fs";
import { pathToFileURL } from "node:url";

/** The names of the packages in `cargo metadata`'s `metadata` that depend on `wasm-bindgen-test`. */
export function wasmTestPackages(metadata) {
  return metadata.packages
    .filter((pkg) => pkg.dependencies.some((dependency) => dependency.name === "wasm-bindgen-test"))
    .map((pkg) => pkg.name);
}

/**
 * The parsed `cargo metadata` JSON in `output`. Cargo's own notes (or a
 * wrapper's) can share the stream; the JSON is the line that opens with a brace.
 */
export function parseMetadata(output) {
  const json = output.split("\n").find((line) => line.startsWith("{"));
  if (json === undefined) throw new Error("no cargo metadata JSON in the output");
  return JSON.parse(json);
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  for (const name of wasmTestPackages(parseMetadata(readFileSync(0, "utf8")))) {
    console.log(name);
  }
}
