// The disclosure menu (technical design §4.5,
// features/ui/components/disclosure_menu.feature, and the header's Settings
// menu): a button that shows a panel of links under it. The panel is found
// through the button's `aria-controls`, as a screen reader finds it; its
// rows by role, and only their boxes and styles are read.

import assert from "node:assert/strict";

import { type DataTable, Then, When } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import { composite, contrastRatio, namePattern } from "../support/keyboard.ts";
import { pointer } from "../support/pointer.ts";
import type { BrowserWorld } from "../support/world.ts";

// Layout rounds to fractions of a pixel; anything within this is one place.
const ROUNDING_PX = 1;

function buttonNamed(page: Page, name: string): Locator {
  return page.getByRole("button", { name: namePattern(name), exact: false });
}

/** The panel the button `name` controls, by its `aria-controls`. */
async function menuOf(page: Page, name: string): Promise<Locator> {
  const button = buttonNamed(page, name);
  await expect(button).toHaveCount(1);
  const controls = await button.getAttribute("aria-controls");
  assert.ok(controls, `the button "${name}" controls nothing`);
  const panel = page.locator(`[id="${controls}"]`);
  await expect(panel).toHaveCount(1);
  return panel;
}

// Its rows are its links and buttons, in the order of Tab.
Then("the menu of the button {string} holds these, in order:", async function (this: BrowserWorld, name: string, table: DataTable) {
  const panel = await menuOf(this.page, name);
  await expect(panel).toBeVisible();
  const wanted = (table.hashes() as { row: string }[]).map(({ row }) => row);
  const rows = await panel.locator("a, button").evaluateAll((found) => found.map((row) => (row.textContent ?? "").trim()));
  assert.deepEqual(rows, wanted, `the menu of the button "${name}"`);
});

// Under its button, its inline end at the button's, so it opens into the
// page rather than past the window's edge.
Then(
  "the menu of the button {string} stands under its button, ending where it ends",
  async function (this: BrowserWorld, name: string) {
    const panel = await menuOf(this.page, name);
    const [button, box] = await Promise.all([
      buttonNamed(this.page, name).evaluate((el) => el.getBoundingClientRect().toJSON() as DOMRect),
      panel.evaluate((el) => el.getBoundingClientRect().toJSON() as DOMRect),
    ]);
    assert.ok(
      box.top >= button.bottom - ROUNDING_PX,
      `the menu starts ${Math.round(button.bottom - box.top)}px above its button's foot`,
    );
    assert.ok(
      Math.abs(box.right - button.right) <= ROUNDING_PX,
      `the menu ends ${Math.round(box.right - button.right)}px from its button's end`,
    );
  },
);

/** Checks every edge of the panel of the button `name` is within the
 * window's width: nothing of it is past an edge a reader would have to
 * scroll sideways to (1.4.10). */
export async function menuLiesInsideWindow(page: Page, name: string): Promise<void> {
  const panel = await menuOf(page, name);
  await expect(panel).toBeVisible();
  const drawn = await panel.evaluate((el) => {
    const box = el.getBoundingClientRect();
    return { left: box.left, right: box.right, top: box.top, width: document.documentElement.clientWidth };
  });
  assert.ok(drawn.left >= -ROUNDING_PX, `the menu starts ${Math.round(-drawn.left)}px past the window's start`);
  assert.ok(
    drawn.right <= drawn.width + ROUNDING_PX,
    `the menu ends ${Math.round(drawn.right - drawn.width)}px past the window's end`,
  );
  assert.ok(drawn.top >= -ROUNDING_PX, "the menu starts above the window");
}

Then("the menu of the button {string} lies inside the window", async function (this: BrowserWorld, name: string) {
  await menuLiesInsideWindow(this.page, name);
});

// Its edge is what tells the panel from the page under it (1.4.11).
Then(
  "the menu of the button {string} has an edge at least {int} to 1 against what it lies on",
  async function (this: BrowserWorld, name: string, least: number) {
    const panel = await menuOf(this.page, name);
    await expect(panel).toBeVisible();
    const colors = await panel.evaluate((box) => {
      const layers: string[] = [];
      for (let el = box.parentElement; el !== null; el = el.parentElement) layers.push(getComputedStyle(el).backgroundColor);
      const style = getComputedStyle(box);
      return { edge: style.borderTopColor, width: Number.parseFloat(style.borderTopWidth), on: layers };
    });
    assert.ok(colors.width >= 1, `the menu's edge is ${colors.width}px wide`);
    const ratio = contrastRatio(composite([colors.edge]), composite(colors.on));
    assert.ok(ratio >= least, `the menu's edge is ${ratio.toFixed(2)}:1 against what it lies on`);
  },
);

// The page the reader is on is one of the menu's: its button says so to a
// screen reader with `aria-current`, and to the eye as a current link is
// drawn, bold with a bar at its inline start.
Then(
  "the button {string} is marked as the current page's group, drawn bold with a bar at its inline start",
  async function (this: BrowserWorld, name: string) {
    const button = buttonNamed(this.page, name);
    await expect(button).toHaveAttribute("aria-current", "true");
    const drawn = await button.evaluate((el) => {
      const style = getComputedStyle(el);
      return {
        weight: Number(style.fontWeight),
        bar: Number.parseFloat(style.borderInlineStartWidth),
        style: style.borderInlineStartStyle,
      };
    });
    assert.ok(drawn.weight >= 700, `the button "${name}" is drawn at weight ${drawn.weight}, not bold`);
    assert.ok(drawn.bar >= 3 && drawn.style === "solid", `the button "${name}"'s bar is ${drawn.bar}px ${drawn.style}`);
  },
);

Then("the button {string} is not marked as the current page's group", async function (this: BrowserWorld, name: string) {
  const button = buttonNamed(this.page, name);
  await expect(button).not.toHaveAttribute("aria-current");
  const drawn = await button.evaluate((el) => {
    const style = getComputedStyle(el);
    return { weight: Number(style.fontWeight), bar: Number.parseFloat(style.borderInlineStartWidth) };
  });
  assert.ok(drawn.weight < 700, `the button "${name}" is drawn bold`);
  assert.equal(drawn.bar, 0, `the button "${name}" has a bar`);
});

// A press on the page's main heading, which takes no focus: outside any
// menu, the press alone, with no focus moving, is what closes an open one.
When("I press the pointer on the main heading", async function (this: BrowserWorld) {
  const heading = this.page.getByRole("main").getByRole("heading", { level: 1 });
  await pointer(this.page).holdOn(heading);
  await pointer(this.page).letGo();
});
