// Journey steps for the shared badge in the gallery (technical design §4.5):
// a count or a tag on a pill of its tone's colors, said in words. A badge has
// no role, so it has no name to find it by: it is found by its class inside
// its demo, which is found by its heading, as a status badge is, and one of
// the demo's badges by its words.

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";
import type { DataTable } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import { composite, contrastRatio, whereFocusIs } from "../support/keyboard.ts";
import { besideIconProblems, wordsBesideIcon } from "../support/parts.ts";
import type { BrowserWorld } from "../support/world.ts";
import { demoSection, eachOf } from "../support/gallery.ts";

// The gallery's badges (`gallery.rs`): a count, a tag and a long tag, then
// one badge named for each of its five tones.
const BADGES = 8;

// Half a pixel of rounding in a laid-out length.
const ROUNDING_PX = 0.5;

// The marks Fluent puts around a value in a message, such as a count.
const ISOLATION_MARKS = /[\u2066-\u2069]/g;

const badges = (page: Page, component: string): Locator => demoSection(page, component).locator(".badge");

const near = (a: number, b: number, slack = ROUNDING_PX) => Math.abs(a - b) <= slack;

// Never color alone (§4.5, 1.4.1): a badge's words say what it counts or
// names, read as plain text, with no role of its own and nothing to focus;
// an icon beside them is decoration, so a screen reader reads the words once.
Then(
  "every badge of the {string} demo says its words with no role and nothing in it to focus, any icon in it a decoration a screen reader skips",
  async function (this: BrowserWorld, component: string) {
    let icons = 0;
    for (const badge of await eachOf(badges(this.page, component), BADGES)) {
      const found = await badge.evaluate((element) => {
        const parts = [element, ...element.querySelectorAll("*")];
        const icon = element.querySelector("svg");
        return {
          words: element.textContent?.trim() ?? "",
          hidden: element.closest("[aria-hidden]")?.getAttribute("aria-hidden") ?? null,
          roles: parts.filter((part) => part.hasAttribute("role")).length,
          live: parts.filter((part) => part.hasAttribute("aria-live")).length,
          focusable: parts.filter((part) => part.matches("[tabindex], a[href], button, input, select, textarea, [contenteditable]"))
            .length,
          icon: icon === null ? null : { hidden: icon.getAttribute("aria-hidden"), focusable: icon.getAttribute("focusable") },
        };
      });
      const words = found.words.replace(ISOLATION_MARKS, "");
      assert.notEqual(words, "", "a badge without words");
      assert.equal(found.hidden, null, `the badge "${words}" is hidden from a screen reader`);
      assert.deepEqual(
        { roles: found.roles, live: found.live, focusable: found.focusable },
        { roles: 0, live: 0, focusable: 0 },
        words,
      );
      if (found.icon !== null) {
        icons += 1;
        assert.deepEqual(found.icon, { hidden: "true", focusable: "false" }, words);
      }
    }
    assert.ok(icons > 0 && icons < BADGES, `the demo shows ${icons} badges with an icon of ${BADGES}: some with one and some without`);
  },
);

// Tab never stops on a badge: from the last stop before its demo, the next
// press lands after the demo, never inside it, and never on the page body.
Then("focus has passed the {string} demo without stopping in it", async function (this: BrowserWorld, component: string) {
  const section = demoSection(this.page, component);
  await expect(section).toHaveCount(1);
  const where = await section.evaluate((element) => {
    const active = document.activeElement;
    return {
      body: active === null || active === document.body,
      inside: active !== null && element.contains(active),
      after: active !== null && (element.compareDocumentPosition(active) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0,
    };
  });
  const focused = await whereFocusIs(this.page);
  assert.ok(!where.body, "focus fell to the page body");
  assert.ok(!where.inside, `focus stopped in the ${component} demo, on ${focused}`);
  assert.ok(where.after, `focus is on ${focused}, before the ${component} demo`);
});

// Each tone is a pair of tokens the contrast gate checks (§4.5): the words in
// the text token on the background token, the icon in the words' color, and
// the pair at 4.5:1 or more as the page draws it. The badge is named for its
// tone in words, so the color never says the tone alone.
Then(
  "the badges of the {string} demo named for their tones are drawn on their pairs of tokens, at least {float}:1 apart:",
  async function (this: BrowserWorld, component: string, floor: number, table: DataTable) {
    const all = await eachOf(badges(this.page, component), BADGES);
    for (const { badge: name, words, fill } of table.hashes() as { badge: string; words: string; fill: string }[]) {
      const named = badges(this.page, component).filter({ hasText: new RegExp(`^\\s*${name}\\s*$`) });
      await expect(named, `the badge "${name}"`).toHaveCount(1);
      const drawn = await named.evaluate(
        (element, tokens) => {
          const resolve = (property: "color" | "backgroundColor", token: string) => {
            const probe = document.createElement("span");
            probe.style[property] = `var(${token})`;
            document.body.append(probe);
            const color = getComputedStyle(probe)[property];
            probe.remove();
            return color;
          };
          const style = getComputedStyle(element);
          const icon = element.querySelector("svg");
          return {
            words: resolve("color", tokens.words),
            fill: resolve("backgroundColor", tokens.fill),
            color: style.color,
            background: style.backgroundColor,
            stroke: icon === null ? null : getComputedStyle(icon).stroke,
          };
        },
        { words, fill },
      );
      // An opaque token: a missing one would leave both sides transparent
      // and alike.
      assert.match(drawn.words, /^rgb\(/, `${words} is ${drawn.words}, not an opaque color`);
      assert.match(drawn.fill, /^rgb\(/, `${fill} is ${drawn.fill}, not an opaque color`);
      assert.equal(drawn.color, drawn.words, `the words of the badge "${name}"`);
      assert.equal(drawn.background, drawn.fill, `the fill of the badge "${name}"`);
      if (drawn.stroke !== null) assert.equal(drawn.stroke, drawn.color, `the icon of the badge "${name}"`);
      const ratio = contrastRatio(composite([drawn.color]), composite([drawn.background]));
      assert.ok(ratio >= floor, `the badge "${name}" is ${ratio.toFixed(2)}:1`);
    }
    assert.ok(table.hashes().length < all.length, "the demo shows badges other than its tones too");
  },
);

// Forced colors (§4.5): the system drops the fill and draws the words in its
// text color, so the words alone tell the tones apart, and a 1px edge in that
// color, drawn inside the badge's box so nothing around it moves, keeps each
// badge apart from the text and the badge beside it.
Then(
  "every badge of the {string} demo draws its words and icon in the color of the text around it, edged {int} pixel(s) wide in that color",
  async function (this: BrowserWorld, component: string, width: number) {
    const around = await demoSection(this.page, component).evaluate((element) => getComputedStyle(element).color);
    for (const badge of await eachOf(badges(this.page, component), BADGES)) {
      const drawn = await badge.evaluate((element) => {
        const style = getComputedStyle(element);
        const icon = element.querySelector("svg");
        return {
          words: element.textContent?.trim() ?? "",
          color: style.color,
          stroke: icon === null ? null : getComputedStyle(icon).stroke,
          edge: [style.outlineWidth, style.outlineStyle, style.outlineColor, style.outlineOffset],
        };
      });
      assert.equal(drawn.color, around, drawn.words);
      if (drawn.stroke !== null) assert.equal(drawn.stroke, around, `the icon of the badge "${drawn.words}"`);
      assert.deepEqual(drawn.edge, [`${width}px`, "solid", around, `-${width}px`], `the edge of the badge "${drawn.words}"`);
    }
  },
);

// The icon comes first in the page's direction (§4.5): left of the words in
// English and right of them in Arabic, centered on the words' line and as
// tall as the text, so it grows with it. The demo is found by the id the
// gallery gives it, since a pseudolocale renames its heading.
Then(
  "the icon of each badge of the badge demo stands before its words, centered on their line and as tall as the text, in a page that runs {word} to {word}",
  async function (this: BrowserWorld, from: string, to: string) {
    assert.ok(
      (from === "left" && to === "right") || (from === "right" && to === "left"),
      `a page runs left to right or right to left, not ${from} to ${to}`,
    );
    const demo = this.page.locator("section#badge-demo");
    await expect(demo).toHaveCount(1);
    const direction = await demo.evaluate((section) => getComputedStyle(section).direction);
    assert.equal(direction, from === "left" ? "ltr" : "rtl", "the page's direction");
    let icons = 0;
    for (const badge of await eachOf(demo.locator(".badge"), BADGES)) {
      const drawn = await badge.evaluate((element) => {
        const icon = element.querySelector("svg");
        if (icon === null) return null;
        // The words start after the icon's box: the icon itself, or the box
        // the badge holds it in.
        let glyph: Element = icon;
        while (glyph.parentElement !== null && glyph.parentElement !== element) glyph = glyph.parentElement;
        const range = document.createRange();
        range.selectNodeContents(element);
        range.setStartAfter(glyph);
        const line = Array.from(range.getClientRects()).find((rect) => rect.width > 0);
        const box = icon.getBoundingClientRect();
        return {
          words: element.textContent?.trim() ?? "",
          font: Number.parseFloat(getComputedStyle(element).fontSize),
          icon: { left: box.left, right: box.right, middle: box.top + box.height / 2, height: box.height },
          line: line === undefined ? null : { left: line.left, right: line.right, middle: line.top + line.height / 2 },
        };
      });
      if (drawn === null) continue;
      icons += 1;
      const { words, icon, line } = drawn;
      assert.ok(line !== null, `the badge "${words}" has no words beside its icon`);
      const before: boolean = from === "left" ? icon.right <= line.left + ROUNDING_PX : icon.left >= line.right - ROUNDING_PX;
      assert.ok(before, `the badge "${words}": its icon spans ${icon.left}–${icon.right}px, its words ${line.left}–${line.right}px`);
      assert.ok(
        near(icon.middle, line.middle, 1),
        `the badge "${words}": its icon's middle is at ${icon.middle}px, its words' at ${line.middle}px`,
      );
      assert.ok(near(icon.height, drawn.font), `the badge "${words}": its icon is ${icon.height}px tall, its text ${drawn.font}px`);
    }
    assert.ok(icons > 0, "the demo has no badge with an icon");
  },
);

// The 320 rule with 200 % text (1.4.10): a badge's words wrap inside its
// pill, so no badge runs past the room its demo gives it.
Then("every badge of the {string} demo keeps within the room it is given", async function (this: BrowserWorld, component: string) {
  const room = await demoSection(this.page, component).evaluate((element) => {
    const { left, right } = element.getBoundingClientRect();
    return { left, right };
  });
  for (const badge of await eachOf(badges(this.page, component), BADGES)) {
    const drawn = await badge.evaluate((element) => {
      const { left, right } = element.getBoundingClientRect();
      return { words: element.textContent?.trim() ?? "", left, right, overflows: element.scrollWidth > element.clientWidth + 1 };
    });
    assert.ok(
      drawn.left >= room.left - ROUNDING_PX && drawn.right <= room.right + ROUNDING_PX,
      `the badge "${drawn.words}" spans ${drawn.left}–${drawn.right}px in a room of ${room.left}–${room.right}px`,
    );
    assert.ok(!drawn.overflows, `the words of the badge "${drawn.words}" run past its pill`);
  }
});

// The words wrap between words beside the icon (#1070): a badge too long for
// its room takes more lines after its icon, which stands at the start of the
// first, never on a line of its own above the words.
Then(
  "the badge {string} of the {string} demo wraps its words beside its icon, which stands at the start of its first line",
  async function (this: BrowserWorld, words: string, component: string) {
    const badge = badges(this.page, component).filter({ hasText: words });
    await expect(badge).toHaveCount(1);
    const drawn = await badge.evaluate(wordsBesideIcon, { icon: "svg", words: "" });
    assert.ok(drawn.lines.length > 1, `the badge "${drawn.text}" takes ${drawn.lines.length} line(s), not more than one`);
    assert.deepEqual(besideIconProblems(drawn), []);
  },
);
