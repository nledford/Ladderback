// Steps of the API token journeys in Settings (features/security/api_tokens.feature):
// the admin signed in and tokens created before the page opens, through the
// browser context's own requests, which share its cookies; the secret the
// dialog shows once, and where it must not stay. Elements are found by role
// and accessible name, as in every journey.

import assert from "node:assert/strict";

import { Given, Then } from "@cucumber/cucumber";
import type { BrowserContext } from "playwright";

import { expect } from "../support/expect.ts";
import { namePattern } from "../support/keyboard.ts";
import { type BrowserWorld, type Engine, engine } from "../support/world.ts";

// How long the page may still hold the token once the dialog is left: a
// check that it doesn't stay, so a window of its own rather than the step's
// limit (#1104).
const TOKEN_GONE_MS = 10_000;

/** The secret of each token a Given created, by its label, per scenario. */
const secrets = new WeakMap<BrowserWorld, Map<string, string>>();

/** The token the dialog showed, per scenario. */
const shown = new WeakMap<BrowserWorld, string>();

/** The text field `name` of a JSON answer, when it has one. */
function textField(answer: unknown, name: string): string | undefined {
  if (typeof answer !== "object" || answer === null || !(name in answer)) return undefined;
  const value: unknown = Reflect.get(answer, name);
  return typeof value === "string" ? value : undefined;
}

/** Where the scenario's own daemon listens: these steps need one with
 *  sign-in on, which only a scenario's own daemon has. */
export function ownBase(world: BrowserWorld): string {
  assert.ok(world.baseUrl, "no Given started a daemon of the scenario's own with sign-in on");
  return world.baseUrl;
}

/** The headers a changed request of the page carries: its own origin, which
 *  the request checks admit, and the session's CSRF token. */
export async function changeHeaders(context: BrowserContext, base: string): Promise<Record<string, string>> {
  const session = await context.request.get(`${base}/api/v1/session`);
  assert.equal(session.status(), 200, "the session could not be read");
  const token = textField(await session.json(), "csrf-token");
  assert.ok(token, "the session has no CSRF token: the admin is not signed in");
  return { Origin: base, "X-CSRF-Token": token };
}

/** Creates a token through the token route, as Settings does, and keeps its
 *  secret by its label. */
async function createToken(world: BrowserWorld, context: BrowserContext, label: string, scopes: string[]): Promise<void> {
  const base = ownBase(world);
  const answer = await context.request.post(`${base}/api/v1/tokens`, {
    data: { label, scopes },
    headers: await changeHeaders(context, base),
  });
  assert.equal(answer.status(), 201, `the token ${label} was not created: ${await answer.text()}`);
  const secret = textField(await answer.json(), "secret");
  assert.ok(secret, `the answer that created the token ${label} holds no secret`);
  const kept = secrets.get(world) ?? new Map<string, string>();
  kept.set(label, secret);
  secrets.set(world, kept);
}

Given("the admin is signed in with {string}", function (this: BrowserWorld, password: string) {
  const base = ownBase(this);
  this.contextRoutes.push(async (context) => {
    const answer = await context.request.post(`${base}/api/v1/session`, {
      data: { password },
      headers: { Origin: base },
    });
    assert.equal(answer.status(), 200, `the sign-in was refused: ${await answer.text()}`);
  });
});

Given(
  "the admin has created the API token {string} with the scopes {string}",
  function (this: BrowserWorld, label: string, scopes: string) {
    const listed = scopes.split(",").map((scope) => scope.trim());
    this.contextRoutes.push(async (context) => createToken(this, context, label, listed));
  },
);

Given("the admin has created {int} API tokens", function (this: BrowserWorld, count: number) {
  this.contextRoutes.push(async (context) => {
    for (let n = 1; n <= count; n += 1) await createToken(this, context, `token-${n}`, ["read"]);
  });
});

// The daemon holds the page's next creation of a token for a while before it
// answers, so the reader can leave its tab while it waits.
Given("the daemon answers the next creation of an API token {int} seconds late", function (this: BrowserWorld, seconds: number) {
  let held = false;
  this.contextRoutes.push(async (context) => {
    await context.route(
      (url) => url.pathname === "/api/v1/tokens",
      async (route) => {
        if (route.request().method() === "POST" && !held) {
          held = true;
          await new Promise((done) => setTimeout(done, seconds * 1_000));
        }
        await route.continue();
      },
    );
  });
});

// A program presents the token as a program does, with no cookie, so the
// daemon records its use.
Given("a program has used the API token {string}", function (this: BrowserWorld, label: string) {
  const base = ownBase(this);
  this.contextRoutes.push(async () => {
    const secret = secrets.get(this)?.get(label);
    assert.ok(secret, `no Given created the API token ${label}`);
    const answer = await fetch(`${base}/api/v1/operations`, { headers: { Authorization: `Bearer ${secret}` } });
    assert.equal(answer.status, 200, `the API token ${label} was refused: ${await answer.text()}`);
  });
});

// The daemon answers the list of tokens as it does when it can't read them
// (503 api-tokens-unavailable), whichever daemon serves the page.
Given("the daemon can't read its API tokens", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/tokens", (route) =>
      route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ code: "api-tokens-unavailable" }),
      }),
    );
  });
});

// Each engine names what the page needs differently, and refuses a name it
// doesn't know: Chromium both permissions; WebKit only reading, since it lets
// a click write; Firefox neither, since Playwright's Firefox lets a page use
// the clipboard without asking.
export const clipboardPermissions: Record<Engine, string[]> = {
  chromium: ["clipboard-read", "clipboard-write"],
  webkit: ["clipboard-read"],
  firefox: [],
};

Given("the page may use the clipboard", function (this: BrowserWorld) {
  this.contextOptions = { ...this.contextOptions, permissions: clipboardPermissions[engine] };
});

// A browser that won't let the page write to the clipboard, as one does whose
// reader said no: each write is refused. Firefox and WebKit let a pressed
// button write without asking, so the refusal is the page's clipboard's own.
Given("the browser refuses the page the clipboard", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context) => {
    await context.addInitScript(() => {
      Clipboard.prototype.writeText = () => Promise.reject(new DOMException("Write permission denied.", "NotAllowedError"));
    });
  });
});

Then("the dialog {string} shows a token that starts with {string}", async function (this: BrowserWorld, name: string, prefix: string) {
  const field = this.page.getByRole("dialog", { name: namePattern(name) }).getByRole("textbox");
  await expect(field).toHaveValue(new RegExp(`^${prefix}`));
  shown.set(this, await field.inputValue());
});

Then("the clipboard holds the token the dialog showed", async function (this: BrowserWorld) {
  const token = shown.get(this);
  assert.ok(token, "no step read the token the dialog showed");
  await expect.poll(() => this.page.evaluate(() => navigator.clipboard.readText())).toBe(token);
});

// I4, and the secret is shown once: once the dialog is left, no text, no
// attribute and no field of the page holds it.
Then("nothing on the page holds the token the dialog showed", async function (this: BrowserWorld) {
  const token = shown.get(this);
  assert.ok(token, "no step read the token the dialog showed");
  await expect
    .poll(
      () =>
        this.page.evaluate(
          (secret) =>
            [
              document.documentElement.outerHTML,
              ...Array.from(document.querySelectorAll("input, textarea"), (field) =>
                field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement ? field.value : "",
              ),
            ].some((text) => text.includes(secret)),
          token,
        ),
      { timeout: TOKEN_GONE_MS, message: "the page still holds the token the dialog showed" },
    )
    .toBe(false);
});
