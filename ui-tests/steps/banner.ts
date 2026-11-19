// Journey steps for the shared banner in the gallery (technical design §4.5):
// a page-wide notice above a page's content, an icon that names its tone in
// words, then the words, which the reader may dismiss for good in this
// browser. A banner has no role, so it has no name to find it by: it is found
// by its class inside its demo, which is found by its heading, or by its id
// under a pseudolocale, as a skeleton is. Its Dismiss button is found by role
// and name, by keyboard alone.

import assert from "node:assert/strict";

import type { DataTable } from "@cucumber/cucumber";
import { Then } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import type { BrowserWorld } from "../support/world.ts";
import { demoSection, eachOf } from "./components.ts";
import { ISOLATION, namePattern } from "../support/keyboard.ts";

// The gallery's banners as it opens (`gallery.rs`): one in each tone it
// takes, then the notice the reader can dismiss.
const BANNERS = 3;

// Half a pixel of rounding in a laid-out length.
const ROUNDING_PX = 0.5;

// The token each tone draws its icon and its edge in (`style.css`).
const TONE_TOKENS: Record<string, string> = {
  primary: "--accent",
  warning: "--warning",
};

const banners = (page: Page, component: string): Locator => demoSection(page, component).locator(".banner");

const near = (a: number, b: number, slack = ROUNDING_PX) => Math.abs(a - b) <= slack;

// Not a live region (§4.5): what is urgent goes through the announcer, so a
// banner carries no live role and no `aria-live`, nor does anything inside
// it; it is no landmark either, and no stop for Tab itself.
Then("no banner of the {string} demo is a live region, nor holds one", async function (this: BrowserWorld, component: string) {
  for (const banner of await eachOf(banners(this.page, component), BANNERS)) {
    const found = await banner.evaluate((element) => {
      const parts = [element, ...element.querySelectorAll("*")];
      return {
        element: element.localName,
        role: element.getAttribute("role"),
        tabindex: element.getAttribute("tabindex"),
        live: parts.filter(
          (part) =>
            part.hasAttribute("aria-live") || ["alert", "status", "log", "marquee", "timer"].includes(part.getAttribute("role") ?? ""),
        ).length,
      };
    });
    assert.deepEqual(found, { element: "div", role: null, tabindex: null, live: 0 });
  }
});

// Never color alone (1.4.1): each banner's icon is an image named for its
// tone, which a screen reader hears before its words, and its shape is its
// tone's own.
Then(
  "the banners of the {string} demo show these icons, each named for its tone, one shape per tone, in this order:",
  async function (this: BrowserWorld, component: string, table: DataTable) {
    const rows = table.hashes() as { tone: string; name: string }[];
    await eachOf(banners(this.page, component), rows.length);
    const drawn = await demoSection(this.page, component).evaluate((section) =>
      Array.from(section.querySelectorAll(".banner"), (banner) => {
        const icon = banner.querySelector(".banner-icon svg");
        return {
          classes: [...banner.classList],
          role: icon?.getAttribute("role") ?? null,
          name: icon?.getAttribute("aria-label") ?? null,
          shape: icon?.innerHTML ?? "",
        };
      }),
    );
    rows.forEach((row, at) => {
      const banner = drawn[at];
      assert.ok(banner !== undefined, `no banner ${at + 1}`);
      assert.ok(banner.classes.includes(`tone-${row.tone}`), `banner ${at + 1} is ${banner.classes.join(" ")}`);
      assert.equal(banner.role, "img", `banner ${at + 1}'s icon`);
      assert.equal(banner.name, row.name, `banner ${at + 1}'s icon`);
      assert.notEqual(banner.shape, "", `banner ${at + 1}'s icon draws nothing`);
      rows.forEach((other, at2) => {
        const same = drawn[at2]?.shape === banner.shape;
        assert.equal(same, other.tone === row.tone, `banners ${at + 1} and ${at2 + 1}: one shape per tone`);
      });
    });
  },
);

Then(
  "the {string} demo shows no banner with the words {string}",
  async function (this: BrowserWorld, component: string, words: string) {
    await expect(banners(this.page, component).filter({ hasText: words })).toHaveCount(0);
  },
);

Then(
  "the {string} demo shows a banner with the words {string}",
  async function (this: BrowserWorld, component: string, words: string) {
    const banner = banners(this.page, component).filter({ hasText: words });
    await expect(banner).toHaveCount(1);
    await expect(banner).toBeVisible();
  },
);

// After a load, the demo's other banners show first, so the one a dismissal
// keeps away is known to be missing, not yet to come.
Then(
  "the {string} demo shows {int} banners, none with the words {string}",
  async function (this: BrowserWorld, component: string, count: number, words: string) {
    await eachOf(banners(this.page, component), count);
    await expect(banners(this.page, component).filter({ hasText: words })).toHaveCount(0);
  },
);

// Look (§4.5): a square strip of Ledger Gray across the content, the words
// in the text's color, so a warning is not a colored banner (DESIGN.md), and
// the tone in the icon and in a thick edge at the start of the line, both of
// which the contrast gate's tone pairs hold to 4.5:1 on Ledger Gray.
Then(
  "every banner of the {string} demo lies square on {word} with its words in {word}, its icon and its {float} rem start edge in its tone's color",
  async function (this: BrowserWorld, component: string, surface: string, words: string, edge: number) {
    await eachOf(banners(this.page, component), BANNERS);
    const drawn = await demoSection(this.page, component).evaluate(
      (section, tokens) => {
        const resolve = (token: string) => {
          const probe = document.createElement("span");
          probe.style.color = `var(${token})`;
          document.body.append(probe);
          const color = getComputedStyle(probe).color;
          probe.remove();
          return color;
        };
        return {
          rem: Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
          surface: resolve(tokens.surface),
          words: resolve(tokens.words),
          tones: Object.fromEntries(Object.entries(tokens.tones).map(([tone, token]) => [tone, resolve(token)])),
          banners: Array.from(section.querySelectorAll(".banner"), (banner) => {
            const style = getComputedStyle(banner);
            const icon = banner.querySelector(".banner-icon svg");
            const text = banner.querySelector(".banner-words");
            return {
              tone: [...banner.classList].find((name) => name.startsWith("tone-"))?.slice("tone-".length) ?? "",
              background: style.backgroundColor,
              corners: [
                style.borderStartStartRadius,
                style.borderStartEndRadius,
                style.borderEndStartRadius,
                style.borderEndEndRadius,
              ].map(Number.parseFloat),
              edgeWidth: Number.parseFloat(style.borderInlineStartWidth),
              edgeStyle: style.borderInlineStartStyle,
              edgeColor: style.borderInlineStartColor,
              otherEdges: [style.borderBlockStartWidth, style.borderInlineEndWidth, style.borderBlockEndWidth].map(Number.parseFloat),
              icon: icon === null ? "" : getComputedStyle(icon).stroke,
              words: text === null ? "" : getComputedStyle(text).color,
            };
          }),
        };
      },
      { surface, words, tones: TONE_TOKENS },
    );
    assert.match(drawn.surface, /^rgb\(/, `${surface} is ${drawn.surface}, not an opaque color`);
    for (const banner of drawn.banners) {
      const tone = drawn.tones[banner.tone];
      assert.ok(tone !== undefined, `a banner in the tone "${banner.tone}"`);
      assert.equal(banner.background, drawn.surface, `the ${banner.tone} banner's fill`);
      assert.deepEqual(banner.corners, [0, 0, 0, 0], `the ${banner.tone} banner's corners`);
      assert.equal(banner.words, drawn.words, `the ${banner.tone} banner's words`);
      assert.equal(banner.icon, tone, `the ${banner.tone} banner's icon`);
      assert.equal(banner.edgeStyle, "solid", `the ${banner.tone} banner's start edge`);
      assert.equal(banner.edgeColor, tone, `the ${banner.tone} banner's start edge`);
      assert.ok(near(banner.edgeWidth, edge * drawn.rem), `the ${banner.tone} banner's start edge is ${banner.edgeWidth}px wide`);
      assert.deepEqual(banner.otherEdges, [0, 0, 0], `the ${banner.tone} banner's other edges`);
    }
  },
);

// Forced colors (§4.5): the system drops the fill, so an edge all round in
// the system's text color keeps the banner's box in view, its start edge
// still the thick one; the icon takes the color of the banner's text, as
// every tone does, which a forced-colors mode makes CanvasText (a WebKit that
// only reports the media query keeps the author's text color).
Then(
  "every banner of the {string} demo is edged {int} pixel(s) wide in CanvasText, its start edge {float} rem, and its icon in the text's color",
  async function (this: BrowserWorld, component: string, width: number, edge: number) {
    for (const banner of await eachOf(banners(this.page, component), BANNERS)) {
      const drawn = await banner.evaluate((element) => {
        const probe = document.createElement("span");
        probe.style.setProperty("forced-color-adjust", "none");
        probe.style.color = "CanvasText";
        document.body.append(probe);
        const text = getComputedStyle(probe).color;
        probe.remove();
        const style = getComputedStyle(element);
        const start = style.direction === "rtl" ? "right" : "left";
        const icon = element.querySelector(".banner-icon svg");
        return {
          text,
          words: style.color,
          rem: Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
          icon: icon === null ? "" : getComputedStyle(icon).stroke,
          edges: ["top", "right", "bottom", "left"].map((side) => ({
            start: side === start,
            width: Number.parseFloat(style.getPropertyValue(`border-${side}-width`)),
            style: style.getPropertyValue(`border-${side}-style`),
            color: style.getPropertyValue(`border-${side}-color`),
          })),
        };
      });
      assert.equal(drawn.icon, drawn.words, "the icon");
      for (const side of drawn.edges) {
        assert.deepEqual([side.style, side.color], ["solid", drawn.text], JSON.stringify(side));
        const wide = side.start ? edge * drawn.rem : width;
        assert.ok(near(side.width, wide), `an edge ${side.width}px wide, not ${wide}px: ${JSON.stringify(side)}`);
      }
    }
  },
);

// Direction (§4.5, The Logical Rule): the thick edge and the icon stand at
// the start of the line, the left in English and the right in Arabic, and the
// Dismiss button at its end. The demo is found by the id the gallery gives
// it, since a pseudolocale renames its heading.
Then(
  "every banner of the banner demo has its edge on its start side, its icon before its words and its Dismiss button after them, in a page that runs {word} to {word}",
  async function (this: BrowserWorld, from: string, to: string) {
    const direction = from === "right" && to === "left" ? "rtl" : "ltr";
    await eachOf(this.page.locator("#banner-demo .banner"), BANNERS);
    const drawn = await this.page.locator("#banner-demo").evaluate((section) =>
      Array.from(section.querySelectorAll(".banner"), (banner) => {
        const style = getComputedStyle(banner);
        const box = (part: Element | null) => {
          const rect = part?.getBoundingClientRect();
          return rect === undefined ? null : { left: rect.left, right: rect.right };
        };
        return {
          direction: style.direction,
          left: Number.parseFloat(style.borderLeftWidth),
          right: Number.parseFloat(style.borderRightWidth),
          icon: box(banner.querySelector(".banner-icon")),
          words: box(banner.querySelector(".banner-words")),
          dismiss: box(banner.querySelector(".banner-dismiss")),
        };
      }),
    );
    assert.ok(
      drawn.some((banner) => banner.dismiss !== null),
      "no banner of the demo can be dismissed",
    );
    for (const [at, banner] of drawn.entries()) {
      const which = `banner ${at + 1}`;
      assert.equal(banner.direction, direction, which);
      assert.ok(banner.icon !== null && banner.words !== null, `${which} has no icon or no words`);
      if (direction === "ltr") {
        assert.ok(banner.left > banner.right, `${which}'s thick edge is on its right`);
        assert.ok(banner.icon.right <= banner.words.left + ROUNDING_PX, `${which}'s icon is after its words`);
        if (banner.dismiss !== null)
          assert.ok(banner.dismiss.left >= banner.words.right - ROUNDING_PX, `${which}'s Dismiss is before its words`);
      } else {
        assert.ok(banner.right > banner.left, `${which}'s thick edge is on its left`);
        assert.ok(banner.icon.left >= banner.words.right - ROUNDING_PX, `${which}'s icon is after its words`);
        if (banner.dismiss !== null)
          assert.ok(banner.dismiss.right <= banner.words.left + ROUNDING_PX, `${which}'s Dismiss is before its words`);
      }
    }
  },
);

// The 320 Rule (DESIGN.md): on a small screen with larger text, a banner's
// words wrap inside it, a long path too, and its icon and Dismiss button stay
// beside them inside its edge.
Then(
  "every banner of the {string} demo holds its icon, its words and its Dismiss button inside its edge",
  async function (this: BrowserWorld, component: string) {
    for (const banner of await eachOf(banners(this.page, component), BANNERS)) {
      const drawn = await banner.evaluate((element) => {
        // Inside its padding box: an icon drawn over the thick start edge
        // is not inside it.
        const box = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        const outer = {
          left: box.left + parseFloat(style.borderLeftWidth),
          right: box.right - parseFloat(style.borderRightWidth),
          top: box.top + parseFloat(style.borderTopWidth),
          bottom: box.bottom - parseFloat(style.borderBottomWidth),
        };
        const words = element.querySelector(".banner-words");
        return {
          text: element.textContent?.trim() ?? "",
          overflows: words === null ? true : words.scrollWidth > words.clientWidth + 1,
          parts: [".banner-icon", ".banner-words", ".banner-dismiss"].flatMap((selector) => {
            const part = element.querySelector(selector);
            if (part === null) return [];
            const rect = part.getBoundingClientRect();
            return [
              {
                selector,
                inside:
                  rect.left >= outer.left - 0.5 &&
                  rect.right <= outer.right + 0.5 &&
                  rect.top >= outer.top - 0.5 &&
                  rect.bottom <= outer.bottom + 0.5,
              },
            ];
          }),
        };
      });
      assert.equal(drawn.overflows, false, `the words of "${drawn.text}" run out of their room`);
      assert.ok(drawn.parts.length >= 2, `"${drawn.text}" has no icon or no words`);
      for (const part of drawn.parts) assert.ok(part.inside, `${part.selector} of "${drawn.text}" lies outside its banner`);
    }
  },
);

// The tone each banner a page shows is drawn in, by the word a scenario
// names it with: its class, and the word its icon is named by (§4.5).
const PAGE_TONES: Record<string, { tone: string; icon: string }> = {
  information: { tone: "primary", icon: "Information" },
  warning: { tone: "warning", icon: "Warning" },
};

// Words a banner says, among the rest of its words: fetched text in them
// may carry bidi isolation marks (§4.5).
const saying = (words: string): RegExp =>
  new RegExp([...words].map((ch) => ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(ISOLATION));

// A page's notice is the shared banner (#930): found by its words, as a
// banner has no role, it is one banner in the tone named, its icon an image
// named for that tone, and its words after it, never a live region itself.
Then("the page shows a(n) {word} banner that says {string}", async function (this: BrowserWorld, tone: string, words: string) {
  const expected = PAGE_TONES[tone];
  assert.ok(expected, `no banner tone "${tone}"`);
  const banner = this.page.locator(".banner").filter({ hasText: saying(words) });
  await expect(banner).toHaveCount(1);
  await expect(banner).toHaveClass(new RegExp(`(^|\\s)tone-${expected.tone}(\\s|$)`));
  await expect(banner.locator(".banner-icon").getByRole("img", { name: namePattern(expected.icon) })).toHaveCount(1);
  await expect(banner.locator(".banner-words")).toContainText(saying(words));
  await expect(banner).not.toHaveAttribute("role", /.+/);
  await expect(banner).not.toHaveAttribute("aria-live", /.+/);
});
