// Tests for scripts/release-test-packages.ts, which picks the packages
// `just test-release` runs, their features, and the development features
// their tests must not turn on. `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { leaks, type Metadata, parseTree, releaseRoot, releaseTestPackages } from "./release-test-packages.ts";

const root = "/work/ladderback";

function metadata(packages: { name: string; dir: string; features: Record<string, string[]> }[]): Metadata {
  return {
    workspace_root: root,
    packages: packages.map(({ name, dir, features }) => ({ name, manifest_path: `${root}${dir}/Cargo.toml`, features })),
  };
}

test("a package with default features is run, in cargo metadata's order", () => {
  const workspace = metadata([
    { name: "store", dir: "/crates/adapters/store", features: { default: ["hub"], hub: [] } },
    { name: "l10n", dir: "/crates/l10n", features: { default: ["pseudo"], pseudo: ["dep:fluent-pseudo"] } },
  ]);
  assert.deepEqual(releaseTestPackages(workspace), ["store", "l10n"]);
});

test("a package without default features, or with an empty default, is left out", () => {
  const workspace = metadata([
    { name: "domain", dir: "/crates/domain", features: {} },
    { name: "engine", dir: "/crates/adapters/engine", features: { "history-fixture": [] } },
    { name: "app", dir: "/crates/app", features: { default: [], fakes: [] } },
  ]);
  assert.deepEqual(releaseTestPackages(workspace), []);
});

test("the release builds the package at the workspace's root", () => {
  const workspace = metadata([
    { name: "l10n", dir: "/crates/l10n", features: {} },
    { name: "ladderback", dir: "", features: {} },
  ]);
  assert.equal(releaseRoot(workspace), "ladderback");
  assert.throws(() => releaseRoot(metadata([{ name: "l10n", dir: "/crates/l10n", features: {} }])), /workspace root/);
});

test("the tree gives each workspace crate the union of its features, and ignores the rest", () => {
  const output = [
    `mcp v0.5.0 (${root}/crates/adapters/mcp)|`,
    `l10n v0.5.0 (${root}/crates/l10n)|icu`,
    "serde v1.0.228|derive,std",
    "[dev-dependencies]",
    `l10n v0.5.0 (${root}/crates/l10n)|icu,pseudo (*)`,
    "store v9.9.9 (/elsewhere/store)|hub",
    `web v9.9.9 (${root}-fork/crates/web)|hub`,
  ].join("\n");
  assert.deepEqual(
    parseTree(output, root),
    new Map([
      ["mcp", []],
      ["l10n", ["icu", "pseudo"]],
    ]),
  );
});

test("a workspace crate's hub or pseudo is a leak; any other feature isn't", () => {
  const tree = new Map([
    ["l10n", ["icu", "pseudo"]],
    ["store", ["hub"]],
    ["app", ["fakes"]],
  ]);
  assert.deepEqual(leaks(tree), ["l10n/pseudo", "store/hub"]);
  assert.deepEqual(leaks(new Map([["l10n", ["icu"]]])), []);
});
