// The language chosen in Settings (technical design §8.4, decision 101):
// `features/ui/language.feature`.
import assert from "node:assert/strict";

import { type DataTable, Given, Then, When } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import { journey } from "../support/keyboard.ts";
import { patienceMs } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";

// Where the page keeps the language chosen, in the browser's local storage
// (`crates/web/src/page.rs`).
const CHOSEN_KEY = "ladderback-language";

// Kept once, before the scenario's first page loads: a later load of the same
// tab finds what the page itself kept since, as a person's browser would.
Given("the language chosen in this browser is {string}", function (this: BrowserWorld, chosen: string) {
  this.contextRoutes.push(async (context) => {
    await context.addInitScript(
      ({ key, chosen }) => {
        if (sessionStorage.getItem("language-seeded") !== null) return;
        sessionStorage.setItem("language-seeded", "yes");
        localStorage.setItem(key, chosen);
      },
      { key: CHOSEN_KEY, chosen },
    );
  });
});

// A browser whose settings, or a private window, let a site keep nothing: it
// refuses every write to its storage, as such a browser does.
Given("this browser keeps nothing a page stores", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context) => {
    await context.addInitScript(() => {
      Storage.prototype.setItem = () => {
        throw new DOMException("The quota has been exceeded.", "QuotaExceededError");
      };
    });
  });
});

Then("the group {string} offers the radios, in order:", async function (this: BrowserWorld, group: string, radios: DataTable) {
  const names = radios.raw().flat();
  const offered = this.page.getByRole("radiogroup", { name: group, exact: true }).getByRole("radio");
  await expect(offered).toHaveCount(names.length);
  for (const [index, name] of names.entries()) {
    await expect(offered.nth(index)).toHaveAccessibleName(name);
  }
});

// The language a screen reader reads the name in, and the direction it runs:
// the `lang` and `dir` the radio's label declares itself (WCAG 3.1.2), not
// ones it inherits from the page, which a page in another language changes.
Then(
  "the label of the radio {string} is in the language {string}, running {word}",
  async function (this: BrowserWorld, name: string, language: string, direction: string) {
    const radio = this.page.getByRole("radio", { name, exact: true });
    await expect(radio).toHaveCount(1);
    const marked = await radio.evaluate((input) => {
      const label = (input as HTMLInputElement).labels?.[0];
      return {
        lang: label?.getAttribute("lang") ?? null,
        dir: label?.getAttribute("dir") ?? null,
      };
    });
    assert.deepEqual(marked, { lang: language, dir: direction }, `the label of the radio "${name}"`);
  },
);

// Saving a language shows the page again in it: the press loads the page anew,
// and the step waits for the new page's heading.
When("I press Enter, and the page loads again", async function (this: BrowserWorld) {
  await Promise.all([this.page.waitForEvent("load", { timeout: patienceMs() }), journey(this.page).press("Enter")]);
  await this.page.getByRole("heading", { level: 1 }).waitFor({ timeout: patienceMs() });
});

Then("no language is chosen in this browser", async function (this: BrowserWorld) {
  assert.equal(await this.page.evaluate((key) => localStorage.getItem(key), CHOSEN_KEY), null);
});
