// The footer (features/ui/components/footer.feature) and the running version
// it shows (features/ui/version.feature). The footer is found by its role,
// the contentinfo landmark, and the version by its paragraph's role and its
// words; only their boxes and computed styles are read.

import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { demoSection, LIVE_REGION } from "../support/gallery.ts";
import { namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";
import { runningBuild } from "./start_page.ts";

// Layout rounds to fractions of a pixel; anything within this is one place.
const ROUNDING_PX = 1;

/** The page's one contentinfo landmark: the footer outside every section. */
function footer(world: BrowserWorld): Locator {
  return world.page.getByRole("contentinfo");
}

/** The footer's paragraph whose words are `words`, isolation marks aside. */
function footerParagraph(world: BrowserWorld, words: string | RegExp): Locator {
  return footer(world)
    .getByRole("paragraph")
    .filter({ hasText: typeof words === "string" ? namePattern(words) : words });
}

// The footer is the page's last landmark: after the main landmark in reading
// order, outside it, and below it on the screen. The version is plain text,
// never a stop for Tab (#502), and names the build the daemon runs (#516).
Then("the footer shows the running version after the main content", async function (this: BrowserWorld) {
  const version = footerParagraph(this, `Version ${await runningBuild()}`);
  await expect(version).toBeVisible();
  await expect(footer(this)).toHaveCount(1);
  const main = this.page.getByRole("main");
  const placed = await main.evaluate(
    (landmark, foot) => {
      if (foot === null) return "no footer";
      if (landmark.contains(foot)) return "inside the main landmark";
      if ((landmark.compareDocumentPosition(foot) & Node.DOCUMENT_POSITION_FOLLOWING) === 0) return "before the main landmark";
      const [mainBox, footBox] = [landmark.getBoundingClientRect(), foot.getBoundingClientRect()];
      if (footBox.top < mainBox.bottom - 1) return `${Math.round(mainBox.bottom - footBox.top)}px over the main landmark`;
      return null;
    },
    await footer(this).elementHandle(),
  );
  assert.equal(placed, null, `the footer is ${placed}`);
  assert.equal(await version.evaluate((shown) => shown.matches(":is(a, button, input, [tabindex])")), false);
});

Then("the page has no footer", async function (this: BrowserWorld) {
  await expect(this.page.getByRole("main")).toBeVisible();
  await expect(footer(this)).toHaveCount(0);
});

// The running version's width depends on the commit the build names, whose
// digits differ in width, so a layout measured with this build's own commit
// would pass or fail by the commit (#908). Of the versions a build between
// releases shows, the widest is the widest digit throughout, with changes
// (`0.3.1+4444444.dirty` in Chromium on macOS). The digits are measured in
// the page's own font, which differs by browser and system. From now on,
// every answer naming the daemon's build names that one in place of its own;
// the page reads it when it next loads.
const shownCommits = new WeakMap<BrowserWorld, RegExp>();

When("the running version is the widest one a build between releases can show", async function (this: BrowserWorld) {
  const digits = await this.page.locator("body").evaluate((body) => {
    const probe = document.createElement("span");
    probe.style.cssText = "position: absolute; visibility: hidden; white-space: nowrap";
    body.append(probe);
    const range = document.createRange();
    const measured = [..."0123456789abcdef"].map((digit) => {
      probe.textContent = digit.repeat(10);
      range.selectNodeContents(probe);
      return { digit, width: range.getBoundingClientRect().width };
    });
    probe.remove();
    return measured;
  });
  const { digit } = digits.reduce((best, next) => (next.width > best.width ? next : best));
  const build = { commit: digit.repeat(40), dirty: true, release: false };
  await this.page.route(
    (url) => url.pathname === "/api/v1/session",
    async (route) => {
      const answer = await route.fetch();
      const body: unknown = await answer.json().catch(() => undefined);
      if (typeof body === "object" && body !== null && typeof (body as Record<string, unknown>).version === "string") {
        await route.fulfill({ response: answer, json: { ...body, ...build } });
      } else {
        await route.fulfill({ response: answer });
      }
    },
  );
  shownCommits.set(this, new RegExp(`^Version\\s.*\\+[\\u2066-\\u2069]*${digit}{7}\\.dirty[\\u2066-\\u2069]*$`));
});

// Every line of the version's words lies inside the footer's box, so a
// version too long for its line wraps rather than standing out of the page.
Then("the footer shows that version, every line of it inside the footer", async function (this: BrowserWorld) {
  const shownCommit = shownCommits.get(this);
  assert.ok(shownCommit, "no version was chosen for the build");
  const version = footerParagraph(this, shownCommit);
  await expect(version).toBeVisible();
  const outside = await footer(this).evaluate(
    (foot, paragraph) => {
      if (paragraph === null) return ["no version"];
      const box = foot.getBoundingClientRect();
      const range = document.createRange();
      range.selectNodeContents(paragraph);
      return [...range.getClientRects()]
        .filter((line) => line.width > 0 && (line.left < box.left - 1 || line.right > box.right + 1))
        .map((line) => `a line from ${Math.round(line.left)}px to ${Math.round(line.right)}px`);
    },
    await version.elementHandle(),
  );
  assert.deepEqual(outside, [], "the version's lines outside the footer");
});

// Tab from the demo's heading leaves the demo: nothing in a footer is a stop
// for Tab (§4.5).
Then("focus has left the {string} demo", async function (this: BrowserWorld, demo: string) {
  const section = await demoSection(this.page, demo).elementHandle();
  assert.ok(section, `the gallery has no "${demo}" demo`);
  const where = await this.page.evaluate((held) => {
    const active = document.activeElement;
    if (active === null || active === document.body) return "on the page itself";
    return held.contains(active) ? active.outerHTML.slice(0, 120) : null;
  }, section);
  assert.equal(where, null, `focus is still in the "${demo}" demo, or fell to the page: ${where}`);
});

// The footer's version is read where it stands: nothing in the footer takes
// focus, and it is no live region, nor holds one (A3).
Then("the footer holds no stop for Tab and no live region", async function (this: BrowserWorld) {
  await expect(footer(this)).toHaveCount(1);
  const found = await footer(this).evaluate(
    (foot, live) => ({
      stops: [...foot.querySelectorAll("a[href], button, input, select, textarea, [tabindex]")].map((el) => el.outerHTML.slice(0, 80)),
      live: foot.matches(live) || foot.querySelector(live) !== null,
    }),
    LIVE_REGION,
  );
  assert.deepEqual(found.stops, [], "the footer's stops for Tab");
  assert.equal(found.live, false, "the footer is or holds a live region");
});

// The demos' footers stand inside a section, where a footer is no landmark,
// so the page's one contentinfo landmark is its own footer: a sibling after
// the main landmark, never inside it.
Then("the page has one footer, the contentinfo landmark, after the main landmark and outside it", async function (this: BrowserWorld) {
  await expect(footer(this)).toHaveCount(1);
  const placed = await this.page.getByRole("main").evaluate(
    (landmark, foot) => {
      if (foot === null) return "missing";
      if (landmark.contains(foot)) return "inside the main landmark";
      return landmark.compareDocumentPosition(foot) & Node.DOCUMENT_POSITION_FOLLOWING ? null : "before the main landmark";
    },
    await footer(this).elementHandle(),
  );
  assert.equal(placed, null, `the footer is ${placed}`);
});

// Quiet: the version in its token on the page's own background, no fill of
// the footer's own, a 1px rule above in its token, and no shadow (the Flat
// Ledger Rule). Each token is read through a probe, as the page resolves it.
Then(
  "the footer draws its version in {string} on the page's background, under a 1px rule in {string}, with no shadow",
  async function (this: BrowserWorld, words: string, rule: string) {
    const version = footerParagraph(this, `Version ${await runningBuild()}`);
    await expect(version).toBeVisible();
    const drawn = await footer(this).evaluate(
      (foot, { paragraph, tokens }) => {
        const probe = document.createElement("span");
        document.body.append(probe);
        const resolve = (token: string) => {
          probe.style.color = `var(${token}, rgb(1, 2, 3))`;
          return getComputedStyle(probe).color;
        };
        const expected = { words: resolve(tokens.words), rule: resolve(tokens.rule) };
        probe.remove();
        const style = getComputedStyle(foot);
        return {
          expected,
          words: paragraph === null ? "no version" : getComputedStyle(paragraph).color,
          fill: style.backgroundColor,
          rule: style.borderBlockStartColor,
          ruleWidth: style.borderBlockStartWidth,
          ruleStyle: style.borderBlockStartStyle,
          otherEdges: [style.borderBlockEndWidth, style.borderInlineStartWidth, style.borderInlineEndWidth],
          shadow: style.boxShadow,
        };
      },
      { paragraph: await version.elementHandle(), tokens: { words, rule } },
    );
    assert.equal(drawn.words, drawn.expected.words, `the version is not drawn in ${words}`);
    assert.equal(drawn.fill, "rgba(0, 0, 0, 0)", "the footer has a fill of its own");
    assert.deepEqual(
      [drawn.rule, drawn.ruleWidth, drawn.ruleStyle],
      [drawn.expected.rule, "1px", "solid"],
      `the footer's rule is not 1px solid ${rule}`,
    );
    assert.deepEqual(drawn.otherEdges, ["0px", "0px", "0px"], "the footer has an edge other than its rule");
    assert.equal(drawn.shadow, "none", "the footer casts a shadow");
  },
);

// The footer's frame is the main landmark's: the box around its version's
// paragraph starts and ends where the main landmark's box does, and its
// words start where the main content's text does, 1rem in from the frame, at
// the start of the line in the page's direction. The version is found by its
// number, which a pseudolocale leaves as it is.
Then(
  "the footer's version starts where the main content's text starts, in a frame as wide as the main content's",
  async function (this: BrowserWorld) {
    const version = footer(this)
      .getByRole("paragraph")
      .filter({ hasText: /\d+\.\d+\.\d+/ });
    await expect(version).toBeVisible();
    const measured = await this.page.getByRole("main").evaluate(
      (landmark, paragraph) => {
        if (paragraph?.parentElement == null) return undefined;
        const style = getComputedStyle(landmark);
        const rtl = style.direction === "rtl";
        const mainBox = landmark.getBoundingClientRect();
        const frame = paragraph.parentElement.getBoundingClientRect();
        const range = document.createRange();
        range.selectNodeContents(paragraph);
        const words = range.getBoundingClientRect();
        const textStart = rtl
          ? mainBox.right - Number.parseFloat(style.paddingRight)
          : mainBox.left + Number.parseFloat(style.paddingLeft);
        return {
          start: Math.abs(rtl ? frame.right - mainBox.right : frame.left - mainBox.left),
          end: Math.abs(rtl ? frame.left - mainBox.left : frame.right - mainBox.right),
          words: Math.abs((rtl ? words.right : words.left) - textStart),
        };
      },
      await version.elementHandle(),
    );
    assert.ok(measured, "the footer shows no version");
    assert.ok(measured.start <= ROUNDING_PX, `the footer's frame starts ${Math.round(measured.start)}px from the main content's`);
    assert.ok(measured.end <= ROUNDING_PX, `the footer's frame ends ${Math.round(measured.end)}px from the main content's`);
    assert.ok(measured.words <= ROUNDING_PX, `the version starts ${Math.round(measured.words)}px from the main content's text`);
  },
);

// A page shorter than the window leaves no gap under its footer: the footer's
// foot is the window's, and the page is no taller than the window.
Then("the footer stands at the foot of the window, and the page doesn't scroll", async function (this: BrowserWorld) {
  await expect(footer(this)).toBeVisible();
  const measured = await footer(this).evaluate((foot) => ({
    gap: document.documentElement.clientHeight - foot.getBoundingClientRect().bottom,
    overflow: document.documentElement.scrollHeight - document.documentElement.clientHeight,
  }));
  assert.ok(Math.abs(measured.gap) <= ROUNDING_PX, `the footer ends ${Math.round(measured.gap)}px above the foot of the window`);
  assert.ok(measured.overflow <= 0, `the page is ${measured.overflow}px taller than the window`);
});
