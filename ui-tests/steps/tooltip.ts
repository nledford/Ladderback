// Journey steps for the shared tooltip in the gallery (technical design §4.5):
// when it shows and goes, where it stands, what a screen reader meets, its
// look. Controls are reached by keyboard and found by role and name; a
// tooltip is found by its role and words, or as the one its control names.
// The pointer only moves (support/pointer.ts), for 1.4.13's hover contract.

import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import { untilStepLimit } from "../support/patience.ts";
import { pointer } from "../support/pointer.ts";
import type { BrowserWorld } from "../support/world.ts";

// A tooltip shows once a pointer has rested 300 ms, or at once for focus,
// and goes 150 ms after the pointer leaves: the steps wait for either until
// the step's limit (#1104).

// A tooltip shows only once the pointer has rested this long: a little less
// than its 300 ms, for the time a step takes to start.
const REST_MS = 250;

// Past the grace a tooltip keeps after the pointer leaves it.
const PAST_GRACE_MS = 400;

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** When the pointer came to rest on a control, per page. */
const restedAt = new WeakMap<Page, number>();

/** The one button with this name. */
async function button(page: Page, name: string): Promise<Locator> {
  const found = page.getByRole("button", { name, exact: true });
  await expect(found).toHaveCount(1);
  return found;
}

interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** The button's box and its tooltip's, as the page draws them now. */
interface Drawn {
  open: boolean;
  button: Box;
  tip: Box;
  viewport: { width: number; height: number };
  em: number;
  text: string;
  hidden: string | null;
  fits: { across: boolean; down: boolean };
  style: {
    background: string;
    color: string;
    shadow: string;
    border: string;
    borderColor: string;
    fontSize: number;
    durations: string;
    properties: string;
  };
  page: { canvas: string; canvasText: string; fontSize: number; fg: string; bg: string };
}

/** Reads the button and the tooltip its control names: the one its
 *  `aria-describedby` points at, or for an icon-only control the one beside
 *  it whose words are its name. */
function drawn(found: Locator): Promise<Drawn> {
  return found.evaluate((el) => {
    const described = document.getElementById(el.getAttribute("aria-describedby") ?? "");
    const tip = described ?? el.parentElement?.querySelector(".tooltip");
    if (!tip) throw new Error("the button has no tooltip");
    const box = (r: DOMRect) => ({ left: r.left, top: r.top, right: r.right, bottom: r.bottom });
    const style = getComputedStyle(tip);
    const body = getComputedStyle(document.body);
    // A color as the page resolves it, kept from forced colors' own
    // override, so a token reads as the token and a system color as the
    // system's.
    const probe = (value: string) => {
      const span = document.createElement("span");
      span.style.setProperty("forced-color-adjust", "none");
      span.style.color = value;
      document.body.append(span);
      const color = getComputedStyle(span).color;
      span.remove();
      return color;
    };
    return {
      open: tip.matches(":popover-open"),
      button: box(el.getBoundingClientRect()),
      tip: box(tip.getBoundingClientRect()),
      viewport: { width: document.documentElement.clientWidth, height: document.documentElement.clientHeight },
      em: Number.parseFloat(getComputedStyle(el).fontSize),
      text: (tip.textContent ?? "").trim(),
      hidden: tip.getAttribute("aria-hidden"),
      fits: {
        across: tip.scrollWidth <= tip.clientWidth + 1,
        down: tip.scrollHeight <= tip.clientHeight + 1,
      },
      style: {
        background: style.backgroundColor,
        color: style.color,
        shadow: style.boxShadow,
        border: `${style.borderTopStyle} ${style.borderTopWidth}`,
        borderColor: style.borderTopColor,
        fontSize: Number.parseFloat(style.fontSize),
        durations: style.transitionDuration,
        properties: style.transitionProperty,
      },
      page: {
        canvas: probe("Canvas"),
        canvasText: probe("CanvasText"),
        fontSize: Number.parseFloat(body.fontSize),
        fg: probe("var(--fg)"),
        bg: probe("var(--bg)"),
      },
    };
  });
}

/** The button and its tooltip once the tooltip shows. */
async function shown(page: Page, name: string): Promise<Drawn> {
  const found = await button(page, name);
  return untilStepLimit(
    `the tooltip of the button "${name}" shows`,
    () => drawn(found),
    (now) => now.open,
  );
}

const overlaps = (a: Box, b: Box) => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;

const inside = (box: Box, viewport: Drawn["viewport"]) =>
  box.left >= -0.5 && box.top >= -0.5 && box.right <= viewport.width + 0.5 && box.bottom <= viewport.height + 0.5;

/** Which side of the button the tooltip stands on, in the viewport. */
function sideOf({ button, tip }: Drawn): string | undefined {
  if (tip.bottom <= button.top + 0.5) return "above";
  if (tip.top >= button.bottom - 0.5) return "below";
  if (tip.right <= button.left + 0.5) return "to the left of";
  if (tip.left >= button.right - 0.5) return "to the right of";
  return undefined;
}

/** Whether a tooltip shows anywhere on the page. */
function anyShown(page: Page): Promise<boolean> {
  return page.evaluate(() => [...document.querySelectorAll(".tooltip")].some((tip) => tip.matches(":popover-open")));
}

// 2.4.11: a tooltip never covers the control it belongs to.
Then(
  "the tooltip {string} shows beside the button {string}, without covering it",
  async function (this: BrowserWorld, text: string, name: string) {
    await expect(this.page.getByRole("tooltip", { name: text, exact: true })).toBeVisible();
    const now = await shown(this.page, name);
    assert.equal(now.text, text);
    assert.ok(!overlaps(now.tip, now.button), `the tooltip covers its button: ${JSON.stringify(now)}`);
    assert.ok(inside(now.tip, now.viewport), `the tooltip leaves the window: ${JSON.stringify(now)}`);
  },
);

// The step before took the pointer or focus away, or pressed Escape.
Then("no tooltip shows", async function (this: BrowserWorld) {
  await untilStepLimit(
    "no tooltip shows",
    () => anyShown(this.page),
    (any) => !any,
  );
  await expect(this.page.getByRole("tooltip")).toHaveCount(0);
});

// The control may sit below the window's edge, where a pointer can't reach;
// the page scrolls it into view first, as a reader would.
When("I rest the pointer on the button {string}", async function (this: BrowserWorld, name: string) {
  const found = await button(this.page, name);
  await found.scrollIntoViewIfNeeded();
  await pointer(this.page).restOn(found);
  restedAt.set(this.page, Date.now());
});

// A pointer passing over the page doesn't flash every tooltip on its way:
// nothing shows until the pointer has rested a moment.
Then("the tooltip {string} shows only once the pointer has rested a moment", async function (this: BrowserWorld, text: string) {
  const since = restedAt.get(this.page);
  assert.ok(since !== undefined, "the pointer never rested");
  // The page reads its clock as it looks, so a look the busy machine
  // delays past the 300 ms isn't taken for one made before them.
  for (;;) {
    const look = await this.page.evaluate(() => ({
      at: Date.now(),
      shown: [...document.querySelectorAll(".tooltip")].some((tip) => tip.matches(":popover-open")),
    }));
    if (look.at - since >= REST_MS) break;
    assert.equal(look.shown, false, `a tooltip showed ${look.at - since} ms after the pointer came`);
    await sleep(25);
  }
  await expect(this.page.getByRole("tooltip", { name: text, exact: true })).toBeVisible();
});

// 1.4.13, Hoverable: the pointer can move onto the tooltip, across the gap
// between it and its control, and the tooltip stays.
When("I move the pointer onto the tooltip {string}", async function (this: BrowserWorld, text: string) {
  const tip = this.page.getByRole("tooltip", { name: text, exact: true });
  await expect(tip).toBeVisible();
  await pointer(this.page).restOn(tip);
});

// A look at once, past the grace: a check that the tooltip didn't go, so it
// must not wait for one that went to come back (#1104).
Then("the tooltip {string} still shows a moment later", async function (this: BrowserWorld, text: string) {
  await sleep(PAST_GRACE_MS);
  await expect(this.page.getByRole("tooltip", { name: text, exact: true })).toBeVisible({ timeout: 1 });
});

When("I move the pointer away from the button {string} and its tooltip", async function (this: BrowserWorld, name: string) {
  const now = await shown(this.page, name);
  // The corner of the window farthest from both.
  const x = now.button.left > now.viewport.width / 2 ? 2 : now.viewport.width - 2;
  const y = now.button.top > now.viewport.height / 2 ? 2 : now.viewport.height - 2;
  await pointer(this.page).moveTo(x, y);
});

Then(
  /^the tooltip of the button "([^"]*)" stands (above|below|to the left of|to the right of) it$/,
  async function (this: BrowserWorld, name: string, where: string) {
    const now = await shown(this.page, name);
    assert.equal(sideOf(now), where, JSON.stringify(now));
    assert.ok(inside(now.tip, now.viewport), `the tooltip leaves the window: ${JSON.stringify(now)}`);
  },
);

Then(
  "the tooltip of the button {string} stands above or below it, inside the window",
  async function (this: BrowserWorld, name: string) {
    const now = await shown(this.page, name);
    assert.ok(["above", "below"].includes(sideOf(now) ?? ""), `it stands ${sideOf(now)}: ${JSON.stringify(now)}`);
    assert.ok(inside(now.tip, now.viewport), `the tooltip leaves the window: ${JSON.stringify(now)}`);
  },
);

Then(
  "the tooltip of the button {string} wraps onto lines within the window's width, none of its words cut, without covering the button",
  async function (this: BrowserWorld, name: string) {
    const now = await shown(this.page, name);
    const lineHeight = now.style.fontSize * 1.2;
    assert.ok(now.tip.bottom - now.tip.top > 2 * lineHeight, `the note takes one line: ${JSON.stringify(now)}`);
    assert.ok(
      now.tip.left >= -0.5 && now.tip.right <= now.viewport.width + 0.5,
      `the note is wider than the window: ${JSON.stringify(now)}`,
    );
    assert.ok(now.fits.across && now.fits.down, `the note's words are cut: ${JSON.stringify(now)}`);
    assert.ok(!overlaps(now.tip, now.button), `the note covers its button: ${JSON.stringify(now)}`);
  },
);

// The Reader's Size Rule: a tooltip is sized in em, so it grows with the
// reader's text, a step smaller than the page's, and wraps at 20 em.
Then(
  "the tooltip of the button {string} is set a step smaller than the page's text and wraps at 20 times its text size",
  async function (this: BrowserWorld, name: string) {
    const now = await shown(this.page, name);
    const ratio = now.style.fontSize / now.page.fontSize;
    assert.ok(now.page.fontSize >= 31.5, `the page's text is ${now.page.fontSize}px, not doubled`);
    assert.ok(ratio > 0.8 && ratio < 1, `the note's text is ${now.style.fontSize}px, the page's ${now.page.fontSize}px`);
    const width = now.tip.right - now.tip.left;
    const most = 20 * now.style.fontSize;
    assert.ok(width <= most + 0.5 && width > 0.75 * most, `the note is ${width}px wide, 20 em is ${most}px`);
    assert.ok(now.fits.across && now.fits.down, `the note's words are cut: ${JSON.stringify(now)}`);
  },
);

// An icon-only control is named on the control (its `aria-label`), and its
// tooltip shows that name to a sighted reader; a screen reader skips the
// tooltip, so the name is heard once.
Then(
  "the tooltip of the button {string} shows {string}, which a screen reader skips",
  async function (this: BrowserWorld, name: string, text: string) {
    const now = await shown(this.page, name);
    assert.equal(now.text, text);
    assert.equal(now.hidden, "true", "the tooltip is hidden from a screen reader");
    assert.ok(!overlaps(now.tip, now.button), `the tooltip covers its button: ${JSON.stringify(now)}`);
  },
);

// The look the maintainer chose: the page's colors swapped (the fill is the
// text's token, the words the page's), flat as every surface is.
Then(
  "the tooltip of the button {string} is filled with the page's text color, its words in the page's background color, with no shadow",
  async function (this: BrowserWorld, name: string) {
    const now = await shown(this.page, name);
    assert.equal(now.style.background, now.page.fg, "the fill is --fg");
    assert.equal(now.style.color, now.page.bg, "the words are --bg");
    assert.equal(now.style.shadow, "none");
  },
);

Then(
  "the tooltip of the button {string} is drawn in the page's own colors, edged in its text color",
  async function (this: BrowserWorld, name: string) {
    const now = await shown(this.page, name);
    assert.equal(now.style.background, now.page.canvas, "the fill is Canvas");
    assert.equal(now.style.color, now.page.canvasText, "the words are CanvasText");
    assert.equal(now.style.border, "solid 1px");
    assert.equal(now.style.borderColor, now.page.canvasText, "the edge is CanvasText");
  },
);

/** The duration a computed transition gives `property`, in seconds. */
function durationOf(style: Drawn["style"], property: string): number | undefined {
  const properties = style.properties.split(",").map((p) => p.trim());
  const durations = style.durations.split(",").map((d) => Number.parseFloat(d));
  const at = properties.findIndex((p) => p === property || p === "all");
  return at < 0 ? undefined : durations[at % durations.length];
}

Then("the tooltip of the button {string} fades in over 100 ms", async function (this: BrowserWorld, name: string) {
  const now = await shown(this.page, name);
  assert.equal(durationOf(now.style, "opacity"), 0.1, `${now.style.properties}: ${now.style.durations}`);
});

// Exit faster than entrance (DESIGN.md's motion): once Escape hides it, the
// tooltip fades out over 60 ms, the transition its hidden state gives.
Then("the tooltip of the button {string} fades out over 60 ms", async function (this: BrowserWorld, name: string) {
  const found = await button(this.page, name);
  const now = await untilStepLimit(
    `the tooltip of the button "${name}" hides`,
    () => drawn(found),
    (now) => !now.open,
  );
  assert.equal(durationOf(now.style, "opacity"), 0.06, `${now.style.properties}: ${now.style.durations}`);
});

// The page scrolls a few lines up, focus staying on the button. Not by keys:
// WebKit as Playwright drives it doesn't scroll on the arrow keys, and a
// Page Up carries the button out of the window. Not a pointer action either:
// the page scrolls itself, then the step waits two frames, for the scroll
// events and the tooltip's move.
When("the page scrolls up a few lines", async function (this: BrowserWorld) {
  const [before, after] = await this.page.evaluate(async () => {
    const from = window.scrollY;
    window.scrollBy({ top: -120, behavior: "instant" });
    for (let frames = 0; frames < 2; frames += 1) await new Promise(requestAnimationFrame);
    return [from, window.scrollY];
  });
  assert.ok(after < before, `the page didn't scroll: it was at ${before}px and is at ${after}px`);
});

Then("the tooltip of the button {string} stands beside it, without covering it", async function (this: BrowserWorld, name: string) {
  const now = await shown(this.page, name);
  assert.ok(!overlaps(now.tip, now.button), `the tooltip covers its button: ${JSON.stringify(now)}`);
  const distances: Record<string, number> = {
    above: now.button.top - now.tip.bottom,
    below: now.tip.top - now.button.bottom,
    "to the left of": now.button.left - now.tip.right,
    "to the right of": now.tip.left - now.button.right,
  };
  const away = distances[sideOf(now) ?? "above"] ?? Number.NaN;
  assert.ok(away >= 0 && away <= now.em, `the tooltip is ${away}px from its button: ${JSON.stringify(now)}`);
});
