// The Firefox workaround for issue #774 (support/labeledby-crash.ts), tried
// in a real browser on a page of the test's own: with it, a page whose names
// come through `aria-labelledby` survives an accessibility scan followed by
// lookups by role and name, and the scan still reads those names, so it
// passes the ones that name something and reports the one that names nothing.
// It tests the test tooling, not the product, so it lives beside the tooling.
//
// `just test-ui <engine>` runs it with `node --test`, before the scenarios, in
// the engine `LADDERBACK_UI_BROWSER` names (Chromium when unset), as
// support/focus-check.browser.test.ts does.

import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import AxeBuilder from "@axe-core/playwright";
import { type Browser, chromium, firefox, type Page, webkit } from "playwright";

import { avoidLabeledByCrash } from "./labeledby-crash.ts";
import type { Engine } from "./world.ts";

const engines = { chromium, firefox, webkit };
const chosen = process.env.LADDERBACK_UI_BROWSER || "chromium";
const isEngine = (name: string): name is Engine => Object.hasOwn(engines, name);
if (!isEngine(chosen)) {
  throw new Error(`LADDERBACK_UI_BROWSER is "${chosen}"; it must be one of chromium, firefox, webkit`);
}
const engine: Engine = chosen;
const browserType = engines[engine];

// A scenario step's limit (support/world.ts), for the launch and for each test.
const LIMIT_MS = 30_000;

// The scenarios' scan (steps/gallery.ts).
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

// A table and a button, each named by a heading through aria-labelledby, as
// the app's tables and dialogs are, and a button whose aria-labelledby names
// an element the page doesn't have, so it has no name at all. The buttons are
// as large as a target must be (2.5.8), so their names are all the scan judges.
const PAGE = `<!doctype html>
<html lang="en">
  <head>
    <title>Names by reference</title>
    <style>button { min-inline-size: 24px; min-block-size: 24px; }</style>
  </head>
  <body>
    <main>
      <h1 id="plans-heading">Plans</h1>
      <table aria-labelledby="plans-heading">
        <thead><tr><th scope="col">Plan</th></tr></thead>
        <tbody><tr><td>demo-home</td></tr></tbody>
      </table>
      <h2 id="save-heading">Save</h2>
      <button type="button" aria-labelledby="save-heading"></button>
      <button type="button" id="nameless" aria-labelledby="no-such-heading"></button>
    </main>
  </body>
</html>`;

let browser: Browser | undefined;

before(
  async () => {
    browser = await browserType.launch();
  },
  { timeout: LIMIT_MS },
);

after(async () => {
  await browser?.close();
});

/** `PAGE`, open in a context of its own with the workaround in place. */
async function withPage<T>(use: (page: Page) => Promise<T>): Promise<T> {
  assert.ok(browser !== undefined, "the browser is not running: `before` failed");
  const context = await browser.newContext();
  try {
    await avoidLabeledByCrash(context, engine);
    const page = await context.newPage();
    await page.setContent(PAGE);
    return await use(page);
  } finally {
    await context.close();
  }
}

// Firefox alone loses the property, so the other engines' pages are what a
// reader's are; and a Firefox that moved it off `Element.prototype`, where
// the delete would find nothing, fails here rather than crash a scenario.
test("only Firefox's pages lack ariaLabelledByElements", { timeout: LIMIT_MS }, async () => {
  const present = await withPage((page) => page.evaluate(() => "ariaLabelledByElements" in document.body));
  assert.equal(present, engine !== "firefox");
});

test("a page named through aria-labelledby is scanned, then read by role and name", { timeout: LIMIT_MS }, async () => {
  const cells = await withPage(async (page) => {
    await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
    const table = page.getByRole("table", { name: "Plans", exact: true });
    await page.getByRole("button", { name: "Save", exact: true }).waitFor();
    return table.getByRole("cell").evaluateAll((found) => found.map((cell) => cell.textContent));
  });
  assert.deepEqual(cells, ["demo-home"]);
});

test("the scan passes a name given through aria-labelledby and reports one that names nothing", { timeout: LIMIT_MS }, async () => {
  const violations = await withPage(async (page) => {
    const { violations } = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
    return violations.map(({ id, nodes }) => ({ id, targets: nodes.map((node) => node.target.join(" ")) }));
  });
  assert.deepEqual(violations, [{ id: "button-name", targets: ["#nameless"] }]);
});
