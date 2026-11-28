// The card (features/ui/components/card.feature; DESIGN.md, The Summary Card
// Rule): an article named by its heading, never a control, drawn flat. Cards
// are found by role and accessible name; only the attributes that name a
// card and make it a control, and computed styles, are read.

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { demoSection } from "../support/gallery.ts";
import { namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

/** The card named `name` in the card's demo. */
function demoCard(world: BrowserWorld, name: string): Locator {
  return demoSection(world.page, "Card").getByRole("article", { name: namePattern(name) });
}

/** That `card` is drawn flat (the Flat Ledger Rule; The Summary Card Rule):
 *  no fill, image, stripe or filter of its own, so the page's color shows
 *  through; a 1px solid edge all round in the token `edge`; each corner the
 *  token `corner`'s radius, where one is given; and no shadow. Each token is
 *  read through a probe, as the page resolves it, with a fallback no token
 *  has, so a token style.css lacks fails here. */
export async function drawnFlat(card: Locator, edge: string, corner?: string): Promise<void> {
  const missing = { color: "rgb(1, 2, 3)", radius: "1234px" };
  const drawn = await card.evaluate(
    (element, { edge, corner, missing }) => {
      const probe = document.createElement("span");
      document.body.append(probe);
      probe.style.color = `var(${edge}, ${missing.color})`;
      probe.style.borderTopLeftRadius = corner === null ? "" : `var(${corner}, ${missing.radius})`;
      const probed = getComputedStyle(probe);
      const expected = { color: probed.color, radius: probed.borderTopLeftRadius };
      probe.remove();
      const style = getComputedStyle(element);
      // A stripe drawn beside the content, in a box of its own.
      const generated = ["::before", "::after"].filter((part) => getComputedStyle(element, part).content !== "none");
      return {
        expected,
        fill: style.backgroundColor,
        image: style.backgroundImage,
        filter: style.filter,
        generated,
        edges: [style.borderTopWidth, style.borderRightWidth, style.borderBottomWidth, style.borderLeftWidth],
        styles: [style.borderTopStyle, style.borderRightStyle, style.borderBottomStyle, style.borderLeftStyle],
        colors: [style.borderTopColor, style.borderRightColor, style.borderBottomColor, style.borderLeftColor],
        corners: [style.borderTopLeftRadius, style.borderTopRightRadius, style.borderBottomRightRadius, style.borderBottomLeftRadius],
        shadow: style.boxShadow,
      };
    },
    { edge, corner: corner ?? null, missing },
  );
  assert.notEqual(drawn.expected.color, missing.color, `${edge} is not defined`);
  assert.equal(drawn.fill, "rgba(0, 0, 0, 0)", "a card has a fill of its own");
  assert.equal(drawn.image, "none", "a card has a background image, such as a stripe");
  assert.deepEqual(drawn.generated, [], "a card draws a part of its own beside its content, such as a stripe");
  assert.deepEqual(drawn.edges, ["1px", "1px", "1px", "1px"], "a card's edge isn't 1px all round");
  assert.deepEqual(drawn.styles, ["solid", "solid", "solid", "solid"], "a card's edge isn't solid");
  const { color, radius } = drawn.expected;
  assert.deepEqual(drawn.colors, [color, color, color, color], `a card's edge isn't in ${edge} all round`);
  if (corner !== undefined) {
    assert.notEqual(radius, missing.radius, `${corner} is not defined`);
    assert.deepEqual(drawn.corners, [radius, radius, radius, radius], `a card's corners aren't ${corner}`);
  }
  assert.equal(drawn.shadow, "none", "a card casts a shadow");
  assert.equal(drawn.filter, "none", "a card is filtered, as by a drop shadow");
}

// The card's role is its element's, and its name its heading's words: the
// article is named by the heading inside it (`aria-labelledby`), at the
// level its caller gave.
Then(
  "the demo's card {string} is an article named by its heading at level {int}",
  async function (this: BrowserWorld, name: string, level: number) {
    const card = demoCard(this, name);
    await expect(card).toHaveCount(1);
    const heading = card.getByRole("heading", { level, name: namePattern(name) });
    await expect(heading).toHaveCount(1);
    const named = await card.evaluate(
      (article, shown) => shown !== null && shown.id !== "" && article.getAttribute("aria-labelledby") === shown.id,
      await heading.elementHandle(),
    );
    assert.ok(named, `the card "${name}" isn't named by its heading`);
  },
);

// A script, as when a part of the card that held focus goes, can hand focus
// to the card's heading: a `tabindex` of -1 lets it, and keeps the heading
// out of Tab's order (the Tab walk checks that order). The steps never move
// focus themselves (scripts/lint-journeys.ts), so the attribute is read.
Then("the heading of the demo's card {string} takes focus from a script", async function (this: BrowserWorld, name: string) {
  const heading = demoCard(this, name).getByRole("heading", { name: namePattern(name) });
  await expect(heading).toHaveCount(1);
  await expect(heading).toHaveAttribute("tabindex", "-1");
});

// The card itself is never a control (The Summary Card Rule): not a link or
// a button, inside none, given no role, handler or Tab stop of one, no place
// in Tab's order, and no pointer over it that would offer a press. A handler
// added by a script leaves no attribute, but a card that took a press would
// show the pointer a control does.
Then("the demo's card {string} is no control and takes no focus", async function (this: BrowserWorld, name: string) {
  const card = demoCard(this, name);
  await expect(card).toHaveCount(1);
  const found = await card.evaluate((article) => {
    const faults: string[] = [];
    const control = article.parentElement?.closest(
      "a[href], button, label, summary, [role='link'], [role='button'], [tabindex]:not([tabindex='-1'])",
    );
    if (control) faults.push(`it stands inside a ${control.tagName.toLowerCase()}`);
    for (const attribute of ["role", "tabindex", "href", "onclick", "contenteditable"]) {
      if (article.hasAttribute(attribute)) faults.push(`it has a ${attribute}`);
    }
    if ((article as HTMLElement).tabIndex >= 0) faults.push("it is a stop for Tab");
    if (getComputedStyle(article).cursor === "pointer") faults.push("it shows a pointer that offers a press");
    return faults;
  });
  assert.deepEqual(found, [], `the card "${name}" is a control`);
});

// A card with no actions has no stop for Tab inside it, nor anything else a
// press or a script's focus would land on, but its heading, which only a
// script focuses.
Then("the demo's card {string} holds nothing that takes focus", async function (this: BrowserWorld, name: string) {
  const card = demoCard(this, name);
  await expect(card).toHaveCount(1);
  await expect(card.getByRole("button")).toHaveCount(0);
  await expect(card.getByRole("link")).toHaveCount(0);
  const stops = await card.evaluate((article) =>
    [...article.querySelectorAll<HTMLElement>("*")]
      .filter((part) => part.tabIndex >= 0 || part.isContentEditable)
      .map((part) => part.outerHTML.slice(0, 80)),
  );
  assert.deepEqual(stops, [], `the card "${name}" holds a stop for Tab`);
});

// The actions are the card's controls, found by role: they stand after the
// body's words, at the card's foot, each inside the card's padding, the
// last ending at the inline end of its content (the right, left to right),
// wrapping onto more lines rather than standing out of it (1.4.10).
Then(
  "the actions of the demo's card {string} stand at its foot and its inline end, each inside it",
  async function (this: BrowserWorld, name: string) {
    const card = demoCard(this, name);
    const actions = card.getByRole("button").or(card.getByRole("link"));
    await expect(actions.first()).toBeVisible();
    const words = card.getByRole("paragraph");
    await expect(words.first()).toBeVisible();
    const placed = await card.evaluate(
      (article, { actions, words }) => {
        const style = getComputedStyle(article);
        const box = article.getBoundingClientRect();
        const px = (value: string) => Number.parseFloat(value) || 0;
        const content = {
          left: box.left + px(style.borderLeftWidth) + px(style.paddingLeft),
          right: box.right - px(style.borderRightWidth) - px(style.paddingRight),
        };
        const boxes = actions.map((action) => (action as Element).getBoundingClientRect());
        const bodyEnd = Math.max(...words.map((paragraph) => (paragraph as Element).getBoundingClientRect().bottom));
        const rtl = style.direction === "rtl";
        return {
          outside: boxes.filter((one) => one.left < content.left - 1 || one.right > content.right + 1).length,
          aboveBody: boxes.filter((one) => one.top < bodyEnd - 1).length,
          endGap: rtl
            ? Math.min(...boxes.map((one) => one.left)) - content.left
            : content.right - Math.max(...boxes.map((one) => one.right)),
        };
      },
      { actions: await actions.elementHandles(), words: await words.elementHandles() },
    );
    assert.equal(placed.outside, 0, `${placed.outside} of the card's actions stand outside its content`);
    assert.equal(placed.aboveBody, 0, `${placed.aboveBody} of the card's actions stand above the end of its body`);
    assert.ok(Math.abs(placed.endGap) <= 1, `the card's actions end ${Math.round(placed.endGap)}px before its inline end`);
  },
);

Then(
  "each card of the {string} demo is drawn on the page's background inside a 1px edge in {string} with the corner {string}, and no shadow",
  async function (this: BrowserWorld, demo: string, edge: string, corner: string) {
    const cards = demoSection(this.page, demo).getByRole("article");
    await expect(cards.first()).toBeVisible();
    for (const card of await cards.all()) {
      await drawnFlat(card, edge, corner);
    }
  },
);
