// Steps of a machine whose ID the hub refused for a second computer
// (features/ui/second_computer_refused.feature; §16, decision 150): the
// hub's list of machines, given the time of that refusal for some of them,
// what each machine's row on the Machines tab says and the warnings above
// the tables. A second computer can't connect from the one computer a
// journey runs on, which has one machine identifier, so the hub's own
// answer is given the refusals on its way to the page, as the start page's
// machine notice is (copied_data_folder.ts). Rows are found by role and
// accessible name, as in every journey; a warning, which has no role, by
// its class, as alert.ts finds one.

import { Given, Then } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";
import { ISOLATION_MARKS } from "./pairing_dialog.ts";

/** A listed machine, as far as these steps read and change it. */
interface Listed {
  hostname: string;
  since: string;
  "seconds-ago": number;
  "second-machine-at"?: string;
}

// The refusal stands half a second further back than the step says, so the
// whole seconds the page shows are the step's however the hub's sub-second
// times fall.
const HALF_SECOND_MS = 500;

// How many minutes ago the hub refused a second computer as each machine,
// by hostname, for each journey. One route gives them all: a second route
// would answer the page in the first one's place.
const refusals = new WeakMap<BrowserWorld, Map<string, number>>();

Given(
  "the hub refused a second computer as {string} {int} minutes ago",
  function (this: BrowserWorld, hostname: string, minutes: number) {
    const known = refusals.get(this);
    if (known !== undefined) {
      known.set(hostname, minutes);
      return;
    }
    const given = new Map([[hostname, minutes]]);
    refusals.set(this, given);
    this.contextRoutes.push(async (context) => {
      await context.route("**/api/v1/hub/machines", async (route) => {
        if (route.request().method() !== "GET") {
          await route.fallback();
          return;
        }
        const answer = await route.fetch();
        const body = (await answer.json()) as { machines?: Listed[] };
        for (const agent of body.machines ?? []) {
          const ago = given.get(agent.hostname);
          if (ago === undefined) continue;
          // The hub's now, as the page reads it: when the row's status began
          // and how long ago that was, by the hub's clock.
          const now = Date.parse(agent.since) + agent["seconds-ago"] * 1000;
          agent["second-machine-at"] = new Date(now - ago * 60_000 - HALF_SECOND_MS).toISOString();
        }
        await route.fulfill({ response: answer, json: body });
      });
    });
  },
);

/** The commands the warnings above the tables give, in the page's order. */
function commands(world: BrowserWorld): Locator {
  return world.page.locator(".alert.tone-warning code");
}

/** The row of the table `table` whose header is the machine `hostname`. */
function row(world: BrowserWorld, table: string, hostname: string): Locator {
  return world.page
    .getByRole("table", { name: namePattern(table) })
    .getByRole("row")
    .filter({ has: world.page.getByRole("rowheader", { name: namePattern(hostname) }) });
}

// The row's text drops the isolation marks around fetched text, which a
// reader doesn't hear.
Then(
  "the row of {string} in the table {string} says {string}",
  async function (this: BrowserWorld, hostname: string, table: string, words: string) {
    await expect
      .poll(async () => ((await row(this, table, hostname).textContent()) ?? "").replace(ISOLATION_MARKS, ""))
      .toContain(words);
  },
);

Then(
  "the row of {string} in the table {string} says nothing of a refused computer",
  async function (this: BrowserWorld, hostname: string, table: string) {
    await expect(row(this, table, hostname)).toHaveCount(1);
    await expect(row(this, table, hostname)).not.toContainText("refused another computer");
    await expect(row(this, table, hostname)).not.toContainText("--forget-computer");
  },
);

// A line of its own: the command's box is one line tall and starts at its
// paragraph's start, so nothing of it wrapped and no words run before it.
Then("the page shows the command {string} on a line of its own", async function (this: BrowserWorld, command: string) {
  const code = commands(this).filter({ hasText: command });
  await expect(code).toHaveCount(1);
  await expect(code).toHaveText(command);
  const box = await code.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    const line = Number.parseFloat(getComputedStyle(element).lineHeight);
    const words = element.parentElement?.getBoundingClientRect();
    return { tall: rect.height, line, startsLine: words !== undefined && Math.abs(rect.left - words.left) < 1 };
  });
  expect(box.tall).toBeLessThan(box.line * 1.5);
  expect(box.startsLine).toBe(true);
});

Then(
  "the warnings above the tables give the commands for {string}, then {string}",
  async function (this: BrowserWorld, first: string, second: string) {
    await expect(commands(this)).toHaveText([
      `ladderback pairing-token --forget-computer ${first}`,
      `ladderback pairing-token --forget-computer ${second}`,
    ]);
  },
);
