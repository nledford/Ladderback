import assert from "node:assert/strict";

import { Given, Then, When } from "@cucumber/cucumber";

import { journey } from "../support/keyboard.mjs";

const NON_ASCII = /[^\x00-\x7f]/;

Given("the browser's language is {string}", function (tag) {
  this.contextOptions.locale = tag;
});

When("I open the web UI asking for the locale {string}", async function (locale) {
  await this.open(`/?locale=${encodeURIComponent(locale)}`);
});

Then("the page language is {string}", async function (language) {
  assert.equal(await this.page.evaluate(() => document.documentElement.lang), language);
});

// Both the attribute the page declares and the direction the browser lays out
// in, so a style can't undo what the page says.
Then("the text direction is {string}", async function (direction) {
  const { declared, laidOut } = await this.page.evaluate(() => ({
    declared: document.documentElement.getAttribute("dir"),
    laidOut: getComputedStyle(document.documentElement).direction,
  }));
  assert.equal(declared, direction, "the dir attribute of the page");
  assert.equal(laidOut, direction, "the direction the page is laid out in");
});

// §8.4: in a pseudolocale all visible text comes out of the catalog, so a text
// with letters and no transformed character was written in the code. The
// exemption for data (IDs, paths, hostnames, verbatim rustic text marked
// lang="en") arrives with the first screen that shows data: the start page
// has none.
Then("every visible text is transformed", async function () {
  const { inspected, plain } = await this.page.evaluate(() => {
    const plain = [];
    let inspected = 0;
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node.textContent.trim();
      const parent = node.parentElement;
      if (!/\p{L}/u.test(text) || !parent?.checkVisibility({ visibilityProperty: true })) continue;
      inspected += 1;
      if (/^[\x00-\x7f]*$/.test(text)) plain.push(text);
    }
    return { inspected, plain };
  });
  assert.ok(inspected > 0, "the page shows no text to check");
  assert.deepEqual(plain, [], "visible text that the pseudolocale did not transform");
});

Then("the page title is transformed", async function () {
  const title = await this.page.title();
  assert.match(title, NON_ASCII, `the title "${title}" has no transformed letter`);
});

Then("the skip link is at the {word} edge of the page", async function (edge) {
  assert.ok(["left", "right"].includes(edge), `unknown edge "${edge}"`);
  const link = journey(this.page).byRole("link");
  assert.equal(await link.count(), 1, "the start page has exactly one link, the skip link");
  const box = await link.boundingBox();
  assert.ok(box, "the skip link is not shown");
  const gaps = { left: box.x, right: this.page.viewportSize().width - (box.x + box.width) };
  const far = edge === "left" ? "right" : "left";
  assert.ok(
    gaps[edge] >= 0 && gaps[edge] < gaps[far],
    `the skip link is ${gaps.left}px from the left edge and ${gaps.right}px from the right`,
  );
});

// The first line of the heading's text against the content box of <main>.
Then("the main heading starts at the {word} edge of the content", async function (edge) {
  assert.ok(["left", "right"].includes(edge), `unknown edge "${edge}"`);
  const keys = journey(this.page);
  const text = await keys.byRole("heading", { level: 1 }).evaluate((heading) => {
    const range = document.createRange();
    range.selectNodeContents(heading);
    const line = range.getClientRects()[0];
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
