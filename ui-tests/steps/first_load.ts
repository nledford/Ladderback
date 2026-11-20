// Journey steps for a page's first load (technical design §4.5, the
// skeleton's caller): while the daemon's first answer is on its way, the page
// says the wait in words just before the region it fills, outside it, and
// the region is busy and shows the still shape of what is coming. A skeleton
// is hidden from a screen reader and has no role, so it is found by its class
// inside the busy region, which has no role or name either: it is found by
// the attribute that makes it busy, as the gallery's skeleton steps do. The
// words are found by their text.

import assert from "node:assert/strict";
import { setTimeout as sleep } from "node:timers/promises";

import { Given, Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import type { BrowserWorld } from "../support/world.ts";

// A pixel of rounding between skeletons laid out side by side.
const ROUNDING_PX = 1;

interface Box {
  top: number;
  height: number;
}

/** The main content's one busy region in view, once it shows skeletons that
 *  a screen reader skips. */
async function busyRegion(world: BrowserWorld): Promise<Locator> {
  const busy = world.page.getByRole("main").locator('[aria-busy="true"]:visible');
  await expect(busy).toHaveCount(1);
  const skeletons = busy.locator(".skeleton");
  assert.ok((await skeletons.count()) > 0, "the busy region shows no skeleton");
  for (const skeleton of await skeletons.all()) {
    await expect(skeleton).toHaveAttribute("aria-hidden", "true");
  }
  return busy;
}

/** Asserts `words`, the one visible run of them in `scope`, stand just
 *  before a busy region and outside it, where a screen reader reads them:
 *  ARIA lets assistive technology leave a busy region's content unread until
 *  it stops being busy (§4.5, the skeleton's caller). "Just before" is the
 *  next element after the words' line, the innermost element around them
 *  with anything after it. */
export async function saidJustBeforeBusy(scope: Locator, words: string): Promise<void> {
  const said = scope.getByText(words, { exact: true });
  await expect(said).toBeVisible();
  const placed = await said.evaluate((element) => {
    let line: Element | null = element;
    while (line && !line.nextElementSibling) {
      line = line.parentElement;
    }
    return {
      insideBusy: element.closest('[aria-busy="true"]') !== null,
      hidden: element.closest('[aria-hidden="true"]') !== null,
      beforeBusy: line?.nextElementSibling?.getAttribute("aria-busy") === "true",
    };
  });
  assert.deepEqual(
    placed,
    { insideBusy: false, hidden: false, beforeBusy: true },
    `"${words}" isn't just before a busy region, outside it, where a screen reader reads it`,
  );
}

/** The region's skeletons line by line, top to bottom: those that stand
 *  side by side share a line. */
async function linesOf(busy: Locator): Promise<Box[][]> {
  const boxes: Box[] = await busy.locator(".skeleton").evaluateAll((all) =>
    all.map((skeleton) => {
      const { top, height } = skeleton.getBoundingClientRect();
      return { top, height };
    }),
  );
  const lines: Box[][] = [];
  for (const box of boxes.sort((a, b) => a.top - b.top)) {
    const line = lines.at(-1);
    const first = line?.at(0);
    if (line && first && Math.abs(first.top - box.top) <= ROUNDING_PX) {
      line.push(box);
    } else {
      lines.push([box]);
    }
  }
  return lines;
}

// A table's shape: its header, then its rows, each a skeleton per column.
Then(
  "a region of the page is busy and shows the still shape of a table, its header and {int} rows of {int} columns",
  async function (this: BrowserWorld, rows: number, columns: number) {
    const lines = await linesOf(await busyRegion(this));
    assert.equal(lines.length, rows + 1, `the skeleton has ${lines.length} lines, not a header and ${rows} rows`);
    for (const [index, line] of lines.entries()) {
      assert.equal(line.length, columns, `line ${index + 1} of the skeleton has ${line.length} columns, not ${columns}`);
    }
  },
);

// A form's shape: each field a label above its box, which is taller than the
// label's line, then the button below them.
Then(
  "a region of the page is busy and shows the still shape of a form, {int} field(s) and its button",
  async function (this: BrowserWorld, fields: number) {
    const lines = await linesOf(await busyRegion(this));
    assert.equal(lines.length, fields * 2 + 1, `the skeleton has ${lines.length} lines, not ${fields} fields and a button`);
    assert.ok(
      lines.every((line) => line.length === 1),
      "a form's skeleton stands one part to a line",
    );
    const parts = lines.flat();
    for (let field = 0; field < fields; field += 1) {
      const [label, box] = [parts.at(field * 2), parts.at(field * 2 + 1)];
      assert.ok(label && box && box.height > label.height, `field ${field + 1} is no taller than its label`);
    }
    const [label, button] = [parts.at(0), parts.at(-1)];
    assert.ok(label && button && button.height > label.height, "the button is no taller than a label");
  },
);

Then("the page says {string} just before its busy region, outside it", async function (this: BrowserWorld, words: string) {
  await saidJustBeforeBusy(this.page.getByRole("main"), words);
});

Then("no region of the page is busy", async function (this: BrowserWorld) {
  await expect(this.page.getByRole("main").locator('[aria-busy="true"]')).toHaveCount(0);
});

// The page's timers run on a clock that stands still from before the page
// loads, so the steps say when the wait it shows begins and ends, however
// long the browser takes over them.
Given("the page's clock stands still from the start until a step moves it on", function (this: BrowserWorld) {
  this.fakeClock = true;
  this.clockStillFromStart = true;
});

// Each timer of the page that falls due meanwhile runs, in order.
When("the page's clock moves on {int} millisecond(s)", async function (this: BrowserWorld, ms: number) {
  assert.ok(this.fakeClock, "the page's clock can't be moved on: no Given installed it");
  await this.page.clock.runFor(ms);
});

// Before the wait has lasted long enough to show (§4.5): the region is busy
// from the start, but neither its still shape nor the words that say the wait
// show, so an answer that comes sooner flashes nothing.
Then(
  "a region of the page is busy but shows nothing yet, neither the still shape of what is coming nor {string}",
  async function (this: BrowserWorld, words: string) {
    const main = this.page.getByRole("main");
    const busy = main.locator('[aria-busy="true"]');
    await expect(busy).toHaveCount(1);
    await expect(busy).toBeHidden();
    await expect(busy.locator(".skeleton").filter({ visible: true })).toHaveCount(0);
    await expect(main.getByText(words, { exact: true }).filter({ visible: true })).toHaveCount(0);
  },
);

// How long, in real time, the page is watched while its clock stands still.
const KEPT_WATCH_MS = 500;
const KEPT_LOOK_EVERY_MS = 50;

// The wait, once shown, stays its least time (§4.5) even though the answer
// has come: watched for half a real second while the page's clock stands
// still, the region stays busy with its still shape, the words stay, and the
// content hasn't taken their place.
Then(
  "the page keeps the still shape of what is coming and says {string} while its clock stands still",
  async function (this: BrowserWorld, words: string) {
    const main = this.page.getByRole("main");
    const ends = Date.now() + KEPT_WATCH_MS;
    do {
      const shapes = await main.locator('[aria-busy="true"] .skeleton').filter({ visible: true }).count();
      const said = await main.getByText(words, { exact: true }).filter({ visible: true }).count();
      const tables = await main.getByRole("table").count();
      assert.ok(
        shapes > 0 && said === 1 && tables === 0,
        `the page stopped showing its wait: ${shapes} skeletons, "${words}" shown ${said} times, ${tables} tables`,
      );
      await sleep(KEPT_LOOK_EVERY_MS);
    } while (Date.now() < ends);
  },
);

// The reading is refused at the browser's door, as a stopped daemon's is.
Given("the Snapshots page's reading of the repositories reaches no daemon", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/repositories", (route) => route.abort("connectionrefused"));
  });
});
