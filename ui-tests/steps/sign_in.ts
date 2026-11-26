// Steps of the sign-in journeys (features/security/sign_in.feature): a daemon
// of the scenario's own with the admin password set, the page's clock moved on
// to a session's limits, and what the form and the dialogs say. Elements are
// found by role and accessible name, as in every journey.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { cp, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import { Given, Then, When } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import { namePattern } from "../support/keyboard.ts";
import { patienceMs, untilSettled } from "../support/patience.ts";
import { type BrowserWorld, binary, binaryEnv, fixtureRepo, startServer } from "../support/world.ts";
import { expand } from "./repositories.ts";

/** The folders a scenario's daemon keeps its data, config and cache in. */
export function folders(scratch: string): Record<string, string> {
  return {
    LADDERBACK_CONFIG_DIR: path.join(scratch, "config"),
    LADDERBACK_DATA_DIR: path.join(scratch, "data"),
    LADDERBACK_CACHE_DIR: path.join(scratch, "cache"),
  };
}

/** Sets the admin password of the daemon whose folders are in `scratch`, as
 *  a person would from standard input: `set-password` never takes it as an
 *  argument. */
export async function setPassword(scratch: string, password: string): Promise<void> {
  const child = spawn(binary, ["set-password"], {
    stdio: ["pipe", "ignore", "pipe"],
    env: binaryEnv(scratch, folders(scratch)),
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
Given("a daemon with sign-in on and the admin password {string}", async function (this: BrowserWorld, password: string) {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-sign-in-"));
  await cp(fixtureRepo, path.join(scratch, "repo"), { recursive: true });
  await setPassword(scratch, password);
  const { child, url } = await startServer(scratch, folders(scratch));
  this.ownServer = { child, scratch };
  this.baseUrl = url;
});

// A daemon of the scenario's own with sign-in off, listening on every address
// with one host name configured: on the network, as a home server is. The
// page is opened at the loopback address, which reaches the same daemon and
// which the request checks always admit, so no name has to resolve to it.
Given(
  "a daemon with sign-in off listening on every address for the host name {string}",
  async function (this: BrowserWorld, name: string) {
    const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-network-"));
    const { child, url } = await startServer(scratch, folders(scratch), ["--listen", "0.0.0.0:0", "--allowed-host", name]);
    this.ownServer = { child, scratch };
    const address = new URL(url);
    address.hostname = "127.0.0.1";
    this.baseUrl = address.origin;
  },
);

// A daemon of the scenario's own with sign-in off, on loopback, and no admin
// password yet: one set while it runs waits for a restart to turn sign-in on.
Given("a daemon with sign-in off", async function (this: BrowserWorld) {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-sign-in-off-"));
  const { child, url } = await startServer(scratch, folders(scratch));
  this.ownServer = { child, scratch };
  this.baseUrl = url;
});

// Set from standard input while the daemon runs, as a person would.
When("the admin password {string} is set", async function (this: BrowserWorld, password: string) {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  await setPassword(own.scratch, password);
});

// Written while the daemon runs, as a damaged disk or an editor would leave
// it: the next answer the daemon gives says the file is unreadable.
When("the data folder holds an admin password file that is not a password hash", async function (this: BrowserWorld) {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  await writeFile(path.join(own.scratch, "data", "admin-password"), "not a password hash\n", { mode: 0o600 });
});

// What a screen reader reads after the page's heading, which takes focus when
// a reader navigates to the page: a notice above it would be passed over.
Then("the page's heading is described by {string}", async function (this: BrowserWorld, needle: string) {
  const escaped = needle.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  await expect(this.page.getByRole("heading", { level: 1 })).toHaveAccessibleDescription(new RegExp(escaped));
});

// The daemon reads the admin password's hash as it checks each session, so
// one set anew ends every session at once, with no restart: the answers a
// scripted daemon plays for it too.
When("the admin password is set anew to {string}", async function (this: BrowserWorld, password: string) {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  await setPassword(own.scratch, password);
  this.scriptedDaemon?.endSessions();
});

// A page's reading the daemon holds back: whether it has reached the daemon,
// and how to let it through.
interface HeldReading {
  arrived: Promise<void>;
  letThrough: () => void;
}

// One per scenario: the world is new for each.
const heldReadings = new WeakMap<BrowserWorld, HeldReading>();

// The readings a scenario can hold back, by what the page reads, and the
// address each is read from.
const HELD_READINGS: Record<string, string> = {
  "the start page's reading of the plans list": "/api/v1/plans",
  "the Settings page's reading of how long signing in lasts": "/api/v1/settings/sign-in",
  "the Settings page's reading of what AI agents may do without a token": "/api/v1/settings/without-token",
};

for (const [reading, address] of Object.entries(HELD_READINGS)) {
  // The first reading waits at the daemon's door until the scenario lets it
  // through, so the daemon answers it only after what the scenario does
  // meanwhile. Every later reading goes through at once.
  Given(`the daemon holds back its answer to ${reading}`, function (this: BrowserWorld) {
    const { promise: arrived, resolve: arrive } = Promise.withResolvers<void>();
    const { promise: through, resolve: letThrough } = Promise.withResolvers<void>();
    heldReadings.set(this, { arrived, letThrough });
    let readings = 0;
    this.contextRoutes.push(async (context) => {
      await context.route(
        (url) => url.pathname === address,
        async (route) => {
          if (route.request().method() === "GET") {
            readings += 1;
            if (readings === 1) {
              arrive();
              await through;
            }
          }
          await route.continue();
        },
      );
    });
  });

  When(`the daemon gives the answer it held back to ${reading}`, async function (this: BrowserWorld) {
    const held = heldReadings.get(this);
    assert.ok(held, `no Given held back ${reading}`);
    await untilSettled(held.arrived, `the page never made ${reading}`);
    held.letThrough();
  });

  // As above, then waits until the answer has reached the page whole, so a
  // step after it sees what the page does with it, not the wait before it.
  When(`the daemon gives the answer it held back to ${reading}, and the page has it`, async function (this: BrowserWorld) {
    const held = heldReadings.get(this);
    assert.ok(held, `no Given held back ${reading}`);
    await untilSettled(held.arrived, `the page never made ${reading}`);
    const answered = this.page.waitForResponse((response) => new URL(response.url()).pathname === address, {
      timeout: patienceMs(),
    });
    held.letThrough();
    await (await answered).finished();
  });
}

// A page that lost its session reads the history again on its own, a back-off
// after the reading the dialog was closed on: this waits for the daemon's
// refusal of it to reach the page.
When("the daemon refuses the page's own next reading of the history", async function (this: BrowserWorld) {
  await this.page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/api/v1/operations" &&
      response.request().headers()["x-ladderback-background"] === "1" &&
      response.status() === 401,
    { timeout: patienceMs() },
  );
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
Then("the textbox {string} has the autocomplete token {string}", async function (this: BrowserWorld, name: string, token: string) {
  const field = this.page.getByRole("textbox", { name: namePattern(name) });
  await expect(field).toHaveAttribute("autocomplete", token);
});

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
    await opened.waitFor({ state: "visible", timeout: patienceMs() });
    await expect(opened.getByRole("button", { name: namePattern(button) })).toBeFocused();
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
  assert.deepEqual(marks, [false, ...marks.slice(1).map(() => true)], "which readings of the history the page marked as its own");
});
