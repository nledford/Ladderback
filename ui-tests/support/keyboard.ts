// The only way journey steps touch the page: key presses, the focused
// element, and role locators (A1, A2). `npm run lint` rejects pointer
// actions in steps/, so a journey can't pass by clicking.

import assert from "node:assert/strict";

import type { Locator, Page } from "playwright";

import { expect } from "./expect.ts";
import { type Box, focusProbes, type Point } from "./focus-probes.ts";

/** Playwright's name for the engine that drives `page`: `chromium`, `firefox`
 *  or `webkit`. It comes from the page, not from `world.ts`'s `engine`: only
 *  cucumber-js can load `world.ts`, and the check's own browser test
 *  (`focus-check.browser.test.ts`) runs this file under `node --test`. */
function engineOf(page: Page): string {
  const browser = page.context().browser();
  if (browser === null) throw new Error("the page's context has no browser, so its engine is unknown");
  return browser.browserType().name();
}

// WebKit on macOS, as Playwright drives it, scrolls neither the page nor a
// focused frame on a bare arrow key, though it does on Page Down and on
// Option+arrow, which scrolls by most of the frame's width. WebKit on Linux,
// where CI runs, scrolls on the bare arrows, as Chromium and Firefox do.
export const scrollKey = (page: Page, key: string): string =>
  engineOf(page) === "webkit" && process.platform === "darwin" ? `Alt+${key}` : key;

// A name the page built from fetched text carries bidi isolation marks
// around the fetched part (§4.5): the button "Cancel ⁨demo-home⁩" is the
// journey's "Cancel demo-home". The marks are optional wherever they may
// stand, and the anchors keep the match whole.
export const ISOLATION = "[\\u2066-\\u2069]*";
export const namePattern = (name: string): RegExp =>
  new RegExp(`^${[...name].map((ch) => ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(ISOLATION)}${ISOLATION}$`);

export function journey(page: Page) {
  // WebKit on macOS moves Tab only between text fields and lists, as Safari
  // does until its reader turns on "Press Tab to highlight each item";
  // Option+Tab moves to every control there. WebKit on Linux, where CI runs,
  // moves Tab to every control.
  const tabModifier = engineOf(page) === "webkit" && process.platform === "darwin" ? "Alt+" : "";
  return {
    async press(key: string): Promise<void> {
      await page.keyboard.press(key === "Tab" || key === "Shift+Tab" ? tabModifier + key : key);
      if (key === "Tab" || key === "Shift+Tab") {
        await assertFocusVisibleAndUnobscured(page, key);
      }
    },

    /** Types `text` into the focused element, key by key. */
    async type(text: string): Promise<void> {
      await page.keyboard.type(text);
    },

    /** The elements with ARIA role `role`. A step's role comes from the
     *  scenario's text, and Playwright refuses a role it doesn't know. */
    byRole(role: string, options?: Parameters<Page["getByRole"]>[1]): Locator {
      return page.getByRole(role as Parameters<Page["getByRole"]>[0], options);
    },

    /** Asserts that `locator` resolves to the focused element. */
    async assertFocused(locator: Locator, description: string): Promise<void> {
      const focused = await locator.evaluate((el) => el === document.activeElement);
      assert.ok(focused, `expected focus on ${description}`);
    },
  };
}

// The most presses a walk makes before it gives up. A walk on a page's
// content starts in `<main>`, which the skip link moves focus to ("I skip to
// the content"), so the limit counts stops within `<main>` only: a link added
// to the header uses up no walk's presses but the header's own. A page with
// more stops than this is crossed in stretches: a scenario first stops at a
// named control partway down, then presses on from there. An arrow walk
// through a real snapshot's tree takes two presses a folder from its root, so
// a scratch source under a deep temporary folder uses up more of it.
export const MAX_PRESSES = 30;

// The pages whose clock a step has stopped ("the page's clock stands still
// until a step moves it on"). There the page's own timers and frames run only
// when a step moves the clock on, so the focus check can't wait on them in
// WebKit; it waits instead for the browser to bring focus into view, through
// Playwright's own check, which keeps real time, until the step's limit
// (#1104). An element still out of view then fails as ever.
const stoppedClocks = new WeakSet<Page>();

/** Records that `page`'s clock now stands still until a step moves it on. */
export function clockStandsStill(page: Page): void {
  stoppedClocks.add(page);
}

/** Where focus is, as the end of a sentence. */
export function whereFocusIs(page: Page): Promise<string> {
  return page.evaluate(() => {
    const el = document.activeElement;
    return el === null || el === document.body
      ? "on the page body"
      : `on <${el.localName}> ${JSON.stringify((el.textContent ?? "").trim().slice(0, 40))}`;
  });
}

/** Presses `key` until focus is on the one element `target` finds, or, given
 *  `reached`, until it says so. Each press but one to the header itself must
 *  stay out of the header and the skip link: a walk on the page's content
 *  starts after them, so that a change to the header reaches no other walk.
 *  `afterEach` runs after every press, for a page that changes as it is
 *  walked. */
export async function pressUntilFocused(
  page: Page,
  key: string,
  target: Locator,
  description: string,
  { reached, afterEach }: { reached?: () => Promise<boolean>; afterEach?: () => Promise<void> } = {},
): Promise<void> {
  const keys = journey(page);
  const focused =
    reached ?? (async () => (await target.count()) === 1 && (await target.evaluate((el) => el === document.activeElement)));
  for (let presses = 0; presses < MAX_PRESSES && !(await focused()); presses += 1) {
    await keys.press(key);
    await afterEach?.();
    if ((await focusIsAboveTheContent(page)) && !(await isAboveTheContent(target))) {
      assert.fail(
        `the walk to ${description} crossed the header, ${await whereFocusIs(page)}: ` +
          'a walk on the page\'s content starts after it, with "I skip to the content"',
      );
    }
  }
  assert.ok(await focused(), `focus did not reach ${description} within ${MAX_PRESSES} ${key} presses`);
}

/** Whether focus is on the skip link or in the header: outside `<main>` and
 *  outside any dialog, which the page opens beside `<main>`. */
function focusIsAboveTheContent(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const el = document.activeElement;
    const main = document.querySelector("main");
    return (
      el !== null &&
      el !== document.body &&
      main !== null &&
      !main.contains(el) &&
      el.closest("dialog, [role=dialog], [role=alertdialog]") === null
    );
  });
}

/** Whether every element `target` finds, and at least one, is outside `<main>`:
 *  a walk to the header's own controls crosses the header. */
function isAboveTheContent(target: Locator): Promise<boolean> {
  return target.evaluateAll((els) => {
    const main = document.querySelector("main");
    return els.length > 0 && main !== null && els.every((el) => !main.contains(el));
  });
}

// WCAG 2.4.7 and 2.4.13: the focused element shows an indicator that is at
// least 2 px thick, differs from the unfocused look and has 3:1 contrast
// against what it sits on (1.4.11). WCAG 2.4.11: it is at least partly in
// sight, checked at the center of each line it stands on (`focusProbes`):
// what lies over every one of them covers it.
async function assertFocusVisibleAndUnobscured(page: Page, key: string): Promise<void> {
  const state = await lookAtFocus(page, key);
  assert.ok(state.restored, `could not restore focus to ${state.name}`);
  assert.ok(
    state.unobscured,
    `focused element is covered: ${state.name} (its lines' centers find ${state.found || "nothing, out of view"})`,
  );

  const { focusedLook: now, unfocusedLook: before } = state;
  const indicators: { kind: string; color: Color }[] = [];
  const outlineColor = parseColor(now.outlineColor);
  if (now.outlineStyle !== "none" && parseFloat(now.outlineWidth) >= 2 && outlineColor.a > 0) {
    indicators.push({ kind: "outline", color: outlineColor });
  }
  const shadowColor = firstVisibleColor(now.boxShadow);
  if (now.boxShadow !== "none" && shadowColor) {
    indicators.push({ kind: "box-shadow", color: shadowColor });
  }
  assert.ok(
    indicators.length > 0,
    `no visible focus indicator on ${state.name}: outline is ${now.outlineStyle} ${now.outlineWidth} ${now.outlineColor}, box-shadow is ${now.boxShadow} (needs an outline of 2px or more, or a shadow, in a visible color)`,
  );
  assert.notDeepEqual(now, before, `the focus indicator on ${state.name} looks the same as when it isn't focused`);

  const background = composite(state.focusedBackgrounds);
  const ratios = indicators.map((i) => ({
    kind: i.kind,
    ratio: contrastRatio(composite([cssRgba(i.color), ...state.focusedBackgrounds]), background),
  }));
  assert.ok(
    ratios.some((r) => r.ratio >= 3),
    `focus indicator on ${state.name} has less than 3:1 contrast with its background: ${ratios
      .map((r) => `${r.kind} ${r.ratio.toFixed(2)}:1`)
      .join(", ")}`,
  );
}

// How many times the check looks for the focused element before it gives up
// on a page that keeps moving it. A page that draws something new above the
// element, as when a skeleton gives way to its table, moves it once; the
// looks after that are for a page that draws it in several steps.
const LOOKS = 5;

/** The focused element's look, focused and not, and whether any of
 *  `focusProbes`' points finds it. The page is read at least twice, the
 *  points worked out between the reads; every read reads the one element
 *  focused after `key`, and the check fails if focus has left it in between.
 *  The points are worked out from a measure of the element, and the read that
 *  looks at them measures it again first: if it moved in between, as when the
 *  page draws something above it, the points would find what took its place,
 *  so the check works them out again from the new measure and looks again,
 *  and fails if the element has moved before each of `LOOKS` looks. */
async function lookAtFocus(page: Page, key: string) {
  const engine = engineOf(page);
  // WebKit scrolls a newly focused element into view from a timer that the
  // focus starts with no delay, not inside the key press as Chromium and
  // Firefox do, and a busy page can draw frames before that timer runs: two
  // frames alone left the gallery's progress bar out of view on CI (#973).
  // WebKit runs due timers in the order they fell due, so a timeout of 0 set
  // after the press runs after that scroll; the check waits for one, then for
  // two frames, to see the page as its reader does.
  const revealsFocusFromATimer = engine === "webkit";
  // Firefox scrolls a focused element only until part of it shows, Chromium
  // and WebKit until all of it does: on a small screen with larger text the
  // element's center can lie past the viewport's edge, where nothing covers
  // it. There the check looks at the part of it in view; an element wholly
  // out of view still fails it.
  const revealsFocusInPart = engine === "firefox";
  const stopped = revealsFocusFromATimer && stoppedClocks.has(page);
  if (stopped) {
    // Its failure is only the end of the wait: the measure below is the check.
    // The `expect` of the steps waits until the step's limit.
    await expect(page.locator(":focus"))
      .toBeInViewport()
      .catch(() => undefined);
  }
  const focused = await page.evaluateHandle(async (fromATimer) => {
    if (fromATimer) {
      await new Promise((done) => setTimeout(done, 0));
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    }
    const el = document.activeElement;
    return el === null || el === document.body || el === document.documentElement ? null : el;
  }, revealsFocusFromATimer && !stopped);
  try {
    let read = await focused.evaluate(readFocus, null);
    for (let looks = 0; read.focus === "measured"; looks += 1) {
      assert.ok(
        looks < LOOKS,
        `after ${key} the focused element kept moving while the check looked at it, ${LOOKS} times: ${read.name}`,
      );
      const { fragments, whole, viewport } = read.measure;
      const probes = focusProbes(fragments, whole, viewport, revealsFocusInPart);
      read = await focused.evaluate(readFocus, { probes, measure: read.measure });
    }
    assert.ok(read.focus !== "none", `after ${key} focus is on the page body or nowhere, so a keyboard user has no focus to see`);
    assert.ok(read.focus !== "left", `after ${key} focus moved while the check looked at it, ${await whereFocusIs(page)}`);
    return read.look;
  } finally {
    await focused.dispose();
  }
}

/** Where the focused element stands: its boxes, one for each line, and the
 *  box around them all, in a viewport of this size. */
interface Measure {
  fragments: Box[];
  whole: Box;
  viewport: { width: number; height: number };
}

/** One read of the page by `lookAtFocus`, in the page, of the element `el` it
 *  found focused: none, focus gone from it, its measure (when `probing` is
 *  null, or when it no longer stands where `probing` measured it), or its
 *  look, found or not by `probing`'s points. */
function readFocus(el: Element | null, probing: { probes: Point[]; measure: Measure } | null) {
  if (el === null) return { focus: "none" } as const;
  if (document.activeElement !== el) return { focus: "left" } as const;
  const box = ({ left, top, right, bottom }: DOMRect): Box => ({ left, top, right, bottom });
  const measure: Measure = {
    fragments: [...el.getClientRects()].map(box),
    whole: box(el.getBoundingClientRect()),
    viewport: { width: document.documentElement.clientWidth, height: document.documentElement.clientHeight },
  };
  const name = el.outerHTML.slice(0, 80);
  if (probing === null || JSON.stringify(measure) !== JSON.stringify(probing.measure)) {
    return { focus: "measured", measure, name } as const;
  }
  // HTML, SVG and MathML elements, the ones that take focus, can all blur
  // and focus again (the HTMLOrSVGElement mixin).
  const focusable = el as Element & HTMLOrSVGElement;
  const look = () => {
    const s = getComputedStyle(focusable);
    return {
      outlineStyle: s.outlineStyle,
      outlineWidth: s.outlineWidth,
      outlineColor: s.outlineColor,
      outlineOffset: s.outlineOffset,
      boxShadow: s.boxShadow,
    };
  };
  // Layers a ring can sit on, nearest first. A ring outside the element
  // sits on the parent's background; one drawn inward sits on the element.
  const backgrounds = (from: Element | null) => {
    const layers: string[] = [];
    for (let node = from; node; node = node.parentElement) {
      layers.push(getComputedStyle(node).backgroundColor);
    }
    return layers;
  };
  const focusedLook = look();
  const hits = probing.probes.map(({ x, y }) => document.elementFromPoint(x, y));
  const inward = parseFloat(focusedLook.outlineOffset) < 0;
  const focusedBackgrounds = backgrounds(inward ? focusable : focusable.parentElement);
  const unobscured = hits.some((hit) => hit !== null && (hit === focusable || focusable.contains(hit)));
  const found = hits.map((hit) => (hit === null ? "nothing, out of view" : `<${hit.localName}>`)).join(", ");

  // Compare with the same element unfocused, then put focus back.
  focusable.blur();
  const unfocusedLook = look();
  focusable.focus({ focusVisible: true });
  return {
    focus: "seen",
    look: {
      name,
      unobscured,
      found,
      focusedLook,
      unfocusedLook,
      focusedBackgrounds,
      restored: document.activeElement === focusable,
    },
  } as const;
}

const COLOR = /rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+%?))?\s*\)/g;

interface Rgb {
  r: number;
  g: number;
  b: number;
}
interface Color extends Rgb {
  a: number;
}

function toColor(match: RegExpMatchArray): Color {
  const alpha = match[4] === undefined ? 1 : match[4].endsWith("%") ? parseFloat(match[4]) / 100 : parseFloat(match[4]);
  return { r: Number(match[1]), g: Number(match[2]), b: Number(match[3]), a: alpha };
}

function parseColor(text: string): Color {
  const match = [...text.matchAll(COLOR)][0];
  assert.ok(match, `cannot read the color "${text}"; the focus check understands rgb() and rgba()`);
  return toColor(match);
}

/** The first color in a box-shadow list that isn't fully transparent. */
function firstVisibleColor(boxShadow: string): Color | undefined {
  return [...boxShadow.matchAll(COLOR)].map(toColor).find((c) => c.a > 0);
}

function cssRgba({ r, g, b, a }: Color): string {
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

/** Blends CSS colors, nearest layer first, over an opaque white canvas. */
export function composite(layers: string[]): Rgb {
  let out: Rgb = { r: 255, g: 255, b: 255 };
  for (const text of [...layers].reverse()) {
    const c = parseColor(text);
    out = {
      r: c.r * c.a + out.r * (1 - c.a),
      g: c.g * c.a + out.g * (1 - c.a),
      b: c.b * c.a + out.b * (1 - c.a),
    };
  }
  return out;
}

function luminance({ r, g, b }: Rgb): number {
  const [lr, lg, lb] = [r, g, b].map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  }) as [number, number, number];
  return 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
}

export function contrastRatio(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}
