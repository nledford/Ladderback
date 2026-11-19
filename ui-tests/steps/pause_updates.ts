// Journey steps for the shared pause control (technical design §4.5): the
// banner that describes its button while live updates are paused.

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import type { BrowserWorld } from "../support/world.ts";

// The banner sits beside the focused button, so an edge in the focus ring's
// color would read as a second focus ring (2.4.7): its edge has the color of a
// rule, not of focus.
Then("the banner describing the button {string} is edged unlike its focus ring", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("button", { name, exact: true });
  await expect(button).toBeFocused();
  const [ring, edge] = await button.evaluate((el) => {
    const banner = document.getElementById(el.getAttribute("aria-describedby") ?? "");
    if (banner === null) {
      throw new Error("the button is described by no banner");
    }
    const outline = getComputedStyle(el);
    const border = getComputedStyle(banner);
    return [
      outline.outlineStyle === "none" ? null : outline.outlineColor,
      border.borderTopStyle === "none" ? null : border.borderTopColor,
    ];
  });
  assert.notEqual(ring, null, `the button ${JSON.stringify(name)} shows no focus ring`);
  assert.notEqual(edge, null, "the banner has no edge");
  assert.notEqual(edge, ring, `the banner's edge is ${edge}, the focus ring's color`);
});
