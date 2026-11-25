// Steps of the hub's pairing dialog (features/ui/pairing_dialog.feature;
// §16, decision 121, "The hub's dialog"): a daemon of the scenario's own
// serving as a hub, the token the page made as the hub answered it, and what
// the dialog shows, keeps and cancels. Elements are found by role and
// accessible name, as in every journey.

import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { cp, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";

import { Given, Then, When } from "@cucumber/cucumber";
import type { BrowserContext, Locator, Request } from "playwright";

import { expect } from "../support/expect.ts";
import { focusComesTo, journey, namePattern, pressUntilFocused } from "../support/keyboard.ts";
import { patienceMs, untilSettled } from "../support/patience.ts";
import { type BrowserWorld, binary, binaryEnv, fixtureRepo, startServer } from "../support/world.ts";
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
export const ISOLATION_MARKS = /[\u2066-\u2069]/g;

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

/** The hub key's fingerprint as the page reads it. */
async function hubFingerprint(world: BrowserWorld): Promise<string> {
  const answer = await world.context.request.get(`${ownBase(world)}/api/v1/hub`);
  assert.equal(answer.status(), 200, `the hub's key could not be read: ${await answer.text()}`);
  const read: unknown = await answer.json();
  const fingerprint = field(read, "fingerprint");
  assert.ok(fingerprint, `the hub's key reads ${JSON.stringify(read)}`);
  return fingerprint;
}

/** The hub key's fingerprint the hub's terminal printed, per scenario. */
const printedHubKeys = new WeakMap<BrowserWorld, string>();

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
export async function tabTo(world: BrowserWorld, role: string, name: string): Promise<void> {
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

// `ladderback pairing-token` at the hub's terminal before its daemon starts,
// as a person makes a token without the page: it prints the hub key's
// fingerprint, which the page must show as the same string.
Given("a hub with sign-in off whose terminal printed its key's fingerprint", async function (this: BrowserWorld) {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-hub-"));
  const { stderr } = await promisify(execFile)(binary, ["pairing-token", "--address", "http://127.0.0.1:9"], {
    env: binaryEnv(scratch, folders(scratch)),
  });
  const printed = /The hub key: (\S+)/.exec(stderr.replace(ISOLATION_MARKS, ""))?.[1];
  assert.ok(printed, `the terminal printed no hub key:\n${stderr}`);
  printedHubKeys.set(this, printed);
  await startHub(this, scratch);
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

When("I type the hub key's fingerprint", async function (this: BrowserWorld) {
  await this.page.keyboard.type(await hubFingerprint(this));
});

// A paste of what the page's own Copy button put on the clipboard, by the
// keyboard, as a person's paste lands.
When("I paste what the page copied", async function (this: BrowserWorld) {
  await journey(this.page).press("ControlOrMeta+V");
});

// The step-up's description holds the value to type, as the terminal
// printed it: one string for the hub key on every screen (#1344).
Then(
  "the textbox {string} is described by the fingerprint the hub's terminal printed",
  async function (this: BrowserWorld, name: string) {
    const printed = printedHubKeys.get(this);
    assert.ok(printed, "no step had the hub's terminal print its key");
    const field = this.page.getByRole("textbox", { name: namePattern(name) });
    await expect(field).toHaveAccessibleDescription(new RegExp(printed));
  },
);

/** Asks for a token as a keyboard walk does, in short: the fingerprint typed
 *  in its field, then the form sent from its button. */
async function askForToken(world: BrowserWorld): Promise<void> {
  await world.open("/settings#machines");
  const fingerprint = await hubFingerprint(world);
  await tabTo(world, "textbox", "Hub key fingerprint");
  await world.page.keyboard.type(fingerprint);
  await tabTo(world, "button", "Make a pairing token");
  await world.page.keyboard.press("Enter");
}

When("I ask for a pairing token on the Machines tab", async function (this: BrowserWorld) {
  await askForToken(this);
});

// The steps a keyboard walk takes in the first scenario, in short, until
// the dialog shows the token.
When("I make a pairing token on the Machines tab", async function (this: BrowserWorld) {
  await askForToken(this);
  await dialog(this).waitFor({ state: "visible", timeout: patienceMs() });
});

// Back on the form once a dialog closed, without opening the page again:
// the fingerprint typed in its field, which is before the button, then the
// form sent again. The closing dialog hands focus back to its button a frame
// later; a walk started before then has focus move under its first check.
When("I make another pairing token", async function (this: BrowserWorld) {
  await focusComesTo(
    this.page,
    this.page.getByRole("button", { name: namePattern("Make a pairing token") }),
    'the button "Make a pairing token"',
  );
  const fingerprint = await hubFingerprint(this);
  await pressUntilFocusIsOn(this, "Shift+Tab", "textbox", "Hub key fingerprint");
  await this.page.keyboard.type(fingerprint);
  await pressUntilFocusIsOn(this, "Tab", "button", "Make a pairing token");
  await this.page.keyboard.press("Enter");
  await dialog(this).waitFor({ state: "visible", timeout: patienceMs() });
});

// Settings is left within the page, so its work on the way still answers;
// once the hub holds a making, only after the page has sent it. The link is
// reached back from where focus is, by keyboard.
When("I leave Settings by the link {string}", async function (this: BrowserWorld, name: string) {
  const held = heldMakes.get(this);
  if (held) await untilSettled(held.request, "the page sent no making for the hub to hold");
  const link = this.page.getByRole("navigation", { name: namePattern("Pages") }).getByRole("link", { name: namePattern(name) });
  await pressUntilFocused(this.page, "Shift+Tab", link, `the link "${name}"`);
  await journey(this.page).press("Enter");
  await expect(this.page.getByRole("heading", { level: 1, name: namePattern(name) })).toBeVisible();
});

When("I set the address machines will use to {string}", async function (this: BrowserWorld, address: string) {
  await tabTo(this, "textbox", "Address machines will use");
  await this.page.keyboard.press("ControlOrMeta+A");
  await this.page.keyboard.type(address);
});

// The hub serves its built-in TLS on the port its page came in on, so its
// https address is the page's own over TLS.
When("I set the address machines will use to the hub's https address", async function (this: BrowserWorld) {
  const page = new URL(this.baseUrl ?? "");
  await tabTo(this, "textbox", "Address machines will use");
  await this.page.keyboard.press("ControlOrMeta+A");
  await this.page.keyboard.type(`https://${page.host}`);
});

When("the page is printed", async function (this: BrowserWorld) {
  await this.page.emulateMedia({ media: "print" });
});

// The fingerprint in full, a character at a time (decision 121, "The
// fingerprint"), as the hub reads its own key. Read with
// locator.ariaSnapshot(), since toMatchAriaSnapshot() only works inside a
// Playwright test; the marks isolating the group's name aren't heard.
Then("the dialog {string} shows the hub key's fingerprint in full", async function (this: BrowserWorld, name: string) {
  const fingerprint = await hubFingerprint(this);
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

/** The page canceled the token the hub made for it, and the hub agreed:
 *  its cancel answered 204, and a cancel now finds none waiting. */
async function hubCanceled(world: BrowserWorld): Promise<void> {
  const { id } = madeToken(world);
  await expect
    .poll(() => (pageCancels.get(world) ?? []).includes(`204 ${id}`), { message: "the page's cancel of the token answered 204" })
    .toBe(true);
  const base = ownBase(world);
  const session: unknown = await (await world.context.request.get(`${base}/api/v1/session`)).json();
  const csrf = field(session, "csrf-token");
  const answer = await world.context.request.delete(`${base}/api/v1/hub/pairing-tokens/${id}`, {
    headers: { Origin: base, "Content-Type": "application/json", ...(csrf ? { "X-CSRF-Token": csrf } : {}) },
  });
  assert.equal(`${answer.status()} ${field(await answer.json().catch(() => null), "code") ?? ""}`, "404 pairing-token-unknown");
}

Then("the hub has canceled the token the dialog showed", async function (this: BrowserWorld) {
  await hubCanceled(this);
});

// A token made once its dialog can no longer show it: nobody saw it, so the
// page cancels it.
Then("the hub has canceled the token it made", async function (this: BrowserWorld) {
  await hubCanceled(this);
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

/** A request of the page's the hub holds, and the step's way to let the
 *  hub answer it. */
interface Held {
  request: Promise<Request>;
  answer: () => void;
}

/** The page's first cancel of a token the hub holds, per scenario. */
const heldCancels = new WeakMap<BrowserWorld, Held>();
/** The page's first making of a token the hub holds, per scenario. */
const heldMakes = new WeakMap<BrowserWorld, Held>();

/** Holds the page's first `method` request to `url` until a step lets the
 *  hub answer it: as the hub does, or, when `fails`, as a hub that can't
 *  keep its tokens (503 hub-unavailable). Every other request goes to the
 *  hub at once. */
function holdFirst(world: BrowserWorld, held: WeakMap<BrowserWorld, Held>, method: string, url: string, fails: boolean): void {
  const answered = Promise.withResolvers<void>();
  const request = Promise.withResolvers<Request>();
  held.set(world, { request: request.promise, answer: answered.resolve });
  let holding = false;
  world.contextRoutes.push(async (context) => {
    await context.route(url, async (route) => {
      if (route.request().method() !== method || holding) {
        await route.fallback();
        return;
      }
      holding = true;
      request.resolve(route.request());
      await answered.promise;
      if (fails) {
        await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ code: "hub-unavailable" }) });
      } else {
        await route.fallback();
      }
    });
  });
}

/** Lets the hub answer what it held, and waits until the page has the
 *  answer and a frame has passed, so what the page does with it is done. */
async function answerHeld(world: BrowserWorld, held: Held | undefined, what: string): Promise<void> {
  assert.ok(held, `the hub holds no ${what}`);
  const request = await untilSettled(held.request, `the page sent no ${what} for the hub to hold`);
  held.answer();
  await untilSettled(request.response(), `the page got no answer to its ${what}`);
  await world.page.evaluate(() => {
    const { promise, resolve } = Promise.withResolvers<void>();
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    return promise;
  });
}

// The page's first cancel of a token waits for its answer until a step lets
// the hub give it, so the page is left while it is on its way.
Given("the hub holds its answer to the page's first cancel of a pairing token", function (this: BrowserWorld) {
  holdFirst(this, heldCancels, "DELETE", "**/api/v1/hub/pairing-tokens/*", false);
});

// As above, and the answer, when it comes, is that the hub couldn't cancel it.
Given("the hub holds the page's first cancel of a pairing token, then fails it", function (this: BrowserWorld) {
  holdFirst(this, heldCancels, "DELETE", "**/api/v1/hub/pairing-tokens/*", true);
});

// The page's first making of a token waits for its answer until a step lets
// the hub give it, so Settings is left while it is on its way.
Given("the hub holds its answer to the page's first making of a pairing token", function (this: BrowserWorld) {
  holdFirst(this, heldMakes, "POST", "**/api/v1/hub/pairing-tokens", false);
});

When("the hub answers the cancel it held", async function (this: BrowserWorld) {
  await answerHeld(this, heldCancels.get(this), "cancel");
});

When("the hub answers the making it held", async function (this: BrowserWorld) {
  await answerHeld(this, heldMakes.get(this), "making");
});

// The hub's key never comes: the page's read of it waits with no answer.
Given("the hub doesn't answer the page's read of its key", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/hub", (route) =>
      route.request().method() === "GET" ? Promise.withResolvers<void>().promise : route.fallback(),
    );
  });
});

// The hub answers the page's first read of its key, and refuses each one
// after it as a hub that can't read its keys (503 hub-unavailable).
Given("the hub refuses each read of its key after the first", function (this: BrowserWorld) {
  let reads = 0;
  this.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/hub", async (route) => {
      if (route.request().method() !== "GET") {
        await route.fallback();
        return;
      }
      reads += 1;
      if (reads === 1) {
        await route.fallback();
      } else {
        await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ code: "hub-unavailable" }) });
      }
    });
  });
});

// Only the held cancel went out for the token, and the hub agreed to it: a
// second one, sent as the dialog was left, would find no token and answer
// 404.
Then("the page sent the hub one cancel of the token the dialog showed, which it agreed to", async function (this: BrowserWorld) {
  const { id } = madeToken(this);
  const held = heldCancels.get(this);
  assert.ok(held, "the hub holds no cancel");
  const response = await untilSettled((await held.request).response(), "the held cancel was never answered");
  const answered = `${response?.status() ?? "no answer"} ${id}`;
  await expect.poll(() => pageCancels.get(this) ?? [], { message: "the page saw the held cancel's answer" }).toContain(answered);
  assert.deepEqual(
    (pageCancels.get(this) ?? []).filter((cancel) => cancel.endsWith(` ${id}`)),
    [`204 ${id}`],
  );
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

// The warning describes the field it is about, so a screen reader says it
// with the field (1.3.1).
Then("the textbox {string} warns {string}", async function (this: BrowserWorld, name: string, words: string) {
  await expect(this.page.getByRole("textbox", { name: namePattern(name) })).toHaveAccessibleDescription(new RegExp(words));
});

Then("the textbox {string} warns of nothing", async function (this: BrowserWorld, name: string) {
  const field = this.page.getByRole("textbox", { name: namePattern(name) });
  await expect(field).not.toHaveAccessibleDescription(/This address is/);
  await expect(this.page.getByRole("main").getByText("This address is")).toHaveCount(0);
});

Then("the dialog {string} doesn't say {string}", async function (this: BrowserWorld, name: string, words: string) {
  await expect(this.page.getByRole("dialog", { name: namePattern(name) })).not.toContainText(words);
});

Then("Settings has no tab {string}", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("tab", { name: namePattern("Language") })).toBeVisible();
  await expect(this.page.getByRole("tab", { name: namePattern(name) })).toHaveCount(0);
});
