// Journey steps for the shared text field in the gallery (technical design
// §4.5): its description and its error, its edge, its size, its icon and its
// colors under forced colors.

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";
import type { ElementHandle, Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { demoSection } from "../support/gallery.ts";
import { namePattern } from "../support/keyboard.ts";
import { patienceMs } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";

// The error element each scenario's field last showed, with its words, so a
// later step can tell the same error drawn again from one left in place.
const shownErrors = new WeakMap<BrowserWorld, { name: string; error: string; element: ElementHandle | null }>();

// The field's own error is on screen and in its description; a form's error
// summary may repeat the words as a link, so they are looked for among what
// the field points at, not anywhere on the page. The field points at its
// error only once the error comes, which may be after a slow check on the
// daemon, so what it points at is read again until then.
Then("the field {string} shows the error {string}", async function (this: BrowserWorld, name: string, error: string) {
  const input = this.page.getByRole("textbox", { name });
  await expect
    .poll(
      async () => {
        const ids = ((await input.getAttribute("aria-describedby")) ?? "").split(/\s+/).filter(Boolean);
        return (
          ids.length > 0 &&
          (await this.page
            .locator(ids.map((id) => `#${id}`).join(", "))
            .filter({ hasText: error })
            .first()
            .isVisible())
        );
      },
      { message: `the field "${name}" shows the error "${error}"` },
    )
    .toBe(true);
  const id = await input.getAttribute("id");
  shownErrors.set(this, { name, error, element: id === null ? null : await this.page.$(`[id="${id}-error"]`) });
});

// A second try refused alike is heard only when its error is drawn afresh: a
// live region, or an alert, left in place with the same words says nothing
// new (§4.5, "Step-up field"). So the field's error element is a new one,
// an alert again, with the same words.
Then("the field {string} shows that error again, as a new alert", async function (this: BrowserWorld, name: string) {
  const shown = shownErrors.get(this);
  assert.ok(shown?.name === name && shown.element !== null, `no error of the field ${JSON.stringify(name)} was seen before`);
  const input = this.page.getByRole("textbox", { name });
  const error = this.page.locator(`[id="${await input.getAttribute("id")}-error"]`);
  await expect
    .poll(async () => !(await shown.element?.evaluate((element) => element.isConnected)), {
      message: `the field "${name}" keeps its error "${shown.error}" in place rather than draw it again`,
    })
    .toBe(true);
  await expect(error).toHaveRole("alert");
  await expect(error).toHaveText(shown.error);
});

Then("the field {string} is described by {string}", async function (this: BrowserWorld, name: string, description: string) {
  const describedBy = await this.page.getByRole("textbox", { name }).getAttribute("aria-describedby");
  const text = (
    await Promise.all(
      (describedBy ?? "")
        .split(/\s+/)
        .filter(Boolean)
        .map((id) => this.page.locator(`#${id}`).textContent()),
    )
  ).join(" ");
  assert.ok(
    text.includes(description),
    `the field ${JSON.stringify(name)} is described by ${JSON.stringify(text)}, not ${JSON.stringify(description)}`,
  );
});

Then("the field {string} is marked invalid", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("textbox", { name })).toHaveAttribute("aria-invalid", "true");
});

Then("the field {string} is not marked invalid", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("textbox", { name })).not.toHaveAttribute("aria-invalid", "true");
});

// An error an earlier try left is gone from what the field points at; the
// announcer may still hold the words it said then, so the page as a whole
// isn't searched.
Then("the field {string} no longer shows the error {string}", async function (this: BrowserWorld, name: string, error: string) {
  const input = this.page.getByRole("textbox", { name });
  await expect
    .poll(
      async () => {
        const ids = ((await input.getAttribute("aria-describedby")) ?? "").split(/\s+/).filter(Boolean);
        const shown = await Promise.all(ids.map((id) => this.page.locator(`#${id}`).allTextContents()));
        return shown.flat().some((text) => text.includes(error));
      },
      { message: `the field "${name}" still shows the error "${error}"` },
    )
    .toBe(false);
});

// The field has no error element, whatever its words: none on the page under
// the id the field's error takes (`text_field::rules::error_id`), and none
// among what the field points at.
Then("the field {string} shows no error", async function (this: BrowserWorld, name: string) {
  const input = this.page.getByRole("textbox", { name });
  const id = await input.getAttribute("id");
  assert.ok(id, `the field ${JSON.stringify(name)} has no id for its error to take`);
  const errorId = `${id}-error`;
  await expect(this.page.locator(`[id="${errorId}"]`)).toHaveCount(0);
  const describedBy = ((await input.getAttribute("aria-describedby")) ?? "").split(/\s+/);
  assert.ok(!describedBy.includes(errorId), `the field ${JSON.stringify(name)} still points at its error`);
});

Then("the field {string} is described only by {string}", async function (this: BrowserWorld, name: string, description: string) {
  const describedBy = await this.page.getByRole("textbox", { name }).getAttribute("aria-describedby");
  const text = (
    await Promise.all(
      (describedBy ?? "")
        .split(/\s+/)
        .filter(Boolean)
        .map((id) => this.page.locator(`#${id}`).textContent()),
    )
  ).join(" ");
  assert.ok(
    text === description,
    `the field ${JSON.stringify(name)} is described by ${JSON.stringify(text)}, not only ${JSON.stringify(description)}`,
  );
});

// A multi-line field holds what was typed, each entry on its own line: Enter
// started a line rather than submitting anything.
Then(
  "the textbox {string} holds the lines {string} and {string}",
  async function (this: BrowserWorld, name: string, first: string, second: string) {
    await expect(this.page.getByRole("textbox", { name })).toHaveValue(`${first}\n${second}`);
  },
);

// A text field (§4.5), found by its label as a screen reader finds it, the
// whole of it ("New password" is never "New password again"): its input,
// and the parts the input's id names, the label above it, its description
// and its error.
const textField = (world: BrowserWorld, name: string): Locator => world.page.getByRole("textbox", { name: namePattern(name) });

// The field's edge, as wide as it is drawn, and in a token's color where
// the scenario names one, resolved on the field's own place in the page so
// light and dark each give their own.
Then(
  /^the field "([^"]*)" is edged (\d) pixels? wide(?: in (--[\w-]+))?$/,
  async function (this: BrowserWorld, name: string, width: string, token: string | undefined) {
    const field = textField(this, name);
    await expect(field).toHaveCount(1);
    const drawn = await field.evaluate((element, token) => {
      const style = getComputedStyle(element);
      let wanted: string | null = null;
      if (token) {
        const probe = document.createElement("span");
        probe.style.color = `var(${token})`;
        element.parentElement?.append(probe);
        wanted = getComputedStyle(probe).color;
        probe.remove();
      }
      return { width: style.borderTopWidth, color: style.borderTopColor, wanted };
    }, token);
    assert.equal(drawn.width, `${width}px`, `the edge of "${name}"`);
    if (token) assert.equal(drawn.color, drawn.wanted, `the edge of "${name}" in ${token}`);
  },
);

Then("the field {string} is edged with dashes", async function (this: BrowserWorld, name: string) {
  await expect(textField(this, name)).toHaveCSS("border-top-style", "dashed");
});

Then("the field {string} is disabled", async function (this: BrowserWorld, name: string) {
  await expect(textField(this, name)).toBeDisabled();
});

Then(
  "the field {string} shows its label, its description and its error above it, in that order",
  async function (this: BrowserWorld, name: string) {
    const field = textField(this, name);
    await expect(field).toHaveCount(1);
    const tops = await field.evaluate((input) => {
      const top = (element: Element | null) => element?.getBoundingClientRect();
      return [
        top(document.querySelector(`label[for="${input.id}"]`)),
        top(document.getElementById(`${input.id}-description`)),
        top(document.getElementById(`${input.id}-error`)),
        top(input),
      ].map((rect) => rect && { top: rect.top, bottom: rect.bottom });
    });
    const parts = ["label", "description", "error", "field"];
    const shown = tops.map((rect, at) => {
      assert.ok(rect, `"${name}" shows no ${parts[at]}`);
      return rect;
    });
    for (const [at, below] of shown.entries()) {
      const above = shown[at - 1];
      if (above === undefined) continue;
      assert.ok(above.bottom <= below.top + 0.5, `the ${parts[at - 1]} of "${name}" isn't above its ${parts[at]}`);
    }
  },
);

// A password is hidden while its type is `password`, and shown as text.
Then("the field {string} hides what it holds", async function (this: BrowserWorld, name: string) {
  await expect(textField(this, name)).toHaveAttribute("type", "password");
});

Then(
  "the field {string} shows what it holds, {string}, its caret {int} character(s) before its end",
  async function (this: BrowserWorld, name: string, value: string, back: number) {
    const field = textField(this, name);
    await expect(field).toHaveAttribute("type", "text");
    await expect(field).toHaveValue(value);
    const caret = await field.evaluate((input: HTMLInputElement) => [input.selectionStart, input.selectionEnd]);
    assert.deepEqual(caret, [value.length - back, value.length - back], `the caret of "${name}"`);
  },
);

// A number is text that asks a phone for its keyboard of digits: never
// `type="number"`, whose value a scroll of the wheel changes.
Then("the textbox {string} asks for the keyboard of digits, and is a text input", async function (this: BrowserWorld, name: string) {
  const field = textField(this, name);
  await expect(field).toHaveAttribute("inputmode", "numeric");
  await expect(field).toHaveAttribute("type", "text");
});

// The marker is shown after the label and hidden from a screen reader,
// which hears "required" once, from `aria-required`.
Then(
  "the field {string} shows {string} after its label, which a screen reader skips",
  async function (this: BrowserWorld, name: string, marker: string) {
    const field = textField(this, name);
    await expect(field).toHaveCount(1);
    const label = this.page.locator(`label[for="${await field.getAttribute("id")}"]`);
    await expect(label).toHaveText(`${name} ${marker}`);
    await expect(label.locator('[aria-hidden="true"]')).toHaveText(marker);
  },
);

Then("the field {string} is required, its name its label alone", async function (this: BrowserWorld, name: string) {
  const field = this.page.getByRole("textbox", { name, exact: true });
  await expect(field).toHaveCount(1);
  await expect(field).toHaveAttribute("aria-required", "true");
});

Then(
  "the field {string} is {float} times as tall as the page's text",
  async function (this: BrowserWorld, name: string, times: number) {
    const field = textField(this, name);
    await expect(field).toHaveCount(1);
    const { tall, text } = await field.evaluate((element) => ({
      tall: element.getBoundingClientRect().height,
      text: Number.parseFloat(getComputedStyle(document.body).fontSize),
    }));
    assert.ok(Math.abs(tall - times * text) < 0.5, `"${name}" is ${tall}px tall, the page's text ${text}px`);
  },
);

Then(
  "the field {string} is as tall as the button {string} beside it, their feet level",
  async function (this: BrowserWorld, name: string, button: string) {
    const field = textField(this, name);
    const pressed = this.page.getByRole("button", { name: button, exact: true });
    const boxOf = (locator: Locator) =>
      locator.evaluate((element) => {
        const rect = element.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom, left: rect.left, right: rect.right };
      });
    const [a, b] = await Promise.all([boxOf(field), boxOf(pressed)]);
    assert.ok(
      Math.abs(a.bottom - a.top - (b.bottom - b.top)) < 0.5,
      `"${name}" ${JSON.stringify(a)}, "${button}" ${JSON.stringify(b)}`,
    );
    assert.ok(Math.abs(a.bottom - b.bottom) < 0.5, `"${name}" and "${button}" don't stand level`);
    assert.ok(a.right <= b.left || b.right <= a.left, `"${button}" isn't beside "${name}"`);
  },
);

// Found by its part, as a pseudolocale renames the demo's heading and its
// fields.
Then(
  /^the field with an icon shows it at its start, inside its edge, in a page that runs (left to right|right to left)$/,
  async function (this: BrowserWorld, direction: string) {
    const box = this.page.locator(".text-field-with-icon");
    await expect(box).toHaveCount(1);
    const found = await box.evaluate((element) => {
      const rect = (part: Element | null) => {
        const r = part?.getBoundingClientRect();
        return r && { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
      };
      const input = element.querySelector("input");
      const svg = element.querySelector(".text-field-icon svg");
      return {
        rtl: getComputedStyle(document.documentElement).direction === "rtl",
        input: rect(input),
        icon: rect(svg),
        hidden: svg?.getAttribute("aria-hidden"),
        edge: input ? Number.parseFloat(getComputedStyle(input).borderTopWidth) : 0,
        // Where the input's text starts and ends, inside its padding.
        textStart: input ? input.getBoundingClientRect().left + Number.parseFloat(getComputedStyle(input).paddingLeft) : 0,
        textEnd: input ? input.getBoundingClientRect().right - Number.parseFloat(getComputedStyle(input).paddingRight) : 0,
      };
    });
    assert.equal(found.rtl, direction === "right to left", "the page's direction");
    const { input, icon } = found;
    assert.ok(input && icon, `the field with an icon: ${JSON.stringify(found)}`);
    assert.equal(found.hidden, "true", "a screen reader skips the icon");
    const inside =
      icon.left >= input.left + found.edge &&
      icon.right <= input.right - found.edge &&
      icon.top >= input.top + found.edge &&
      icon.bottom <= input.bottom - found.edge;
    assert.ok(inside, `the icon isn't inside the field's edge: ${JSON.stringify(found)}`);
    // At the start, before where the text starts: the text's padding at the
    // start makes room for the icon.
    const atStart = found.rtl ? icon.left >= found.textEnd : icon.right <= found.textStart;
    assert.ok(atStart, `the icon isn't at the field's start: ${JSON.stringify(found)}`);
  },
);

Then(
  "every field of the {string} demo is drawn in the reader's field colors, a disabled one in the color of unavailable text",
  async function (this: BrowserWorld, component: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: patienceMs() });
    const found = await section.evaluate((element) => {
      const probe = document.createElement("span");
      document.body.append(probe);
      const system = (color: string) => {
        probe.style.color = color;
        return getComputedStyle(probe).color;
      };
      probe.style.backgroundColor = "Field";
      const field = getComputedStyle(probe).backgroundColor;
      const [text, gray] = [system("FieldText"), system("GrayText")];
      probe.remove();
      return {
        field,
        text,
        gray,
        inputs: Array.from(element.querySelectorAll(".text-field-input"), (input) => {
          const style = getComputedStyle(input);
          return {
            id: input.id,
            disabled: (input as HTMLInputElement).disabled,
            drawn: { text: style.color, fill: style.backgroundColor, edge: style.borderTopColor },
          };
        }),
      };
    });
    assert.ok(
      found.inputs.some((input) => input.disabled),
      `the ${component} demo has no disabled field`,
    );
    for (const input of found.inputs) {
      const ink = input.disabled ? found.gray : found.text;
      assert.deepEqual(input.drawn, { text: ink, fill: found.field, edge: ink }, input.id);
    }
  },
);
