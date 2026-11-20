// Steps of the hub's pairing dialog (features/ui/pairing_dialog.feature;
// §16, decision 121, "The hub's dialog"): a daemon of the scenario's own
// serving as a hub, the token the page made as the hub answered it, and what
// the dialog shows, keeps and cancels. Elements are found by role and
// accessible name, as in every journey.

import assert from "node:assert/strict";
import { cp, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import { Given, Then, When } from "@cucumber/cucumber";
import type { BrowserContext, Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { journey, namePattern } from "../support/keyboard.ts";
import { patienceMs } from "../support/patience.ts";
import { type BrowserWorld, fixtureRepo, startServer } from "../support/world.ts";
import { pressUntilFocusIsOn } from "./accessibility.ts";
import { ownBase } from "./api_tokens.ts";
import { folders, setPassword } from "./sign_in.ts";

/** The dialog that shows a new pairing token. */
const DIALOG = "Copy the pairing token";

/** How long the page may still hold the token once the dialog is left: a
 *  check that it doesn't stay, so a window of its own, as for an API token. */
const TOKEN_GONE_MS = 10_000;

// A page's words isolate each name they hold (§4.5), as `\u2068…\u2069`;
// what a reader hears has no such marks.
const ISOLATION_MARKS = /[\u2066-\u2069]/g;

/** A pairing token as the hub answered the page that made it. */
interface Made {
  token: string;
  id: string;
}

/** The last token the hub made for the page, per scenario. */
const made = new WeakMap<BrowserWorld, Made>();

/** The token the hub made for the page, which a step before made. */
function madeToken(world: BrowserWorld): Made {
  const token = made.get(world);
  assert.ok(token, "the hub made no pairing token for the page");
  return token;
}

/** Text field `name` of a JSON answer, when it has one. */
function field(answer: unknown, name: string): string | undefined {
  if (typeof answer !== "object" || answer === null || !(name in answer)) return undefined;
  const value: unknown = Reflect.get(answer, name);
  return typeof value === "string" ? value : undefined;
}

/** The hub's key as the page reads it: its fingerprint and key ID. */
async function hubKey(world: BrowserWorld): Promise<{ fingerprint: string; keyId: string }> {
  const answer = await world.context.request.get(`${ownBase(world)}/api/v1/hub`);
  assert.equal(answer.status(), 200, `the hub's key could not be read: ${await answer.text()}`);
  const read: unknown = await answer.json();
  const fingerprint = field(read, "fingerprint");
  const keyId = field(read, "key-id");
  assert.ok(fingerprint && keyId, `the hub's key reads ${JSON.stringify(read)}`);
  return { fingerprint, keyId };
}

/** The cancels the page sent, per scenario, each as its answer's status and
 *  the token's ID. */
const pageCancels = new WeakMap<BrowserWorld, string[]>();

/** Keeps each token the hub makes for the page as it answers, and each
 *  cancel the page sends. */
function keepMadeTokens(world: BrowserWorld): void {
  world.contextRoutes.push(async (context: BrowserContext) => {
    context.on("response", async (response) => {
      const request = response.request();
      const cancel = /\/api\/v1\/hub\/pairing-tokens\/([^/?]+)$/.exec(response.url());
      if (request.method() === "DELETE" && cancel) {
        pageCancels.set(world, [...(pageCancels.get(world) ?? []), `${response.status()} ${cancel[1]}`]);
        return;
      }
      if (request.method() !== "POST" || !response.url().endsWith("/api/v1/hub/pairing-tokens") || response.status() !== 201) {
        return;
      }
      const answer: unknown = await response.json();
      const token = field(answer, "token");
      const id = field(answer, "token-id");
      if (token && id) made.set(world, { token, id });
    });
  });
}

/** Starts the scenario's own daemon serving as a hub, its folders in a
 *  scratch folder the After hook of live.ts removes. */
async function startHub(world: BrowserWorld, scratch: string): Promise<void> {
  const { child, url } = await startServer(scratch, folders(scratch), ["--hub", "--listen", "127.0.0.1:0"]);
  world.ownServer = { child, scratch };
  world.baseUrl = url;
  keepMadeTokens(world);
}

/** Moves focus to the one element with this role and name by keyboard
 *  alone: past the header by the skip link when focus isn't in the content
 *  yet, then Tab until it is there. No press when it is there already. */
async function tabTo(world: BrowserWorld, role: string, name: string): Promise<void> {
  const inContent = await world.page.evaluate(() => document.querySelector("main")?.contains(document.activeElement) ?? false);
  if (!inContent) {
    const keys = journey(world.page);
    await keys.press("Tab");
    await keys.press("Enter");
  }
  await pressUntilFocusIsOn(world, "Tab", role, name);
}

function dialog(world: BrowserWorld): Locator {
  return world.page.getByRole("dialog", { name: namePattern(DIALOG) });
}

Given("a hub with sign-in off", async function (this: BrowserWorld) {
  await startHub(this, await mkdtemp(path.join(tmpdir(), "ladderback-ui-hub-")));
});

Given("a hub with sign-in on and the admin password {string}", async function (this: BrowserWorld, password: string) {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-hub-"));
  await cp(fixtureRepo, path.join(scratch, "repo"), { recursive: true });
  await setPassword(scratch, password);
  await startHub(this, scratch);
});

// The browser's wall clock a day ahead of the hub's, as a machine whose clock
// was set wrong: the page counts a token's life from the hub's answer, never
// from its own wall clock.
Given("the browser's clock is a day ahead", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context: BrowserContext) => {
    await context.clock.install({ time: Date.now() + 24 * 60 * 60 * 1000 });
  });
});

When("I type the hub's key ID", async function (this: BrowserWorld) {
  const { keyId } = await hubKey(this);
  await this.page.keyboard.type(keyId);
});

// The steps a keyboard walk takes in the first scenario, in short: the key
// ID typed in its field, then the form sent from its button.
When("I make a pairing token on the Agents tab", async function (this: BrowserWorld) {
  await this.open("/settings#agents");
  const { keyId } = await hubKey(this);
  await tabTo(this, "textbox", "Hub key ID");
  await this.page.keyboard.type(keyId);
  await tabTo(this, "button", "Make a pairing token");
  await this.page.keyboard.press("Enter");
  await dialog(this).waitFor({ state: "visible", timeout: patienceMs() });
});

When("I set the address agents will use to {string}", async function (this: BrowserWorld, address: string) {
  await tabTo(this, "textbox", "Address agents will use");
  await this.page.keyboard.press("ControlOrMeta+A");
  await this.page.keyboard.type(address);
});

When("the page is printed", async function (this: BrowserWorld) {
  await this.page.emulateMedia({ media: "print" });
});

// The fingerprint in full, a character at a time (decision 121, "The
// fingerprint"), as the hub reads its own key. Read with
// locator.ariaSnapshot(), since toMatchAriaSnapshot() only works inside a
// Playwright test; the marks isolating the group's name aren't heard.
Then("the dialog {string} shows the hub key's fingerprint in full", async function (this: BrowserWorld, name: string) {
  const { fingerprint } = await hubKey(this);
  const spoken = [...fingerprint.replaceAll("-", "")].map((character) => `  - img "${character}"`).join("\n");
  const group = this.page
    .getByRole("dialog", { name: namePattern(name) })
    .getByRole("group", { name: namePattern("Hub key fingerprint") });
  const snapshot = (await group.ariaSnapshot()).trim().replace(ISOLATION_MARKS, "");
  assert.equal(snapshot, `- group "Hub key fingerprint":\n${spoken}`);
});

Then("the clipboard holds the pairing token the dialog showed", async function (this: BrowserWorld) {
  const { token } = madeToken(this);
  await expect.poll(() => this.page.evaluate(() => navigator.clipboard.readText())).toBe(token);
});

// The token is in a read-only field, so a keyboard can select and copy it
// where the page can't.
Then("the textbox {string} holds the pairing token the dialog showed", async function (this: BrowserWorld, name: string) {
  const { token } = madeToken(this);
  await expect(dialog(this).getByRole("textbox", { name: namePattern(name) })).toHaveValue(token);
});

// The page canceled the token, and the hub agreed: its cancel answered 204,
// and a cancel now finds none waiting.
Then("the hub has canceled the token the dialog showed", async function (this: BrowserWorld) {
  const { id } = madeToken(this);
  await expect
    .poll(() => (pageCancels.get(this) ?? []).includes(`204 ${id}`), { message: "the page's cancel of the token answered 204" })
    .toBe(true);
  const base = ownBase(this);
  const session: unknown = await (await this.context.request.get(`${base}/api/v1/session`)).json();
  const csrf = field(session, "csrf-token");
  const answer = await this.context.request.delete(`${base}/api/v1/hub/pairing-tokens/${id}`, {
    headers: { Origin: base, "Content-Type": "application/json", ...(csrf ? { "X-CSRF-Token": csrf } : {}) },
  });
  assert.equal(`${answer.status()} ${field(await answer.json().catch(() => null), "code") ?? ""}`, "404 pairing-token-unknown");
});

// The hub answers each cancel as it does when it can't keep its tokens
// (503 hub-unavailable), so the token still works until it lapses.
Given("the hub fails to cancel pairing tokens", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/hub/pairing-tokens/*", (route) =>
      route.request().method() === "DELETE"
        ? route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ code: "hub-unavailable" }) })
        : route.fallback(),
    );
  });
});

// I4, and the token is shown once: once the dialog is left, no text, no
// attribute and no field of the page holds it.
Then("nothing on the page holds the pairing token the dialog showed", async function (this: BrowserWorld) {
  const { token } = madeToken(this);
  await expect
    .poll(
      () =>
        this.page.evaluate(
          (secret) =>
            [
              document.documentElement.outerHTML,
              ...Array.from(document.querySelectorAll("input, textarea"), (input) =>
                input instanceof HTMLInputElement || input instanceof HTMLTextAreaElement ? input.value : "",
              ),
            ].some((text) => text.includes(secret)),
          token,
        ),
      { timeout: TOKEN_GONE_MS, message: "the page still holds the pairing token the dialog showed" },
    )
    .toBe(false);
});

// Neither its text nor any field of the dialog holds the token.
Then("the dialog {string} doesn't hold the pairing token it showed", async function (this: BrowserWorld, name: string) {
  const { token } = madeToken(this);
  const shown = this.page.getByRole("dialog", { name: namePattern(name) });
  await expect
    .poll(
      () =>
        shown.evaluate(
          (element, secret) =>
            [
              element.innerHTML,
              ...Array.from(element.querySelectorAll("input, textarea"), (input) =>
                input instanceof HTMLInputElement || input instanceof HTMLTextAreaElement ? input.value : "",
              ),
            ].some((text) => text.includes(secret)),
          token,
        ),
      { timeout: TOKEN_GONE_MS },
    )
    .toBe(false);
});

// The token's field is on the page, and a printout leaves it out.
Then("the dialog {string} shows no pairing token", async function (this: BrowserWorld, name: string) {
  const shown = this.page
    .getByRole("dialog", { name: namePattern(name) })
    .getByRole("textbox", { name: namePattern("The pairing token"), includeHidden: true });
  await expect(shown).toHaveCount(1);
  await expect(shown).toBeHidden();
});

Then("the page warns {string}", async function (this: BrowserWorld, words: string) {
  await expect(this.page.getByRole("main").getByText(words)).toBeVisible();
});

Then("the page warns of no address", async function (this: BrowserWorld) {
  await expect(this.page.getByRole("main").getByText("This address is")).toHaveCount(0);
});
