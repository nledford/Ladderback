import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { journey, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

// More Tab presses than any page has stops: the gallery's, the longest, and
// the skip link and the navigation's three links before it.
const MAX_TABS = 30;

// A polite message waits up to two seconds for company, and its text is set a
// moment after the region is cleared (§4.5); five seconds is generous.
const REGION_PATIENCE_MS = 5_000;

// The time a region stays silent: past the merge window and the set delay.
const SILENCE_MS = 2_500;

const REGIONS: Record<string, string> = {
  polite: "status",
  assertive: "alert",
};

const ISOLATION_MARKS = /[\u2066-\u2069]/g;

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

When("I open the component gallery", async function (this: BrowserWorld) {
  await this.open("/_gallery");
});

When("I open the address {string}", async function (this: BrowserWorld, address: string) {
  await this.open(address);
});

/** Presses `key` until focus is on the one element with this role and name. */
async function pressUntilFocusIsOn(world: BrowserWorld, key: string, role: string, name: string): Promise<void> {
  const keys = journey(world.page);
  const target = keys.byRole(role, { name: namePattern(name) });
  const focused = async () =>
    (await target.count()) === 1 && (await target.evaluate((el) => el === document.activeElement));
  for (let presses = 0; presses < MAX_TABS && !(await focused()); presses += 1) {
    await keys.press(key);
  }
  assert.ok(await focused(), `focus did not reach the ${role} "${name}" within ${MAX_TABS} ${key} presses`);
}

When("I press Tab until focus is on the {word} {string}", async function (this: BrowserWorld, role: string, name: string) {
  await pressUntilFocusIsOn(this, "Tab", role, name);
});

When("I press Shift+Tab until focus is on the {word} {string}", async function (this: BrowserWorld, role: string, name: string) {
  await pressUntilFocusIsOn(this, "Shift+Tab", role, name);
});

Then("focus is on the main heading {string}", async function (this: BrowserWorld, name: string) {
  const keys = journey(this.page);
  await keys.assertFocused(
    keys.byRole("heading", { level: 1, name }),
    `the main heading "${name}"`,
  );
});

Then("focus is on the main heading", async function (this: BrowserWorld) {
  const keys = journey(this.page);
  await keys.assertFocused(keys.byRole("heading", { level: 1 }), "the main heading");
});

/** Where focus is, as the end of a sentence. */
function whereFocusIs(page: Page): Promise<string> {
  return page.evaluate(() => {
    const el = document.activeElement;
    return el === null || el === document.body
      ? "on the page body"
      : `on <${el.localName}> ${JSON.stringify((el.textContent ?? "").trim().slice(0, 40))}`;
  });
}

// Focus an update did not take away (WCAG 2.4.3): the reader reached the
// control before the update, and what the update changed was not hers to
// notice. A control the update replaced is a new element, and has no focus.
Then("focus stays on the {word} {string}", async function (this: BrowserWorld, role: string, name: string) {
  const target = journey(this.page).byRole(role, { name: namePattern(name) });
  assert.equal(await target.count(), 1, `the ${role} "${name}" is no longer on the page`);
  const stays = await target.evaluate((el) => el === document.activeElement);
  assert.ok(stays, `focus left the ${role} "${name}": it is ${await whereFocusIs(this.page)}`);
});

Then("focus is inside the dialog {string}", async function (this: BrowserWorld, name: string) {
  const keys = journey(this.page);
  const dialog = keys.byRole("dialog", { name });
  assert.equal(await dialog.count(), 1, `the dialog "${name}" is not open`);
  const inside = await dialog.evaluate((el) => el.contains(document.activeElement));
  assert.ok(inside, `focus is not inside the dialog "${name}"`);
});

/** The one live region of this politeness, found by its role. */
async function region(world: BrowserWorld, politeness: string): Promise<Locator> {
  const role = REGIONS[politeness];
  assert.ok(role !== undefined, `no live region is polite "${politeness}": say polite or assertive`);
  const found = journey(world.page).byRole(role);
  assert.equal(
    await found.count(),
    1,
    `the page needs exactly one ${politeness} region (role="${role}")`,
  );
  return found;
}

const textOf = async (found: Locator) => ((await found.textContent()) ?? "").trim();

// The text is read until it matches or the patience runs out, so that the
// two seconds a polite message waits don't fail the step. What it reports
// when it never matches is the last text read.
Then("the {word} region reads {string}", async function (this: BrowserWorld, politeness: string, expected: string) {
  const found = await region(this, politeness);
  const deadline = Date.now() + REGION_PATIENCE_MS;
  // What a reader hears has no bidi isolation marks (§4.5).
  let actual = (await textOf(found)).replace(ISOLATION_MARKS, "");
  while (actual !== expected && Date.now() < deadline) {
    await sleep(100);
    actual = (await textOf(found)).replace(ISOLATION_MARKS, "");
  }
  assert.equal(actual, expected, `the ${politeness} region`);
});

// The regions merge what arrives within the merge window (§4.5), so a live
// page's announcement shares the region with the ones before it, and a run
// that ends quickly may finish before the journey reads the start: what a
// journey checks is that the region comes to contain the message it expects.
Then("the {word} region comes to contain {string}", async function (this: BrowserWorld, politeness: string, expected: string) {
  const found = await region(this, politeness);
  const deadline = Date.now() + REGION_PATIENCE_MS;
  // What a reader hears has no bidi isolation marks (§4.5).
  const heard = async () => (await textOf(found)).replace(ISOLATION_MARKS, "");
  let actual = await heard();
  while (!actual.includes(expected) && Date.now() < deadline) {
    await sleep(100);
    actual = await heard();
  }
  assert.ok(
    actual.includes(expected),
    `the ${politeness} region (last read ${JSON.stringify(actual)})`,
  );
});

Then("the {word} region is empty", async function (this: BrowserWorld, politeness: string) {
  assert.equal(await textOf(await region(this, politeness)), "", `the ${politeness} region`);
});

// What a region keeps saying: the text is there now and stays, unchanged and
// never cleared, past the merge window, so nothing else was said meanwhile.
// The region is watched for every change rather than read now and then: a
// rewrite clears it for a moment only, which a reading between two moments
// would miss, and a rewrite of the same text is said again.
Then("the {word} region keeps reading {string}", async function (this: BrowserWorld, politeness: string, expected: string) {
  const found = await region(this, politeness);
  assert.equal((await textOf(found)).replace(ISOLATION_MARKS, ""), expected, `the ${politeness} region`);
  const changes = await found.evaluate((el, ms) => {
    const { promise, resolve } = Promise.withResolvers<number>();
    let changes = 0;
    const observer = new MutationObserver((records) => {
      changes += records.length;
    });
    observer.observe(el, { childList: true, characterData: true, subtree: true });
    setTimeout(() => {
      observer.disconnect();
      resolve(changes);
    }, ms);
    return promise;
  }, SILENCE_MS);
  assert.equal(changes, 0, `the ${politeness} region was written again`);
  assert.equal((await textOf(found)).replace(ISOLATION_MARKS, ""), expected, `the ${politeness} region`);
});

// Silence is shown by waiting: a region that fills in later is not silent.
Then("the {word} region stays empty", async function (this: BrowserWorld, politeness: string) {
  const found = await region(this, politeness);
  const until = Date.now() + SILENCE_MS;
  do {
    assert.equal(await textOf(found), "", `the ${politeness} region`);
    await sleep(100);
  } while (Date.now() < until);
});

Then("the live regions stay silent", async function (this: BrowserWorld) {
  const regions = [await region(this, "polite"), await region(this, "assertive")];
  const until = Date.now() + SILENCE_MS;
  do {
    for (const found of regions) assert.equal(await textOf(found), "", "a live region");
    await sleep(100);
  } while (Date.now() < until);
});
