// Steps of the start page's machine notice journeys
// (features/ui/copied_data_folder.feature): where the agent's machine stands,
// as a Given sets it in the daemon's answers, the notice the start page shows
// for a copy or a machine its hub refused or removed, and the request that
// takes a new machine ID. Elements are found by role and accessible name, as
// in every journey.

import assert from "node:assert/strict";

import { Given, Then } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import { namePattern } from "../support/keyboard.ts";
import { patienceMs, untilSettled } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";

// How long a check that the page shows no notice gives one on its way out
// (#1104), as the Full Disk Access steps do.
const GONE_MS = 10_000;

// The notice's headings, which name its region: a copied data folder's, a
// machine its hub refused, one its hub removed, and any of them once a new
// machine ID is taken.
const NOTICE = "This data folder came from another computer";
const REFUSED = "Your hub took another computer as this one";
const REMOVED = "Your hub removed this computer";
const TAKEN = "New machine ID taken";

type Standing = "same" | "copied" | "refused-by-hub" | "removed-by-hub" | "new-id-taken" | "unread";

// What the scripted answers say, and what the page asked of them.
interface Scripted {
  standing: Standing;
  taken: number;
  read: Promise<void>;
  wasRead: () => void;
}

// One per scenario: the world is new for each.
const scripted = new WeakMap<BrowserWorld, Scripted>();

// The scenario's answers, set up on its first Given: the daemon's own check
// at start is replaced, so each journey is the same on every computer.
function answers(world: BrowserWorld): Scripted {
  const known = scripted.get(world);
  if (known) return known;
  const { promise: read, resolve: wasRead } = Promise.withResolvers<void>();
  const fresh: Scripted = { standing: "same", taken: 0, read, wasRead };
  scripted.set(world, fresh);
  world.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/machine**", async (route) => {
      const request = route.request();
      const path = new URL(request.url()).pathname;
      if (request.method() === "GET" && path === "/api/v1/machine") {
        await route.fulfill({ json: { standing: fresh.standing } });
        fresh.wasRead();
      } else if (request.method() === "POST" && path === "/api/v1/machine/new-id") {
        fresh.taken += 1;
        fresh.standing = "new-id-taken";
        await route.fulfill({ json: { standing: fresh.standing } });
      } else {
        await route.fulfill({ status: 404, json: { code: "not-found" } });
      }
    });
  });
  return fresh;
}

function notice(world: BrowserWorld, heading = NOTICE) {
  return world.page.getByRole("region", { name: namePattern(heading) });
}

Given("the agent found its data folder was copied from another computer", function (this: BrowserWorld) {
  answers(this).standing = "copied";
});

Given("the agent's hub refused it, signed, as a second computer", function (this: BrowserWorld) {
  answers(this).standing = "refused-by-hub";
});

Given("the agent's hub removed it, signed", function (this: BrowserWorld) {
  answers(this).standing = "removed-by-hub";
});

Given("the agent found its data folder on the computer it was made on", function (this: BrowserWorld) {
  answers(this).standing = "same";
});

Then("the page says the data folder came from another computer", async function (this: BrowserWorld) {
  await notice(this).waitFor({ state: "visible", timeout: patienceMs() });
  await expect(notice(this)).toContainText("Backups run as usual");
});

Then("the page says the hub took another computer as this one", async function (this: BrowserWorld) {
  await notice(this, REFUSED).waitFor({ state: "visible", timeout: patienceMs() });
  await expect(notice(this, REFUSED)).toContainText("Backups run as usual");
  await expect(notice(this, REFUSED)).toContainText("ladderback pairing-token --forget-computer");
});

Then("the page says the hub removed this machine, and to pair it again under a new ID", async function (this: BrowserWorld) {
  await notice(this, REMOVED).waitFor({ state: "visible", timeout: patienceMs() });
  await expect(notice(this, REMOVED)).toContainText("Backups run as usual");
  await expect(notice(this, REMOVED)).toContainText("pair it again");
});

// The refused computer may be the original: nothing on the page says its
// data folder came from another one.
Then("the page doesn't say the data folder came from another computer", async function (this: BrowserWorld) {
  await expect(notice(this)).toHaveCount(0);
  await expect(this.page.getByText(/came from another computer|isn't the one its data folder was made on/)).toHaveCount(0);
});

Then("the page says a new machine ID was taken, to be used once Ladderback restarts", async function (this: BrowserWorld) {
  await notice(this, TAKEN).waitFor({ state: "visible", timeout: patienceMs() });
  await expect(notice(this, TAKEN)).toContainText("Restart Ladderback to start using it");
});

Then("the page offers the link {string}", async function (this: BrowserWorld, name: string) {
  const link = notice(this, TAKEN).getByRole("link", { name: namePattern(name) });
  await expect(link).toHaveCount(1);
  await expect(link).toHaveAttribute("href", "/settings#restart");
});

Then("the agent was asked once to take a new machine ID", async function (this: BrowserWorld) {
  await expect.poll(() => answers(this).taken, { timeout: patienceMs() }).toBe(1);
});

Then("the agent was never asked to take a new machine ID", function (this: BrowserWorld) {
  assert.equal(answers(this).taken, 0);
});

// Only once the page has read where the machine stands and drawn what it
// read: a page that never asked would show no notice either.
Then("the page says nothing of a copied data folder", async function (this: BrowserWorld) {
  await untilSettled(answers(this).read, "the page never read where the machine stands");
  await this.page.evaluate(() => {
    const { promise, resolve } = Promise.withResolvers<void>();
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    return promise;
  });
  // None of the notice's headings: a copy's, a refused or removed
  // machine's, or a new ID taken.
  for (const heading of [NOTICE, REFUSED, REMOVED, TAKEN]) {
    await expect(notice(this, heading)).toHaveCount(0, { timeout: Math.min(GONE_MS, patienceMs()) });
  }
});
