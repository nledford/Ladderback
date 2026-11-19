// Journey steps for the shared checkbox and radio group in the gallery
// (technical design §4.5): each box drawn from the tokens, its mark once
// chosen, its size and its error. A box is found by its role and name; its
// mark, which a screen reader skips, is the element drawn over it, the
// box's next sibling (`components/checkbox.rs`).

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";
import { expect } from "playwright/test";
import type { Locator } from "playwright";

import type { BrowserWorld } from "../support/world.ts";

// Half a pixel of rounding in a laid-out length.
const ROUNDING_PX = 0.5;

// A color the scenario names: a token (`--accent`) or a system color
// (`FieldText`).
const COLOR = String.raw`(--[\w-]+|[A-Z]\w+)`;

function choice(world: BrowserWorld, role: string, name: string): Locator {
  return world.page.getByRole(role as "checkbox" | "radio", { name, exact: true });
}

Then(
  new RegExp(String.raw`^the (checkbox|radio) "([^"]*)" is edged (\d) pixels? wide(?: in ${COLOR})?$`),
  async function (this: BrowserWorld, role: string, name: string, width: string, color: string | undefined) {
    const box = choice(this, role, name);
    await expect(box).toHaveCount(1);
    const drawn = await box.evaluate((element, color) => {
      const style = getComputedStyle(element);
      // The color named, computed where the box stands, so a token gives
      // its light or dark value and a system color the reader's.
      let wanted: string | null = null;
      if (color) {
        const probe = document.createElement("span");
        probe.style.color = color.startsWith("--") ? `var(${color})` : color;
        element.parentElement?.append(probe);
        wanted = getComputedStyle(probe).color;
        probe.remove();
      }
      return { width: style.borderTopWidth, style: style.borderTopStyle, color: style.borderTopColor, wanted };
    }, color);
    assert.equal(drawn.style, "solid", `the edge of the ${role} "${name}"`);
    assert.equal(drawn.width, `${width}px`, `the edge of the ${role} "${name}"`);
    if (color) assert.equal(drawn.color, drawn.wanted, `the edge of the ${role} "${name}" in ${color}`);
  },
);

Then(/^the (checkbox|radio) "([^"]*)" shows no mark$/, async function (this: BrowserWorld, role: string, name: string) {
  const box = choice(this, role, name);
  await expect(box).toHaveCount(1);
  const mark = box.locator("xpath=following-sibling::*[1]");
  await expect(mark).toHaveCount(1);
  await expect(mark).toBeHidden();
});

// The fill of a chosen box, and its mark over it: a check's strokes, drawn
// in the text's color of its icon, or a dot's fill.
Then(
  new RegExp(String.raw`^the (checkbox|radio) "([^"]*)" is filled in ${COLOR}, its mark in ${COLOR}$`),
  async function (this: BrowserWorld, role: string, name: string, fill: string, mark: string) {
    const box = choice(this, role, name);
    await expect(box).toBeChecked();
    await expect(box.locator("xpath=following-sibling::*[1]")).toBeVisible();
    const drawn = await box.evaluate(
      (element, [fill, mark]) => {
        const colorOf = (color: string) => {
          const probe = document.createElement("span");
          probe.style.color = color.startsWith("--") ? `var(${color})` : color;
          element.parentElement?.append(probe);
          const resolved = getComputedStyle(probe).color;
          probe.remove();
          return resolved;
        };
        const drawing = element.nextElementSibling;
        const glyph = drawing?.querySelector("svg");
        return {
          fill: getComputedStyle(element).backgroundColor,
          wantedFill: colorOf(fill),
          mark: drawing && (glyph ? getComputedStyle(glyph).color : getComputedStyle(drawing).backgroundColor),
          wantedMark: colorOf(mark),
        };
      },
      [fill, mark] as const,
    );
    assert.equal(drawn.fill, drawn.wantedFill, `the fill of the ${role} "${name}" in ${fill}`);
    assert.equal(drawn.mark, drawn.wantedMark, `the mark of the ${role} "${name}" in ${mark}`);
  },
);

Then(
  /^the (checkbox|radio) "([^"]*)" is a (square|circle) 1\.5 times its text's size, at least 24 pixels a side, its mark centered in it$/,
  async function (this: BrowserWorld, role: string, name: string, shape: string) {
    const box = choice(this, role, name);
    await expect(box).toBeChecked();
    const drawn = await box.evaluate((element) => {
      const style = getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      const mark = element.nextElementSibling?.getBoundingClientRect();
      return {
        width: rect.width,
        height: rect.height,
        text: parseFloat(style.fontSize),
        corner: parseFloat(style.borderTopLeftRadius),
        box: { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 },
        mark: mark && mark.width > 0 ? { x: mark.x + mark.width / 2, y: mark.y + mark.height / 2 } : null,
      };
    });
    const label = `the ${role} "${name}"`;
    assert.ok(Math.abs(drawn.width - drawn.height) <= ROUNDING_PX, `${label} is ${drawn.width} by ${drawn.height}`);
    assert.ok(
      Math.abs(drawn.width - Math.max(24, 1.5 * drawn.text)) <= ROUNDING_PX,
      `${label} is ${drawn.width} pixels a side, its text ${drawn.text}`,
    );
    if (shape === "circle") assert.ok(drawn.corner >= drawn.width / 2, `${label} has a corner of ${drawn.corner}, not round`);
    else assert.ok(drawn.corner < drawn.width / 4, `${label} has a corner of ${drawn.corner}, not square`);
    assert.ok(drawn.mark, `${label} shows no mark`);
    assert.ok(
      Math.abs(drawn.mark.x - drawn.box.x) <= ROUNDING_PX && Math.abs(drawn.mark.y - drawn.box.y) <= ROUNDING_PX,
      `the mark of ${label} stands at ${JSON.stringify(drawn.mark)}, its box's middle at ${JSON.stringify(drawn.box)}`,
    );
  },
);

// The box at the start of its label's line, in the page's direction, and
// under the label, in its column, its description, then its error.
Then(
  "the checkbox {string} stands before its label, its description and its error under the label, in that order",
  async function (this: BrowserWorld, name: string) {
    const box = choice(this, "checkbox", name);
    await expect(box).toHaveCount(1);
    const parts = await box.evaluate((input) => {
      const rect = (element: Element | null) => {
        const r = element?.getBoundingClientRect();
        return r && { top: r.top, bottom: r.bottom, start: r.left, end: r.right };
      };
      return {
        box: rect(input),
        label: rect(document.querySelector(`label[for="${input.id}"]`)),
        description: rect(document.getElementById(`${input.id}-description`)),
        error: rect(document.getElementById(`${input.id}-error`)),
        rtl: getComputedStyle(input).direction === "rtl",
      };
    });
    const { box: drawn, label, description, error } = parts;
    assert.ok(drawn && label && description && error, `"${name}" lacks a part: ${JSON.stringify(parts)}`);
    const before = parts.rtl ? drawn.start >= label.end - ROUNDING_PX : drawn.end <= label.start + ROUNDING_PX;
    assert.ok(before, `the box of "${name}" doesn't stand before its label`);
    assert.ok(Math.abs(drawn.top - label.top) <= drawn.bottom - drawn.top, `the box of "${name}" isn't on its label's line`);
    for (const [above, below, names] of [
      [label, description, "label and description"],
      [description, error, "description and error"],
    ] as const) {
      assert.ok(above.bottom <= below.top + ROUNDING_PX, `the ${names} of "${name}" aren't one under the other`);
      assert.ok(Math.abs(below.start - label.start) <= ROUNDING_PX, `the ${names} of "${name}" aren't in one column`);
    }
  },
);

Then("the checkbox {string} is marked invalid", async function (this: BrowserWorld, name: string) {
  await expect(choice(this, "checkbox", name)).toHaveAttribute("aria-invalid", "true");
});

// ARIA 1.2 allows aria-invalid on a radiogroup, not on a radio.
Then("the radio group {string} is marked invalid, and its radios are not", async function (this: BrowserWorld, group: string) {
  const radiogroup = this.page.getByRole("radiogroup", { name: group, exact: true });
  await expect(radiogroup).toHaveAttribute("aria-invalid", "true");
  const radios = radiogroup.getByRole("radio");
  await expect(radios.first()).toBeVisible();
  for (const radio of await radios.all()) await expect(radio).not.toHaveAttribute("aria-invalid");
});

// The error a group's description ends with, between its legend and its
// first choice.
Then(
  "the group {string} shows its legend, its error and its choices, in that order",
  async function (this: BrowserWorld, name: string) {
    const group = this.page.getByRole("radiogroup", { name, exact: true });
    await expect(group).toHaveCount(1);
    const tops = await group.evaluate((fieldset) => {
      const ids = (fieldset.getAttribute("aria-describedby") ?? "").split(/\s+/).filter(Boolean);
      const rect = (element: Element | null | undefined) => {
        const r = element?.getBoundingClientRect();
        return r && { top: r.top, bottom: r.bottom };
      };
      return [
        rect(fieldset.querySelector("legend")),
        rect(ids.map((id) => document.getElementById(id)).find((element) => element?.id.endsWith("-error"))),
        rect(fieldset.querySelector("input[type=radio]")),
      ];
    });
    const parts = ["legend", "error", "first choice"];
    tops.forEach((rect, at) => assert.ok(rect, `the group "${name}" shows no ${parts[at]}`));
    for (let at = 1; at < tops.length; at++) {
      assert.ok(tops[at - 1]!.bottom <= tops[at]!.top + ROUNDING_PX, `the ${parts[at - 1]} of "${name}" isn't above its ${parts[at]}`);
    }
  },
);
