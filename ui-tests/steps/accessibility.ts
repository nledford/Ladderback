import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import { errors, type Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { LIVE_REGION } from "../support/gallery.ts";
import { journey, namePattern, pressUntilFocused, whereFocusIs } from "../support/keyboard.ts";
import { patienceMs } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";

// How long a wait for a region's next change goes before it reads the region
// anew: a page that renders the region again leaves the old one unchanged.
const REGION_REREAD_MS = 1_000;

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

/** The anchor of a demo of the gallery, its section's `id`: its heading in
 *  English, in lower case with hyphens for spaces, then `-demo`
 *  (`crates/web/src/gallery.rs`, `DemoHeading`). A name that makes no such
 *  anchor fails the step here, not as a selector that finds nothing. */
function demoAnchor(demo: string): string {
  const anchor = `${demo.toLowerCase().replaceAll(" ", "-")}-demo`;
  if (!/^[a-z0-9-]+$/.test(anchor))
    assert.fail(`the demo "${demo}" has no anchor: its name makes "${anchor}", not letters, digits and hyphens`);
  return anchor;
}

// The address of a demo of the gallery names its section, and the page puts
// focus on the demo's heading as it loads, as it does for a section of
// Settings (`page::focus_when_named`), so a scenario can start at its own demo
// (technical design §13.7.5). Reaching a demo by its address is navigation,
// not the keyboard behaviour under test; the walk from its heading to its
// controls is.
When("I open the {string} demo", async function (this: BrowserWorld, demo: string) {
  const anchor = demoAnchor(demo);
  const address = `/_gallery#${anchor}`;
  await this.open(address);
  // The section the address names, by its `id`, as the browser finds it.
  const heading = this.page.locator(`#${anchor}`).getByRole("heading", { level: 2 }).first();
  try {
    await expect(heading).toBeFocused();
  } catch {
    assert.fail(
      `the address ${address} did not put focus on the heading of the "${demo}" demo: it is ${await whereFocusIs(this.page)}`,
    );
  }
  await expect(heading).toHaveAccessibleName(demo);
});

// The same in a locale of choice. A pseudolocale renames the demo's heading,
// so the anchor is the English heading's, and only in en-US is the heading's
// name the demo's.
When("I open the {string} demo asking for the locale {string}", async function (this: BrowserWorld, demo: string, locale: string) {
  const anchor = demoAnchor(demo);
  const address = `/_gallery?locale=${encodeURIComponent(locale)}#${anchor}`;
  await this.open(address);
  const heading = this.page.locator(`#${anchor}`).getByRole("heading", { level: 2 }).first();
  try {
    await expect(heading).toBeFocused();
  } catch {
    assert.fail(
      `the address ${address} did not put focus on the heading of the "${demo}" demo: it is ${await whereFocusIs(this.page)}`,
    );
  }
  if (locale === "en-US") await expect(heading).toHaveAccessibleName(demo);
});

When("I open the address {string}", async function (this: BrowserWorld, address: string) {
  await this.open(address);
});

/** Presses `key` until focus is on the one element with this role and name.
 *  The walk starts once such an element shows: one the page builds from its
 *  first read isn't there yet when the page opens, and presses made before it
 *  is would run past the last stop onto the page body. */
export async function pressUntilFocusIsOn(world: BrowserWorld, key: string, role: string, name: string): Promise<void> {
  const keys = journey(world.page);
  const target = keys.byRole(role, { name: namePattern(name) });
  try {
    await target.first().waitFor({ state: "visible", timeout: patienceMs() });
  } catch (error) {
    if (!(error instanceof errors.TimeoutError)) throw error;
    assert.fail(`the ${role} "${name}" did not show by the step's limit, so no ${key} press could reach it`);
  }
  await pressUntilFocused(world.page, key, target, `the ${role} "${name}"`);
}

// A walk on a page's content starts where a keyboard user skips the header
// to (§4.5): the skip link, the page's first stop and its first link, moves
// focus to `<main>`, and the walk's next press goes to the first control in
// it. Only the header's own walks start above it.
When("I skip to the content", async function (this: BrowserWorld) {
  const keys = journey(this.page);
  await keys.press("Tab");
  await keys.assertFocused(keys.byRole("link").first(), `the skip link, the page's first stop, not ${await whereFocusIs(this.page)}`);
  await keys.press("Enter");
  await keys.assertFocused(keys.byRole("main"), `the main landmark, not ${await whereFocusIs(this.page)}`);
});

// What a step that has followed a link by keyboard arrived at, where the
// link's name is in a pseudolocale and so is the page's heading.
Then("the page's address is {string}", async function (this: BrowserWorld, path: string) {
  assert.equal(new URL(this.page.url()).pathname, path, "the page's address");
});

// What names a part of the page in its address after the path, such as a
// tab of Settings.
Then("the page's address ends in the fragment {string}", async function (this: BrowserWorld, fragment: string) {
  await expect.poll(() => new URL(this.page.url()).hash, { message: "the page's address's fragment" }).toBe(fragment);
});

// The reader changes the address's fragment alone, typing another in the
// address bar or going Back to an address that differs from this one only by
// it: the browser doesn't load the page again, and tells it with a
// `hashchange`. These steps wait for that event and two frames after it, so
// whatever the page does about it has been drawn, and a step after them that
// says nothing changed isn't early.
When("I change the address's fragment to {string}", async function (this: BrowserWorld, fragment: string) {
  const now = new URL(this.page.url()).hash;
  assert.notEqual(now, fragment, `the address's fragment is "${now}" already, and the browser tells of no change to it`);
  await this.page.evaluate(async (hash) => {
    const changed = new Promise((done) => window.addEventListener("hashchange", done, { once: true }));
    window.location.hash = hash;
    await changed;
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
  }, fragment);
});

When("I go back to the address before", async function (this: BrowserWorld) {
  await this.page.evaluate(async () => {
    const changed = new Promise((done) => window.addEventListener("hashchange", done, { once: true }));
    history.back();
    await changed;
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
  });
});

When("I press Tab until focus is on the {word} {string}", async function (this: BrowserWorld, role: string, name: string) {
  await pressUntilFocusIsOn(this, "Tab", role, name);
});

When("I press Shift+Tab until focus is on the {word} {string}", async function (this: BrowserWorld, role: string, name: string) {
  await pressUntilFocusIsOn(this, "Shift+Tab", role, name);
});

// Within a radio group the arrow keys move focus and choose as they go;
// Tab only enters and leaves the group (§4.5).
When("I press the down arrow until focus is on the {word} {string}", async function (this: BrowserWorld, role: string, name: string) {
  await pressUntilFocusIsOn(this, "ArrowDown", role, name);
});

// WebKit doesn't wrap from the last choice to the first, so a walk to an
// earlier choice goes up rather than down past the end.
When("I press the up arrow until focus is on the {word} {string}", async function (this: BrowserWorld, role: string, name: string) {
  await pressUntilFocusIsOn(this, "ArrowUp", role, name);
});

Then("focus is on the radio {string}", async function (this: BrowserWorld, name: string) {
  const radio = journey(this.page).byRole("radio", { name: namePattern(name) });
  assert.equal(await radio.count(), 1, `the radio "${name}" is not on the page once`);
  await journey(this.page).assertFocused(radio, `the radio "${name}", not ${await whereFocusIs(this.page)}`);
});

// A radio group is one stop: Tab from the choice made goes past the others.
Then("focus has left the group {string}", async function (this: BrowserWorld, name: string) {
  const group = journey(this.page).byRole("radiogroup", { name: namePattern(name) });
  assert.equal(await group.count(), 1, `the group "${name}" is not on the page once`);
  const inside = await group.evaluate((el) => el.contains(document.activeElement));
  assert.ok(!inside, `focus is still in the group "${name}": it is ${await whereFocusIs(this.page)}`);
});

Then("focus is on the main heading {string}", async function (this: BrowserWorld, name: string) {
  const keys = journey(this.page);
  await keys.assertFocused(keys.byRole("heading", { level: 1, name }), `the main heading "${name}"`);
});

Then("focus is on the main heading", async function (this: BrowserWorld) {
  const keys = journey(this.page);
  await keys.assertFocused(keys.byRole("heading", { level: 1 }), "the main heading");
});

// Focus an update did not take away (WCAG 2.4.3): the reader reached the
// control before the update, and what the update changed was not hers to
// notice. A control the update replaced is a new element, and has no focus.
Then("focus stays on the {word} {string}", async function (this: BrowserWorld, role: string, name: string) {
  const target = journey(this.page).byRole(role, { name: namePattern(name) });
  assert.equal(await target.count(), 1, `the ${role} "${name}" is no longer on the page`);
  const stays = await target.evaluate((el) => el === document.activeElement);
  assert.ok(stays, `focus left the ${role} "${name}": it is ${await whereFocusIs(this.page)}`);
});

// Focus a control handed on as it left the page (WCAG 2.4.3): the header of
// its row, which takes focus from a script and is no stop in the tab order.
Then("focus is on the row header {string}", async function (this: BrowserWorld, name: string) {
  const header = journey(this.page).byRole("rowheader", { name: namePattern(name) });
  assert.equal(await header.count(), 1, `the row header "${name}" is not on the page once`);
  const focused = await header.evaluate((el) => el === document.activeElement);
  assert.ok(focused, `focus is not on the row header "${name}": it is ${await whereFocusIs(this.page)}`);
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
  assert.equal(await found.count(), 1, `the page needs exactly one ${politeness} region (role="${role}")`);
  return found;
}

const textOf = async (found: Locator) => ((await found.textContent()) ?? "").trim();

/** Waits until what a region says, without the bidi isolation marks a reader
 *  never hears (§4.5), is `wanted`, and returns it, or fails at the step's
 *  limit with the last text read. The page tells of each change to the region
 *  as it sets an announcement, so the wait ends then, and a text said only for
 *  a moment is heard too. */
async function untilRegionSays(found: Locator, politeness: string, wanted: (said: string) => boolean): Promise<string> {
  const ends = Date.now() + patienceMs();
  let said = (await textOf(found)).replace(ISOLATION_MARKS, "");
  while (!wanted(said)) {
    const left = ends - Date.now();
    if (left <= 0) assert.fail(`the ${politeness} region (last read ${JSON.stringify(said)})`);
    const changed = found.evaluate((region) => {
      const { promise, resolve } = Promise.withResolvers<string>();
      const observer = new MutationObserver(() => {
        observer.disconnect();
        resolve((region.textContent ?? "").trim());
      });
      observer.observe(region, { childList: true, characterData: true, subtree: true });
      return promise;
    });
    // A page that goes away before its region changes is read anew, as is one
    // whose region didn't change for a while.
    const unheard = () => null;
    const told = await Promise.race([changed.catch(unheard), sleep(Math.min(REGION_REREAD_MS, left)).then(unheard)]);
    said = (told ?? (await textOf(found))).replace(ISOLATION_MARKS, "");
  }
  return said;
}

// The region is waited on, so that the two seconds a polite message waits
// don't fail the step.
Then("the {word} region reads {string}", async function (this: BrowserWorld, politeness: string, expected: string) {
  await untilRegionSays(await region(this, politeness), politeness, (said) => said === expected);
});

// The regions merge what arrives within the merge window (§4.5), so a live
// page's announcement shares the region with the ones before it, and a run
// that ends quickly may finish before the journey reads the start: what a
// journey checks is that the region comes to contain the message it expects.
Then("the {word} region comes to contain {string}", async function (this: BrowserWorld, politeness: string, expected: string) {
  await untilRegionSays(await region(this, politeness), politeness, (said) => said.includes(expected));
});

Then("the {word} region is empty", async function (this: BrowserWorld, politeness: string) {
  assert.equal(await textOf(await region(this, politeness)), "", `the ${politeness} region`);
});

// One place talks to a screen reader (§4.5): the announcer's two regions,
// which stand outside the page's mount point. A live region of the page's
// own would read its sentence a second time or race the announcer. A modal
// dialog keeps its own, since its modal page silences the announcer (the
// Dialog contract), so what stands inside a dialog isn't counted.
Then("the page has no live region but the announcer's", async function (this: BrowserWorld) {
  const others = await this.page.evaluate(
    (live) =>
      [...document.querySelectorAll(live)]
        .filter((found) => !found.matches("#live-polite, #live-assertive") && found.closest("dialog") === null)
        .map(
          (found) => `${found.tagName.toLowerCase()} role=${found.getAttribute("role")} aria-live=${found.getAttribute("aria-live")}`,
        ),
    LIVE_REGION,
  );
  assert.deepEqual(others, [], "live regions of the page's own");
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

// The same wait, for one message among whatever else the page says.
Then("the {word} region never says {string}", async function (this: BrowserWorld, politeness: string, unsaid: string) {
  const found = await region(this, politeness);
  const until = Date.now() + SILENCE_MS;
  do {
    assert.ok(!(await textOf(found)).includes(unsaid), `the ${politeness} region says "${unsaid}"`);
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

// The reader asks her system for less motion while the page is open, and the
// page follows at once (WCAG 2.3.3).
When("the reader asks for less motion", async function (this: BrowserWorld) {
  await this.page.emulateMedia({ reducedMotion: "reduce" });
});
