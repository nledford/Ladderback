// Tests for the arguments scripts/install-restart.ts gives `just install`.
// The script as a whole, with stand-ins for the build, the installed program
// and the service manager, runs in features/cli/install_restart.feature.
// `just lint` runs these with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { rootProblem, stagingArgs } from "./install-restart.ts";

test("the build goes to the staging folder, with the other arguments as given", () => {
  assert.deepEqual(stagingArgs(["--features", "hub", "--locked"], "/stage"), ["--root", "/stage", "--features", "hub", "--locked"]);
  assert.deepEqual(stagingArgs([], "/stage"), ["--root", "/stage"]);
});

test("a --root given for the install gives way to the staging folder, in either form", () => {
  assert.deepEqual(stagingArgs(["--root", "/home/me/.local", "--locked"], "/stage"), ["--root", "/stage", "--locked"]);
  assert.deepEqual(stagingArgs(["--locked", "--root=/home/me/.local"], "/stage"), ["--root", "/stage", "--locked"]);
});

test("the staging folder comes before an argument that ends the options", () => {
  assert.deepEqual(stagingArgs(["--locked", "--", "x"], "/stage"), ["--root", "/stage", "--locked", "--", "x"]);
});

test("a --root with a folder is fine, in either form", () => {
  assert.equal(rootProblem([]), undefined);
  assert.equal(rootProblem(["--root", "/home/me/.local", "--locked"]), undefined);
  assert.equal(rootProblem(["--root=/home/me/.local"]), undefined);
});

test("a --root with no folder is refused, as cargo refuses it", () => {
  for (const args of [["--root"], ["--locked", "--root"], ["--root="], ["--root", ""], ["--root", "--force"]]) {
    assert.equal(rootProblem(args), "--root needs a folder", JSON.stringify(args));
  }
});
