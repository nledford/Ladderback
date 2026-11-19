// Journey steps for the shared table (technical design §4.5): the order of
// its rows and what its headers say of it, a row's action keeping its focus,
// its numbers' alignment, what shows in its place, and its frame, which
// scrolls sideways on a small screen with each row's header pinned at its
// start. Keyboard only, role-and-name locators, as everywhere in steps/.
//
// The table is named by its caption, and its frame, a region, by the same
// caption through aria-labelledby.

import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import { expect } from "playwright/test";
import type { Locator, Page } from "playwright";

import { journey, namePattern, scrollKey } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

// More presses than a frame needs to scroll from one end to the other.
const MAX_SCROLL_PRESSES = 40;

function table(page: Page, name: string): Locator {
  return page.getByRole("table", { name, exact: true });
}

/** A column header's own words: its first line, before its sort button's. */
function headerWords(text: string): string {
  const [words] = text.split("\n", 1);
  assert.ok(words !== undefined, `a column header reads nothing: ${JSON.stringify(text)}`);
  return words.trim();
}

/** The cell of `row` (its row header's words) in the column headed `column`. */
async function cellOf(page: Page, name: string, row: string, column: string): Promise<Locator> {
  const found = table(page, name);
  const headers = (await found.locator("thead th").allInnerTexts()).map(headerWords);
  const index = headers.indexOf(column);
  assert.ok(index >= 0, `the table "${name}" has no column "${column}": ${headers.join(", ")}`);
  const line = found.locator("tr", { has: page.getByRole("rowheader", { name: row, exact: true }) });
  return line.locator("th, td").nth(index);
}

/** Each cell of the column headed `column`, its header's and footer's too. */
async function columnOf(page: Page, name: string, column: string): Promise<Locator[]> {
  const found = table(page, name);
  const headers = (await found.locator("thead th").allInnerTexts()).map(headerWords);
  const index = headers.indexOf(column);
  assert.ok(index >= 0, `the table "${name}" has no column "${column}": ${headers.join(", ")}`);
  const rows = found.locator("tr");
  await expect(rows.first()).toBeVisible();
  const cells: Locator[] = [];
  for (const row of await rows.all()) {
    cells.push(row.locator(":scope > th, :scope > td").nth(index));
  }
  return cells;
}

Then("the rows of the table {string} are {string}", async function (this: BrowserWorld, name: string, rows: string) {
  await expect(table(this.page, name).locator("tbody th")).toHaveText(rows.split(", "));
});

Then("no column header of the table {string} says it is sorted", async function (this: BrowserWorld, name: string) {
  await expect(table(this.page, name).locator("thead th[aria-sort]")).toHaveCount(0);
});

// One sorted column at a time (the WAI-ARIA table pattern): only its header
// carries aria-sort.
Then(
  "the column header {string} alone of the table {string} says it is sorted {string}",
  async function (this: BrowserWorld, header: string, name: string, direction: string) {
    const sorted = table(this.page, name).locator("thead th[aria-sort]");
    await expect(sorted).toHaveCount(1);
    await expect(sorted).toHaveAttribute("aria-sort", direction);
    expect(headerWords(await sorted.innerText())).toBe(header);
  },
);

Then(
  "the row {string} of the table {string} reads {string} in the column {string}",
  async function (this: BrowserWorld, row: string, name: string, text: string, column: string) {
    await expect(await cellOf(this.page, name, row, column)).toHaveText(text);
  },
);

// A row that moved is moved, not built again, so its control is the one the
// reader was on; focus goes back to it once the move is made (WCAG 2.4.3).
Then("focus is kept on the button {string}", async function (this: BrowserWorld, name: string) {
  const button = journey(this.page).byRole("button", { name: namePattern(name) });
  await expect(button).toHaveCount(1);
  await expect(button).toBeFocused();
});

// Numbers line up by their last digit: every cell's text ends at the cell's
// end, and its figures are all one width (`tabular-nums`).
Then(
  "the column {string} of the table {string} is aligned to its end in figures of one width",
  async function (this: BrowserWorld, column: string, name: string) {
    for (const cell of await columnOf(this.page, name, column)) {
      const found = await cell.evaluate((el) => {
        const style = getComputedStyle(el);
        const range = document.createRange();
        range.selectNodeContents(el);
        const text = [...range.getClientRects()].filter((rect) => rect.width > 0);
        const box = el.getBoundingClientRect();
        const end = box.right - parseFloat(style.paddingRight) - parseFloat(style.borderRightWidth);
        return {
          text: (el.textContent ?? "").trim(),
          align: style.textAlign,
          figures: style.fontVariantNumeric,
          gap: text.length === 0 ? 0 : end - Math.max(...text.map((rect) => rect.right)),
        };
      });
      assert.ok(["end", "right"].includes(found.align), `"${found.text}" is aligned ${found.align}`);
      assert.match(found.figures, /tabular-nums/, `"${found.text}" is in figures of ${found.figures}`);
      assert.ok(Math.abs(found.gap) <= 1, `"${found.text}" ends ${found.gap.toFixed(1)}px before its cell's end`);
    }
  },
);

Then(
  "the column {string} of the table {string} is aligned to its start",
  async function (this: BrowserWorld, column: string, name: string) {
    for (const cell of await columnOf(this.page, name, column)) {
      const align = await cell.evaluate((el) => getComputedStyle(el).textAlign);
      assert.ok(["start", "left"].includes(align), `a cell of "${column}" is aligned ${align}`);
    }
  },
);

// What shows in the table's place: the sentence, and no table of that name.
Then(
  "the {string} demo says {string} in the place of the table {string}",
  async function (this: BrowserWorld, component: string, sentence: string, name: string) {
    const section = this.page.locator("section", {
      has: this.page.getByRole("heading", { level: 2, name: component, exact: true }),
    });
    await expect(section.getByText(sentence, { exact: true })).toBeVisible();
    await expect(table(this.page, name)).toHaveCount(0);
  },
);

// The frame is a focusable region, so the arrow keys scroll it (Option and
// the arrow in WebKit on macOS: `scrollKey`); its end is the right in a
// left-to-right page and the left in a right-to-left one, where
// `scrollLeft` counts down from 0. Each press waits for its scroll to end,
// as a reader's next press does: WebKit animates a key's scroll, and a press
// during the animation cuts it short (12 presses without a wait moved the
// gallery's frame 187 px of its 838; with 100 ms between them, all of it).
async function scrollSettles(frame: Locator): Promise<void> {
  await frame.evaluate(async (el) => {
    let last = el.scrollLeft;
    let still = 0;
    for (let frames = 0; still < 6 && frames < 120; frames += 1) {
      await new Promise(requestAnimationFrame);
      still = el.scrollLeft === last ? still + 1 : 0;
      last = el.scrollLeft;
    }
  });
}

When(
  "I press {word} until the region {string} has scrolled to its end",
  async function (this: BrowserWorld, key: string, name: string) {
    const frame = journey(this.page).byRole("region", { name: namePattern(name) });
    const press = scrollKey(this.page, key);
    await expect(frame).toBeFocused();
    const atEnd = () => frame.evaluate((el) => el.scrollWidth - el.clientWidth - Math.abs(el.scrollLeft) < 1);
    const scrolls = await frame.evaluate((el) => el.scrollWidth > el.clientWidth + 1);
    assert.ok(scrolls, `the region "${name}" has nothing to scroll`);
    for (let presses = 0; presses < MAX_SCROLL_PRESSES && !(await atEnd()); presses += 1) {
      await this.page.keyboard.press(press);
      await scrollSettles(frame);
    }
    await expect.poll(atEnd, { message: `the region "${name}" didn't scroll to its end by ${press}` }).toBe(true);
  },
);

// A row header is pinned at the frame's start (`position: sticky`), so a
// reader scrolled sideways still sees whose row it is.
Then(
  "each row header of the table {string} stands whole at the start of its frame",
  async function (this: BrowserWorld, name: string) {
    const headers = table(this.page, name).locator("tbody th");
    await expect(headers.first()).toBeVisible();
    for (const header of await headers.all()) {
      const found = await header.evaluate((el) => {
        const frame = el.closest('[role="region"]');
        if (frame === null) return { problem: "the row header is in no frame" };
        const box = el.getBoundingClientRect();
        const room = frame.getBoundingClientRect();
        const rtl = getComputedStyle(frame).direction === "rtl";
        const start = rtl ? room.right - frame.clientLeft : room.left + frame.clientLeft;
        const at = rtl ? box.right : box.left;
        return {
          problem:
            Math.abs(at - start) > 1 || box.left < room.left - 0.5 || box.right > room.right + 0.5
              ? `"${el.textContent}" stands at ${Math.round(box.left)}..${Math.round(box.right)}px in a frame at ${Math.round(room.left)}..${Math.round(room.right)}px`
              : null,
        };
      });
      assert.equal(found.problem, null, found.problem ?? "");
    }
  },
);

// What the cells scrolled under a pinned row header would show through: the
// header's fill is the page's own background, the system's in forced colors.
Then(
  "each row header of the table {string} is filled with the page's background color",
  async function (this: BrowserWorld, name: string) {
    const headers = table(this.page, name).locator("tbody th");
    await expect(headers.first()).toBeVisible();
    for (const header of await headers.all()) {
      const [fill, page] = await header.evaluate((el) => [
        getComputedStyle(el).backgroundColor,
        getComputedStyle(document.body).backgroundColor,
      ]);
      assert.notEqual(fill, "rgba(0, 0, 0, 0)", "a row header has no fill");
      assert.equal(fill, page, "a row header's fill is not the page's background");
    }
  },
);
