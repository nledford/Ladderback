// Tests for scripts/release-test-packages.ts, which picks the packages
// `just test-release` runs. `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { type Metadata, releaseTestPackages } from "./release-test-packages.ts";

test("a package with default features is run, in cargo metadata's order", () => {
  const metadata: Metadata = {
    packages: [
      { name: "store", features: { default: ["hub"], hub: [] } },
      { name: "l10n", features: { default: ["pseudo"], pseudo: ["dep:fluent-pseudo"] } },
    ],
  };
  assert.deepEqual(releaseTestPackages(metadata), ["store", "l10n"]);
});

test("a package without default features, or with an empty default, is left out", () => {
  const metadata: Metadata = {
    packages: [
      { name: "domain", features: {} },
      { name: "engine", features: { "history-fixture": [] } },
      { name: "app", features: { default: [], fakes: [] } },
    ],
  };
  assert.deepEqual(releaseTestPackages(metadata), []);
});
