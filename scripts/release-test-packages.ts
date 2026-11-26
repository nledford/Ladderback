#!/usr/bin/env node
// What `just test-release` runs: each package with default features (the
// development pseudolocales, `pseudo`), which a release
// build's flags turn off, and the features to test it with: the ones the
// release build itself turns on in that package (`l10n`'s `icu`). A package
// without default features builds the same way in `just test`, so running its
// tests again would add time and no coverage. All of it is read from Cargo
// (`cargo metadata`, `cargo tree`), so a package's first default feature, or
// a feature the release starts to turn on, needs no list to add it to.
//
// Before any test runs, it checks each package's test build, dev-dependencies
// included, as `cargo tree` resolves it for the build nextest makes: if that
// build turns on a workspace crate's `pseudo`, those tests would not
// be testing the build a release is, and it fails, naming the package and
// printing the path that turns the feature on.
//
// Usage: node scripts/release-test-packages.ts <release flags…>
// Prints one line per package, in the order `cargo metadata` lists them: its
// name, a tab, and the features to turn on, comma-separated (empty for none).

import { execFileSync } from "node:child_process";
import { realpathSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { parseMetadata } from "./wasm-test-packages.ts";

/** The part of `cargo metadata`'s JSON this reads. */
export interface Metadata {
  workspace_root: string;
  packages: { name: string; manifest_path: string; features: Record<string, string[]> }[];
}

/** The features a release build leaves out that no test build of a package may turn back on. */
export const DEVELOPMENT_FEATURES = ["pseudo"];

/** The names of the packages in `metadata` whose `default` feature turns on anything. */
export function releaseTestPackages(metadata: Metadata): string[] {
  return metadata.packages.filter((pkg) => (pkg.features.default ?? []).length > 0).map((pkg) => pkg.name);
}

/** The package at the workspace's root: the program a release builds. */
export function releaseRoot(metadata: Metadata): string {
  const manifest = path.join(metadata.workspace_root, "Cargo.toml");
  const root = metadata.packages.find((pkg) => pkg.manifest_path === manifest);
  if (root === undefined) throw new Error(`no package at the workspace root (${manifest})`);
  return root.name;
}

/**
 * The features each workspace crate is built with in `output`, from
 * `cargo tree --prefix none --format '{p}|{f}'`: a workspace crate is one whose
 * path lies under `workspaceRoot`. A crate listed twice (once per edge kind)
 * gets the union.
 */
export function parseTree(output: string, workspaceRoot: string): Map<string, string[]> {
  const crates = new Map<string, Set<string>>();
  for (const line of output.split("\n")) {
    const bar = line.lastIndexOf("|");
    const id = line.slice(0, bar);
    if (bar < 0 || !(id.includes(`(${workspaceRoot})`) || id.includes(`(${workspaceRoot}/`))) continue;
    const name = line.slice(0, line.indexOf(" "));
    const features = line
      .slice(bar + 1)
      .replace(/ \(\*\)$/, "")
      .split(",")
      .filter((feature) => feature !== "");
    const known = crates.get(name) ?? new Set<string>();
    for (const feature of features) known.add(feature);
    crates.set(name, known);
  }
  return new Map([...crates].map(([name, features]) => [name, [...features].sort()]));
}

/** The development features (`crate/feature`) that `tree` turns on. */
export function leaks(tree: Map<string, string[]>): string[] {
  return [...tree].flatMap(([name, features]) =>
    features.filter((feature) => DEVELOPMENT_FEATURES.includes(feature)).map((feature) => `${name}/${feature}`),
  );
}

function cargo(args: string[]): string {
  return execFileSync("cargo", args, { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"], maxBuffer: 1 << 26 });
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  const flags = process.argv.slice(2);
  const metadata = parseMetadata<Metadata>(cargo(["metadata", "--no-deps", "--format-version", "1"]));
  const tree = (pkg: string, edges: string, features: string[]) => {
    const select = features.length > 0 ? ["--features", features.join(",")] : [];
    // `--color never`: CI sets CARGO_TERM_COLOR=always, which wraps the ` (*)`
    // marker of a repeated crate in escape codes that `parseTree` would read
    // as a feature.
    const output = cargo([
      "tree",
      "--package",
      pkg,
      ...flags,
      ...select,
      "--edges",
      edges,
      "--prefix",
      "none",
      "--format",
      "{p}|{f}",
      "--color",
      "never",
    ]);
    const crates = parseTree(output, metadata.workspace_root);
    // A tree that doesn't list the package itself was misread: fail rather than find nothing.
    if (!crates.has(pkg)) throw new Error(`cargo tree of ${pkg} lists no ${pkg} under ${metadata.workspace_root}`);
    return { crates, select };
  };
  const release = tree(releaseRoot(metadata), "normal,build", []).crates;
  const plan: string[] = [];
  let leaked = false;
  for (const pkg of releaseTestPackages(metadata)) {
    const features = release.get(pkg) ?? [];
    const { crates, select } = tree(pkg, "normal,build,dev", features);
    for (const leak of leaks(crates)) {
      leaked = true;
      const crate = leak.slice(0, leak.indexOf("/"));
      console.error(`error: the tests of ${pkg}, built as a release is, turn on ${leak}, which a release leaves out:`);
      console.error(cargo(["tree", "--package", pkg, ...flags, ...select, "--edges", "features,normal,build,dev", "--invert", crate]));
    }
    plan.push(`${pkg}\t${features.join(",")}`);
  }
  if (leaked) process.exit(1);
  for (const line of plan) console.log(line);
}
