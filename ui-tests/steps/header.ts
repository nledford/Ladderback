// The header bar (features/ui/header.feature): where its parts sit and how
// tall Sign out is, measured from layout. Elements are found by role and
// accessible name; only their boxes are read.

import assert from "node:assert/strict";

import { type DataTable, Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { composite, contrastRatio, journey, namePattern } from "../support/keyboard.ts";
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

// A screen narrower than 41rem (decision 143): the pages' links and Sign out
// sit in a panel behind a hamburger button. The header as the reader sees it
// is the band behind the banner, its parent, padding included.
Then("the header is under {int} rem tall", async function (this: BrowserWorld, limit: number) {
  const { height, rem } = await this.page.getByRole("banner").evaluate((header) => ({
    height: (header.parentElement ?? header).getBoundingClientRect().height,
    rem: Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
  }));
  assert.ok(height < limit * rem - 0.5, `the header is ${height.toFixed(1)}px tall, not under ${limit}rem (${limit * rem}px)`);
});

Then("the banner shows no button {string}", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("banner").getByRole("button", { name: namePattern(name) })).toHaveCount(0);
});

// The button that shows the menu is a disclosure: `aria-expanded` says
// whether the panel shows, and `aria-controls` names it.
Then("the button {string} is collapsed", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("banner").getByRole("button", { name: namePattern(name) });
  await expect(button).toHaveAttribute("aria-expanded", "false");
});

Then("the button {string} is expanded", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("banner").getByRole("button", { name: namePattern(name) });
  await expect(button).toHaveAttribute("aria-expanded", "true");
});

Then("the button {string} names the menu panel", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("banner").getByRole("button", { name: namePattern(name) });
  const controls = await button.getAttribute("aria-controls");
  assert.ok(controls, `the button "${name}" controls nothing`);
  const panel = this.page.locator(`[id="${controls}"]`);
  await expect(panel).toHaveCount(1);
  await expect(panel.getByRole("navigation")).toHaveCount(1);
  await expect(panel.getByRole("button", { name: namePattern("Sign out") })).toHaveCount(1);
});

// An icon-only button: its icon, and no words written in it (its name is on
// the button, not in it).
Then("the button {string} shows an icon and no words", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("banner").getByRole("button", { name: namePattern(name) });
  await expect(button).toBeVisible();
  const found = await button.evaluate((control) => ({
    icons: control.querySelectorAll("svg").length,
    words: (control.textContent ?? "").trim(),
  }));
  assert.equal(found.icons, 1, `the button "${name}" shows ${found.icons} icons`);
  assert.equal(found.words, "", `the button "${name}" writes "${found.words}"`);
});

// A target a finger finds: 44 CSS pixels a side at least, 3em at the page's
// text size, so it grows with the reader's text.
Then("the button {string} is at least {int} pixels square", async function (this: BrowserWorld, name: string, side: number) {
  const button = this.page.getByRole("banner").getByRole("button", { name: namePattern(name) });
  await expect(button).toBeVisible();
  const box = await button.evaluate((control) => {
    const rect = control.getBoundingClientRect();
    return { width: rect.width, height: rect.height };
  });
  assert.ok(box.width >= side - 0.5, `the button "${name}" is ${box.width.toFixed(1)}px wide, under ${side}px`);
  assert.ok(box.height >= side - 0.5, `the button "${name}" is ${box.height.toFixed(1)}px tall, under ${side}px`);
  assert.ok(Math.abs(box.width - box.height) <= 0.5, `the button "${name}" is ${box.width}px by ${box.height}px`);
});

// Closed, Sign out is not drawn below 41rem.
Then("the button {string} is hidden", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("banner").getByRole("button", { name: namePattern(name) })).toBeHidden();
});

Then("the pages' links are hidden", async function (this: BrowserWorld) {
  await expect(this.page.getByRole("banner").getByRole("navigation")).toBeHidden();
});

// Shown, the links stand under the brand's line, each tall enough for a
// finger.
Then(
  "the pages' links are shown below the brand's line, each at least {int} rem tall",
  async function (this: BrowserWorld, least: number) {
    const banner = this.page.getByRole("banner");
    const navigation = banner.getByRole("navigation");
    await expect(navigation).toBeVisible();
    const brandBottom = await banner
      .getByRole("link", { name: namePattern("Ladderback") })
      .evaluate((link) => link.getBoundingClientRect().bottom);
    const drawn = await navigation.getByRole("link").evaluateAll((links) =>
      links.map((link) => {
        const box = link.getBoundingClientRect();
        return { name: (link.textContent ?? "").trim(), top: box.top, height: box.height };
      }),
    );
    assert.ok(drawn.length > 0, "the navigation has no links");
    const rem = await this.page.evaluate(() => Number.parseFloat(getComputedStyle(document.documentElement).fontSize));
    const beside = drawn.filter((link) => link.top < brandBottom - ROUNDING_PX).map((link) => link.name);
    assert.deepEqual(beside, [], "links on the brand's line");
    const short = drawn.filter((link) => link.height < least * rem - 0.5).map((link) => `${link.name}: ${link.height.toFixed(1)}px`);
    assert.deepEqual(short, [], `links under ${least}rem`);
  },
);

// The brand's link and the menu button stand on one line, level: the button
// has no words of its own, so their middles meet.
Then(
  "the link {string} and the button {string} share a line",
  async function (this: BrowserWorld, linkName: string, buttonName: string) {
    const banner = this.page.getByRole("banner");
    const link = banner.getByRole("link", { name: namePattern(linkName) });
    const button = banner.getByRole("button", { name: namePattern(buttonName) });
    await Promise.all([expect(link).toBeVisible(), expect(button).toBeVisible()]);
    const middle = (box: { top: number; box: number }) => (box.top + box.box) / 2;
    const apart = Math.abs(middle(await extent(link)) - middle(await extent(button)));
    assert.ok(apart <= ROUNDING_PX, `the link "${linkName}" and the button "${buttonName}" stand ${apart.toFixed(1)}px apart`);
  },
);

/** The panel the Menu button controls. */
function menuPanel(world: BrowserWorld): Locator {
  return world.page.locator("#menu");
}

// The panel is a box, not loose rows: an edge all round and rounded corners,
// the width of the header's content, under the brand's line.
Then("the menu panel is a bordered box with rounded corners, under the brand's line", async function (this: BrowserWorld) {
  const panel = menuPanel(this);
  await expect(panel).toBeVisible();
  const brandBottom = await this.page
    .getByRole("banner")
    .getByRole("link", { name: namePattern("Ladderback") })
    .evaluate((link) => link.getBoundingClientRect().bottom);
  const drawn = await panel.evaluate((box) => {
    const style = getComputedStyle(box);
    const header = box.closest("header");
    const rect = box.getBoundingClientRect();
    const inner = header ? header.getBoundingClientRect() : rect;
    const edges = [style.borderTopWidth, style.borderRightWidth, style.borderBottomWidth, style.borderLeftWidth].map(
      Number.parseFloat,
    );
    return {
      edges,
      styles: [style.borderTopStyle, style.borderRightStyle, style.borderBottomStyle, style.borderLeftStyle],
      radius: Number.parseFloat(style.borderTopLeftRadius),
      top: rect.top,
      width: rect.width,
      headerWidth: inner.width - Number.parseFloat(getComputedStyle(header ?? box).paddingInlineStart) * 2,
    };
  });
  assert.ok(
    drawn.edges.every((edge) => edge >= 1) && drawn.styles.every((style) => style === "solid"),
    `the panel's edges are ${drawn.edges.join("/")}px, ${drawn.styles.join("/")}`,
  );
  assert.ok(drawn.radius > 0, "the panel's corners are square");
  assert.ok(
    drawn.top >= brandBottom - ROUNDING_PX,
    `the panel starts ${Math.round(brandBottom - drawn.top)}px above the foot of the brand's line`,
  );
  assert.ok(
    Math.abs(drawn.width - drawn.headerWidth) <= ROUNDING_PX,
    `the panel is ${drawn.width}px wide in a header ${drawn.headerWidth}px wide`,
  );
});

// One row for each page and one for Sign out, in the order of Tab, each a
// block as wide as the panel's inside and tall enough for a finger.
Then(
  "the menu panel's rows are these, in order, each at least {int} rem tall and as wide as the panel:",
  async function (this: BrowserWorld, least: number, table: DataTable) {
    const wanted = (table.hashes() as { row: string }[]).map(({ row }) => row);
    const rem = await this.page.evaluate(() => Number.parseFloat(getComputedStyle(document.documentElement).fontSize));
    const rows = await menuPanel(this).evaluate((panel) => {
      const inside = panel.clientWidth;
      return Array.from(panel.querySelectorAll("nav a, .site-account button")).map((row) => {
        const rect = row.getBoundingClientRect();
        return { name: (row.textContent ?? "").trim(), height: rect.height, width: rect.width, inside };
      });
    });
    assert.deepEqual(
      rows.map((row) => row.name),
      wanted,
      "the panel's rows",
    );
    const short = rows.filter((row) => row.height < least * rem - 0.5).map((row) => `${row.name}: ${row.height.toFixed(1)}px`);
    assert.deepEqual(short, [], `rows under ${least}rem`);
    const narrow = rows
      .filter((row) => row.width < row.inside - 1)
      .map((row) => `${row.name}: ${row.width.toFixed(1)}px of ${row.inside}px`);
    assert.deepEqual(narrow, [], "rows narrower than the panel");
  },
);

// The current page is told by its weight and by a bar at its inline start,
// which no other row has, so the rows' words stay aligned.
Then(
  "the link {string} is marked by a bar at its inline start that no other row has",
  async function (this: BrowserWorld, name: string) {
    const bars = await menuPanel(this)
      .locator("nav a")
      .evaluateAll((links) =>
        links.map((link) => ({
          name: (link.textContent ?? "").trim(),
          width: Number.parseFloat(getComputedStyle(link).borderInlineStartWidth),
          paddingStart: Number.parseFloat(getComputedStyle(link).paddingInlineStart),
        })),
      );
    const current = bars.find((bar) => bar.name === name);
    assert.ok(current, `the panel has no link "${name}"`);
    assert.ok(current.width >= 3, `the bar of the link "${name}" is ${current.width}px wide`);
    assert.deepEqual(
      bars.filter((bar) => bar.name !== name && bar.width > 0).map((bar) => bar.name),
      [],
      "other links with a bar",
    );
    const others = bars.filter((bar) => bar.name !== name);
    assert.ok(
      others.every((bar) => Math.abs(bar.paddingStart - (current.paddingStart + current.width)) <= 0.5),
      "the bar moves the words of the current page",
    );
  },
);

// What the panel's edge and the current page's bar lie on is the colors
// under them, layer on layer: each is 3:1 at least against it (1.4.11).
Then(
  "the menu panel's border and the bar of the link {string} show, at least {int} to 1 against what they lie on",
  async function (this: BrowserWorld, name: string, least: number) {
    const link = menuPanel(this).locator("nav a", { hasText: name });
    await expect(link).toHaveCount(1);
    const colors = await menuPanel(this).evaluate(
      (panel, barred) => {
        // The backgrounds from an element out to the page, nearest first.
        const behind = (start: Element | null) => {
          const layers: string[] = [];
          for (let el = start; el !== null; el = el.parentElement) layers.push(getComputedStyle(el).backgroundColor);
          return layers;
        };
        return {
          edge: getComputedStyle(panel).borderTopColor,
          edgeOn: behind(panel),
          bar: getComputedStyle(barred).borderInlineStartColor,
          barOn: behind(barred),
        };
      },
      await link.elementHandle(),
    );
    const edge = contrastRatio(composite([colors.edge]), composite(colors.edgeOn));
    assert.ok(edge >= least, `the panel's edge is ${edge.toFixed(2)}:1 against what it lies on`);
    const bar = contrastRatio(composite([colors.bar]), composite(colors.barOn));
    assert.ok(bar >= least, `the bar of the link "${name}" is ${bar.toFixed(2)}:1 against what it lies on`);
  },
);
