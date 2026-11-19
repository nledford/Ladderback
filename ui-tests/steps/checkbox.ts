// Journey steps for the shared checkbox (technical design §4.5): whether it
// is checked and what describes it.

import { Then } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import type { BrowserWorld } from "../support/world.ts";

Then("the checkbox {string} is checked", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("checkbox", { name, exact: true })).toBeChecked();
});

Then("the checkbox {string} is not checked", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("checkbox", { name, exact: true })).not.toBeChecked();
});

// What a screen reader reads after the name: the hint, wired with
// aria-describedby.
Then("the checkbox {string} is described by {string}", async function (this: BrowserWorld, name: string, description: string) {
  await expect(this.page.getByRole("checkbox", { name, exact: true })).toHaveAccessibleDescription(description);
});
