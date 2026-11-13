import assert from "node:assert/strict";

import { Given, Then, When } from "@cucumber/cucumber";
import type { Page } from "playwright";
import { expect } from "playwright/test";

import { journey } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

const NON_ASCII = /[^\x00-\x7f]/;

Given("the browser's language is {string}", function (this: BrowserWorld, tag: string) {
  this.contextOptions.locale = tag;
});

When("I open the web UI asking for the locale {string}", async function (this: BrowserWorld, locale: string) {
  await this.open(`/?locale=${encodeURIComponent(locale)}`);
});

Then("the page language is {string}", async function (this: BrowserWorld, language: string) {
  assert.equal(await this.page.evaluate(() => document.documentElement.lang), language);
});

// Both the attribute the page declares and the direction the browser lays out
// in, so a style can't undo what the page says.
Then("the text direction is {string}", async function (this: BrowserWorld, direction: string) {
  const { declared, laidOut } = await this.page.evaluate(() => ({
    declared: document.documentElement.getAttribute("dir"),
    laidOut: getComputedStyle(document.documentElement).direction,
  }));
  assert.equal(declared, direction, "the dir attribute of the page");
  assert.equal(laidOut, direction, "the direction the page is laid out in");
});

// The visible texts of the page that have letters and no transformed one: a
// text written in the code, not drawn from the catalog (§8.4). `data` is a
// selector for what the page shows besides its words, which the catalog did
// not write and so leaves as it is; without it nothing is exempt.
async function untransformed(page: Page, data?: string): Promise<{ inspected: number; plain: string[] }> {
  return page.evaluate((data) => {
    const plain: string[] = [];
    let inspected = 0;
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = (node.textContent ?? "").trim();
      const parent = node.parentElement;
      if (!/\p{L}/u.test(text) || !parent?.checkVisibility({ visibilityProperty: true })) continue;
      if (data && parent.closest(data)) continue;
      inspected += 1;
      if (/^[\x00-\x7f]*$/.test(text)) plain.push(text);
    }
    return { inspected, plain };
  }, data ?? null);
}

// §8.4: in a pseudolocale all visible text comes out of the catalog, so a text
// with letters and no transformed character was written in the code. This
// step exempts nothing: the pages it is used on show no data.
Then("every visible text is transformed", async function (this: BrowserWorld) {
  const { inspected, plain } = await untransformed(this.page);
  assert.ok(inspected > 0, "the page shows no text to check");
  assert.deepEqual(plain, [], "visible text that the pseudolocale did not transform");
});

// What a page shows of a repository is data besides sentences: the names, IDs
// and instants in the cells of a table's body, its row headers included, the
// paths of a list (each in a `bdi`), and the engine's own words
// (`lang="en"`). The catalog wrote none of them, so they stay as they are;
// every other text must be transformed.
Then("every visible text but the data of the tables and lists is transformed", async function (this: BrowserWorld) {
  const { inspected, plain } = await untransformed(this.page, 'tbody th, tbody td, bdi, [lang="en"]');
  assert.ok(inspected > 0, "the page shows no text to check");
  assert.deepEqual(plain, [], "visible text outside the data that the pseudolocale did not transform");
});

Then("the page title is transformed", async function (this: BrowserWorld) {
  const title = await this.page.title();
  assert.match(title, NON_ASCII, `the title "${title}" has no transformed letter`);
});

// A pseudolocale transforms a progress bar's name too, so on a page with one
// bar these find it by its role alone.
Then("the page's one progress bar shows {int} percent", async function (this: BrowserWorld, percent: number) {
  await expect(this.page.getByRole("progressbar")).toHaveAttribute("aria-valuenow", String(percent));
});

Then("the page's one progress bar shows no percentage", async function (this: BrowserWorld) {
  await expect(this.page.getByRole("progressbar")).not.toHaveAttribute("aria-valuenow");
});

// The bar is a stop in the tab order (§4.5), so a keyboard user reaches it
// as she reaches a control; a page with one bar finds it by its role alone.
When("I press Tab until focus is on the page's one progress bar", async function (this: BrowserWorld) {
  const keys = journey(this.page);
  const bar = keys.byRole("progressbar");
  const focused = async () => (await bar.count()) === 1 && (await bar.evaluate((el) => el === document.activeElement));
  for (let presses = 0; presses < 30 && !(await focused()); presses += 1) await keys.press("Tab");
  assert.ok(await focused(), "focus did not reach the page's one progress bar within 30 Tab presses");
});

// What a screen reader reads as the bar's value came out of the catalog, the
// step's message and the amounts' units with it, so each of its words with
// letters has a transformed one.
Then("the value text of the page's one progress bar is transformed", async function (this: BrowserWorld) {
  const bar = this.page.getByRole("progressbar");
  await expect(bar).toHaveAttribute("aria-valuetext", /%/);
  const text = (await bar.getAttribute("aria-valuetext")) ?? "";
  const plain = text.split(/\s+/).filter((word) => /\p{L}/u.test(word) && !NON_ASCII.test(word));
  assert.deepEqual(plain, [], `words of the value text "${text}" the pseudolocale did not transform`);
});

// The cell of a running operation's bar says two things in words, its status
// and the step it is in (a percentage has no letters), and the catalog wrote
// both, so the pseudolocale transformed both.
Then("the cell of the page's one progress bar says its status and its step, both transformed", async function (this: BrowserWorld) {
  const said = () =>
    this.page.getByRole("progressbar").evaluate((bar) => {
      const texts: string[] = [];
      const cell = bar.closest("td");
      if (!cell) return texts;
      const walker = document.createTreeWalker(cell, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const text = (node.textContent ?? "").trim();
        if (/\p{L}/u.test(text) && node.parentElement?.checkVisibility({ visibilityProperty: true })) texts.push(text);
      }
      return texts;
    });
  await expect.poll(said).toHaveLength(2);
  const plain = (await said()).filter((text) => !NON_ASCII.test(text));
  assert.deepEqual(plain, [], "text in the bar's cell that the pseudolocale did not transform");
});

Then("the skip link is at the {word} edge of the page", async function (this: BrowserWorld, edge: string) {
  assert.ok(edge === "left" || edge === "right", `unknown edge "${edge}"`);
  // The skip link is the first link of the page, ahead of the navigation.
  const link = journey(this.page).byRole("link").first();
  const box = await link.boundingBox();
  assert.ok(box, "the skip link is not shown");
  const viewport = this.page.viewportSize();
  assert.ok(viewport, "the page has no viewport");
  const gaps = { left: box.x, right: viewport.width - (box.x + box.width) };
  const far = edge === "left" ? "right" : "left";
  assert.ok(
    gaps[edge] >= 0 && gaps[edge] < gaps[far],
    `the skip link is ${gaps.left}px from the left edge and ${gaps.right}px from the right`,
  );
});

// The first line of the heading's text against the content box of <main>.
Then("the main heading starts at the {word} edge of the content", async function (this: BrowserWorld, edge: string) {
  assert.ok(edge === "left" || edge === "right", `unknown edge "${edge}"`);
  const keys = journey(this.page);
  const text = await keys.byRole("heading", { level: 1 }).evaluate((heading) => {
    const range = document.createRange();
    range.selectNodeContents(heading);
    const line = range.getClientRects()[0];
    if (line === undefined) throw new Error("the main heading has no line of text");
    return { left: line.left, right: line.right };
  });
  const content = await keys.byRole("main").evaluate((main) => {
    const box = main.getBoundingClientRect();
    const style = getComputedStyle(main);
    return {
      left: box.left + parseFloat(style.paddingLeft),
      right: box.right - parseFloat(style.paddingRight),
    };
  });
  const gaps = { left: text.left - content.left, right: content.right - text.right };
  const far = edge === "left" ? "right" : "left";
  assert.ok(
    Math.abs(gaps[edge]) <= 1 && gaps[edge] < gaps[far],
    `the heading is ${gaps.left}px from the left edge of the content and ${gaps.right}px from the right`,
  );
});
