// Journey steps for the shared status badge in the gallery (technical design
// §4.5): its icon, its tone's pair, its stillness and its icon's size.

import assert from "node:assert/strict";

import { type DataTable, Then } from "@cucumber/cucumber";

import { patienceMs } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";
import { demoSection, drawnOnItsFill, wholly } from "../support/gallery.ts";

// A status badge's icon (technical design §4.5) is decoration beside its
// text, so a screen reader reads the label once and Tab never stops on it; it
// is drawn in the text's color, so the contrast gate's badge pairs cover it;
// and its shape is its state's own, so the state reads without the color.
Then(
  "each of the {int} badges of the {string} demo has an icon of its own, in its text's color, which a screen reader skips",
  async function (this: BrowserWorld, count: number, component: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: patienceMs() });
    const badges = await section.evaluate((element) =>
      Array.from(element.querySelectorAll(".badge"), (badge) => {
        const icon = badge.querySelector("svg");
        return {
          text: badge.textContent?.trim() ?? "",
          hidden: icon?.getAttribute("aria-hidden") ?? null,
          focusable: icon?.getAttribute("focusable") ?? null,
          stroke: icon === null ? "" : getComputedStyle(icon).stroke,
          color: getComputedStyle(badge).color,
          shape: icon?.innerHTML ?? "",
        };
      }),
    );
    assert.equal(badges.length, count);
    for (const badge of badges) {
      assert.equal(badge.hidden, "true", badge.text);
      assert.equal(badge.focusable, "false", badge.text);
      assert.equal(badge.stroke, badge.color, badge.text);
      assert.notEqual(badge.shape, "", badge.text);
    }
    assert.equal(new Set(badges.map((badge) => badge.shape)).size, count, "every state its own shape");
  },
);

// A status badge (§4.5) whose operation ended draws its words and its pill in
// the pair of its status tone, found by its class and its words, the whole of
// them ("Succeeded" is never "Succeeded with warnings"): the pill, not the
// words inside it, which lie on its fill.
Then(
  "each of these badges of the {string} demo is drawn on its fill:",
  async function (this: BrowserWorld, component: string, badges: DataTable) {
    const section = demoSection(this.page, component);
    for (const { badge, words, fill } of badges.hashes() as { badge: string; words: string; fill: string }[]) {
      const pill = section.locator(".badge").filter({ hasText: wholly(badge) });
      await drawnOnItsFill(pill, badge, words, fill);
    }
  },
);

// No badge moves (technical design §4.5): a running operation can run for
// hours while the page stays in use, and nothing on the page could stop a
// turning icon or a pulsing pill (WCAG 2.2.2), so the visible text tells the
// state. A badge's count is every animation the browser runs on its pill or
// inside it (CSS animations and transitions, the Web Animations API) and
// every SVG animation element it holds. A badge without an icon fails first,
// so the check can't pass on a badge with nothing in it to move.
Then("none of the {int} badges of the {string} demo moves", async function (this: BrowserWorld, count: number, component: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: patienceMs() });
  const badges = await section.evaluate((element) =>
    Array.from(element.querySelectorAll(".badge"), (badge) => ({
      text: badge.textContent?.trim() ?? "",
      icon: badge.querySelector("svg") !== null,
      motions:
        badge.getAnimations({ subtree: true }).length + badge.querySelectorAll("animate, animateMotion, animateTransform, set").length,
    })),
  );
  assert.equal(badges.length, count);
  for (const badge of badges) {
    assert.ok(badge.icon, `the badge "${badge.text}" has no icon`);
    assert.equal(badge.motions, 0, badge.text);
  }
});

// A badge's icon is as tall as its text, `1em` (technical design §4.5), so it
// grows with the reader's text size (WCAG 1.4.4) rather than staying a small
// mark beside large words. Half a pixel of rounding is allowed.
Then("the icon of each badge of the {string} demo is as tall as its text", async function (this: BrowserWorld, component: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: patienceMs() });
  const badges = await section.evaluate((element) =>
    Array.from(element.querySelectorAll(".badge"), (badge) => ({
      text: badge.textContent?.trim() ?? "",
      font: Number.parseFloat(getComputedStyle(badge).fontSize),
      icon: badge.querySelector("svg")?.getBoundingClientRect().height ?? 0,
    })),
  );
  assert.ok(badges.length > 0, `the ${component} demo has no badge`);
  for (const badge of badges) {
    assert.ok(
      Math.abs(badge.icon - badge.font) < 0.5,
      `the badge "${badge.text}": its icon is ${badge.icon}px tall, its text ${badge.font}px`,
    );
  }
});
