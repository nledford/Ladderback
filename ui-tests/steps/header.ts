// The header bar (features/ui/header.feature): where its parts sit and how
// tall Sign out is, measured from layout. Elements are found by role and
// accessible name; only their boxes are read.

import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { journey, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

// Layout rounds to fractions of a pixel; anything within this is one place.
const ROUNDING_PX = 1;

/** The top and bottom of an element's box, and the bottom of its words, its
 *  text's own boxes: two words in one font size whose bottoms meet stand on
 *  one baseline. Only the text is measured, not the boxes it sits in: a
 *  button's label is a box as tall as its line, half its leading under the
 *  words, which a link's text has none of. */
async function extent(part: Locator): Promise<{ top: number; box: number; words: number }> {
  return part.evaluate((el) => {
    const texts = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    let words = Number.NEGATIVE_INFINITY;
    for (let text = texts.nextNode(); text; text = texts.nextNode()) {
      const range = document.createRange();
      range.selectNodeContents(text);
      for (const line of range.getClientRects()) {
        words = Math.max(words, line.bottom);
      }
    }
    const box = el.getBoundingClientRect();
    return { top: box.top, box: box.bottom, words };
  });
}

// The pages' links and Sign out stand on one baseline, and nothing in the
// header lies under them: an empty line would leave a band below the links,
// and a part on its own baseline would sit off the line. Nor does Sign out
// reach above or below the links' boxes, so the line is as tall as the links
// make it, as with sign-in off, when there is no Sign out.
async function assertLastLineShared(world: BrowserWorld, signOut: [string, Locator]): Promise<void> {
  const banner = world.page.getByRole("banner");
  const links = await banner.getByRole("navigation").getByRole("link").all();
  assert.ok(links.length > 0, "the navigation has no links");
  const parts: [string, Locator][] = [...links.map((link, at): [string, Locator] => [`link ${at + 1}`, link]), signOut];
  const measured = await Promise.all(parts.map(async ([part, locator]) => ({ part, ...(await extent(locator)) })));
  const baseline = measured[0]?.words ?? 0;
  const off = measured.filter(({ words }) => Math.abs(words - baseline) > ROUNDING_PX);
  assert.deepEqual(
    off.map(({ part, words }) => `${part}: ${Math.round(words - baseline)}px off the line`),
    [],
    "the header's last line",
  );
  const linkBoxes = measured.slice(0, links.length);
  const [linksTop, linksBottom] = [Math.min(...linkBoxes.map(({ top }) => top)), Math.max(...linkBoxes.map(({ box }) => box))];
  const taller = measured.filter(({ top, box }) => top < linksTop - ROUNDING_PX || box > linksBottom + ROUNDING_PX);
  assert.deepEqual(
    taller.map(
      ({ part, top, box }) => `${part}: ${Math.round(linksTop - top)}px above, ${Math.round(box - linksBottom)}px below the links`,
    ),
    [],
    "the header's last line is as tall as its links",
  );
  const inner = await banner.evaluate((header) => {
    const style = getComputedStyle(header);
    return (
      header.getBoundingClientRect().bottom - Number.parseFloat(style.paddingBlockEnd) - Number.parseFloat(style.borderBlockEndWidth)
    );
  });
  const lowest = Math.max(...measured.map(({ box }) => box));
  assert.ok(inner - lowest <= ROUNDING_PX, `the header holds ${Math.round(inner - lowest)}px under its last line`);
}

Then("the pages' links and the button {string} share the header's last line", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("banner").getByRole("button", { name: namePattern(name) });
  await assertLastLineShared(this, [`the button "${name}"`, button]);
});

// A pseudolocale words Sign out in letters of its own, so the button is the
// banner's one button, found by its role alone.
Then("the pages' links and the banner's one button share the header's last line", async function (this: BrowserWorld) {
  const button = this.page.getByRole("banner").getByRole("button");
  assert.equal(await button.count(), 1, "the banner has not one button");
  await assertLastLineShared(this, ["the banner's button", button]);
});

// Sign out isn't a page, so it stands outside the navigation landmark, and
// so outside its list of pages too.
Then("the button {string} is in the banner, outside the navigation", async function (this: BrowserWorld, name: string) {
  const banner = this.page.getByRole("banner");
  const button = banner.getByRole("button", { name: namePattern(name) });
  assert.equal(await button.count(), 1, `the banner has no button "${name}"`);
  const pages = banner.getByRole("navigation");
  const inside = await pages.evaluate((nav, held) => held !== null && nav.contains(held), await button.elementHandle());
  assert.equal(inside, false, `the button "${name}" is in the navigation`);
});

// Focus on a control of the page, not on the main landmark itself (which
// start_page.ts's "focus is in the main content" asks for).
Then("focus is on a control in the main content", async function (this: BrowserWorld) {
  const main = journey(this.page).byRole("main");
  const inside = await main.evaluate((landmark) => landmark !== document.activeElement && landmark.contains(document.activeElement));
  assert.ok(inside, "expected focus on a control inside the main content");
});

// The daemon never hears the next sign-out: the request is lost on its way,
// as when the daemon has stopped, and every request after it reaches the
// daemon.
When("the next sign-out reaches no daemon", async function (this: BrowserWorld) {
  let lost = false;
  await this.page.route(
    (url) => url.pathname === "/api/v1/session",
    async (route) => {
      if (!lost && route.request().method() === "DELETE") {
        lost = true;
        await route.abort("connectionrefused");
        return;
      }
      await route.fallback();
    },
  );
});

const notedHeights = new WeakMap<BrowserWorld, number>();

When("the height of the header is noted", async function (this: BrowserWorld) {
  notedHeights.set(this, await this.page.getByRole("banner").evaluate((header) => header.getBoundingClientRect().height));
});

Then("the header keeps the height noted", async function (this: BrowserWorld) {
  const noted = notedHeights.get(this);
  assert.ok(noted !== undefined, "the header's height was never noted");
  const now = await this.page.getByRole("banner").evaluate((header) => header.getBoundingClientRect().height);
  assert.ok(Math.abs(now - noted) <= ROUNDING_PX, `the header was ${Math.round(noted)}px tall, now ${Math.round(now)}px`);
});

// On a narrow screen Sign out wraps under the pages' links and ends its line
// at the header's inline end, the right in English and the left right to
// left, so it never stands alone at the line's start.
Then("the button {string} ends its line, at the header's inline end", async function (this: BrowserWorld, name: string) {
  const banner = this.page.getByRole("banner");
  const button = await banner.getByRole("button", { name: namePattern(name) }).elementHandle();
  assert.ok(button, `the banner has no button "${name}"`);
  const end = await banner.evaluate((header, control) => {
    const style = getComputedStyle(header);
    const box = header.getBoundingClientRect();
    const left = box.left + Number.parseFloat(style.borderLeftWidth) + Number.parseFloat(style.paddingLeft);
    const right = box.right - Number.parseFloat(style.borderRightWidth) - Number.parseFloat(style.paddingRight);
    const drawn = control.getBoundingClientRect();
    return style.direction === "rtl" ? left - drawn.left : drawn.right - right;
  }, button);
  assert.ok(Math.abs(end) <= ROUNDING_PX, `the button "${name}" ends ${Math.round(-end)}px before the header's inline end`);
});

// A button that people press with a finger or a pointer: 2rem at least, so it
// grows with the reader's text size (2.5.8, DESIGN.md's Buttons).
Then("the button {string} is at least 2rem tall, the size of the reader's text", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("banner").getByRole("button", { name: namePattern(name) });
  const { height, rem } = await button.evaluate((control) => ({
    height: control.getBoundingClientRect().height,
    rem: Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
  }));
  assert.ok(height >= 2 * rem - 0.5, `the button "${name}" is ${height.toFixed(1)}px tall, under 2rem (${2 * rem}px)`);
});
