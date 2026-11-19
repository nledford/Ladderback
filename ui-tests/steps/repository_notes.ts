// Steps of the Repositories page's notes (features/ui/repositories.feature,
// #923): how the agent finds the keyring is started, as a Given sets it in
// the daemon's answer, and the note the page shows for it. The answer is
// scripted, so each journey is the same whatever keyring the computer that
// runs it has. A note is a banner (#930), which has no role and so no name:
// it is found by its class and its words, as the banner's own steps find it.

import assert from "node:assert/strict";

import { Given, Then } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import { patienceMs, untilSettled } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";

// How long a check that the page notes nothing gives a note on its way out:
// a check that something isn't there keeps a window of its own rather than
// the step's limit (#1104).
const GONE_MS = 10_000;

// What the note says, in English.
const KEYRING_NOTE = /nothing can start it when a program asks for it/;

// The scenario's notes, and whether the page has read them yet.
interface Scripted {
  read: Promise<void>;
}

// One per scenario: the world is new for each.
const scripted = new WeakMap<BrowserWorld, Scripted>();

// Answers the page's request for its notes with `notes`, as the daemon would.
function answerNotes(world: BrowserWorld, notes: Array<{ code: string }>): void {
  const { promise: read, resolve: wasRead } = Promise.withResolvers<void>();
  scripted.set(world, { read });
  world.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/repositories/_notes", async (route) => {
      if (route.request().method() !== "GET") return route.fallback();
      await route.fulfill({ json: { notes } });
      wasRead();
    });
  });
}

function keyringNote(world: BrowserWorld) {
  return world.page.locator(".banner").filter({ hasText: KEYRING_NOTE });
}

Given("the agent finds the keyring running but not startable on demand", function (this: BrowserWorld) {
  answerNotes(this, [{ code: "keyring-not-activatable" }]);
});

Given("the agent finds the keyring startable on demand", function (this: BrowserWorld) {
  answerNotes(this, []);
});

Then("the page notes that the keyring can't be started on demand", async function (this: BrowserWorld) {
  await keyringNote(this).waitFor({ state: "visible", timeout: patienceMs() });
});

// Only once the page has read its notes: a page that never asked would show
// no note either. Its words, in a banner or not, are nowhere on the page.
Then("the page notes nothing of the keyring", async function (this: BrowserWorld) {
  const answers = scripted.get(this);
  assert.ok(answers, "no Given set how the agent finds the keyring");
  await untilSettled(answers.read, "the page never asked for its notes");
  await expect(this.page.getByText(KEYRING_NOTE)).toHaveCount(0, { timeout: GONE_MS });
});
