// The width of the pages chosen in Settings (technical design §4.5):
// `features/ui/page_width.feature`.
import assert from "node:assert/strict";

import { Given, Then } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

// Where the page keeps the width chosen, in the browser's local storage, and
// what it keeps for each choice: Full width as `full`, Narrow as nothing
// (`crates/web/src/page.rs`, `crates/web/src/page_width.rs`).
const CHOSEN_KEY = "ladderback-page-width";
const KEPT: Record<string, string | null> = { Narrow: null, "Full width": "full" };

// Kept once, before the scenario's first page loads: a later load of the same
// tab finds what the page itself kept since, as a person's browser would.
Given("the page width chosen in this browser is {string}", function (this: BrowserWorld, chosen: string) {
  const kept = KEPT[chosen];
  assert.ok(kept !== undefined, `"${chosen}" is not a page width Settings offers`);
  if (kept === null) return;
  this.contextRoutes.push(async (context) => {
    await context.addInitScript(
      ({ key, kept }) => {
        if (sessionStorage.getItem("page-width-seeded") !== null) return;
        sessionStorage.setItem("page-width-seeded", "yes");
        localStorage.setItem(key, kept);
      },
      { key: CHOSEN_KEY, kept },
    );
  });
});

// The width of `<main>` as drawn, its padding included, which the window's
// width minus its scroll bar (`clientWidth` of the root) bounds, and the room
// the window leaves on each side of it.
async function mainWidths(world: BrowserWorld) {
  return world.page.locator("main").evaluate((main) => {
    const drawn = main.getBoundingClientRect();
    const window = document.documentElement.clientWidth;
    return {
      main: drawn.width,
      window,
      before: drawn.left,
      after: window - drawn.right,
      rem: Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
    };
  });
}

// A choice applies once the page has drawn it, so each step waits for it.
Then("the main content is no wider than {int} rem", async function (this: BrowserWorld, limit: number) {
  await expect
    .poll(
      async () => {
        const { main, rem } = await mainWidths(this);
        return main <= limit * rem + 0.5;
      },
      { message: `the main content is wider than ${limit} rem` },
    )
    .toBe(true);
});

Then("the main content is as wide as the window", async function (this: BrowserWorld) {
  await expect
    .poll(
      async () => {
        const { main, window } = await mainWidths(this);
        return Math.abs(main - window) <= 0.5;
      },
      { message: "the main content is not as wide as the window" },
    )
    .toBe(true);
});

Then("the main content is centered in the window", async function (this: BrowserWorld) {
  await expect
    .poll(
      async () => {
        const { before, after } = await mainWidths(this);
        return before > 0 && Math.abs(before - after) <= 0.5;
      },
      { message: "the main content is not centered in the window" },
    )
    .toBe(true);
});

Then("the table {string} is at least {int} pixels wide", async function (this: BrowserWorld, name: string, least: number) {
  const table = this.page.getByRole("table", { name, exact: true });
  await expect(table).toHaveCount(1);
  await expect
    .poll(async () => (await table.boundingBox())?.width ?? 0, { message: `the width of the table "${name}"` })
    .toBeGreaterThanOrEqual(least);
});

// A readable line length: each paragraph's and each form's text runs no wider
// than `limit` characters of its own font, measured as CSS measures them, by
// the width of "0" (`ch`): a probe `limit`ch wide, placed in the element, gives
// it. One probe of the whole measure, not `limit` times a 1ch probe: layout
// snaps a box's width to a fraction of a pixel, and multiplying a snapped 1ch
// by 70 can miss `max-inline-size: 70ch` by more than a pixel.
Then("no paragraph or form of the main content is wider than {int} characters", async function (this: BrowserWorld, limit: number) {
  const tooWide = await this.page.locator("main").evaluate((main, limit) => {
    const wide: string[] = [];
    for (const element of main.querySelectorAll<HTMLElement>("p, form")) {
      if (element.getClientRects().length === 0) continue;
      const probe = document.createElement("span");
      probe.style.cssText = `position: absolute; inline-size: ${limit}ch; block-size: 0; visibility: hidden`;
      element.append(probe);
      const measure = probe.getBoundingClientRect().width;
      probe.remove();
      const style = getComputedStyle(element);
      const edges = ["padding-inline-start", "padding-inline-end", "border-inline-start-width", "border-inline-end-width"]
        .map((property) => Number.parseFloat(style.getPropertyValue(property)) || 0)
        .reduce((sum, width) => sum + width, 0);
      const inner = element.getBoundingClientRect().width - edges;
      if (inner > measure + 0.5) {
        wide.push(
          `<${element.localName}> "${(element.textContent ?? "").trim().slice(0, 40)}…" is ${(inner / (measure / limit)).toFixed(2)} characters wide`,
        );
      }
    }
    return wide;
  }, limit);
  assert.deepEqual(tooWide, [], `paragraphs or forms wider than ${limit} characters`);
});

// The header's content stands in the page's frame (decision 141): the band
// behind the header spans the window, and the header's own content box starts
// and ends where `<main>`'s does, so the product's name starts where each
// page's heading does. Each landmark is found by role; only boxes are read.
async function contentEdges(landmark: Locator) {
  return landmark.evaluate((element) => {
    const drawn = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    const window = document.documentElement.clientWidth;
    return {
      before: drawn.left + Number.parseFloat(style.paddingLeft),
      after: window - drawn.right + Number.parseFloat(style.paddingRight),
    };
  });
}

Then("the header's band is as wide as the window", async function (this: BrowserWorld) {
  const { band, window } = await this.page.getByRole("banner").evaluate((header) => ({
    band: (header.parentElement ?? header).getBoundingClientRect().width,
    window: document.documentElement.clientWidth,
  }));
  assert.ok(Math.abs(band - window) <= 0.5, `the header's band is ${band}px wide in a ${window}px window`);
});

async function assertSharedEdge(world: BrowserWorld, edge: "before" | "after", side: string) {
  const [banner, main] = await Promise.all([contentEdges(world.page.getByRole("banner")), contentEdges(world.page.getByRole("main"))]);
  assert.ok(
    Math.abs(banner[edge] - main[edge]) <= 0.5,
    `the header's content is ${banner[edge]}px from the window's ${side}, the main content ${main[edge]}px`,
  );
}

Then(
  "the banner's content and the main content start at the same distance from the window's inline start",
  async function (this: BrowserWorld) {
    await assertSharedEdge(this, "before", "inline start");
  },
);

Then(
  "the banner's content and the main content end at the same distance from the window's inline end",
  async function (this: BrowserWorld) {
    await assertSharedEdge(this, "after", "inline end");
  },
);

Then("the link {string} starts where the main heading starts", async function (this: BrowserWorld, name: string) {
  const link = this.page.getByRole("banner").getByRole("link", { name: namePattern(name) });
  const heading = this.page.getByRole("main").getByRole("heading", { level: 1 });
  await expect(link).toBeVisible();
  await expect(heading).toBeVisible();
  const [linkStart, headingStart] = [
    await link.evaluate((element) => element.getBoundingClientRect().left),
    await heading.evaluate((element) => element.getBoundingClientRect().left),
  ];
  assert.ok(
    Math.abs(linkStart - headingStart) <= 1,
    `the link "${name}" starts at ${linkStart}px, the main heading at ${headingStart}px`,
  );
});
