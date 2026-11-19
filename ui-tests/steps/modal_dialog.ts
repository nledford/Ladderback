// Journey steps for the shared modal dialog in the gallery (technical design
// §4.5): its title, its polite region and its focus trap.

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import { journey, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

// A dialog is named by its title, a heading at the level its place in the
// page calls for (§4.5): the one its `aria-labelledby` points at.
Then("the dialog {string} is titled by a heading at level {int}", async function (this: BrowserWorld, name: string, level: number) {
  const dialog = journey(this.page).byRole("dialog", { name: namePattern(name) });
  const title = dialog.getByRole("heading", { level, name: namePattern(name) });
  assert.equal(await title.count(), 1, `the dialog "${name}" has no heading "${name}" at level ${level}`);
  assert.equal(
    await dialog.getAttribute("aria-labelledby"),
    await title.getAttribute("id"),
    `the dialog "${name}" is not named by its heading at level ${level}`,
  );
});

// A review says in its own polite region what the page's announcer can't
// while the modal is open (§4.5). The region has no role of its own (a second
// `status` would hide the page's one), so it is found as the live region
// holding the text.
Then("the dialog {string} politely says {string}", async function (this: BrowserWorld, name: string, text: string) {
  const dialog = journey(this.page).byRole("dialog", { name: namePattern(name) });
  await expect
    .poll(
      () =>
        dialog.evaluate((el, said) => {
          const marks = /[\u2066-\u2069]/g;
          return [...el.querySelectorAll('[aria-live="polite"]')].some((region) =>
            (region.textContent ?? "").replace(marks, "").includes(said),
          );
        }, text),
      { message: `the dialog "${name}" never said "${text}" in its polite region` },
    )
    .toBe(true);
});

// The counterpart of a button held with `aria-disabled`: it can be pressed.
Then("the dialog {string} has its button {string} available", async function (this: BrowserWorld, name: string, button: string) {
  const found = journey(this.page)
    .byRole("dialog", { name: namePattern(name) })
    .getByRole("button", { name: namePattern(button) });
  await expect(found).toHaveCount(1);
  await expect(found).not.toHaveAttribute("aria-disabled", "true");
  await expect(found).toHaveAccessibleDescription("");
});

// The wrap the dialog's own trap does: Tab on its last control puts focus on
// its first, not just anywhere inside.
Then("focus is on the first control of the dialog {string}", async function (this: BrowserWorld, name: string) {
  const first = this.page
    .getByRole("dialog", { name })
    .locator(
      'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    )
    .first();
  await journey(this.page).assertFocused(first, `the first control of the dialog ${JSON.stringify(name)}`);
});
