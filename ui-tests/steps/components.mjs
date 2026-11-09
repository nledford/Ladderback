// Journey steps for the shared component set in the gallery (technical design
// §4.5): the dialog, the data table, the form field, the progress bar and the
// toast. Keyboard only, role-and-name locators, as everywhere in steps/.

import assert from "node:assert/strict";

import AxeBuilder from "@axe-core/playwright";
import { Then, When } from "@cucumber/cucumber";
import { expect } from "playwright/test";

import { journey } from "../support/keyboard.mjs";

// WCAG 2.2 A and AA, as in steps/start_page.mjs.
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

/** The gallery section of a component, found by its heading, not by a test id. */
function demoSection(page, component) {
  return page.locator("section", {
    has: page.getByRole("heading", { level: 2, name: component }),
  });
}

Then("focus is on the button {string}", async function (name) {
  await journey(this.page).assertFocused(
    journey(this.page).byRole("button", { name }),
    `the button ${JSON.stringify(name)}`,
  );
});

Then("the table {string} is sorted by repository ascending", async function (name) {
  const cells = await this.page
    .getByRole("table", { name })
    .getByRole("cell")
    .allTextContents();
  assert.deepEqual(
    cells,
    [...cells].sort((a, b) => a.localeCompare(b)),
    `the table ${JSON.stringify(name)} is not sorted ascending: ${cells.join(", ")}`,
  );
});

Then("the field {string} shows the error {string}", async function (name, error) {
  await expect(this.page.getByText(error)).toBeVisible();
  const input = this.page.getByRole("textbox", { name });
  const describedBy = await input.getAttribute("aria-describedby");
  assert.ok(
    describedBy?.split(/\s+/).some((id) => id) &&
      (await Promise.all(
        (describedBy ?? "")
          .split(/\s+/)
          .filter(Boolean)
          .map((id) => this.page.locator(`#${id}`).textContent()),
      ))
        .join(" ")
        .includes(error),
    `the field ${JSON.stringify(name)} is not described by ${JSON.stringify(error)} (aria-describedby=${describedBy})`,
  );
});

Then("the field {string} is described by {string}", async function (name, description) {
  const describedBy = await this.page
    .getByRole("textbox", { name })
    .getAttribute("aria-describedby");
  const text = (
    await Promise.all(
      (describedBy ?? "")
        .split(/\s+/)
        .filter(Boolean)
        .map((id) => this.page.locator(`#${id}`).textContent()),
    )
  ).join(" ");
  assert.ok(
    text.includes(description),
    `the field ${JSON.stringify(name)} is described by ${JSON.stringify(text)}, not ${JSON.stringify(description)}`,
  );
});

Then("the progress bar {string} shows {int} percent", async function (name, percent) {
  await expect(this.page.getByRole("progressbar", { name })).toHaveAttribute(
    "aria-valuenow",
    String(percent),
  );
});

Then("a toast reads {string}", async function (text) {
  await expect(this.page.locator("main").getByText(text)).toBeVisible();
});

// A toast is transient; ten seconds is generous for however long it shows.
// Scoped to main: the polite live region carries the same text.
When("the toast {string} is gone", async function (text) {
  await expect(this.page.locator("main").getByText(text)).toBeHidden({
    timeout: 10_000,
  });
});

Then("the {string} demo has no accessibility violations", async function (component) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: 5_000 });
  const { violations } = await new AxeBuilder({ page: this.page })
    .withTags(WCAG_TAGS)
    .analyze(section);
  const summary = violations.map((v) => `${v.id}: ${v.help} (${v.nodes.length} nodes)`);
  assert.deepEqual(summary, [], `${component} demo violations: ${summary.join("; ")}`);
});

Then("the {string} demo matches its ARIA snapshot", async function (component) {
  const snapshot = await demoSection(this.page, component).ariaSnapshot();
  assert.equal(
    snapshot.trim(),
    SNAPSHOTS[component].trim(),
    `the ${component} demo's ARIA snapshot`,
  );
});

// The pinned structure of each demo (roles and names, not styling), compared
// with locator.ariaSnapshot(): toMatchAriaSnapshot() only works inside a
// Playwright test, and these run under Cucumber. Regenerate a snapshot by
// logging demoSection(page, name).ariaSnapshot() and pasting it here.
const SNAPSHOTS = {
  Dialog: `- heading "Dialog" [level=2]
- button "Open the demo dialog"`,
  "Data table": `- heading "Data table" [level=2]
- table "Demo repositories":
  - caption: Demo repositories
  - rowgroup:
    - row "Repository Sort by repository":
      - columnheader "Repository Sort by repository":
        - text: Repository
        - button "Sort by repository"
  - rowgroup:
    - row "moor":
      - cell "moor"
    - row "harbor":
      - cell "harbor"
    - row "anchor":
      - cell "anchor"`,
  "Form field": `- heading "Form field" [level=2]
- text: Repository name
- textbox "Repository name"
- paragraph: The name of the repository to check.
- paragraph:
  - button "Check the repository name"`,
  "Status badge": `- heading "Status badge" [level=2]
- paragraph: Loading Empty Error Success`,
  Progress: `- heading "Progress" [level=2]
- progressbar "Demo backup progress"
- paragraph:
  - button "Advance the demo progress"`,
  Toast: `- heading "Toast" [level=2]
- paragraph:
  - button "Save the demo settings"`,
};
