// Steps of the Plans page (features/ui/plans.feature): typing over what a
// field holds, a plan's row gone from the list, the run the scheduler planned
// for a saved plan, and the change form of a plan on a page whose names a
// pseudolocale transformed. Elements are found by role and accessible name,
// as in every journey, or by a plan's ID, which is data no catalog changes.

import assert from "node:assert/strict";

import { Given, Then, When } from "@cucumber/cucumber";
import type { Request, Route } from "playwright";

import { journey, namePattern, pressUntilFocused } from "../support/keyboard.ts";
import { type BrowserWorld, evaluateAll } from "../support/world.ts";
import { plannedRun, rowNamed, until } from "./live.ts";
import { expand } from "./repositories.ts";

// A saved plan's list is read again as soon as the daemon answers, and the
// scheduler plans its run a moment later; ten seconds is generous for both.
const SAVE_PATIENCE_MS = 10_000;

// The reads of the plans list each world's daemon holds back, as the page's
// requests, and what lets them go, so a step can release them and wait until
// the page has the answers.
interface HeldReads {
  requests: Request[];
  release: () => void;
}
const heldReads = new WeakMap<BrowserWorld, HeldReads>();

// The page reads the plans list when it opens, and again once a plan is
// saved; the step holds back every read after the first until "the daemon has
// answered the held reads of the plans list" lets them go, so the reader can
// leave the form or the page before a saved plan's list comes back, however
// long the walk there takes. A read the page makes after that goes through.
Given("the daemon holds back its answer to each read of the plans list after the first", function (this: BrowserWorld) {
  let reads = 0;
  const { promise: released, resolve: release } = Promise.withResolvers<void>();
  const held: HeldReads = { requests: [], release };
  heldReads.set(this, held);
  this.contextRoutes.push(async (context) => {
    await context.route(
      (url) => url.pathname === "/api/v1/plans",
      async (route) => {
        if (route.request().method() === "GET") {
          reads += 1;
          if (reads > 1) {
            held.requests.push(route.request());
            await released;
          }
        }
        await route.continue();
      },
    );
  });
});

// Every read of the plans list after the first, the one a save makes among
// them, is answered by `answer` instead of the daemon.
function answerReadsAfterFirst(world: BrowserWorld, answer: (route: Route) => Promise<void>): void {
  let reads = 0;
  world.contextRoutes.push(async (context) => {
    await context.route(
      (url) => url.pathname === "/api/v1/plans",
      async (route) => {
        if (route.request().method() === "GET") {
          reads += 1;
          if (reads > 1) return answer(route);
        }
        await route.continue();
      },
    );
  });
}

// The daemon refuses the list as it does when it can't read the
// configuration file (503 config-unavailable).
Given("the daemon refuses each read of the plans list after the first", function (this: BrowserWorld) {
  answerReadsAfterFirst(this, (route) =>
    route.fulfill({
      status: 503,
      contentType: "application/json",
      body: JSON.stringify({
        code: "config-unavailable",
        args: { file: { type: "text", value: "/etc/ladderback/config.toml" } },
      }),
    }),
  );
});

// The read of one plan, the one its Change button makes, is refused as the
// list's is when the daemon can't read the configuration file.
Given("the daemon refuses the read of the plan {string}", function (this: BrowserWorld, id: string) {
  this.contextRoutes.push(async (context) => {
    await context.route(
      (url) => url.pathname === `/api/v1/plans/${encodeURIComponent(id)}`,
      async (route) => {
        if (route.request().method() !== "GET") return route.continue();
        await route.fulfill({
          status: 503,
          contentType: "application/json",
          body: JSON.stringify({
            code: "config-unavailable",
            args: { file: { type: "text", value: "/etc/ladderback/config.toml" } },
          }),
        });
      },
    );
  });
});

// The read is lost on its way, as when the daemon has stopped.
Given("each read of the plans list after the first reaches no daemon", function (this: BrowserWorld) {
  answerReadsAfterFirst(this, (route) => route.abort("connectionrefused"));
});

// Lets the held reads go, then waits until the page has each one's whole
// answer and has done what it does with it: a saved plan's page closes the
// form on the answer, then moves focus a timer later, so the step lets the
// page's queued timers and a frame run. The reads it lets go are the ones
// that have reached the daemon by then: a scenario walks the page between
// the save and this step, so the save's read is among them.
When("the daemon has answered the held reads of the plans list", async function (this: BrowserWorld) {
  const held = heldReads.get(this);
  assert.ok(held !== undefined && held.requests.length > 0, "the daemon held back no read of the plans list");
  held.release();
  for (const request of held.requests) {
    const response = await request.response();
    assert.ok(response, `the read of the plans list at ${request.url()} got no answer`);
    await response.finished();
  }
  await this.page.evaluate(
    () =>
      new Promise<void>((done) => {
        setTimeout(() => setTimeout(() => requestAnimationFrame(() => done()), 0), 0);
      }),
  );
});

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
        const titles = await evaluateAll(
          found.getByRole("cell"),
          (cells) => cells.map((cell) => cell.getAttribute("title")),
          undefined,
        ).catch((): (string | null)[] => []);
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
    const button = this.page
      .getByRole("row")
      .filter({ has: this.page.getByRole("rowheader", { name: namePattern(plan) }) })
      .getByRole("button")
      .first();
    await button.waitFor({ timeout: SAVE_PATIENCE_MS });
    await pressUntilFocused(this.page, "Tab", button, `the first button of the plan ${plan}`);
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

