#!/usr/bin/env node
// The packages whose tests `just test-release` runs: those with default
// features, which a release build's `--no-default-features` turns off (the
// development pseudolocales, `pseudo`, and the hub, `hub`). A package without
// default features builds the same way in `just test`, so running its tests
// again would add time and no coverage. Read from `cargo metadata`, so a
// package's first default feature needs no list to add it to.
//
// Usage: cargo metadata --no-deps --format-version 1 | node scripts/release-test-packages.ts
// Prints one package name per line, in the order `cargo metadata` lists them.

import { readFileSync, realpathSync } from "node:fs";
import { pathToFileURL } from "node:url";

import { parseMetadata } from "./wasm-test-packages.ts";

/** The part of `cargo metadata`'s JSON this reads. */
export interface Metadata {
  packages: { name: string; features: Record<string, string[]> }[];
}

/** The names of the packages in `metadata` whose `default` feature turns on anything. */
export function releaseTestPackages(metadata: Metadata): string[] {
  return metadata.packages.filter((pkg) => (pkg.features.default ?? []).length > 0).map((pkg) => pkg.name);
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  for (const name of releaseTestPackages(parseMetadata<Metadata>(readFileSync(0, "utf8")))) {
    console.log(name);
  }
}
