// Journey steps for the tree (technical design §4.5) and a snapshot's page,
// which shows a snapshot's folders and files in one (§4.4): the gallery's
// demo tree and a real snapshot a scenario's daemon backed up. Keyboard
// only, role-and-name locators, as everywhere in steps/.

import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";
import { expect } from "playwright/test";

import { ISOLATION, journey, namePattern, pressUntilFocused } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";
import { latestSnapshotOf } from "./maintenance.ts";

// A folder the daemon reads from a real repository, or a page that reads the
// snapshot list first: generous, as the other journeys that wait for a read.
const READ_PATIENCE_MS = 10_000;

// Past the tree's type-ahead window (500 ms), so what a step types starts a
// name afresh rather than continuing the last one.
const FRESH_TYPING_MS = 600;

// The alias of the repository the demo plans back up to (steps/live.ts).
const ALIAS = "demo";

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** `text` as a reader hears it: bidi isolation marks, which a reader never
 *  hears (§4.5), may stand between any two characters. */
const heard = (text: string): string =>
  [...text].map((ch) => ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(ISOLATION);

function treeItem(world: BrowserWorld, name: string): Locator {
  return journey(world.page).byRole("treeitem", { name: namePattern(name) });
}

/** Waits until no tree item is waiting for its folder to be read. */
async function settled(world: BrowserWorld): Promise<void> {
  await expect(world.page.locator('[role="treeitem"][aria-busy="true"]')).toHaveCount(0, {
    timeout: READ_PATIENCE_MS,
  });
}

/** Presses `key` until focus is on `target`, giving each folder a press
 *  opened time to be read before the next press. */
async function pressUntilFolderFocused(world: BrowserWorld, key: string, target: Locator, what: string): Promise<void> {
  await pressUntilFocused(world.page, key, target, what, {
    afterEach: async () => {
      // A press that opens a folder marks it busy as the page draws again.
      await sleep(50);
      await settled(world);
    },
  });
}

When("I open the component gallery asking for the locale {string}", async function (this: BrowserWorld, locale: string) {
  await this.open(`/_gallery?locale=${encodeURIComponent(locale)}`);
});

// The tree is drawn once its first folder is read: Tab waits for it.
When("I press Tab until focus is on a tree item", async function (this: BrowserWorld) {
  const items = journey(this.page).byRole("treeitem");
  await items.first().waitFor({ state: "visible", timeout: READ_PATIENCE_MS });
  await pressUntilFocused(this.page, "Tab", items, "a tree item", {
    reached: () => this.page.evaluate(() => document.activeElement?.getAttribute("role") === "treeitem"),
  });
});

When("I press {word} until focus is on the tree item {string}", async function (this: BrowserWorld, key: string, name: string) {
  await pressUntilFolderFocused(this, key, treeItem(this, name), `the tree item "${name}"`);
});

When("I type {string} in the tree", async function (this: BrowserWorld, text: string) {
  await sleep(FRESH_TYPING_MS);
  await journey(this.page).type(text);
});

// The link a snapshot's row shows is its short ID, the first 8 hex digits,
// as rustic names a snapshot. The list is read after the page opens.
When(
  "I press Tab until focus is on the link to the newest snapshot of the plan {string}",
  async function (this: BrowserWorld, plan: string) {
    const short = (await latestSnapshotOf(this, plan)).slice(0, 8);
    const link = journey(this.page).byRole("link", { name: namePattern(short) });
    await link.waitFor({ state: "visible", timeout: READ_PATIENCE_MS });
    await pressUntilFolderFocused(this, "Tab", link, `the link to the snapshot ${short}`);
  },
);

When("I open the newest snapshot of the plan {string}", async function (this: BrowserWorld, plan: string) {
  const id = await latestSnapshotOf(this, plan);
  await this.open(`/snapshots/${ALIAS}/${id}`);
});

Then(
  "the main heading names the newest snapshot of the plan {string} in {string}",
  async function (this: BrowserWorld, plan: string, alias: string) {
    const short = (await latestSnapshotOf(this, plan)).slice(0, 8);
    await expect(journey(this.page).byRole("heading", { level: 1 })).toHaveAccessibleName(
      namePattern(`Snapshot ${short} of ${alias}`),
    );
  },
);

// One stop for Tab (§4.5): one row is in the tab order, every other row and
// everything in a row is reached by the arrow keys.
Then("the tree {string} has one tab stop", async function (this: BrowserWorld, name: string) {
  const tree = journey(this.page).byRole("tree", { name: namePattern(name) });
  await expect(tree).toHaveCount(1);
  const stops = await tree.evaluate(
    (el) => [...el.querySelectorAll<HTMLElement>("*")].filter((node) => node.tabIndex >= 0).length,
  );
  assert.equal(stops, 1, `the tree "${name}" has ${stops} tab stops`);
});

Then("focus has left the tree {string}", async function (this: BrowserWorld, name: string) {
  const tree = journey(this.page).byRole("tree", { name: namePattern(name) });
  await expect(tree).toHaveCount(1);
  const where = await tree.evaluate((el) => ({
    inside: el.contains(document.activeElement),
    nowhere: document.activeElement === null || document.activeElement === document.body,
  }));
  assert.ok(!where.inside, `focus is still in the tree "${name}"`);
  assert.ok(!where.nowhere, "focus is on the page body, not on the stop after the tree");
});

// Focus moves to a row once it is drawn, a moment after the key.
Then("focus is on the tree item {string}", async function (this: BrowserWorld, name: string) {
  await expect(treeItem(this, name)).toBeFocused({ timeout: READ_PATIENCE_MS });
});

Then(
  "the tree item {string} is item {int} of {int} at level {int}",
  async function (this: BrowserWorld, name: string, position: number, size: number, level: number) {
    const item = treeItem(this, name);
    await expect(item).toHaveAttribute("aria-posinset", String(position), { timeout: READ_PATIENCE_MS });
    await expect(item).toHaveAttribute("aria-setsize", String(size));
    await expect(item).toHaveAttribute("aria-level", String(level));
  },
);

Then("the tree item {string} is item {int} of {int}", async function (this: BrowserWorld, name: string, position: number, size: number) {
  const item = treeItem(this, name);
  await expect(item).toHaveAttribute("aria-posinset", String(position), { timeout: READ_PATIENCE_MS });
  await expect(item).toHaveAttribute("aria-setsize", String(size));
});

// What a screen reader reads after the row's name: what the entry is.
Then("the tree item {string} is described by {string}", async function (this: BrowserWorld, name: string, description: string) {
  await expect(treeItem(this, name)).toHaveAccessibleDescription(
    new RegExp(`^${ISOLATION}${heard(description)}${ISOLATION}$`),
    { timeout: READ_PATIENCE_MS },
  );
});

// A file of a real snapshot: its size and when it was modified are the
// scratch source's, which the journey doesn't fix.
Then("the tree item {string} is described as a file modified at some time", async function (this: BrowserWorld, name: string) {
  await expect(treeItem(this, name)).toHaveAccessibleDescription(
    new RegExp(`^${ISOLATION}${heard("File, ")}.+${heard(", modified ")}.+$`),
    { timeout: READ_PATIENCE_MS },
  );
});

Then("the tree item {string} is expanded", async function (this: BrowserWorld, name: string) {
  await expect(treeItem(this, name)).toHaveAttribute("aria-expanded", "true", { timeout: READ_PATIENCE_MS });
});

Then("the tree item {string} is collapsed", async function (this: BrowserWorld, name: string) {
  await expect(treeItem(this, name)).toHaveAttribute("aria-expanded", "false", { timeout: READ_PATIENCE_MS });
});

Then("the tree item {string} is busy", async function (this: BrowserWorld, name: string) {
  await expect(treeItem(this, name)).toHaveAttribute("aria-busy", "true", { timeout: READ_PATIENCE_MS });
});

// The row focus is on, whichever it is: several rows may be open or closed.
for (const [state, expanded] of [["expanded", "true"], ["collapsed", "false"]] as const) {
  Then(`the focused tree item is ${state}`, async function (this: BrowserWorld) {
    await expect
      .poll(
        () =>
          this.page.evaluate(() => {
            const el = document.activeElement;
            return el?.getAttribute("role") === "treeitem" ? el.getAttribute("aria-expanded") : "no tree item";
          }),
        { timeout: READ_PATIENCE_MS },
      )
      .toBe(expanded);
  });
}

Then("the tree shows no tree item {string}", async function (this: BrowserWorld, name: string) {
  await expect(treeItem(this, name)).toHaveCount(0, { timeout: READ_PATIENCE_MS });
});

Then("the tree has the tree item {string}", async function (this: BrowserWorld, name: string) {
  await expect(treeItem(this, name)).toHaveCount(1, { timeout: READ_PATIENCE_MS });
});

// A name keeps its own direction, whatever the page's (§4.5): one written
// right to left is laid out right to left, its neutral characters with it,
// and the text beside it can't reorder it.
Then("the tree item {string} shows its name right to left", async function (this: BrowserWorld, name: string) {
  const item = treeItem(this, name);
  await expect(item).toHaveCount(1, { timeout: READ_PATIENCE_MS });
  const direction = await item.evaluate((row, shown) => {
    const holder = [...row.querySelectorAll("*")].find(
      (el) => (el.textContent ?? "").replace(/[\u2066-\u2069]/g, "") === shown,
    );
    if (holder === undefined) return "in no element of its own";
    return holder.matches(":dir(rtl)") ? "right to left" : "left to right";
  }, name);
  assert.equal(direction, "right to left", `the tree item "${name}" shows its name ${direction}`);
});

// A refusal is told where a reader is looking, as an alert, whole: the
// assertive live region, an alert too, may say it as well.
Then("the page alerts {string}", async function (this: BrowserWorld, text: string) {
  await journey(this.page)
    .byRole("alert")
    .filter({ hasText: new RegExp(`^\\s*${ISOLATION}${heard(text)}${ISOLATION}\\s*$`) })
    .first()
    .waitFor({ state: "visible", timeout: READ_PATIENCE_MS });
});

Then("the page shows no tree", async function (this: BrowserWorld) {
  await expect(journey(this.page).byRole("tree")).toHaveCount(0);
});
