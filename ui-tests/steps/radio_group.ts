// Journey steps for the shared radio group (technical design §4.5): which
// radio is checked and what describes the group.

import { Then } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import type { BrowserWorld } from "../support/world.ts";

// A radio button found within its group, which names what it chooses.
function radioOfGroup(world: BrowserWorld, name: string, group: string) {
  return world.page.getByRole("radiogroup", { name: group, exact: true }).getByRole("radio", { name, exact: true });
}

Then("the radio {string} of the group {string} is checked", async function (this: BrowserWorld, name: string, group: string) {
  await expect(radioOfGroup(this, name, group)).toBeChecked();
});

Then("the radio {string} of the group {string} is not checked", async function (this: BrowserWorld, name: string, group: string) {
  await expect(radioOfGroup(this, name, group)).not.toBeChecked();
});

// What a screen reader reads after the group's name as it enters the group.
Then("the group {string} is described by {string}", async function (this: BrowserWorld, name: string, description: string) {
  await expect(this.page.getByRole("radiogroup", { name, exact: true })).toHaveAccessibleDescription(description);
});
