// Journey steps for the shared alert in the gallery (technical design §4.5):
// an inline message about a page's state, an icon that names its tone in
// words, then the words. An alert has no role, so it has no name to find it
// by: it is found by its class inside its demo, which is found by its
// heading, or by its id under a pseudolocale, as a skeleton is. Its Dismiss
// button is found by role and name, by keyboard alone.

import assert from "node:assert/strict";

import type { DataTable } from "@cucumber/cucumber";
import { Then } from "@cucumber/cucumber";
import { expect } from "playwright/test";
import type { Locator, Page } from "playwright";

import type { BrowserWorld } from "../support/world.ts";
import { demoSection, eachOf } from "./components.ts";

// The gallery's alerts as it opens (`gallery.rs`): one in each tone, then
// the notice the reader can dismiss.
const ALERTS = 5;

// Half a pixel of rounding in a laid-out length.
const ROUNDING_PX = 0.5;

// The token each tone draws its icon and its edge in (`style.css`).
const TONE_TOKENS: Record<string, string> = {
  primary: "--accent",
  success: "--success",
  warning: "--warning",
  error: "--error",
};

const alerts = (page: Page, component: string): Locator => demoSection(page, component).locator(".alert");

const near = (a: number, b: number, slack = ROUNDING_PX) => Math.abs(a - b) <= slack;

// Not a live region (§4.5): urgent text goes through the announcer, so an
// alert carries no live role and no `aria-live`, nor does anything inside
// it; and it is no stop for Tab itself.
Then("no alert of the {string} demo is a live region, nor holds one", async function (this: BrowserWorld, component: string) {
  for (const alert of await eachOf(alerts(this.page, component), ALERTS)) {
    const found = await alert.evaluate((element) => {
      const parts = [element, ...element.querySelectorAll("*")];
      return {
        role: element.getAttribute("role"),
        tabindex: element.getAttribute("tabindex"),
        live: parts.filter(
          (part) =>
            part.hasAttribute("aria-live") ||
            ["alert", "status", "log", "marquee", "timer"].includes(part.getAttribute("role") ?? ""),
        ).length,
      };
    });
    assert.deepEqual(found, { role: null, tabindex: null, live: 0 });
  }
});

// Never color alone (1.4.1): each alert's icon is an image named for its
// tone, which a screen reader hears before its words, and its shape is its
// tone's own.
Then(
  "the alerts of the {string} demo show these icons, each named for its tone, one shape per tone, in this order:",
  async function (this: BrowserWorld, component: string, table: DataTable) {
    const rows = table.hashes() as { tone: string; name: string }[];
    await eachOf(alerts(this.page, component), rows.length);
    const drawn = await demoSection(this.page, component).evaluate((section) =>
      Array.from(section.querySelectorAll(".alert"), (alert) => {
        const icon = alert.querySelector(".alert-icon svg");
        return {
          classes: [...alert.classList],
          role: icon?.getAttribute("role") ?? null,
          name: icon?.getAttribute("aria-label") ?? null,
          shape: icon?.innerHTML ?? "",
        };
      }),
    );
    rows.forEach((row, at) => {
      const alert = drawn[at];
      assert.ok(alert !== undefined, `no alert ${at + 1}`);
      assert.ok(alert.classes.includes(`tone-${row.tone}`), `alert ${at + 1} is ${alert.classes.join(" ")}`);
      assert.equal(alert.role, "img", `alert ${at + 1}'s icon`);
      assert.equal(alert.name, row.name, `alert ${at + 1}'s icon`);
      assert.notEqual(alert.shape, "", `alert ${at + 1}'s icon draws nothing`);
      rows.forEach((other, at2) => {
        const same = drawn[at2]?.shape === alert.shape;
        assert.equal(same, other.tone === row.tone, `alerts ${at + 1} and ${at2 + 1}: one shape per tone`);
      });
    });
  },
);

Then(
  "the {string} demo shows no alert with the words {string}",
  async function (this: BrowserWorld, component: string, words: string) {
    await expect(alerts(this.page, component).filter({ hasText: words })).toHaveCount(0);
  },
);

Then(
  "the {string} demo shows an alert with the words {string}",
  async function (this: BrowserWorld, component: string, words: string) {
    const alert = alerts(this.page, component).filter({ hasText: words });
    await expect(alert).toHaveCount(1);
    await expect(alert).toBeVisible();
  },
);

// Look (§4.5): Ledger Gray, the words and any link in them in the text's
// color, so a failure is not a red banner (DESIGN.md), and the tone in the
// icon and in a thick edge at the start of the line, both of which the
// contrast gate's tone pairs hold to 4.5:1 on Ledger Gray.
Then(
  "every alert of the {string} demo lies on {word} with its words in {word}, its icon and its {float} rem start edge in its tone's color",
  async function (this: BrowserWorld, component: string, surface: string, words: string, edge: number) {
    await eachOf(alerts(this.page, component), ALERTS);
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
          alerts: Array.from(section.querySelectorAll(".alert"), (alert) => {
            const style = getComputedStyle(alert);
            const icon = alert.querySelector(".alert-icon svg");
            const text = alert.querySelector(".alert-words");
            return {
              tone: [...alert.classList].find((name) => name.startsWith("tone-"))?.slice("tone-".length) ?? "",
              background: style.backgroundColor,
              edgeWidth: Number.parseFloat(style.borderInlineStartWidth),
              edgeStyle: style.borderInlineStartStyle,
              edgeColor: style.borderInlineStartColor,
              otherEdges: [style.borderBlockStartWidth, style.borderInlineEndWidth, style.borderBlockEndWidth].map(Number.parseFloat),
              icon: icon === null ? "" : getComputedStyle(icon).stroke,
              words: text === null ? "" : getComputedStyle(text).color,
              links: Array.from(alert.querySelectorAll(".alert-words a"), (link) => getComputedStyle(link).color),
            };
          }),
        };
      },
      { surface, words, tones: TONE_TOKENS },
    );
    assert.match(drawn.surface, /^rgb\(/, `${surface} is ${drawn.surface}, not an opaque color`);
    assert.ok(drawn.alerts.some((alert) => alert.links.length > 0), "no alert of the demo holds a link");
    for (const alert of drawn.alerts) {
      const tone = drawn.tones[alert.tone];
      assert.ok(tone !== undefined, `an alert in the tone "${alert.tone}"`);
      assert.equal(alert.background, drawn.surface, `the ${alert.tone} alert's fill`);
      assert.equal(alert.words, drawn.words, `the ${alert.tone} alert's words`);
      for (const link of alert.links) assert.equal(link, drawn.words, `a link in the ${alert.tone} alert`);
      assert.equal(alert.icon, tone, `the ${alert.tone} alert's icon`);
      assert.equal(alert.edgeStyle, "solid", `the ${alert.tone} alert's start edge`);
      assert.equal(alert.edgeColor, tone, `the ${alert.tone} alert's start edge`);
      assert.ok(near(alert.edgeWidth, edge * drawn.rem), `the ${alert.tone} alert's start edge is ${alert.edgeWidth}px wide`);
      assert.deepEqual(alert.otherEdges, [0, 0, 0], `the ${alert.tone} alert's other edges`);
    }
  },
);

// Forced colors (§4.5): the system drops the fill, so an edge all round in
// the system's text color keeps the alert's box in view, its start edge
// still the thick one; the icon takes the color of the alert's text, as
// every tone does, which a forced-colors mode makes CanvasText (a WebKit that
// only reports the media query keeps the author's text color).
Then(
  "every alert of the {string} demo is edged {int} pixel(s) wide in CanvasText, its start edge {float} rem, and its icon in the text's color",
  async function (this: BrowserWorld, component: string, width: number, edge: number) {
    for (const alert of await eachOf(alerts(this.page, component), ALERTS)) {
      const drawn = await alert.evaluate((element) => {
        const probe = document.createElement("span");
        probe.style.setProperty("forced-color-adjust", "none");
        probe.style.color = "CanvasText";
        document.body.append(probe);
        const text = getComputedStyle(probe).color;
        probe.remove();
        const style = getComputedStyle(element);
        const start = style.direction === "rtl" ? "right" : "left";
        const icon = element.querySelector(".alert-icon svg");
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
  "every alert of the alert demo has its edge on its start side, its icon before its words and its Dismiss button after them, in a page that runs {word} to {word}",
  async function (this: BrowserWorld, from: string, to: string) {
    const direction = from === "right" && to === "left" ? "rtl" : "ltr";
    const found = this.page.locator("#alert-demo .alert");
    await eachOf(found, ALERTS);
    const drawn = await this.page.locator("#alert-demo").evaluate((section) =>
      Array.from(section.querySelectorAll(".alert"), (alert) => {
        const style = getComputedStyle(alert);
        const box = (part: Element | null) => {
          const rect = part?.getBoundingClientRect();
          return rect === undefined ? null : { left: rect.left, right: rect.right };
        };
        return {
          direction: style.direction,
          left: Number.parseFloat(style.borderLeftWidth),
          right: Number.parseFloat(style.borderRightWidth),
          icon: box(alert.querySelector(".alert-icon")),
          words: box(alert.querySelector(".alert-words")),
          dismiss: box(alert.querySelector(".alert-dismiss")),
        };
      }),
    );
    assert.ok(drawn.some((alert) => alert.dismiss !== null), "no alert of the demo can be dismissed");
    for (const [at, alert] of drawn.entries()) {
      const which = `alert ${at + 1}`;
      assert.equal(alert.direction, direction, which);
      assert.ok(alert.icon !== null && alert.words !== null, `${which} has no icon or no words`);
      if (direction === "ltr") {
        assert.ok(alert.left > alert.right, `${which}'s thick edge is on its right`);
        assert.ok(alert.icon.right <= alert.words.left + ROUNDING_PX, `${which}'s icon is after its words`);
        if (alert.dismiss !== null) assert.ok(alert.dismiss.left >= alert.words.right - ROUNDING_PX, `${which}'s Dismiss is before its words`);
      } else {
        assert.ok(alert.right > alert.left, `${which}'s thick edge is on its left`);
        assert.ok(alert.icon.left >= alert.words.right - ROUNDING_PX, `${which}'s icon is after its words`);
        if (alert.dismiss !== null) assert.ok(alert.dismiss.right <= alert.words.left + ROUNDING_PX, `${which}'s Dismiss is before its words`);
      }
    }
  },
);

// The 320 Rule (DESIGN.md): on a small screen with larger text, an alert's
// words wrap inside it, a long path too, and its icon and Dismiss button stay
// beside them inside its edge.
Then(
  "every alert of the {string} demo holds its icon, its words and its Dismiss button inside its edge",
  async function (this: BrowserWorld, component: string) {
    for (const alert of await eachOf(alerts(this.page, component), ALERTS)) {
      const drawn = await alert.evaluate((element) => {
        const outer = element.getBoundingClientRect();
        const words = element.querySelector(".alert-words");
        return {
          text: element.textContent?.trim() ?? "",
          overflows: words === null ? true : words.scrollWidth > words.clientWidth + 1,
          parts: [".alert-icon", ".alert-words", ".alert-dismiss"].flatMap((selector) => {
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
      for (const part of drawn.parts) assert.ok(part.inside, `${part.selector} of "${drawn.text}" lies outside its alert`);
    }
  },
);
