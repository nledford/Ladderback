// Journey steps for the shared fingerprint in the gallery (§16, decision 121,
// "The fingerprint"): a group named by its caller, one image per character
// named by that character alone, its hyphens hidden from a screen reader, a
// copy of it the plain string, and its groups of four kept whole on a line.

import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

// A page's words isolate each name they hold (§4.5), as `\u2068…\u2069`;
// what a reader hears has no such marks.
const ISOLATION_MARKS = /[\u2066-\u2069]/g;

/** The fingerprint named `name`: the one group of that name on the page. */
function fingerprint(world: BrowserWorld, name: string): Locator {
  return world.page.getByRole("group", { name: namePattern(name) });
}

// What a screen reader is given: the group's name, then one image per
// character named by that character alone, and nothing for the hyphens. Read
// with locator.ariaSnapshot(), since toMatchAriaSnapshot() only works inside
// a Playwright test.
Then(
  "the fingerprint {string} is spoken a character at a time as {string}",
  async function (this: BrowserWorld, name: string, characters: string) {
    const spoken = [...characters].map((character) => `  - img "${character}"`).join("\n");
    const snapshot = (await fingerprint(this, name).ariaSnapshot()).trim().replace(ISOLATION_MARKS, "");
    assert.equal(snapshot, `- group "${name}":\n${spoken}`);
  },
);

// Selected as a person's drag over it would, from its first character to its
// last, so the copy is what the selection holds.
When("I select the whole fingerprint {string}", async function (this: BrowserWorld, name: string) {
  const group = fingerprint(this, name);
  await expect(group).toHaveCount(1);
  await group.evaluate((element) => {
    const selection = window.getSelection();
    if (selection === null) return;
    selection.removeAllRanges();
    const range = document.createRange();
    range.selectNodeContents(element);
    selection.addRange(range);
  });
});

Then("the selection reads {string}", async function (this: BrowserWorld, expected: string) {
  const read = await this.page.evaluate(() => window.getSelection()?.toString() ?? "");
  assert.equal(read, expected);
});

// Each group of four characters stands on one line, and the groups take more
// than one, so the fingerprint broke between groups and never inside one.
Then(
  "the fingerprint {string} takes more than one line, each of its groups whole on one",
  async function (this: BrowserWorld, name: string) {
    const characters = fingerprint(this, name).getByRole("img");
    await expect(characters).toHaveCount(20);
    const tops = await characters.evaluateAll((elements) =>
      elements.map((element) => Math.round(element.getBoundingClientRect().top)),
    );
    const groups = [0, 4, 8, 12, 16].map((start) => tops.slice(start, start + 4));
    for (const [place, group] of groups.entries()) {
      assert.equal(new Set(group).size, 1, `group ${place + 1} is split over lines at ${group.join(", ")}`);
    }
    assert.ok(new Set(tops).size > 1, "the fingerprint stands on one line, so nothing showed where it breaks");
  },
);
