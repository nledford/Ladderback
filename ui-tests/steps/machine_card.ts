// The machine card (features/ui/components/machine_card.feature; §16,
// decision 175), and what the dashboard's steps share of it: a card is an
// article named by its heading, its facts a description list, its attention
// mark the paragraph that starts with "Needs attention:". Elements are found
// by role and accessible name; only boxes and computed styles are read.

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { demoSection, eachOf } from "../support/gallery.ts";
import { ISOLATION, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

// Layout rounds to fractions of a pixel; anything within this is one place.
const ROUNDING_PX = 1;

/** What the attention mark starts with, before the reason's own words. */
const MARK = "Needs attention:";

/** The words of `text` as a pattern, isolation marks aside, unanchored. */
function wordsOf(text: string): string {
  return [...text].map((ch) => ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(ISOLATION);
}

/** The card named `name` inside `within`. */
export function cardIn(within: Locator, name: string): Locator {
  return within.getByRole("article", { name: namePattern(name) });
}

/** The fact a card tells under `term`: the description after its term. */
export function factOf(card: Locator, term: string): Locator {
  return card.locator("dt", { hasText: namePattern(term) }).locator("xpath=following-sibling::dd[1]");
}

/** A card's status: the one status badge it shows, after its connection's
 *  line, as the badge's steps find one (badge.ts). */
export function statusOf(card: Locator): Locator {
  return card.locator(".badge");
}

/** A card's attention mark: its paragraph that starts with the mark's words. */
export function attentionMark(card: Locator): Locator {
  return card.getByRole("paragraph").filter({ hasText: new RegExp(`^${ISOLATION}${wordsOf(MARK)}`) });
}

/** That the card is marked as needing attention, the mark first after its
 *  heading and its words starting `words`. */
export async function markedWith(card: Locator, words: string): Promise<void> {
  const mark = attentionMark(card);
  await expect(mark).toHaveCount(1);
  await expect(mark).toHaveText(new RegExp(`^${ISOLATION}${wordsOf(MARK)}\\s*${ISOLATION}${wordsOf(words)}`));
  const first = await card.evaluate(
    (article, shown) => {
      const after = article.querySelector("h2, h3")?.nextElementSibling ?? null;
      return after !== null && after === shown;
    },
    await mark.elementHandle(),
  );
  assert.ok(first, "the attention mark doesn't come first after the card's heading");
}

/** That `cards` stand one under another, each starting where the first does
 *  and none past the viewport's inline end. */
export async function standInOneColumn(cards: Locator): Promise<void> {
  await expect(cards.first()).toBeVisible();
  const boxes = await cards.evaluateAll((all) =>
    all.map((card) => {
      const box = card.getBoundingClientRect();
      return { left: box.left, right: box.right, top: box.top, bottom: box.bottom };
    }),
  );
  const viewport = await cards.first().evaluate(() => document.documentElement.clientWidth);
  const [first] = boxes;
  assert.ok(first, "no card shows");
  for (const [place, box] of boxes.entries()) {
    assert.ok(
      Math.abs(box.left - first.left) <= ROUNDING_PX,
      `card ${place + 1} starts ${Math.round(box.left - first.left)}px from the first`,
    );
    assert.ok(box.left >= -ROUNDING_PX && box.right <= viewport + ROUNDING_PX, `card ${place + 1} stands outside the page`);
    const before = boxes[place - 1];
    if (before) assert.ok(box.top >= before.bottom - ROUNDING_PX, `card ${place + 1} isn't under card ${place}`);
  }
}

/** That the cards `first` and `second` stand on one line, side by side. */
export async function standSideBySide(first: Locator, second: Locator): Promise<void> {
  await expect(first).toBeVisible();
  await expect(second).toBeVisible();
  const [one, two] = await Promise.all([first.boundingBox(), second.boundingBox()]);
  assert.ok(one && two, "a card has no box");
  assert.ok(Math.abs(one.y - two.y) <= ROUNDING_PX, `the cards start ${Math.round(two.y - one.y)}px apart in height`);
  assert.ok(two.x >= one.x + one.width - ROUNDING_PX || one.x >= two.x + two.width - ROUNDING_PX, "the cards overlap");
}

function demoCards(world: BrowserWorld): Locator {
  return demoSection(world.page, "Machine card").getByRole("article");
}

Then(
  "the article {string} holds the heading {string} at level {int}",
  async function (this: BrowserWorld, name: string, heading: string, level: number) {
    const card = cardIn(demoSection(this.page, "Machine card"), name);
    await expect(card).toHaveCount(1);
    await expect(card.getByRole("heading", { level, name: namePattern(heading) })).toHaveCount(1);
  },
);

// Each fact is a term and its description, read in that order (1.3.1).
Then("the article {string} tells its facts in a description list", async function (this: BrowserWorld, name: string) {
  const card = cardIn(demoSection(this.page, "Machine card"), name);
  const list = card.locator("dl");
  await expect(list).toHaveCount(1);
  const pairs = await list.evaluate((dl) => [...dl.children].map((child) => child.tagName.toLowerCase()));
  assert.ok(pairs.length >= 2, "the list tells no fact");
  for (const [place, tag] of pairs.entries()) {
    // A `div` may group a term with its description, as HTML allows.
    if (tag === "div") continue;
    assert.equal(tag, place % 2 === 0 ? "dt" : "dd", `the list's child ${place + 1} is a ${tag}`);
  }
});

Then("the article {string} starts with the words {string}", async function (this: BrowserWorld, name: string, words: string) {
  const card = cardIn(demoSection(this.page, "Machine card"), name);
  assert.ok(words.startsWith(MARK), `the words "${words}" aren't an attention mark's`);
  await markedWith(card, words.slice(MARK.length).trim());
});

// The icon is decoration beside the words, which say it all (1.1.1).
Then(
  "the attention mark of the article {string} has an icon hidden from a screen reader",
  async function (this: BrowserWorld, name: string) {
    const mark = attentionMark(cardIn(demoSection(this.page, "Machine card"), name));
    await expect(mark).toHaveCount(1);
    const icons = await mark.evaluate((paragraph) =>
      [...paragraph.querySelectorAll("svg")].map((svg) => svg.closest("[aria-hidden='true']") !== null),
    );
    assert.deepEqual(icons, [true], "the mark holds no icon, more than one, or one a screen reader hears");
  },
);

Then("the article {string} has no attention mark", async function (this: BrowserWorld, name: string) {
  const card = cardIn(demoSection(this.page, "Machine card"), name);
  await expect(card).toHaveCount(1);
  await expect(attentionMark(card)).toHaveCount(0);
});

// The Flat Ledger Rule: no fill of the card's own, a 1px edge all round in
// its token, and no shadow, in light and dark. The token is read through a
// probe, as the page resolves it.
Then(
  "each card of the demo is drawn on the page's background inside a 1px edge in {string}, with no shadow",
  async function (this: BrowserWorld, token: string) {
    for (const card of await eachOf(demoCards(this), 3)) {
      const drawn = await card.evaluate((article, edge) => {
        const probe = document.createElement("span");
        document.body.append(probe);
        probe.style.color = `var(${edge}, rgb(1, 2, 3))`;
        const expected = getComputedStyle(probe).color;
        probe.remove();
        const style = getComputedStyle(article);
        return {
          expected,
          fill: style.backgroundColor,
          edges: [style.borderTopWidth, style.borderRightWidth, style.borderBottomWidth, style.borderLeftWidth],
          styles: [style.borderTopStyle, style.borderRightStyle, style.borderBottomStyle, style.borderLeftStyle],
          colors: [style.borderTopColor, style.borderRightColor, style.borderBottomColor, style.borderLeftColor],
          shadow: style.boxShadow,
        };
      }, token);
      assert.equal(drawn.fill, "rgba(0, 0, 0, 0)", "a card has a fill of its own");
      assert.deepEqual(drawn.edges, ["1px", "1px", "1px", "1px"], "a card's edge isn't 1px all round");
      assert.deepEqual(drawn.styles, ["solid", "solid", "solid", "solid"], "a card's edge isn't solid");
      assert.deepEqual(
        drawn.colors,
        [drawn.expected, drawn.expected, drawn.expected, drawn.expected],
        `a card's edge isn't in ${token} all round`,
      );
      assert.equal(drawn.shadow, "none", "a card casts a shadow");
    }
  },
);

Then("the demo's cards stand in one column, each inside the page", async function (this: BrowserWorld) {
  await standInOneColumn(demoCards(this));
});

Then("the demo's cards stand side by side", async function (this: BrowserWorld) {
  const cards = demoCards(this);
  await standSideBySide(cards.nth(0), cards.nth(1));
});
