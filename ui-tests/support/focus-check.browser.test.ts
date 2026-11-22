// The keyboard walks' own check that a focused control is in sight (WCAG
// 2.4.11, `journey(page).press` in support/keyboard.ts), tried in a real
// browser on a page of the test's own, where what covers the control is known
// (§4.5 "Tests"). It tests the test tooling, not the product, so it lives
// beside the tooling, not in features/.
//
// `just test-ui <engine>` runs it with `node --test`, before the scenarios, in
// the engine `LADDERBACK_UI_BROWSER` names (Chromium when unset), as
// cucumber-js reads it in support/world.ts, which only cucumber-js can load.
// `just lint` runs `scripts/*.test.ts` alone, so it never starts a browser,
// and cucumber.ts's import glob leaves out `*.test.ts`.

import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { type Browser, chromium, firefox, webkit } from "playwright";

import { journey } from "./keyboard.ts";

const engine = process.env.LADDERBACK_UI_BROWSER || "chromium";
const browserType = Object.entries({ chromium, firefox, webkit }).find(([name]) => name === engine)?.[1];
if (browserType === undefined) {
  throw new Error(`LADDERBACK_UI_BROWSER is "${engine}"; it must be one of chromium, firefox, webkit`);
}

// A scenario step's limit (support/world.ts), for the launch and for each test.
const LIMIT_MS = 30_000;

// A link in a paragraph twelve characters wide, so it wraps onto two lines,
// "Back to the" and "start page", with room between them: its line height is
// twice its text's.
const WRAPPED_LINK = `<!doctype html>
<html lang="en">
  <head>
    <title>Focus check</title>
    <style>
      body { margin: 1rem; background: white; color: black; }
      p { inline-size: 12ch; font: 20px/2 monospace; }
      a:focus-visible { outline: 2px solid black; outline-offset: 2px; }
      .cover { position: fixed; background: gray; }
    </style>
  </head>
  <body>
    <p><a href="#nowhere">Back to the start page</a></p>
  </body>
</html>`;

// The same link a long way down the page, so the browser scrolls to it when
// Tab first focuses it.
const FAR_LINK = `<!doctype html>
<html lang="en">
  <head>
    <title>Focus check</title>
    <style>
      body { margin: 1rem; background: white; color: black; }
      a:focus-visible { outline: 2px solid black; outline-offset: 2px; }
      .spacer { block-size: 3000px; }
    </style>
  </head>
  <body>
    <div class="spacer"></div>
    <p><a href="#nowhere">Back to the start page</a></p>
    <div class="spacer"></div>
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

/** Opens `WRAPPED_LINK` in a context of its own at the scenarios' viewport,
 *  puts a fixed box over `over` of the link's lines, and presses Tab onto the
 *  link through the walks' check. Given `leaves`, the link moves above the
 *  viewport as it takes focus, where no scroll brings it back. Given `moves`,
 *  a block above the link's paragraph grows to 200 px or back each time the
 *  check has measured the link, the first time only or every time, as when a
 *  page draws something above an element between the check's reads: the
 *  link's own `getBoundingClientRect` makes the move after it has answered,
 *  so the move lands between the reads in every engine. Given `blurs`, the
 *  link loses focus the same way once the check has first measured it. What
 *  the check said: `null` when it passed. */
async function tabOntoTheLink(
  over: "every line" | "its first line" | "no line",
  {
    leaves = false,
    moves = "never",
    blurs = false,
  }: { leaves?: boolean; moves?: "never" | "once" | "every time"; blurs?: boolean } = {},
): Promise<string | null> {
  assert.ok(browser !== undefined, "the browser is not running: `before` failed");
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  try {
    const page = await context.newPage();
    await page.setContent(WRAPPED_LINK);
    const lines = await page.evaluate(
      ({ covered, leaves, moves, blurs }) => {
        const link = document.querySelector("a");
        if (link === null) return 0;
        const fragments = [...link.getClientRects()].filter((r) => r.width > 0 && r.height > 0);
        for (const r of fragments.slice(0, covered)) {
          // A pixel past the line on every side, so no edge of it shows.
          const cover = document.createElement("div");
          cover.className = "cover";
          Object.assign(cover.style, {
            left: `${r.left - 1}px`,
            top: `${r.top - 1}px`,
            width: `${r.width + 2}px`,
            height: `${r.height + 2}px`,
          });
          document.body.append(cover);
        }
        if (leaves) {
          link.addEventListener("focus", () => Object.assign(link.style, { position: "fixed", top: "-1000px" }));
        }
        if (moves !== "never" || blurs) {
          const above = document.createElement("div");
          link.parentElement?.before(above);
          const measure = link.getBoundingClientRect.bind(link);
          let measures = 0;
          link.getBoundingClientRect = () => {
            const box = measure();
            measures += 1;
            if (moves === "every time" || (moves === "once" && measures === 1)) {
              above.style.blockSize = above.style.blockSize === "200px" ? "0px" : "200px";
            }
            if (blurs) link.blur();
            return box;
          };
        }
        return fragments.length;
      },
      { covered: { "every line": Infinity, "its first line": 1, "no line": 0 }[over], leaves, moves, blurs },
    );
    assert.equal(lines, 2, "the link stands on two lines");
    try {
      await journey(page).press("Tab");
      return null;
    } catch (error) {
      if (!(error instanceof assert.AssertionError)) throw error;
      return error.message;
    }
  } finally {
    await context.close();
  }
}

test(`${engine}: a wrapped link with a box over every line is reported covered`, { timeout: LIMIT_MS }, async () => {
  const verdict = await tabOntoTheLink("every line");
  // Each line's center finds its box, not the paragraph between the lines.
  assert.match(
    verdict ?? "the check passed",
    /^focused element is covered: <a href="#nowhere">.*\(its lines' centers find <div>, <div>\)$/,
  );
});

test(`${engine}: a wrapped link with a box over its first line only is in sight`, { timeout: LIMIT_MS }, async () => {
  const verdict = await tabOntoTheLink("its first line");
  assert.equal(verdict, null, `the check reported: ${verdict}`);
});

test(`${engine}: a link that leaves the viewport as it takes focus is reported out of view`, { timeout: LIMIT_MS }, async () => {
  // The check waits for the browser's own scroll to the focused element
  // (keyboard.ts), and no wait brings this one back into sight.
  const verdict = await tabOntoTheLink("no line", { leaves: true });
  assert.match(
    verdict ?? "the check passed",
    /^focused element is covered: <a href="#nowhere".*\(its lines' centers find nothing, out of view\)$/,
  );
});

test(`${engine}: a link the page moves once between the check's reads is in sight where it lands`, { timeout: LIMIT_MS }, async () => {
  // Looked for where it was measured, the link's lines' centers would find
  // the block drawn there; the check measures it again and looks where it
  // now stands.
  const verdict = await tabOntoTheLink("no line", { moves: "once" });
  assert.equal(verdict, null, `the check reported: ${verdict}`);
});

test(`${engine}: a link the page moves between every pair of the check's reads is reported moving`, {
  timeout: LIMIT_MS,
}, async () => {
  const verdict = await tabOntoTheLink("no line", { moves: "every time" });
  assert.match(
    verdict ?? "the check passed",
    /^after Tab the focused element kept moving while the check looked at it, 5 times: <a href="#nowhere"/,
  );
});

test(`${engine}: a link that loses focus between the check's reads is reported`, { timeout: LIMIT_MS }, async () => {
  const verdict = await tabOntoTheLink("no line", { blurs: true });
  assert.match(verdict ?? "the check passed", /^after Tab focus moved while the check looked at it, on the page body$/);
});

test(`${engine}: a link focused while the page's timers are busy is in sight once the browser scrolls to it`, {
  timeout: LIMIT_MS,
}, async () => {
  assert.ok(browser !== undefined, "the browser is not running: `before` failed");
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  try {
    const page = await context.newPage();
    await page.setContent(FAR_LINK);
    // Timers due before the press that keep the page busy for about half a
    // second: WebKit's timer that scrolls to the focused link waits behind
    // them, and the page draws frames in between, so a check that only
    // waited two frames found the link still out of view on Linux (#973).
    await page.evaluate(() => {
      for (let i = 0; i < 12; i += 1) {
        setTimeout(() => {
          const end = performance.now() + 40;
          while (performance.now() < end);
        }, 0);
      }
    });
    let verdict: string | null = null;
    try {
      await journey(page).press("Tab");
    } catch (error) {
      if (!(error instanceof assert.AssertionError)) throw error;
      verdict = error.message;
    }
    assert.equal(verdict, null, `the check reported: ${verdict}`);
    assert.ok(await page.evaluate(() => scrollY > 0), "the browser scrolled to the link");
  } finally {
    await context.close();
  }
});

// A scroll region taller than the scenarios' 320 by 256 viewport, as a table's
// frame is at 200 % text, its top already in view below a button: Tab from
// the button focuses it while part of it shows, so Chromium and WebKit, like
// Firefox, scroll only until its top edge shows, and the center of its box
// lies below the viewport (#1292).
const TALL_REGION = `<!doctype html>
<html lang="en">
  <head>
    <title>Focus check</title>
    <style>
      body { margin: 0; background: white; color: black; }
      button { margin: 1rem; }
      button:focus-visible, [role="region"]:focus-visible { outline: 2px solid black; outline-offset: -2px; }
      .spacer { block-size: 120px; }
      [role="region"] { block-size: 700px; }
      .cover { position: fixed; inset: 0; background: gray; }
    </style>
  </head>
  <body>
    <button type="button">Columns</button>
    <div class="spacer"></div>
    <div role="region" aria-label="Repositories" tabindex="0">Rows</div>
    <div class="spacer"></div>
  </body>
</html>`;

/** Opens `TALL_REGION` at the scenarios' viewport and presses Tab onto the
 *  button, then onto the region, through the walks' check, and makes sure
 *  the engine left the region's top in view and the center of its box below
 *  the viewport, the case the check is for. Given `covered`, a fixed box (an
 *  `<aside>`, the page's one) over the whole viewport appears as the region
 *  takes focus. What the check said: `null` when it passed. */
async function tabOntoTheTallRegion(covered: boolean): Promise<string | null> {
  assert.ok(browser !== undefined, "the browser is not running: `before` failed");
  const context = await browser.newContext({ viewport: { width: 320, height: 256 } });
  try {
    const page = await context.newPage();
    await page.setContent(TALL_REGION);
    await page.evaluate((covered) => {
      const region = document.querySelector('[role="region"]');
      if (covered && region !== null) {
        region.addEventListener("focus", () => {
          const cover = document.createElement("aside");
          cover.className = "cover";
          document.body.append(cover);
        });
      }
    }, covered);
    const walk = journey(page);
    await walk.press("Tab");
    let verdict: string | null = null;
    try {
      await walk.press("Tab");
    } catch (error) {
      if (!(error instanceof assert.AssertionError)) throw error;
      verdict = error.message;
    }
    const { top, bottom } = await page.evaluate(() => {
      const box = document.querySelector('[role="region"]')?.getBoundingClientRect();
      return { top: box?.top ?? Number.NaN, bottom: box?.bottom ?? Number.NaN };
    });
    assert.ok(top < 256, `the region's top is in view, at ${top}`);
    assert.ok((top + bottom) / 2 > 256, `the center of the region's box lies below the viewport, at ${(top + bottom) / 2}`);
    return verdict;
  } finally {
    await context.close();
  }
}

test(`${engine}: a region taller than the viewport, focused as its top shows, is in sight`, { timeout: LIMIT_MS }, async () => {
  const verdict = await tabOntoTheTallRegion(false);
  assert.equal(verdict, null, `the check reported: ${verdict}`);
});

test(`${engine}: a region taller than the viewport with a box over all of it in view is reported covered`, {
  timeout: LIMIT_MS,
}, async () => {
  const verdict = await tabOntoTheTallRegion(true);
  assert.match(verdict ?? "the check passed", /^focused element is covered: <div role="region".*\(its lines' centers find <aside>\)$/);
});
