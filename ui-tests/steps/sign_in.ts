// Steps of the sign-in journeys (features/security/sign_in.feature): a daemon
// of the scenario's own with the admin password set, the page's clock moved on
// to a session's limits, and what the form and the dialogs say. Elements are
// found by role and accessible name, as in every journey.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { cp, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import { Given, Then, When } from "@cucumber/cucumber";
import { expect } from "playwright/test";

import { namePattern } from "../support/keyboard.ts";
import { type BrowserWorld, binary, fixtureRepo, startServer } from "../support/world.ts";
import { expand } from "./repositories.ts";

// How long a dialog or a focus move the page makes takes to show, with room.
const PAGE_PATIENCE_MS = 10_000;

/** The folders a scenario's daemon keeps its data, config and cache in. */
function folders(scratch: string): Record<string, string> {
  return {
    LADDERBACK_CONFIG_DIR: path.join(scratch, "config"),
    LADDERBACK_DATA_DIR: path.join(scratch, "data"),
    LADDERBACK_CACHE_DIR: path.join(scratch, "cache"),
  };
}

/** Sets the admin password of the daemon whose folders are in `scratch`, as
 *  a person would from standard input: `set-password` never takes it as an
 *  argument. */
async function setPassword(scratch: string, password: string): Promise<void> {
  const child = spawn(binary, ["set-password"], {
    stdio: ["pipe", "ignore", "pipe"],
    env: { ...process.env, LC_ALL: "C", LANGUAGE: "", ...folders(scratch) },
  });
  let output = "";
  child.stderr.on("data", (chunk: Buffer) => {
    output += chunk;
  });
  child.stdin.end(`${password}\n`);
  const [code] = (await once(child, "exit")) as [number | null];
  assert.equal(code, 0, `set-password failed:\n${output}`);
}

// A daemon of the scenario's own with sign-in on, and a scratch copy of the
// `demo-repo` fixture beside it for the journeys that test a repository. The
// After hook of live.ts stops the daemon and removes the folder.
Given(
  "a daemon with sign-in on and the admin password {string}",
  async function (this: BrowserWorld, password: string) {
    const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-sign-in-"));
    await cp(fixtureRepo, path.join(scratch, "repo"), { recursive: true });
    await setPassword(scratch, password);
    const { child, url } = await startServer(folders(scratch));
    this.ownServer = { child, scratch };
    this.baseUrl = url;
  },
);

// The daemon reads the admin password's hash as it checks each session, so
// one set anew ends every session at once, with no restart.
When("the admin password is set anew to {string}", async function (this: BrowserWorld, password: string) {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  await setPassword(own.scratch, password);
});

// A second tab of the same browser shares the first's cookie, and so its
// session, as a reader's tabs do.
When("I open the address {string} in a second tab", async function (this: BrowserWorld, address: string) {
  await this.openSecondTab(address);
});

When("I go back to the first tab", async function (this: BrowserWorld) {
  await this.backToFirstTab();
});

// The token a password manager reads to know what the field is for.
Then(
  "the textbox {string} has the autocomplete token {string}",
  async function (this: BrowserWorld, name: string, token: string) {
    const field = this.page.getByRole("textbox", { name: namePattern(name) });
    await expect(field).toHaveAttribute("autocomplete", token);
  },
);

Then("the textbox {string} still holds {string}", async function (this: BrowserWorld, name: string, text: string) {
  const field = this.page.getByRole("textbox", { name: namePattern(name) });
  await expect(field).toHaveValue(await expand(this, text));
});

// The page's timers then run on a clock the steps can move on, hours at a
// time, while it otherwise keeps time as a real one does.
Given("the page's clock can be moved on", function (this: BrowserWorld) {
  this.fakeClock = true;
});

// "11:58:00" is hours, minutes and seconds. Nothing the reader does happens
// meanwhile; what the page sends on its own goes on.
When("the page is left unused for {string}", async function (this: BrowserWorld, time: string) {
  assert.ok(this.fakeClock, "the page's clock can't be moved on: no Given installed it");
  await this.page.clock.fastForward(time);
});

Then(
  "the dialog {string} opens with focus on the button {string}",
  async function (this: BrowserWorld, dialog: string, button: string) {
    const opened = this.page.getByRole("dialog", { name: namePattern(dialog) });
    await opened.waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
    await expect(opened.getByRole("button", { name: namePattern(button) })).toBeFocused({
      timeout: PAGE_PATIENCE_MS,
    });
  },
);

// The first reading of the history is the page opening, which a person
// caused; every later one, after an event or a reconnect, the page sent on
// its own and says so, so it never keeps the session from ending unused.
Then("the page marked each later reading of the history as its own", function (this: BrowserWorld) {
  const daemon = this.scriptedDaemon;
  assert.ok(daemon, "this scenario's daemon is not scripted");
  const marks = daemon.backgroundMarks;
  assert.ok(marks.length >= 2, `the page read the history ${marks.length} times, never again after it opened`);
  assert.deepEqual(
    marks,
    [false, ...marks.slice(1).map(() => true)],
    "which readings of the history the page marked as its own",
  );
});
