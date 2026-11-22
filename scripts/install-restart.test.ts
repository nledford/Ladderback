// Tests for the arguments scripts/install-restart.ts gives `just install`,
// and for its keyring check (step 2a). The script as a whole, with
// stand-ins for the build, the installed program and the service manager,
// runs in features/cli/install_restart.feature.
// `just lint` runs these with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { type KeyringCheck, keyringRead, rootProblem, stagingArgs } from "./install-restart.ts";

/** A keyring check whose checks come to `results` in turn, and a person who presses Enter when `presses`; what it said and asked. */
function world(results: boolean[], attended: boolean, presses = true) {
  const said: string[] = [];
  const asked: string[] = [];
  let checks = 0;
  const keyring: KeyringCheck = {
    check: () => results[checks++] ?? assert.fail(`checked ${checks} times`),
    attended,
    ask: async (line) => {
      asked.push(line);
      return presses;
    },
    say: (line) => said.push(line),
  };
  return { keyring, said, asked, checks: () => checks };
}

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

test("a keyring check that reads every entry goes on, asking nothing", async () => {
  const { keyring, said, asked, checks } = world([true], true);
  assert.equal(await keyringRead(keyring), true);
  assert.equal(checks(), 1);
  assert.deepEqual(asked, []);
  assert.deepEqual(said, []);
});

test("without a terminal, a failed keyring check stops at once: nobody is there to answer", async () => {
  const { keyring, said, asked, checks } = world([false, true], false);
  assert.equal(await keyringRead(keyring), false);
  assert.equal(checks(), 1);
  assert.deepEqual(asked, []);
  assert.match(said.join("\n"), /couldn't read every keyring entry/);
});

test("at a terminal, a failed keyring check keeps the new build and checks again once Enter is pressed", async () => {
  const { keyring, said, asked, checks } = world([false, true], true);
  assert.equal(await keyringRead(keyring), true);
  assert.equal(checks(), 2);
  assert.equal(asked.length, 1);
  const question = asked[0] ?? "";
  assert.match(question, /the new build is kept/);
  assert.match(question, /behind other windows/);
  assert.match(question, /Always Allow/);
  assert.match(question, /then press Enter to check again/);
  assert.match(said.join("\n"), /couldn't read every keyring entry/);
});

test("at a terminal, a second failed check stops, saying what to do next, and asks no more", async () => {
  const { keyring, said, asked, checks } = world([false, false], true);
  assert.equal(await keyringRead(keyring), false);
  assert.equal(checks(), 2);
  assert.equal(asked.length, 1);
  assert.match(said.at(-1) ?? "", /still couldn't read every keyring entry/);
  assert.match(said.at(-1) ?? "", /run just install-restart again/);
});

test("at a terminal, input that ends instead of Enter stops without checking again", async () => {
  const { keyring, asked, checks } = world([false, true], true, false);
  assert.equal(await keyringRead(keyring), false);
  assert.equal(checks(), 1);
  assert.equal(asked.length, 1);
});
