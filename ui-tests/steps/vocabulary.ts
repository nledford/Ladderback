// Journey steps for the shared vocabulary of tones, sizes and placements in
// the gallery (technical design §4.5): each tone in words and in its color,
// each status pair, and each placement in its line.

import assert from "node:assert/strict";

import { type DataTable, Then } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import type { BrowserWorld } from "../support/world.ts";
import { demoSection, eachOf, drawnOnItsFill, wholly } from "../support/gallery.ts";

/**
 * The items of one of the vocabulary demo's lists, found by the list's name,
 * which its heading gives it through aria-labelledby.
 */
function vocabularyItems(page: Page, component: string, list: string): Locator {
  return demoSection(page, component).getByRole("list", { name: list, exact: true }).getByRole("listitem");
}

// A tone (technical design §4.5, the shared vocabulary) is said in words, so
// its color never carries the meaning alone (1.4.1). Its swatch is
// decoration beside the words, so a screen reader reads each name once, and
// it is stroked in the words' color, so the contrast gate's tone pairs cover
// it as well as the text.
Then(
  "each of the {int} tones of the {string} demo is named in words, its swatch drawn in its text's color and skipped by a screen reader",
  async function (this: BrowserWorld, count: number, component: string) {
    const items = await eachOf(vocabularyItems(this.page, component, "Tones"), count);
    const tones = await Promise.all(
      items.map((locator) =>
        locator.evaluate((item) => {
          const swatch = item.querySelector("svg");
          return {
            text: item.textContent?.trim() ?? "",
            hidden: swatch?.getAttribute("aria-hidden") ?? null,
            focusable: swatch?.getAttribute("focusable") ?? null,
            stroke: swatch === null ? "" : getComputedStyle(swatch).stroke,
            color: getComputedStyle(item).color,
          };
        }),
      ),
    );
    for (const tone of tones) {
      assert.notEqual(tone.text, "", "a tone without its name");
      assert.equal(tone.hidden, "true", tone.text);
      assert.equal(tone.focusable, "false", tone.text);
      assert.equal(tone.stroke, tone.color, tone.text);
    }
  },
);

// Every tone but Inherit draws in a token of its own: Primary the accent,
// Secondary the muted text, and the three status hues. None is the list's own
// text color either, so a tone whose rule is lost, and which falls back to
// the text around it, fails here.
Then(
  "the {int} tones of the {string} demo other than Inherit each have a color of their own",
  async function (this: BrowserWorld, count: number, component: string) {
    const list = demoSection(this.page, component).getByRole("list", { name: "Tones", exact: true });
    const items = list.getByRole("listitem").filter({
      hasNot: this.page.getByText("Inherit", { exact: true }),
    });
    const colors = await Promise.all(
      (await eachOf(items, count)).map((item) => item.evaluate((element) => getComputedStyle(element).color)),
    );
    const around = await list.evaluate((element) => getComputedStyle(element).color);
    assert.equal(new Set([...colors, around]).size, count + 1, `the tones' colors: ${colors.join(", ")}; the text's: ${around}`);
  },
);

// Under forced colors (1.4.11, §4.5) every tone is the system's text color,
// as the text around it is, so the tones' words, not their colors, tell them
// apart, and no author color is left where the reader chose her own.
Then(
  "the {int} tones of the {string} demo all draw in the color of the text around them",
  async function (this: BrowserWorld, count: number, component: string) {
    const list = demoSection(this.page, component).getByRole("list", { name: "Tones", exact: true });
    const around = await list.evaluate((element) => getComputedStyle(element).color);
    for (const item of await eachOf(list.getByRole("listitem"), count)) {
      const tone = await item.evaluate((element) => ({
        text: element.textContent?.trim() ?? "",
        color: getComputedStyle(element).color,
      }));
      assert.equal(tone.color, around, tone.text);
    }
  },
);

// Inherit is the color of the text around a part, as a spinner inside a
// button takes the button's: the step colors the list around the tone, and
// gives it a tone of its own as an outer part would, and reads the tone's
// color, which must follow the text rather than name a token or take the
// outer part's tone.
Then(
  "the tone {string} of the {string} demo takes the color of the text around it",
  async function (this: BrowserWorld, tone: string, component: string) {
    const list = demoSection(this.page, component).getByRole("list", { name: "Tones", exact: true });
    const item = list.getByRole("listitem").filter({ hasText: tone });
    await expect(item).toHaveCount(1);
    for (const around of ["rgb(1, 2, 3)", "rgb(200, 100, 50)"]) {
      await list.evaluate((element, color) => {
        (element as HTMLElement).style.color = color;
        (element as HTMLElement).style.setProperty("--tone", "rgb(9, 9, 9)");
      }, around);
      assert.equal(await item.evaluate((element) => getComputedStyle(element).color), around, tone);
    }
    await list.evaluate((element) => {
      (element as HTMLElement).style.color = "";
      (element as HTMLElement).style.removeProperty("--tone");
    });
  },
);

// The vocabulary demo's status pairs, each named in words drawn in its pair,
// so the axe row reads each pair's words on its fill too.
Then(
  "each status pair of the {string} demo is named in words, drawn on its fill:",
  async function (this: BrowserWorld, component: string, pairs: DataTable) {
    const list = demoSection(this.page, component).getByRole("list", { name: "Status pairs", exact: true });
    const rows = pairs.hashes() as { pair: string; words: string; fill: string }[];
    await expect(list.getByRole("listitem")).toHaveCount(rows.length);
    for (const { pair, words, fill } of rows) {
      await drawnOnItsFill(list.getByRole("listitem").filter({ hasText: wholly(pair) }), pair, words, fill);
    }
  },
);

// A placement (technical design §4.5) is in logical terms: Start is the
// line's start in the page's direction, the left in English and the right in
// Arabic. Each row of the demo's last list is a part placed in its line (the
// list's content box), its swatch and its name; Inline keeps the part in the
// line of text, as wide as what it holds. The vocabulary demo is the first
// demo that holds lists, and the placements are its last; both are found by
// order, since a pseudolocale changes the names, and the demo first so that a
// later demo's list (the Steps demo's) isn't taken for the placements. A pixel
// of rounding is allowed.
Then(
  /^the placements of the vocabulary demo put each part where they say, in a page that runs (left to right|right to left)$/,
  async function (this: BrowserWorld, direction: string) {
    const vocabulary = this.page
      .getByRole("main")
      .locator("section")
      .filter({ has: this.page.getByRole("list") })
      .first();
    const placements = vocabulary.getByRole("list").last();
    const items = placements.getByRole("listitem");
    await expect(items).toHaveCount(4);
    const rows = await placements.evaluate((list) => {
      const frame = list.getBoundingClientRect();
      const style = getComputedStyle(list);
      const inset = (side: "left" | "right") =>
        Number.parseFloat(style.getPropertyValue(`border-${side}-width`)) +
        Number.parseFloat(style.getPropertyValue(`padding-${side}`));
      return {
        direction: style.direction,
        line: { left: frame.left + inset("left"), right: frame.right - inset("right") },
        parts: Array.from(list.children, (item) => {
          const box = item.getBoundingClientRect();
          const held = Array.from(item.children, (child) => child.getBoundingClientRect());
          return {
            box: { left: box.left, right: box.right },
            left: Math.min(...held.map((rect) => rect.left)),
            right: Math.max(...held.map((rect) => rect.right)),
          };
        }),
      };
    });
    assert.equal(rows.direction, direction === "right to left" ? "rtl" : "ltr", "the page's direction");
    const [inline, start, center, end] = rows.parts;
    assert.ok(inline && start && center && end, "four placements");
    // The gaps between a part and its line's two sides, in reading order.
    const gaps = (part: { left: number; right: number }) => {
      const [left, right] = [part.left - rows.line.left, rows.line.right - part.right];
      return direction === "right to left" ? { before: right, after: left } : { before: left, after: right };
    };
    assert.ok(Math.abs(inline.box.right - inline.box.left - (inline.right - inline.left)) < 1, "Inline is as wide as its part");
    assert.ok(gaps(inline.box).after > 1, "Inline leaves the rest of its line");
    assert.ok(gaps(start).before < 1 && gaps(start).after > 1, `Start: ${JSON.stringify(gaps(start))}`);
    assert.ok(
      Math.abs(gaps(center).before - gaps(center).after) < 1 && gaps(center).before > 1,
      `Center: ${JSON.stringify(gaps(center))}`,
    );
    assert.ok(gaps(end).after < 1 && gaps(end).before > 1, `End: ${JSON.stringify(gaps(end))}`);
  },
);
