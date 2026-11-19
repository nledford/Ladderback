// Journey steps for the shared steps in the gallery (technical design §4.5):
// a process's steps in order, each step's name and its state in words, an
// icon of its own for each state, the step the process stands at marked as
// the current one, and a row where there is room, a column where there
// isn't. A list is found by its role and name; the demo's lists, under a
// pseudolocale too, which renames them, by the demo's id.

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";
import type { DataTable } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { LIVE_REGION } from "../support/gallery.ts";
import { expect } from "../support/expect.ts";
import { said } from "../support/said.ts";
import type { BrowserWorld } from "../support/world.ts";

// The gallery's lists of steps (`gallery/steps.rs`): the hub move's five and the
// pairing's three.
const LISTS = 2;
const STEPS = 8;

// Half a pixel of rounding in a laid-out length.
const ROUNDING_PX = 0.5;

const stepsList = (page: Page, name: string): Locator => page.getByRole("list", { name, exact: true });

const demoLists = (page: Page): Locator => page.locator("#steps-demo").getByRole("list");

type Box = { left: number; right: number; top: number; bottom: number };

// Each step of `list`, where it stands, where its icon stands, and the
// direction the list runs in.
const laidOut = (list: Locator) =>
  list.evaluate((element) => {
    const box = (part: Element | null): Box | null => {
      const rect = part?.getBoundingClientRect();
      return rect === undefined ? null : { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom };
    };
    return {
      name: element.getAttribute("aria-label") ?? "",
      direction: getComputedStyle(element).direction,
      steps: Array.from(element.children, (item) => ({
        step: box(item) as Box,
        icon: box(item.querySelector("svg")),
      })),
    };
  });

// Each step's name and its state in words, read as a screen reader reads
// the item (the icon is hidden from it), in order.
Then("the list of steps {string} reads, in order:", async function (this: BrowserWorld, name: string, table: DataTable) {
  const expected = table.hashes().map((row) => `${row.step} ${row.state}`);
  const items = stepsList(this.page, name).getByRole("listitem");
  await expect(items).toHaveCount(expected.length);
  const all = await items.all();
  const read = await Promise.all(all.map((item) => said(item)));
  assert.deepEqual(read, expected, `the steps of ${name}`);
  // The state is in words a reader sees too (§4.5), not only in words a
  // screen reader says: no box too small to see, none clipped away as
  // visually hidden text is.
  const unseen = await Promise.all(
    all.map((item) =>
      item.evaluate((element) => {
        const state = element.querySelector(".steps-state");
        if (state === null) return "no state in words";
        const rect = state.getBoundingClientRect();
        const style = getComputedStyle(state);
        if (rect.width <= 1 || rect.height <= 1) return `a box of ${rect.width} by ${rect.height} px`;
        if (style.clipPath !== "none" || style.clip !== "auto") return `clipped (${style.clipPath}, ${style.clip})`;
        return null;
      }),
    ),
  );
  for (const [at, why] of unseen.entries()) {
    assert.equal(why, null, `the state of step ${at + 1} of ${name} isn't shown: ${why}`);
  }
});

// One step carries `aria-current="step"`, the one the process stands at.
Then("the current step of the list of steps {string} is {string}", async function (this: BrowserWorld, name: string, step: string) {
  const current = stepsList(this.page, name).locator('[aria-current="step"]');
  await expect(current).toHaveCount(1);
  await expect(current).toHaveRole("listitem");
  await expect(current.locator(".steps-name")).toHaveText(step);
});

// Never color alone (A2): each state's icon has a shape of its own, the same
// for every step in that state, before the step's name, and is decoration a
// screen reader skips, since the words under the name say the state.
Then(
  "each state of a step in the steps demo has an icon of its own before its name, which a screen reader skips",
  async function (this: BrowserWorld) {
    const items = this.page.locator("#steps-demo").getByRole("listitem");
    await expect(items).toHaveCount(STEPS);
    const shapes = new Map<string, Set<string>>();
    for (const item of await items.all()) {
      const found = await item.evaluate((element) => {
        const icons = element.querySelectorAll("svg");
        const icon = icons[0];
        const name = element.querySelector(".steps-name");
        return {
          icons: icons.length,
          hidden: icon?.getAttribute("aria-hidden") ?? null,
          focusable: icon?.getAttribute("focusable") ?? null,
          shape: icon?.innerHTML ?? "",
          before: icon !== undefined && name !== null && (icon.compareDocumentPosition(name) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0,
          state: element.querySelector(".steps-state")?.textContent?.trim() ?? "",
        };
      });
      assert.equal(found.icons, 1, `the ${found.state} step has one icon`);
      assert.equal(found.hidden, "true", `the ${found.state} step's icon is hidden from a screen reader`);
      assert.equal(found.focusable, "false", `the ${found.state} step's icon takes no focus`);
      assert.ok(found.before, `the ${found.state} step's icon stands before its name`);
      const seen = shapes.get(found.state) ?? new Set<string>();
      seen.add(found.shape);
      shapes.set(found.state, seen);
    }
    assert.deepEqual([...shapes.keys()].sort(), ["Current", "Done", "Failed", "To do"]);
    for (const [state, seen] of shapes) assert.equal(seen.size, 1, `every ${state} step has the same icon`);
    const distinct = new Set([...shapes.values()].map((seen) => [...seen][0]));
    assert.equal(distinct.size, shapes.size, "no two states share an icon");
  },
);

// It is no live region (§4.5): the announcer says a change of step.
Then("the list of steps {string} is no live region, nor inside one", async function (this: BrowserWorld, name: string) {
  const live = await stepsList(this.page, name).evaluate((list, selector) => {
    const around = list.closest(selector);
    if (around !== null) return around.outerHTML.slice(0, 120);
    const inside = list.querySelector(selector);
    return inside === null ? null : inside.outerHTML.slice(0, 120);
  }, LIVE_REGION);
  assert.equal(live, null, `the list of steps ${name} is in a live region`);
});

// Tab from the demo's one control leaves the demo: no list of steps after
// it, nor any step, is a stop for Tab (§4.5).
Then("focus has left the steps demo", async function (this: BrowserWorld) {
  const where = await this.page.evaluate(() => {
    const active = document.activeElement;
    if (active === null || active === document.body) return "on the page itself";
    return active.closest("#steps-demo") === null ? null : active.outerHTML.slice(0, 120);
  });
  assert.equal(where, null, `focus is still in the steps demo, or fell to the page: ${where}`);
});

// Where there is room (42rem of the list's own container), the steps stand
// side by side, the first at the start of the line: the left in English, the
// right in Arabic.
Then(
  "every list of steps of the steps demo stands in a row, in a page that runs {word} to {word}",
  async function (this: BrowserWorld, from: string, to: string) {
    const direction = from === "right" && to === "left" ? "rtl" : "ltr";
    const lists = demoLists(this.page);
    await expect(lists).toHaveCount(LISTS);
    for (const list of await lists.all()) {
      const drawn = await laidOut(list);
      assert.equal(drawn.direction, direction, drawn.name);
      drawn.steps.forEach((after, at) => {
        const before = drawn.steps[at - 1];
        if (before === undefined) return;
        assert.ok(Math.abs(before.step.top - after.step.top) <= ROUNDING_PX, `${drawn.name}: ${JSON.stringify(drawn)}`);
        const follows =
          direction === "ltr"
            ? after.step.left >= before.step.right - ROUNDING_PX
            : after.step.right <= before.step.left + ROUNDING_PX;
        assert.ok(follows, `${drawn.name} runs ${direction}: ${JSON.stringify(drawn)}`);
      });
    }
  },
);

// The 320 Rule (1.4.10, 1.4.4): on a small screen with larger text the steps
// stand one under another, each inside the window, its icon at the start
// of its line.
Then(
  "every list of steps of the steps demo stands in a column, every step whole inside the window, in a page that runs {word} to {word}",
  async function (this: BrowserWorld, from: string, to: string) {
    const direction = from === "right" && to === "left" ? "rtl" : "ltr";
    const width = await this.page.evaluate(() => document.documentElement.clientWidth);
    const lists = demoLists(this.page);
    await expect(lists).toHaveCount(LISTS);
    for (const list of await lists.all()) {
      const drawn = await laidOut(list);
      assert.equal(drawn.direction, direction, drawn.name);
      drawn.steps.forEach(({ step, icon }, at) => {
        assert.ok(
          step.left >= -ROUNDING_PX && step.right <= width + ROUNDING_PX,
          `${drawn.name}: step ${at + 1} leaves the window: ${JSON.stringify(step)}`,
        );
        assert.ok(icon !== null, `${drawn.name}: step ${at + 1} has an icon`);
        const start = direction === "ltr" ? icon.left - step.left : step.right - icon.right;
        assert.ok(
          start >= -ROUNDING_PX && start <= ROUNDING_PX,
          `${drawn.name}: step ${at + 1}'s icon isn't at its start: ${JSON.stringify({ step, icon })}`,
        );
        const before = drawn.steps[at - 1];
        if (before !== undefined) {
          assert.ok(
            step.top >= before.step.bottom - ROUNDING_PX,
            `${drawn.name}: step ${at + 1} isn't under the one before it: ${JSON.stringify(drawn)}`,
          );
        }
      });
    }
  },
);

// A long name wraps, never cut: its text takes more than one line.
Then("a step of the steps demo wraps its name onto more than one line", async function (this: BrowserWorld) {
  const lines = await this.page.locator("#steps-demo .steps-name").evaluateAll((names) =>
    names.map((name) => {
      const range = document.createRange();
      range.selectNodeContents(name);
      return new Set(Array.from(range.getClientRects(), (line) => Math.round(line.top))).size;
    }),
  );
  assert.equal(lines.length, STEPS);
  assert.ok(
    lines.some((count) => count > 1),
    `no step's name wraps: ${JSON.stringify(lines)}`,
  );
});

// Under forced colors (A2) the state's icon and the line between the steps
// keep their strokes, which the system draws in its text colors: neither
// vanishes into the page's background.
Then(
  "every icon and connector of the steps demo is drawn in a color of its own, not the page's background",
  async function (this: BrowserWorld) {
    const drawn = await this.page.locator("#steps-demo").evaluate((demo) => {
      const background = getComputedStyle(document.body).backgroundColor;
      const visible = (element: Element) => getComputedStyle(element).visibility === "visible";
      return {
        background,
        icons: Array.from(demo.querySelectorAll("li svg"), (icon) => getComputedStyle(icon).stroke),
        connectors: Array.from(demo.querySelectorAll(".steps-connector"))
          .filter((line) => visible(line) && getComputedStyle(line).display !== "none")
          .map((line) => {
            const style = getComputedStyle(line);
            const vertical = Number.parseFloat(style.borderInlineStartWidth) > 0;
            return vertical
              ? { color: style.borderInlineStartColor, width: style.borderInlineStartWidth, style: style.borderInlineStartStyle }
              : { color: style.borderTopColor, width: style.borderTopWidth, style: style.borderTopStyle };
          }),
      };
    });
    assert.equal(drawn.icons.length, STEPS);
    assert.ok(drawn.connectors.length >= STEPS - LISTS, `too few connectors: ${JSON.stringify(drawn)}`);
    for (const line of drawn.connectors) {
      assert.equal(line.style, "solid", JSON.stringify(drawn));
      assert.ok(Number.parseFloat(line.width) >= 1, `a connector isn't drawn: ${JSON.stringify(drawn)}`);
    }
    for (const color of [...drawn.icons, ...drawn.connectors.map((line) => line.color)]) {
      assert.notEqual(color, drawn.background, JSON.stringify(drawn));
      assert.ok(!/^rgba\(.*, 0\)$|transparent/.test(color), JSON.stringify(drawn));
    }
  },
);
