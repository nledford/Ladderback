// Steps of approving a waiting machine on the hub's page
// (features/ui/approving_machines.feature; §16, decision 121, "Waiting for
// approval"): machines of the scenario's own that pair with its hub through
// `ladderback pair`, each on folders of its own, and what the Machines tab
// lists and its review shows. Elements are found by role and accessible
// name, as in every journey.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import path from "node:path";

import { Given, Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { namePattern } from "../support/keyboard.ts";
import { patienceMs } from "../support/patience.ts";
import { type BrowserWorld, binary, binaryEnv } from "../support/world.ts";
import { ownBase } from "./api_tokens.ts";
import { tabInDialog } from "./accessibility.ts";
import { ISOLATION_MARKS, tabTo } from "./pairing_dialog.ts";
import { folders } from "./sign_in.ts";

/** The key fingerprint each machine showed when it paired, by hostname, per
 *  scenario. */
const shownKeys = new WeakMap<BrowserWorld, Map<string, string>>();

/** The folders of the machine named `hostname`, inside the scenario's own
 *  scratch folder, which the After hook of live.ts removes. */
function machineScratch(world: BrowserWorld, hostname: string): string {
  assert.ok(world.ownServer, "no Given started a hub of the scenario's own");
  return path.join(world.ownServer.scratch, `machine-${hostname}`);
}

/** Runs `ladderback args` on the folders of the machine named `hostname`,
 *  with `input` on its standard input: what it wrote to standard error, once
 *  it succeeded. */
async function onMachine(world: BrowserWorld, hostname: string, args: string[], input = ""): Promise<string> {
  const scratch = machineScratch(world, hostname);
  const child = spawn(binary, args, {
    stdio: ["pipe", "ignore", "pipe"],
    env: binaryEnv(scratch, folders(scratch)),
  });
  let said = "";
  child.stderr.on("data", (chunk: Buffer) => {
    said += chunk;
  });
  child.stdin.end(input);
  const [code] = (await once(child, "exit")) as [number | null];
  assert.equal(code, 0, `ladderback ${args.join(" ")} failed:\n${said}`);
  return said;
}

/** Asks the hub for `route` as its own page would with sign-in off, from
 *  its origin, and answers with its status and its body's text: a Given
 *  runs before the scenario's page and its browser context exist, so it
 *  asks outside them. */
async function askHub(world: BrowserWorld, route: string, body?: unknown): Promise<{ status: number; text: string }> {
  const base = ownBase(world);
  const answer = await fetch(`${base}${route}`, {
    method: body === undefined ? "GET" : "POST",
    headers: { Origin: base, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return { status: answer.status, text: await answer.text() };
}

/** The hub key's ID, which a step-up with sign-in off asks for. */
async function hubKeyId(world: BrowserWorld): Promise<string> {
  const answer = await askHub(world, "/api/v1/hub");
  assert.equal(answer.status, 200, `the hub's key could not be read: ${answer.text}`);
  const { "key-id": keyId } = JSON.parse(answer.text) as { "key-id"?: string };
  assert.ok(keyId, "the hub's key has no ID");
  return keyId;
}

/** A machine named `hostname` pairs with a token the hub's page made, and
 *  waits for approval: the key it showed is kept. */
async function pairMachine(world: BrowserWorld, hostname: string): Promise<void> {
  const base = ownBase(world);
  const keyId = await hubKeyId(world);
  const made = await askHub(world, "/api/v1/hub/pairing-tokens", { address: base, confirmation: keyId });
  assert.equal(made.status, 201, `the hub made no token: ${made.text}`);
  const { token } = JSON.parse(made.text) as { token: string };
  const said = await onMachine(world, hostname, ["pair", base, "--hostname", hostname, "--yes"], `${token}\n`);
  const key = /This machine's key: ([0-9A-Z-]+)\./.exec(said)?.[1];
  assert.ok(key, `the machine showed no key:\n${said}`);
  const keys = shownKeys.get(world) ?? new Map<string, string>();
  keys.set(hostname, key);
  shownKeys.set(world, keys);
}

function shownKey(world: BrowserWorld, hostname: string): string {
  const key = shownKeys.get(world)?.get(hostname);
  assert.ok(key, `no machine named ${hostname} paired`);
  return key;
}

/** What a screen reader hears of a fingerprint group named `name`. */
function spokenFingerprint(name: string, fingerprint: string): string {
  const characters = [...fingerprint.replaceAll("-", "")].map((character) => `  - img "${character}"`).join("\n");
  return `- group "${name}":\n${characters}`;
}

async function showsFingerprint(scope: Locator, hostname: string, fingerprint: string): Promise<void> {
  const name = `${hostname}'s key fingerprint`;
  const group = scope.getByRole("group", { name: namePattern(name) });
  await expect(group).toBeVisible();
  const snapshot = (await group.ariaSnapshot()).trim().replace(ISOLATION_MARKS, "");
  assert.equal(snapshot, spokenFingerprint(name, fingerprint));
}

function table(world: BrowserWorld, name: string): Locator {
  return world.page.getByRole("table", { name: namePattern(name) });
}

Given("a machine named {string} waits for approval on the hub", async function (this: BrowserWorld, hostname: string) {
  await pairMachine(this, hostname);
});

Given("a machine named {string} is approved on the hub", async function (this: BrowserWorld, hostname: string) {
  await pairMachine(this, hostname);
  const listed = await askHub(this, "/api/v1/hub/agents");
  assert.equal(listed.status, 200, `the hub listed no machines: ${listed.text}`);
  const { agents } = JSON.parse(listed.text) as { agents: { "agent-id": string; hostname: string; key: string }[] };
  const machine = agents.find((agent) => agent.hostname === hostname);
  assert.ok(machine, `the hub lists no ${hostname}`);
  const approved = await askHub(this, `/api/v1/hub/agents/${machine["agent-id"]}/approval`, {
    key: machine.key,
    confirmation: await hubKeyId(this),
  });
  assert.equal(approved.status, 200, `the hub approved nothing: ${approved.text}`);
});

When(
  "I approve {string} on the Machines tab, typing a key ID that doesn't match",
  async function (this: BrowserWorld, hostname: string) {
    const keyId = await hubKeyId(this);
    await tabTo(this, "button", `Approve ${hostname}`);
    await this.page.keyboard.press("Enter");
    await tabInDialog(this, "textbox", "Hub key ID");
    await this.page.keyboard.type(keyId === "00000000" ? "11111111" : "00000000");
    await tabInDialog(this, "button", `Approve ${hostname}`);
    await this.page.keyboard.press("Enter");
  },
);

Then(
  "the table {string} lists {string} with the key fingerprint the machine showed",
  async function (this: BrowserWorld, name: string, hostname: string) {
    const row = table(this, name)
      .getByRole("row")
      .filter({ has: this.page.getByRole("rowheader", { name: namePattern(hostname) }) });
    await showsFingerprint(row, hostname, shownKey(this, hostname));
  },
);

Then("the dialog {string} shows the key fingerprint the machine showed", async function (this: BrowserWorld, name: string) {
  const hostname = /^Approve (.+)\?$/.exec(name)?.[1];
  assert.ok(hostname, `a review named for its machine: ${name}`);
  await showsFingerprint(this.page.getByRole("dialog", { name: namePattern(name) }), hostname, shownKey(this, hostname));
});

// The note names the other machine as fetched text, between isolation marks
// a reader doesn't hear.
Then("the table {string} says {string}", async function (this: BrowserWorld, name: string, words: string) {
  await expect.poll(async () => ((await table(this, name).textContent()) ?? "").replace(ISOLATION_MARKS, "")).toContain(words);
});

// The page reads the list again every few seconds: one read, and the frames
// it is drawn in.
When("the page reads the list of machines again", async function (this: BrowserWorld) {
  await this.page.waitForResponse((answer) => new URL(answer.url()).pathname === "/api/v1/hub/agents", {
    timeout: patienceMs(),
  });
  await this.page.evaluate(() => new Promise((drawn) => requestAnimationFrame(() => requestAnimationFrame(drawn))));
});

Then("the page lists no machine waiting for approval", async function (this: BrowserWorld) {
  await expect(table(this, "Waiting for approval")).toHaveCount(0);
  await expect(this.page.getByText("No machine is waiting for approval.")).toBeVisible();
});

Then("the machine {string} is told the hub approved it", async function (this: BrowserWorld, hostname: string) {
  const said = await onMachine(this, hostname, ["hub-status"]);
  assert.match(said, /Connected\. The hub approved this machine\./);
});
