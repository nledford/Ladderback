// The only way journey steps touch the page: key presses, the focused
// element, and role locators (A1, A2). `npm run lint` rejects pointer
// actions in steps/, so a journey can't pass by clicking.

import assert from "node:assert/strict";

// A name the page built from fetched text carries bidi isolation marks
// around the fetched part (§4.5): the button "Cancel ⁨demo-home⁩" is the
// journey's "Cancel demo-home". The marks are optional wherever they may
// stand, and the anchors keep the match whole.
export const ISOLATION = "[\\u2066-\\u2069]*";
export const namePattern = (name) =>
  new RegExp(
    `^${[...name]
      .map((ch) => ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
      .join(ISOLATION)}${ISOLATION}$`,
  );


export function journey(page) {
  return {
    async press(key) {
      await page.keyboard.press(key);
      if (key === "Tab" || key === "Shift+Tab") {
        await assertFocusVisibleAndUnobscured(page, key);
      }
    },

    /** Types `text` into the focused element, key by key. */
    async type(text) {
      await page.keyboard.type(text);
    },

    byRole(role, options) {
      return page.getByRole(role, options);
    },

    /** Asserts that `locator` resolves to the focused element. */
    async assertFocused(locator, description) {
      const focused = await locator.evaluate((el) => el === document.activeElement);
      assert.ok(focused, `expected focus on ${description}`);
    },
  };
}

// WCAG 2.4.7 and 2.4.13: the focused element shows an indicator that is at
// least 2 px thick, differs from the unfocused look and has 3:1 contrast
// against what it sits on (1.4.11). WCAG 2.4.11: nothing else covers it,
// checked at its center.
async function assertFocusVisibleAndUnobscured(page, key) {
  const state = await page.evaluate(() => {
    const el = document.activeElement;
    if (!el || el === document.body || el === document.documentElement) {
      return { focused: false };
    }
    const look = () => {
      const s = getComputedStyle(el);
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
    const backgrounds = (from) => {
      const layers = [];
      for (let node = from; node; node = node.parentElement) {
        layers.push(getComputedStyle(node).backgroundColor);
      }
      return layers;
    };
    const focusedLook = look();
    const box = el.getBoundingClientRect();
    const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    const inward = parseFloat(focusedLook.outlineOffset) < 0;
    const focusedBackgrounds = backgrounds(inward ? el : el.parentElement);
    const name = el.outerHTML.slice(0, 80);
    const unobscured = hit !== null && (hit === el || el.contains(hit));

    // Compare with the same element unfocused, then put focus back.
    el.blur();
    const unfocusedLook = look();
    el.focus({ focusVisible: true });
    return {
      focused: true,
      name,
      unobscured,
      focusedLook,
      unfocusedLook,
      focusedBackgrounds,
      restored: document.activeElement === el,
    };
  });
  assert.ok(
    state.focused,
    `after ${key} focus is on the page body or nowhere, so a keyboard user has no focus to see`,
  );
  assert.ok(state.restored, `could not restore focus to ${state.name}`);
  assert.ok(state.unobscured, `focused element is covered: ${state.name}`);

  const { focusedLook: now, unfocusedLook: before } = state;
  const indicators = [];
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
  assert.notDeepEqual(
    now,
    before,
    `the focus indicator on ${state.name} looks the same as when it isn't focused`,
  );

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

const COLOR = /rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+%?))?\s*\)/g;

function toColor(match) {
  const alpha = match[4] === undefined ? 1 : match[4].endsWith("%") ? parseFloat(match[4]) / 100 : parseFloat(match[4]);
  return { r: +match[1], g: +match[2], b: +match[3], a: alpha };
}

function parseColor(text) {
  const match = [...text.matchAll(COLOR)][0];
  assert.ok(match, `cannot read the color "${text}"; the focus check understands rgb() and rgba()`);
  return toColor(match);
}

/** The first color in a box-shadow list that isn't fully transparent. */
function firstVisibleColor(boxShadow) {
  return [...boxShadow.matchAll(COLOR)].map(toColor).find((c) => c.a > 0);
}

function cssRgba({ r, g, b, a }) {
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

/** Blends CSS colors, nearest layer first, over an opaque white canvas. */
function composite(layers) {
  let out = { r: 255, g: 255, b: 255 };
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

function luminance({ r, g, b }) {
  const [lr, lg, lb] = [r, g, b].map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
}

function contrastRatio(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
