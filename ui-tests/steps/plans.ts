// Steps of the Plans page (features/ui/plans.feature): typing over what a
// field holds, a plan's row gone from the list, the run the scheduler planned
// for a saved plan, and the change form of a plan on a page whose names a
// pseudolocale transformed. Elements are found by role and accessible name,
// as in every journey, or by a plan's ID, which is data no catalog changes.

import assert from "node:assert/strict";

import { Given, Then, When } from "@cucumber/cucumber";

import { journey, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";
import { plannedRun, rowNamed, until } from "./live.ts";
import { expand } from "./repositories.ts";

// A saved plan's list is read again as soon as the daemon answers, and the
// scheduler plans its run a moment later; ten seconds is generous for both.
const SAVE_PATIENCE_MS = 10_000;

// Leaves too few presses for a plan's row to be missed, and more than the
// page has stops before the table.
const MAX_TABS = 30;

// The page reads the plans list when it opens, and again once a plan is
// saved; the step holds every read after the first, so the reader can leave
// the page before a saved plan's list comes back.
Given(
  "the daemon answers each read of the plans list after the first {int} seconds late",
  async function (this: BrowserWorld, seconds: number) {
    let reads = 0;
    this.contextRoutes.push(async (context) => {
      await context.route(
        (url) => url.pathname === "/api/v1/plans",
        async (route) => {
          if (route.request().method() === "GET") {
            reads += 1;
            if (reads > 1) await new Promise((done) => setTimeout(done, seconds * 1_000));
          }
          await route.continue();
        },
      );
    });
  },
);

When("I select all the text and type {string}", async function (this: BrowserWorld, text: string) {
  const keys = journey(this.page);
  await keys.press("ControlOrMeta+A");
  await keys.type(await expand(this, text));
});

Then("the table {string} has no row {string}", async function (this: BrowserWorld, name: string, row: string) {
  const rows = rowNamed(this, name, row);
  await until(
    async () => ((await rows.count()) === 0 ? true : null),
    SAVE_PATIENCE_MS,
    `the table ${name} still has the row ${row}`,
  );
});

// The cell of a plan's next run carries in its `title` the run the daemon
// planned, as the plans list sends it.
Then(
  "the row {string} of the table {string} shows a next run",
  async function (this: BrowserWorld, row: string, name: string) {
    const found = rowNamed(this, name, row).first();
    await until(
      async () => {
        const planned = await plannedRun(this, row);
        if (planned === null) return null;
        const titles = await found
          .getByRole("cell")
          .evaluateAll((cells) => cells.map((cell) => cell.getAttribute("title")))
          .catch((): (string | null)[] => []);
        return titles.includes(planned) ? planned : null;
      },
      SAVE_PATIENCE_MS,
      `the row ${row} of the table ${name} never showed the run the daemon planned`,
    );
  },
);

// For a page whose buttons a pseudolocale named: the first button of the row
// the plan's ID heads.
When(
  "I press Tab until focus is on the first button of the plan {string}",
  async function (this: BrowserWorld, plan: string) {
    const keys = journey(this.page);
    const button = this.page
      .getByRole("row")
      .filter({ has: this.page.getByRole("rowheader", { name: namePattern(plan) }) })
      .getByRole("button")
      .first();
    await button.waitFor({ timeout: SAVE_PATIENCE_MS });
    const focused = () => button.evaluate((el) => el === document.activeElement);
    for (let presses = 0; presses < MAX_TABS && !(await focused()); presses += 1) {
      await keys.press("Tab");
    }
    assert.ok(await focused(), `focus did not reach the first button of the plan ${plan} within ${MAX_TABS} presses`);
  },
);

// The change form of a plan is headed, and named, by the plan's ID, and
// shows once the plan is read.
Then("the page shows the form of the plan {string}", async function (this: BrowserWorld, plan: string) {
  const pattern = new RegExp(plan.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  await this.page.getByRole("heading", { level: 2, name: pattern }).waitFor({ timeout: SAVE_PATIENCE_MS });
  await this.page
    .getByRole("form", { name: pattern })
    .getByRole("textbox")
    .first()
    .waitFor({ timeout: SAVE_PATIENCE_MS });
});

