// The machine card (features/ui/components/machine_card.feature; §16,
// decision 175), and what the dashboard's steps share of it: a card is an
// article named by its heading, its facts a description list, its attention
// mark the paragraph that starts with "Needs attention:". Elements are found
// by role and accessible name, and the status badge, which has no role, by
// its words. Only boxes, computed styles, and the order a card's structure
// sets (its terms with their definitions, its first paragraph) are read.

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { demoSection, eachOf } from "../support/gallery.ts";
import { ISOLATION, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";
import { drawnFlat } from "./card.ts";

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

/** The fact a card tells under `term`: the definition after its term. A
 *  card pairs each term with one definition, in one order (the description
 *  list's step checks the pairs), so the term's place among the terms is
 *  its definition's among the definitions. */
export async function factOf(card: Locator, term: string): Promise<Locator> {
  const terms = card.getByRole("term");
  const definitions = card.getByRole("definition");
  await expect(terms.filter({ hasText: namePattern(term) })).toHaveCount(1);
  await expect(definitions).toHaveCount(await terms.count());
  const place = (await terms.allTextContents()).findIndex((words) => namePattern(term).test(words.trim()));
  assert.ok(place >= 0, `the card has no term "${term}"`);
  return definitions.nth(place);
}

/** A card's status badge reading `status`: the paragraph that holds only
 *  its words. The badge has no role (badge.rs), so it is found by them. */
export function statusOf(card: Locator, status: string): Locator {
  return card.getByRole("paragraph").filter({ hasText: namePattern(status) });
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

// Each fact is a term and its definition, read in that order (1.3.1). The
// list is found from its terms; only how they pair with their definitions
// is read from the page's structure. HTML lets a `div` group a term with
// its definitions; then every child is such a group, and each group holds
// its terms, then its definitions, at least one of each.
Then("the article {string} tells its facts in a description list", async function (this: BrowserWorld, name: string) {
  const card = cardIn(demoSection(this.page, "Machine card"), name);
  const terms = card.getByRole("term");
  await expect(terms.first()).toBeVisible();
  const { lists, grouped, groups } = await terms.first().evaluate((term) => {
    const tags = (element: Element) => [...element.children].map((child) => child.tagName.toLowerCase());
    const lists = term.closest("article")?.querySelectorAll("dl").length ?? 0;
    const list = term.closest("dl");
    if (!list) return { lists, grouped: false, groups: [] };
    const children = [...list.children];
    return children.some((child) => child.tagName === "DIV")
      ? {
          lists,
          grouped: true,
          groups: children.map((child) => (child.tagName === "DIV" ? tags(child) : [child.tagName.toLowerCase()])),
        }
      : { lists, grouped: false, groups: [tags(list)] };
  });
  assert.equal(lists, 1, "the card tells its facts in more than one list, or none");
  assert.ok(groups.length >= 1, "the list tells no fact");
  // A group's terms, then its definitions; a list of no groups, one such
  // run after another.
  const pairs = grouped ? /^dt( dt)* dd( dd)*$/ : /^dt( dt)* dd( dd)*( dt( dt)* dd( dd)*)*$/;
  for (const [place, group] of groups.entries()) {
    assert.match(group.join(" "), pairs, `the list's group ${place + 1} isn't terms then definitions: ${group.join(", ") || "empty"}`);
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

// The Flat Ledger Rule, as every shared card is drawn (card.ts), in light
// and dark: no fill or stripe of its own, a 1px edge all round in the
// token, and no shadow.
Then(
  "each card of the demo is drawn on the page's background inside a 1px edge in {string}, with no shadow",
  async function (this: BrowserWorld, token: string) {
    for (const card of await eachOf(demoCards(this), 3)) {
      await drawnFlat(card, token);
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
