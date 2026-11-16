// Steps of the Full Disk Access journeys (features/ui/full_disk_access.feature):
// what the agent's Full Disk Access probe found, as a Given sets it in the
// daemon's answers, and the warning the start page shows for it. Elements are
// found by role and accessible name, as in every journey.

import assert from "node:assert/strict";

import { Given, Then } from "@cucumber/cucumber";
import { expect } from "playwright/test";

import { namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

// How long the page takes to show what the daemon answered, with room.
const PAGE_PATIENCE_MS = 10_000;

// The warning's heading, which names its region.
const WARNING = "Full Disk Access is missing";

type FoundState = "granted" | "missing" | "unknown";

// What the scripted answers say, as the scenario's Givens set it, and whether
// the page has read it yet.
interface Scripted {
  state: FoundState;
  afterCheck: FoundState | null;
  checkTakesMs: number;
  opensSettings: boolean;
  read: Promise<void>;
  wasRead: () => void;
}

// One per scenario: the world is new for each.
const scripted = new WeakMap<BrowserWorld, Scripted>();

// The scenario's answers, set up on its first Given: the daemon's own answer
// on the agent's platform is replaced, so each journey is the same on every
// computer that runs it.
function answers(world: BrowserWorld): Scripted {
  const known = scripted.get(world);
  if (known) return known;
  const { promise: read, resolve: wasRead } = Promise.withResolvers<void>();
  const fresh: Scripted = { state: "unknown", afterCheck: null, checkTakesMs: 0, opensSettings: false, read, wasRead };
  scripted.set(world, fresh);
  world.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/full-disk-access**", async (route) => {
      const request = route.request();
      const path = new URL(request.url()).pathname;
      if (request.method() === "GET" && path === "/api/v1/full-disk-access") {
        await route.fulfill({ json: { state: fresh.state, "opens-settings": fresh.opensSettings } });
        fresh.wasRead();
      } else if (request.method() === "POST" && path === "/api/v1/full-disk-access/check") {
        if (fresh.checkTakesMs > 0) await new Promise((done) => setTimeout(done, fresh.checkTakesMs));
        if (fresh.afterCheck !== null) fresh.state = fresh.afterCheck;
        await route.fulfill({ json: { state: fresh.state, "opens-settings": fresh.opensSettings } });
      } else if (request.method() === "POST" && path === "/api/v1/full-disk-access/settings") {
        await route.fulfill({ status: 204 });
      } else {
        await route.fulfill({ status: 404, json: { code: "not-found" } });
      }
    });
  });
  return fresh;
}

function foundState(word: string): FoundState {
  assert.ok(word === "granted" || word === "missing" || word === "unknown", `unknown Full Disk Access state "${word}"`);
  return word;
}

function warning(world: BrowserWorld) {
  return world.page.getByRole("region", { name: namePattern(WARNING) });
}

Given("the agent has found Full Disk Access {word}", function (this: BrowserWorld, state: string) {
  answers(this).state = foundState(state);
});

Given("the browser runs on the agent's own Mac", function (this: BrowserWorld) {
  answers(this).opensSettings = true;
});

Given("the agent finds the grant when it checks again", function (this: BrowserWorld) {
  answers(this).afterCheck = "granted";
});

Given("the agent takes {int} seconds to check again", function (this: BrowserWorld, seconds: number) {
  answers(this).checkTakesMs = seconds * 1_000;
});

Then("the page warns that Full Disk Access is missing", async function (this: BrowserWorld) {
  await warning(this).waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});

Then("the page lists the steps to grant Full Disk Access", async function (this: BrowserWorld) {
  const steps = warning(this).getByRole("list").getByRole("listitem");
  await steps.first().waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  assert.ok((await steps.count()) >= 3, `the warning lists ${await steps.count()} steps`);
});

// Only once the page has read what the agent found: a page that never asked
// would show no warning either.
Then("the page doesn't warn of Full Disk Access", async function (this: BrowserWorld) {
  const { read } = answers(this);
  const { promise: late, resolve: tooLate } = Promise.withResolvers<"late">();
  const timer = setTimeout(() => tooLate("late"), PAGE_PATIENCE_MS);
  const outcome = await Promise.race([read.then(() => "read" as const), late]);
  clearTimeout(timer);
  assert.equal(outcome, "read", "the page never read what the agent found of Full Disk Access");
  await expect(warning(this)).toHaveCount(0, { timeout: PAGE_PATIENCE_MS });
});
