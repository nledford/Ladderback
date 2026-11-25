// The navigation bar (technical design §4.5,
// features/ui/components/navigation_bar.feature, and the header's): the
// brand, the pages' links and a group behind a disclosure menu, on one line
// where they fit and behind a hamburger below 54rem. Its parts are found by
// role and name, its panel through the hamburger's `aria-controls`, as a
// screen reader finds it; only their boxes and styles are read.

import assert from "node:assert/strict";

import { type DataTable, Then, When } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import { namePattern } from "../support/keyboard.ts";
import { pointer } from "../support/pointer.ts";
import type { BrowserWorld } from "../support/world.ts";

// Layout rounds to fractions of a pixel; anything within this is one place.
const ROUNDING_PX = 1;

/** The panel the button `name` controls, by its `aria-controls`. */
async function panelOf(page: Page, name: string): Promise<Locator> {
  const button = page.getByRole("button", { name: namePattern(name) });
  await expect(button).toHaveCount(1);
  const controls = await button.getAttribute("aria-controls");
  assert.ok(controls, `the button "${name}" controls nothing`);
  const panel = page.locator(`[id="${controls}"]`);
  await expect(panel).toHaveCount(1);
  return panel;
}

// The panel's rows are its drawn links and buttons, in the order of Tab: a
// row is in a column of its own where no row before it starts where it does,
// and each is tall enough for a finger.
Then(
  "the rows of the panel of the button {string} are these, in order, in {int} column(s), each at least {float} rem tall:",
  async function (this: BrowserWorld, name: string, columns: number, least: number, table: DataTable) {
    const panel = await panelOf(this.page, name);
    await expect(panel).toBeVisible();
    const wanted = (table.hashes() as { row: string }[]).map(({ row }) => row);
    const rem = await this.page.evaluate(() => Number.parseFloat(getComputedStyle(document.documentElement).fontSize));
    const rows = await panel.evaluate((box) =>
      Array.from(box.querySelectorAll("a, button"))
        .map((row) => ({ row, rect: row.getBoundingClientRect() }))
        .filter(({ rect }) => rect.width > 0 && rect.height > 0)
        .map(({ row, rect }) => ({ name: (row.textContent ?? "").trim(), start: rect.left, height: rect.height })),
    );
    assert.deepEqual(
      rows.map((row) => row.name),
      wanted,
      `the rows of the panel of the button "${name}"`,
    );
    const starts: number[] = [];
    for (const row of rows) {
      if (!starts.some((start) => Math.abs(start - row.start) <= ROUNDING_PX)) starts.push(row.start);
    }
    assert.equal(starts.length, columns, `the rows start at ${starts.map((start) => start.toFixed(1)).join(", ")}`);
    const short = rows.filter((row) => row.height < least * rem - 0.5).map((row) => `${row.name}: ${row.height.toFixed(1)}px`);
    assert.deepEqual(short, [], `rows under ${least}rem`);
  },
);

// Where an element stood, so a later step can tell that it hasn't moved.
const noted = new WeakMap<BrowserWorld, { top: number; left: number }>();

async function placeOf(part: Locator): Promise<{ top: number; left: number }> {
  await expect(part).toHaveCount(1);
  return part.evaluate((el) => {
    const box = el.getBoundingClientRect();
    return { top: box.top + window.scrollY, left: box.left + window.scrollX };
  });
}

const mainHeading = (page: Page): Locator => page.getByRole("main").getByRole("heading", { level: 1 });
const buttonNamed = (page: Page, name: string): Locator => page.getByRole("button", { name: namePattern(name) });

When("I note where the main heading stands", async function (this: BrowserWorld) {
  noted.set(this, await placeOf(mainHeading(this.page)));
});

When("I note where the button {string} stands", async function (this: BrowserWorld, name: string) {
  noted.set(this, await placeOf(buttonNamed(this.page, name)));
});

// Nothing moved: the open panel stands over the page rather than pushing
// what follows it down (decision 160).
async function standsWhereItStood(world: BrowserWorld, part: Locator, what: string): Promise<void> {
  const before = noted.get(world);
  assert.ok(before, "no step noted where it stood");
  const now = await placeOf(part);
  assert.ok(
    Math.abs(now.top - before.top) <= ROUNDING_PX && Math.abs(now.left - before.left) <= ROUNDING_PX,
    `${what} stood at ${JSON.stringify(before)} and now stands at ${JSON.stringify(now)}`,
  );
}

Then("the main heading stands where it stood", async function (this: BrowserWorld) {
  await standsWhereItStood(this, mainHeading(this.page), "the main heading");
});

Then("the button {string} stands where it stood", async function (this: BrowserWorld, name: string) {
  await standsWhereItStood(this, buttonNamed(this.page, name), `the button "${name}"`);
});

// The panel covers what lies under it: the topmost element at the middle of
// the part is the panel's.
async function covers(world: BrowserWorld, name: string, part: Locator, what: string): Promise<void> {
  const panel = await panelOf(world.page, name);
  await expect(panel).toBeVisible();
  const id = await panel.getAttribute("id");
  const covered = await part.evaluate((el, over) => {
    const box = el.getBoundingClientRect();
    const top = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    return Boolean(top?.closest(`[id="${over}"]`));
  }, id);
  assert.ok(covered, `the panel of the button "${name}" doesn't cover ${what}`);
}

Then("the panel of the button {string} covers the main heading", async function (this: BrowserWorld, name: string) {
  await covers(this, name, mainHeading(this.page), "the main heading");
});

Then("the panel of the button {string} covers the button {string}", async function (this: BrowserWorld, name: string, under: string) {
  await covers(this, name, buttonNamed(this.page, under), `the button "${under}"`);
});

// The whole of the main heading within the window's height with the page at
// its top, wherever the page is scrolled now: the header leaves room for it
// on the first screen.
Then("the main heading is inside the window", async function (this: BrowserWorld) {
  const drawn = await mainHeading(this.page).evaluate((el) => {
    const box = el.getBoundingClientRect();
    return { bottom: box.bottom + window.scrollY, height: window.innerHeight };
  });
  assert.ok(
    drawn.bottom <= drawn.height + ROUNDING_PX,
    `with the page at its top the main heading ends at ${Math.round(drawn.bottom)}px, past the window's ${drawn.height}px`,
  );
});

// A press on the header's band beside the open panel of the button `name`,
// a hair from the window's inline start, just under the bar's line: no
// part of the panel and nothing that takes focus, so focus goes nowhere,
// which leaves a panel open, and only the press itself can close it (#1582).
When(
  "I press the pointer on the header's band beside the panel of the button {string}",
  async function (this: BrowserWorld, name: string) {
    const panel = await panelOf(this.page, name);
    const point = await panel.evaluate((el) => {
      const box = el.getBoundingClientRect();
      const x = 2;
      const y = box.top + 4;
      const hit = document.elementFromPoint(x, y);
      return { x, y, outside: hit !== null && !el.contains(hit) && hit.closest("main, a, button, [tabindex]") === null };
    });
    assert.ok(point.outside, "the point beside the panel is in it, or in something that takes focus");
    await pointer(this.page).holdAt(point.x, point.y);
    await pointer(this.page).letGo();
  },
);

// A button that is on the page but drawn nowhere (`display: none`), as the
// hamburger is from 54rem up: found among hidden elements too, so a name
// with no button at all fails rather than passing as hidden.
Then("the button {string} is on the page but not drawn", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("button", { name: namePattern(name), includeHidden: true });
  await expect(button).toHaveCount(1);
  await expect(button).toBeHidden();
});

// Every link and button of the bar that holds the navigation `name` stands
// on one line: each one's middle lies within the first one's box.
Then("the navigation {string} and its brand stand on one line", async function (this: BrowserWorld, name: string) {
  const navigation = this.page.getByRole("navigation", { name: namePattern(name) });
  await expect(navigation).toBeVisible();
  const parts = await navigation.evaluate((nav) => {
    const bar = nav.closest(".navigation-bar");
    return Array.from(bar?.querySelectorAll("a, button") ?? [])
      .map((part) => ({ name: (part.textContent ?? "").trim(), brand: !nav.contains(part), rect: part.getBoundingClientRect() }))
      .filter(({ rect }) => rect.width > 0 && rect.height > 0)
      .map(({ name, brand, rect }) => ({ name, brand, top: rect.top, bottom: rect.bottom }));
  });
  assert.ok(
    parts.some((part) => part.brand),
    "no brand is drawn beside the navigation, in its bar",
  );
  assert.ok(
    parts.some((part) => !part.brand),
    "the navigation draws no link or button",
  );
  const top = Math.min(...parts.map((part) => part.top));
  const bottom = Math.min(...parts.map((part) => part.bottom));
  const off = parts.filter((part) => (part.top + part.bottom) / 2 > bottom || (part.top + part.bottom) / 2 < top);
  assert.deepEqual(
    off.map((part) => part.name),
    [],
    "parts below the bar's first line",
  );
});
