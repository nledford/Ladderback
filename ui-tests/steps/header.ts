// The header bar (features/ui/header.feature): where its parts sit and how
// tall Sign out is, measured from layout. Elements are found by role and
// accessible name; only their boxes are read.

import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";
import { expect } from "playwright/test";

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

// The pages' links, the version and Sign out stand on one baseline, and
// nothing in the header lies under them: an empty line would leave a band
// below the links, and a part on its own baseline would sit off the line.
// Nor does the version or Sign out reach above or below the links' boxes, so
// the line is as tall as the links make it, as with sign-in off, when there
// is no Sign out.
Then(
  "the pages' links, the running version and the button {string} share the header's last line",
  async function (this: BrowserWorld, name: string) {
    const banner = this.page.getByRole("banner");
    const links = await banner.getByRole("navigation").getByRole("link").all();
    assert.ok(links.length > 0, "the navigation has no links");
    const parts: [string, Locator][] = [
      ...links.map((link, at): [string, Locator] => [`link ${at + 1}`, link]),
      ["the running version", banner.getByText(/^Version\s/)],
      [`the button "${name}"`, banner.getByRole("button", { name: namePattern(name) })],
    ];
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
  },
);

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

// The running version's width depends on the commit the build names, whose
// digits differ in width, so a layout measured with this build's own commit
// would pass or fail by the commit (#908). Of the versions a build between
// releases shows, the widest is the widest digit throughout, with changes
// (`0.3.1+4444444.dirty` in Chromium on macOS); the narrowest, the narrowest
// digit throughout, without. The digits are measured in the header's own
// font, which differs by browser and system. From now on, every answer naming
// the daemon's build names that one in place of its own.
const shownCommits = new WeakMap<BrowserWorld, RegExp>();

When("the running version is the {word} one a build between releases can show", async function (this: BrowserWorld, width: string) {
  assert.ok(width === "widest" || width === "narrowest", `no version is the "${width}" one`);
  const widest = width === "widest";
  const digits = await this.page.getByRole("banner").evaluate((header) => {
    const probe = document.createElement("span");
    probe.style.cssText = "position: absolute; visibility: hidden; white-space: nowrap";
    header.append(probe);
    const range = document.createRange();
    const measured = [..."0123456789abcdef"].map((digit) => {
      probe.textContent = digit.repeat(10);
      range.selectNodeContents(probe);
      return { digit, width: range.getBoundingClientRect().width };
    });
    probe.remove();
    return measured;
  });
  const { digit } = digits.reduce((best, next) => ((widest ? next.width > best.width : next.width < best.width) ? next : best));
  const build = { commit: digit.repeat(40), dirty: widest, release: false };
  await this.page.route(
    (url) => url.pathname === "/api/v1/session",
    async (route) => {
      const answer = await route.fetch();
      const body: unknown = await answer.json().catch(() => undefined);
      if (typeof body === "object" && body !== null && typeof (body as Record<string, unknown>)["version"] === "string") {
        await route.fulfill({ response: answer, json: { ...body, ...build } });
      } else {
        await route.fulfill({ response: answer });
      }
    },
  );
  shownCommits.set(this, new RegExp(`\\+${digit}+${widest ? "\\.dirty" : "(?!\\.dirty)"}\\b`));
});

// On a narrow screen the version and Sign out wrap under the pages' links.
// Sign out ends its line at the header's inline end. Where the version's
// words, the 1rem between the header's parts and the button fit in the
// header's line, the version stands on Sign out's line; where they don't,
// each line of its words ends at the inline end too.
Then(
  "the running version and the button {string} share a line where they fit, at the header's inline end",
  async function (this: BrowserWorld, name: string) {
    const banner = this.page.getByRole("banner");
    const shownCommit = shownCommits.get(this);
    if (shownCommit) await expect(banner.getByText(/^Version\s/)).toHaveText(shownCommit);
    const version = await banner.getByText(/^Version\s/).elementHandle();
    const button = await banner.getByRole("button", { name: namePattern(name) }).elementHandle();
    assert.ok(version && button, `the banner shows no running version or no button "${name}"`);
    const layout = await banner.evaluate(
      (header, [shown, control]) => {
        const style = getComputedStyle(header);
        const box = header.getBoundingClientRect();
        const left = box.left + Number.parseFloat(style.borderLeftWidth) + Number.parseFloat(style.paddingLeft);
        const right = box.right - Number.parseFloat(style.borderRightWidth) - Number.parseFloat(style.paddingRight);
        const rtl = style.direction === "rtl";
        const end = (rect: DOMRect) => (rtl ? left - rect.left : rect.right - right);
        // Only the text's own boxes, as in `extent`: a range over an
        // element's contents gives the boxes of the elements in it too, and
        // a button's label is a box as tall as its line, half its leading
        // under the words.
        const words = (el: Element) => {
          let [start, stop, bottom] = [Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY, Number.NEGATIVE_INFINITY];
          // The end of each line of the words, by the line's bottom.
          const ends = new Map<number, number>();
          const texts = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
          for (let text = texts.nextNode(); text; text = texts.nextNode()) {
            const range = document.createRange();
            range.selectNodeContents(text);
            for (const rect of range.getClientRects()) {
              if (rect.width > 0) {
                const line = Math.round(rect.bottom);
                ends.set(line, Math.max(ends.get(line) ?? Number.NEGATIVE_INFINITY, end(rect)));
                [start, stop, bottom] = [Math.min(start, rect.left), Math.max(stop, rect.right), Math.max(bottom, rect.bottom)];
              }
            }
          }
          return { width: stop - start, bottom, ends: [...ends.values()] };
        };
        const [versionWords, buttonWords] = [words(shown), words(control)];
        const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
        const buttonBox = control.getBoundingClientRect();
        return {
          fits: versionWords.ends.length === 1 && versionWords.width + rem + buttonBox.width <= right - left - 1,
          shared: Math.abs(versionWords.bottom - buttonWords.bottom) <= 1,
          buttonEnd: end(buttonBox),
          versionEnds: versionWords.ends,
        };
      },
      [version, button] as const,
    );
    assert.ok(
      Math.abs(layout.buttonEnd) <= ROUNDING_PX,
      `the button "${name}" ends ${Math.round(-layout.buttonEnd)}px before the header's inline end`,
    );
    if (layout.fits) {
      assert.ok(layout.shared, `the running version and the button "${name}" fit on one line, but stand on two`);
    } else {
      const short = layout.versionEnds.filter((at) => Math.abs(at) > ROUNDING_PX);
      assert.deepEqual(
        short.map((at) => `a line of the running version ends ${Math.round(-at)}px before the header's inline end`),
        [],
        "the running version's lines",
      );
    }
  },
);

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
