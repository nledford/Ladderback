// Journey steps for the shared link in the gallery and on the pages
// (technical design §4.5): a route change, the current page, an external
// link, its underline and its colors.

import assert from "node:assert/strict";

import { type DataTable, Then, When } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import { namePattern } from "../support/keyboard.ts";
import { pointer } from "../support/pointer.ts";
import type { BrowserWorld } from "../support/world.ts";
import { demoSection, eachOf } from "../support/gallery.ts";

// A link (technical design §4.5): a page of the app is a route change, the
// page the reader is on is marked and bold, a link to another site says so
// in its name and with an icon a screen reader skips, and every link is
// underlined, in Ladder Blue or the color of the text around it, and in the
// system's link color under forced colors. Each link is read alone with
// `evaluate`.

// The links of the gallery's "Link" demo.
const LINK_DEMO_LINKS = 6;

function linkNamed(page: Page, name: string): Locator {
  return page.getByRole("link", { name: namePattern(name) });
}

type Marked = { ladderbackMark?: string };

When("the page carries a mark that a reload would lose", async function (this: BrowserWorld) {
  await this.page.evaluate(() => {
    (window as unknown as Marked).ladderbackMark = "kept";
  });
});

Then("the page still carries its mark", async function (this: BrowserWorld) {
  const mark = await this.page.evaluate(() => (window as unknown as Marked).ladderbackMark);
  assert.equal(mark, "kept", "the page was loaded again: the link was not a route change");
});

Then("the link {string} is the current page, drawn bold", async function (this: BrowserWorld, name: string) {
  const link = linkNamed(this.page, name);
  await expect(link).toHaveAttribute("aria-current", "page");
  const weight = await link.evaluate((el) => Number(getComputedStyle(el).fontWeight));
  assert.ok(weight >= 700, `the link "${name}" is drawn at weight ${weight}, not bold`);
});

Then(
  "the link {string} is not the current page, drawn in the weight of the text around it",
  async function (this: BrowserWorld, name: string) {
    const link = linkNamed(this.page, name);
    await expect(link).not.toHaveAttribute("aria-current");
    const weights = await link.evaluate((el) => ({
      own: getComputedStyle(el).fontWeight,
      around: getComputedStyle(el.parentElement ?? el).fontWeight,
    }));
    assert.equal(weights.own, weights.around, `the link "${name}" is drawn at another weight than its text`);
  },
);

// The link's words come first, then its one icon, at the inline end of the
// words' last line in the line's direction, hidden from a screen reader;
// what the name adds after the words is in a box no sighted reader sees.
Then(
  "the link {string} shows the words {string}, then an icon a screen reader skips",
  async function (this: BrowserWorld, name: string, words: string) {
    const link = linkNamed(this.page, name);
    const found = await link.evaluate((el) => {
      const icons = el.querySelectorAll("svg");
      const icon = icons.item(0);
      if (icons.length !== 1 || icon === null) return { icons: icons.length };
      const range = document.createRange();
      range.setStart(el, 0);
      range.setEndBefore(icon);
      const last = [...range.getClientRects()].filter((rect) => rect.width > 0).at(-1);
      const box = icon.getBoundingClientRect();
      const rtl = getComputedStyle(el).direction === "rtl";
      const note = icon.nextElementSibling;
      return {
        icons: 1,
        // Without Fluent's isolation marks around a variable, which show
        // nothing, as a message with a version is.
        words: range
          .toString()
          .replace(/[\u2068\u2069]/g, "")
          .trim(),
        hidden: icon.getAttribute("aria-hidden"),
        after: last !== undefined && (rtl ? box.right <= last.left + 1 : box.left >= last.right - 1),
        noteUnseen: note !== null && getComputedStyle(note).clipPath === "inset(50%)" && note.nextSibling === null,
      };
    });
    assert.equal(found.icons, 1, `the link "${name}" has ${found.icons} icons`);
    assert.equal(found.words, words, `the words the link "${name}" shows`);
    assert.equal(found.hidden, "true", `the icon of the link "${name}" is not hidden from a screen reader`);
    assert.ok(found.after, `the icon of the link "${name}" doesn't stand after its words in the line's direction`);
    assert.ok(found.noteUnseen, `the rest of the link "${name}"'s name is not in a box no sighted reader sees`);
  },
);

// An outside address is never routed, even one that turns out to be the
// app's own: `external` keeps the router off it.
Then(
  "the link {string} is marked external, sends no referrer and opens where the reader is",
  async function (this: BrowserWorld, name: string) {
    const found = await linkNamed(this.page, name).evaluate((el) => ({
      rel: (el.getAttribute("rel") ?? "").split(/\s+/),
      target: el.getAttribute("target"),
    }));
    assert.ok(found.rel.includes("external"), `the link "${name}" has rel ${JSON.stringify(found.rel)}`);
    assert.ok(found.rel.includes("noreferrer"), `the link "${name}" has rel ${JSON.stringify(found.rel)}`);
    assert.equal(found.target, null, `the link "${name}" opens in ${found.target}`);
  },
);

// A link's underline thickness, as the page draws it now.
function underlineOf(link: Locator): Promise<number> {
  return link.evaluate((el) => Number.parseFloat(getComputedStyle(el).textDecorationThickness));
}

/** A link's underline before the pointer came to rest on it, per page. */
const underlineAtRest = new WeakMap<Page, number>();

// The link may sit below the window's edge, where a pointer can't reach; the
// page scrolls it into view first, as a reader would, and its underline is
// read before the pointer arrives.
When("I rest the pointer on the link {string}", async function (this: BrowserWorld, name: string) {
  const link = linkNamed(this.page, name);
  await link.scrollIntoViewIfNeeded();
  underlineAtRest.set(this.page, await underlineOf(link));
  await pointer(this.page).restOn(link);
});

Then("the underline of the link {string} is thicker than at rest", async function (this: BrowserWorld, name: string) {
  const atRest = underlineAtRest.get(this.page);
  assert.ok(atRest !== undefined, "the pointer never came to rest on a link");
  await expect.poll(() => underlineOf(linkNamed(this.page, name))).toBeGreaterThan(atRest);
});

// Underlined at rest, its line at least a pixel thick and far enough under
// the baseline (a fifth of the text's size) to clear the letters' descenders,
// which ink skipping keeps it off as well.
async function assertUnderlinedBelowLetters(links: Locator, count: number): Promise<void> {
  for (const link of await eachOf(links, count)) {
    const drawn = await link.evaluate((el) => {
      const style = getComputedStyle(el);
      return {
        name: (el.textContent ?? "").trim(),
        line: style.textDecorationLine,
        thickness: Number.parseFloat(style.textDecorationThickness),
        offset: Number.parseFloat(style.textUnderlineOffset) / Number.parseFloat(style.fontSize),
        skip: style.textDecorationSkipInk,
      };
    });
    assert.ok(drawn.line.includes("underline"), `the link "${drawn.name}" is not underlined: ${drawn.line}`);
    assert.ok(drawn.thickness >= 1, `the link "${drawn.name}"'s underline is ${drawn.thickness}px thick`);
    assert.ok(drawn.offset >= 0.2, `the link "${drawn.name}"'s underline is ${drawn.offset}em under its baseline`);
    assert.notEqual(drawn.skip, "none", `the link "${drawn.name}"'s underline crosses its letters`);
  }
}

// Each link in its tone, its words and underline alike: a token's color, or
// the color of the text around it. The table names every link of `scope`.
async function assertTones(scope: Locator, table: DataTable): Promise<void> {
  const rows = table.hashes() as { link: string; color: string }[];
  await eachOf(scope.getByRole("link"), rows.length);
  for (const { link: name, color } of rows) {
    const drawn = await scope.getByRole("link", { name: namePattern(name) }).evaluate((el, token) => {
      let want = getComputedStyle(el.parentElement ?? el).color;
      if (token.startsWith("--")) {
        const probe = document.createElement("span");
        probe.style.color = `var(${token})`;
        document.body.append(probe);
        want = getComputedStyle(probe).color;
        probe.remove();
      }
      const style = getComputedStyle(el);
      return { want, text: style.color, underline: style.textDecorationColor };
    }, color);
    assert.equal(drawn.text, drawn.want, `the link "${name}" is not drawn in ${color}`);
    assert.equal(drawn.underline, drawn.want, `the link "${name}"'s underline is not drawn in ${color}`);
  }
}

async function assertSystemLinkColor(links: Locator, count: number): Promise<void> {
  for (const link of await eachOf(links, count)) {
    const drawn = await link.evaluate((el) => {
      const probe = document.createElement("span");
      probe.style.forcedColorAdjust = "none";
      probe.style.color = "LinkText";
      document.body.append(probe);
      const want = getComputedStyle(probe).color;
      probe.remove();
      const style = getComputedStyle(el);
      return { name: (el.textContent ?? "").trim(), want, text: style.color, underline: style.textDecorationColor };
    });
    assert.equal(drawn.text, drawn.want, `the link "${drawn.name}" is not drawn in LinkText`);
    assert.equal(drawn.underline, drawn.want, `the link "${drawn.name}"'s underline is not drawn in LinkText`);
  }
}

/** A landmark of every page, as a step names it. */
function landmark(page: Page, name: string): Locator {
  return page.getByRole(name === "banner" ? "banner" : "main");
}

Then("every link of the {string} demo is underlined below its letters", async function (this: BrowserWorld, component: string) {
  await assertUnderlinedBelowLetters(demoSection(this.page, component).getByRole("link"), LINK_DEMO_LINKS);
});

Then("each link of the {string} demo is drawn in its tone:", async function (this: BrowserWorld, component: string, table: DataTable) {
  await assertTones(demoSection(this.page, component), table);
});

Then("every link of the {string} demo is drawn in the system's link color", async function (this: BrowserWorld, component: string) {
  await assertSystemLinkColor(demoSection(this.page, component).getByRole("link"), LINK_DEMO_LINKS);
});

// The pages' links are the shared link (§4.5), so they look like the demo's.
// The count is every link of the landmark, so none of them is left out, and
// the step waits for links a page draws from its first read.
Then(
  /^every link in the (banner|main content) is underlined below its letters, (\d+) in all$/,
  async function (this: BrowserWorld, where: string, count: string) {
    await assertUnderlinedBelowLetters(landmark(this.page, where).getByRole("link"), Number(count));
  },
);

Then(
  /^every link in the (banner|main content) is drawn in the system's link color, (\d+) in all$/,
  async function (this: BrowserWorld, where: string, count: string) {
    await assertSystemLinkColor(landmark(this.page, where).getByRole("link"), Number(count));
  },
);

Then(
  /^each link in the (banner|main content) is drawn in its tone:$/,
  async function (this: BrowserWorld, where: string, table: DataTable) {
    await assertTones(landmark(this.page, where), table);
  },
);

Then("the link {string} wraps onto more than one line", async function (this: BrowserWorld, name: string) {
  const lines = await linkNamed(this.page, name).evaluate((el) => {
    const tops = [...el.getClientRects()].filter((rect) => rect.width > 0).map((rect) => Math.round(rect.top));
    return new Set(tops).size;
  });
  assert.ok(lines > 1, `the link "${name}" stands on ${lines} line`);
});
