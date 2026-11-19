// Journey steps for the shared skeleton in the gallery (technical design
// §4.5): the still shape of content on its way. A skeleton is hidden from a
// screen reader and has no role, so it has no name to find it by: it is
// found by its class inside its demo, which is found by its heading, as a
// spinner's drawing is. Its busy region is found by the attribute that makes
// it busy, the words that say its wait by their text, and its control by
// role and name, by keyboard alone.

import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import { composite, contrastRatio } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";
import { demoSection, eachOf } from "../support/gallery.ts";
import { saidJustBeforeBusy } from "./first_load.ts";

// The gallery's skeletons while its region waits (`gallery.rs`): a
// paragraph's lines, a field's block, lines on Ledger Gray and the region's
// lines.
const SKELETONS = 4;
const LINE_SKELETONS = 3;

// Half a pixel of rounding in a laid-out length.
const ROUNDING_PX = 0.5;

const skeletons = (page: Page, component: string): Locator => demoSection(page, component).locator(".skeleton");

/** The demo's one region marked busy while its content is on its way, busy or not. */
async function region(page: Page, component: string): Promise<Locator> {
  const found = demoSection(page, component).locator("[aria-busy]");
  await expect(found).toHaveCount(1);
  return found;
}

const near = (a: number, b: number, slack = ROUNDING_PX) => Math.abs(a - b) <= slack;

// Never a stand-in for words (§4.5): a skeleton is hidden from a screen
// reader, has no role, and holds nothing Tab could stop on.
Then(
  "every skeleton of the {string} demo is hidden from a screen reader, with no role and nothing inside it to focus",
  async function (this: BrowserWorld, component: string) {
    for (const skeleton of await eachOf(skeletons(this.page, component), SKELETONS)) {
      await expect(skeleton).toHaveAttribute("aria-hidden", "true");
      const found = await skeleton.evaluate((element) => ({
        roles: [element, ...element.querySelectorAll("*")].filter((part) => part.hasAttribute("role")).length,
        focusable: [element, ...element.querySelectorAll("*")].filter((part) =>
          part.matches("[tabindex], a[href], button, input, select, textarea, [contenteditable]"),
        ).length,
      }));
      assert.deepEqual(found, { roles: 0, focusable: 0 });
    }
  },
);

// The region the skeleton stands for is busy until its content arrives.
Then("the region of the {string} demo is busy and shows a skeleton", async function (this: BrowserWorld, component: string) {
  const busy = await region(this.page, component);
  await expect(busy).toHaveAttribute("aria-busy", "true");
  await expect(busy.locator(".skeleton")).toHaveCount(1);
  await expect(busy.locator(".skeleton")).toBeVisible();
});

// The page still says the wait in words, visible, just before the busy
// region and outside it, so a screen reader that leaves a busy region unread
// reads them: the skeleton's and the table's.
Then(
  "the {string} demo says {string} just before its busy region, outside it",
  async function (this: BrowserWorld, component: string, words: string) {
    await saidJustBeforeBusy(demoSection(this.page, component), words);
  },
);

const notedHeights = new WeakMap<BrowserWorld, number>();

const heightOf = (locator: Locator) => locator.evaluate((element) => element.getBoundingClientRect().height);

When("the height of the region of the {string} demo is noted", async function (this: BrowserWorld, component: string) {
  notedHeights.set(this, await heightOf(await region(this.page, component)));
});

Then(
  "the region of the {string} demo is no longer busy and shows {string} in place of its skeleton",
  async function (this: BrowserWorld, component: string, text: string) {
    const done = await region(this.page, component);
    await expect(done).not.toHaveAttribute("aria-busy", "true");
    await expect(done.getByText(text)).toBeVisible();
    await expect(done.locator(".skeleton")).toHaveCount(0);
  },
);

// The skeleton takes the room its content will, so the page doesn't jump
// when the content arrives.
Then(
  "the region of the {string} demo is as tall as it was, within {int} pixels",
  async function (this: BrowserWorld, component: string, slack: number) {
    const noted = notedHeights.get(this);
    assert.ok(noted !== undefined, "the region's height was never noted");
    const now = await heightOf(await region(this.page, component));
    assert.ok(near(now, noted, slack), `the region was ${noted}px tall, now ${now}px`);
  },
);

// Still (§4.5, 2.2.2): a skeleton starts by itself beside content the reader
// can use and may stay past 5 s, and nothing on the page could pause it, so
// nothing on it or inside it animates or transitions, whatever the reader's
// motion setting.
Then("no skeleton of the {string} demo moves, fades or changes", async function (this: BrowserWorld, component: string) {
  for (const skeleton of await eachOf(skeletons(this.page, component), SKELETONS)) {
    const found = await skeleton.evaluate((element) => {
      const parts = [element, ...element.querySelectorAll("*")].map((part) => getComputedStyle(part));
      const lasts = (durations: string) => durations.split(",").some((duration) => Number.parseFloat(duration) > 0);
      return {
        animations: element.getAnimations({ subtree: true }).length,
        animated: parts.filter((style) => style.animationName !== "none").length,
        transitions: parts.filter((style) => lasts(style.transitionDuration)).length,
      };
    });
    assert.deepEqual(found, { animations: 0, animated: 0, transitions: 0 });
  }
});

/** The room a skeleton is given: the content box of the element around it. */
const roomAround = (skeleton: Locator) =>
  skeleton.locator("xpath=..").evaluate((around) => {
    const box = around.getBoundingClientRect();
    const style = getComputedStyle(around);
    const left = box.left + Number.parseFloat(style.borderLeftWidth) + Number.parseFloat(style.paddingLeft);
    const right = box.right - Number.parseFloat(style.borderRightWidth) - Number.parseFloat(style.paddingRight);
    return { left, right, width: right - left };
  });

// Lines of text (§4.5): each as tall as the text (1em), on a line of the
// text's height, so the skeleton is as tall as the lines it stands for;
// each fills its room but the last, which is shorter and starts where the
// page's lines start. The demo is found by the id the gallery gives it,
// since a pseudolocale renames its heading, and never across the page, so
// another component's lines can't stand in for its own.
Then(
  "the skeleton lines of the skeleton demo fill their room, each 1 em tall on a line of the text, the last one shorter and at the line's start, in a page that runs {word} to {word}",
  async function (this: BrowserWorld, from: string, to: string) {
    assert.ok(
      (from === "left" && to === "right") || (from === "right" && to === "left"),
      `a page runs left to right or right to left, not ${from} to ${to}`,
    );
    const demo = this.page.locator("section#skeleton-demo");
    await expect(demo).toHaveCount(1);
    const direction = await demo.evaluate((section) => getComputedStyle(section).direction);
    assert.equal(direction, from === "left" ? "ltr" : "rtl", "the page's direction");
    const lines = demo.locator(".skeleton-lines");
    for (const skeleton of await eachOf(lines, LINE_SKELETONS)) {
      const room = await roomAround(skeleton);
      const drawn = await skeleton.evaluate((element) => {
        const style = getComputedStyle(element);
        return {
          height: element.getBoundingClientRect().height,
          font: Number.parseFloat(style.fontSize),
          line: Number.parseFloat(style.lineHeight),
          bars: Array.from(element.children, (bar) => {
            const { left, right, width, height } = bar.getBoundingClientRect();
            return { left, right, width, height };
          }),
        };
      });
      const { bars } = drawn;
      assert.ok(bars.length >= 2, `a skeleton of lines has ${bars.length} line`);
      assert.ok(
        near(drawn.height, bars.length * drawn.line, 1),
        `${bars.length} lines of ${drawn.line}px make a skeleton ${drawn.height}px tall`,
      );
      for (const [index, bar] of bars.entries()) {
        assert.ok(near(bar.height, drawn.font), `line ${index + 1} is ${bar.height}px tall, its text ${drawn.font}px`);
        const atStart: boolean = from === "left" ? near(bar.left, room.left) : near(bar.right, room.right);
        assert.ok(
          atStart,
          `line ${index + 1} starts at ${from === "left" ? bar.left : bar.right}px, its room at ${from === "left" ? room.left : room.right}px`,
        );
      }
      const last = bars.at(-1);
      assert.ok(last !== undefined);
      for (const [index, bar] of bars.slice(0, -1).entries()) {
        assert.ok(near(bar.width, room.width), `line ${index + 1} is ${bar.width}px wide in a room of ${room.width}px`);
      }
      assert.ok(last.width < room.width - 1, `the last line is ${last.width}px wide in a room of ${room.width}px`);
    }
  },
);

// A block (§4.5) is sized in `rem`, so it grows with the reader's text, and
// is capped at the room it is given, so it never pushes the page sideways.
Then(
  "the skeleton block of the {string} demo is {float} rem tall and {float} rem wide, or as wide as its room where that is less",
  async function (this: BrowserWorld, component: string, tall: number, wide: number) {
    const [block] = await eachOf(demoSection(this.page, component).locator(".skeleton-block"), 1);
    assert.ok(block !== undefined);
    const room = (await roomAround(block)).width;
    const drawn = await block.evaluate((element) => {
      const bar = element.firstElementChild?.getBoundingClientRect();
      return {
        rem: Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
        width: bar?.width ?? 0,
        height: bar?.height ?? 0,
      };
    });
    assert.ok(near(drawn.height, tall * drawn.rem), `the block is ${drawn.height}px tall, ${tall} rem ${tall * drawn.rem}px`);
    const expected = Math.min(wide * drawn.rem, room);
    assert.ok(near(drawn.width, expected), `the block is ${drawn.width}px wide, expected ${expected}px (room ${room}px)`);
  },
);

// Look (§4.5): one gray, which shows on the page and on Ledger Gray alike,
// at least 1.4:1 apart from each: a floor for visibility, not a WCAG
// minimum, since a skeleton carries no information. What a skeleton lies
// on is the nearest fill behind it.
Then(
  "every skeleton of the {string} demo is filled in {word}, at least {float}:1 apart from what lies behind it, on the page and on {word}",
  async function (this: BrowserWorld, component: string, fill: string, floor: number, surface: string) {
    await eachOf(skeletons(this.page, component), SKELETONS);
    const drawn = await demoSection(this.page, component).evaluate(
      (section, tokens) => {
        const resolve = (token: string) => {
          const probe = document.createElement("span");
          probe.style.backgroundColor = `var(${token})`;
          document.body.append(probe);
          const color = getComputedStyle(probe).backgroundColor;
          probe.remove();
          return color;
        };
        const behind = (element: Element) => {
          for (let around = element.parentElement; around; around = around.parentElement) {
            const color = getComputedStyle(around).backgroundColor;
            if (color !== "rgba(0, 0, 0, 0)" && color !== "transparent") return color;
          }
          return "";
        };
        return {
          fill: resolve(tokens.fill),
          surface: resolve(tokens.surface),
          skeletons: Array.from(section.querySelectorAll(".skeleton"), (skeleton) => ({
            behind: behind(skeleton),
            fills: Array.from(skeleton.children, (bar) => getComputedStyle(bar).backgroundColor),
          })),
        };
      },
      { fill, surface },
    );
    // An opaque fill: a missing or transparent token would match the bars
    // and pass on the blend over white.
    assert.match(drawn.fill, /^rgb\(/, `${fill} is ${drawn.fill}, not an opaque color`);
    const onSurface = drawn.skeletons.map((skeleton) => skeleton.behind === drawn.surface);
    assert.ok(onSurface.includes(true) && onSurface.includes(false), "the demo has no skeleton on the page and on a surface both");
    for (const skeleton of drawn.skeletons) {
      assert.ok(skeleton.behind !== "", "a skeleton lies on no fill");
      assert.deepEqual(
        skeleton.fills,
        skeleton.fills.map(() => drawn.fill),
        `a skeleton on ${skeleton.behind}`,
      );
      const ratio = contrastRatio(composite([drawn.fill]), composite([skeleton.behind]));
      assert.ok(ratio >= floor, `a skeleton on ${skeleton.behind} is ${ratio.toFixed(2)}:1 apart from it`);
    }
  },
);

// Forced colors (§4.5): the system drops the fill, so a 1px edge in the color
// of unavailable text keeps the shape in view.
Then(
  "every skeleton of the {string} demo is edged {int} pixel(s) wide in GrayText",
  async function (this: BrowserWorld, component: string, width: number) {
    for (const skeleton of await eachOf(skeletons(this.page, component), SKELETONS)) {
      const drawn = await skeleton.evaluate((element) => {
        const probe = document.createElement("span");
        probe.style.setProperty("forced-color-adjust", "none");
        probe.style.color = "GrayText";
        document.body.append(probe);
        const gray = getComputedStyle(probe).color;
        probe.remove();
        return {
          gray,
          edges: Array.from(element.children, (bar) => {
            const style = getComputedStyle(bar);
            return ["top", "right", "bottom", "left"].map((side) => [
              style.getPropertyValue(`border-${side}-width`),
              style.getPropertyValue(`border-${side}-style`),
              style.getPropertyValue(`border-${side}-color`),
            ]);
          }),
        };
      });
      assert.ok(drawn.edges.length > 0, "a skeleton draws nothing");
      for (const edges of drawn.edges) {
        for (const edge of edges) assert.deepEqual(edge, [`${width}px`, "solid", drawn.gray]);
      }
    }
  },
);
