// Journey steps for the shared button in the gallery (technical design §4.5):
// its focus, its description, its pressed and busy states, its size, its
// tones and its colors under forced colors. Keyboard only, role-and-name
// locators, as everywhere in steps/.

import assert from "node:assert/strict";

import { type DataTable, Then, When } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import { focusComesTo, journey, namePattern, pressUntilFocused } from "../support/keyboard.ts";
import { patienceMs } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";
import { demoSection } from "../support/gallery.ts";

// The button's whole name, read past the isolation marks around a name the
// reader gave, such as a token's label.
Then("focus is on the button {string}", async function (this: BrowserWorld, name: string) {
  await focusComesTo(
    this.page,
    journey(this.page).byRole("button", { name: namePattern(name) }),
    `the button ${JSON.stringify(name)}`,
  );
});

// One of several buttons of one name, told apart by the whole of what it is
// described by: each field's Show password button by its field's label.
Then("focus is on the button {string} described by {string}", async function (this: BrowserWorld, name: string, description: string) {
  await focusComesTo(
    this.page,
    journey(this.page).byRole("button", { name: namePattern(name), description: namePattern(description) }),
    `the button ${JSON.stringify(name)} described by ${JSON.stringify(description)}`,
  );
});

// A button of one name in more than one demo, told apart by its demo: the
// alert's Dismiss and the banner's.
function demoButton(page: Page, name: string, component: string): Locator {
  return demoSection(page, component).getByRole("button", { name: namePattern(name) });
}

When(
  "I press Tab until focus is on the button {string} of the {string} demo",
  async function (this: BrowserWorld, name: string, component: string) {
    const description = `the button ${JSON.stringify(name)} of the ${JSON.stringify(component)} demo`;
    await pressUntilFocused(this.page, "Tab", demoButton(this.page, name, component), description);
  },
);

Then("focus is on the button {string} of the {string} demo", async function (this: BrowserWorld, name: string, component: string) {
  await focusComesTo(
    this.page,
    demoButton(this.page, name, component),
    `the button ${JSON.stringify(name)} of the ${JSON.stringify(component)} demo`,
  );
});

// A toggle button tells a screen reader its state with aria-pressed (WAI-ARIA
// button pattern), and its name stays the same whichever state it is in.
Then("the button {string} is pressed", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("button", { name, exact: true, pressed: true })).toHaveCount(1);
});

Then("the button {string} is not pressed", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("button", { name, exact: true, pressed: false })).toHaveCount(1);
});

Then(
  "the button {string} described by {string} is not pressed",
  async function (this: BrowserWorld, name: string, description: string) {
    await expect(
      this.page.getByRole("button", { name: namePattern(name), description: namePattern(description), pressed: false }),
    ).toHaveCount(1);
  },
);

// What a screen reader reads after the button's name and state: the banner
// the button is described by while live updates are paused. Its numbers sit
// in bidi isolation marks, which a reader never hears (§4.5).
Then("the button {string} is described by {string}", async function (this: BrowserWorld, name: string, description: string) {
  const marks = "[\\u2066-\\u2069]*";
  const heard = [...description].map((c) => c.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(marks);
  await expect(this.page.getByRole("button", { name, exact: true })).toHaveAccessibleDescription(
    new RegExp(`^${marks}${heard}${marks}$`),
  );
});

Then("the button {string} has no description", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("button", { name, exact: true })).toHaveAccessibleDescription("");
});

// A browser draws a form control's text in its own fixed size (13.33 px in
// Chromium) unless the page says otherwise, so the label stays put when the
// reader's text size doubles the rest of the page (1.4.4). Compared with the
// body's text, the size every page's text starts from.
Then("the label of the button {string} is as large as the page's text", async function (this: BrowserWorld, name: string) {
  const sizes = await this.page.getByRole("button", { name: namePattern(name) }).evaluate((button) => ({
    label: getComputedStyle(button).fontSize,
    page: getComputedStyle(button.ownerDocument.body).fontSize,
  }));
  assert.equal(sizes.label, sizes.page, `the label of "${name}" is ${sizes.label}, the page's text ${sizes.page}`);
});

// A short visible label with a longer name that starts with it, so a reader
// who speaks the label they see reaches the button (WCAG 2.5.3, Label in
// Name), and a screen reader still hears what the button acts on.
Then(
  "the button {string} shows {string}, which its name begins with",
  async function (this: BrowserWorld, name: string, label: string) {
    const button = this.page.getByRole("button", { name: namePattern(name) });
    await expect(button, `the button "${name}" shows "${label}"`).toHaveText(label);
    assert.ok(name.startsWith(label), `the name "${name}" doesn't begin with "${label}"`);
  },
);

// A sentence of a demo, one of its paragraphs, read without the bidi
// isolation marks a number sits in (§8.4), which a reader never sees. The
// button demo's counts its buttons' presses, so a press that reached the
// page's handler shows in it.
Then("the {string} demo shows the sentence {string}", async function (this: BrowserWorld, component: string, sentence: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: patienceMs() });
  await expect
    .poll(
      () =>
        section.evaluate((element) =>
          Array.from(element.querySelectorAll(":scope > p"), (line) =>
            (line.textContent ?? "").replace(/[\u2066-\u2069]/g, "").trim(),
          ),
        ),
      { message: `the ${component} demo never showed "${sentence}"` },
    )
    .toContain(sentence);
});

// A button (technical design §4.5) draws its appearance in its tone: a
// Filled one its fill and edge in the tone and its label in `--on-fill`, an
// Outlined one its edge and label in the tone, a Ghost one its label alone.
// A token is read through a probe, as the page resolves it; "none" is no
// color at all. Inherit must follow the text around the button, not a tone
// an outer part sets, so the step gives the line around it a color and a
// tone of its own and reads the button against each. Each button is found
// by its name and read alone with `evaluate`.
Then(
  "each button of the {string} demo draws its appearance in its tone:",
  async function (this: BrowserWorld, component: string, rows: DataTable) {
    const section = demoSection(this.page, component);
    type Colors = { label: string; fill: string; edge: string };
    for (const { button: name, ...wanted } of rows.hashes() as (Colors & { button: string })[]) {
      const button = section.getByRole("button", { name, exact: true });
      await expect(button).toHaveCount(1);
      const drawn = await button.evaluate((element, wanted: Colors) => {
        const around = "the text around it";
        const resolve = (token: string) => {
          if (token === "none") return "rgba(0, 0, 0, 0)";
          const probe = document.createElement("span");
          probe.style.color = `var(${token})`;
          document.body.append(probe);
          const color = getComputedStyle(probe).color;
          probe.remove();
          return color;
        };
        const read = (): Colors => {
          const style = getComputedStyle(element);
          return { label: style.color, fill: style.backgroundColor, edge: style.borderTopColor };
        };
        const line = element.parentElement;
        const colors = Object.values(wanted).includes(around) ? ["rgb(1, 2, 3)", "rgb(200, 100, 50)"] : [""];
        const drawn: Colors[] = [];
        const expected: Colors[] = [];
        for (const color of colors) {
          if (color !== "" && line) {
            line.style.color = color;
            line.style.setProperty("--tone", "rgb(9, 9, 9)");
          }
          const pick = (token: string) => (token === around ? color : resolve(token));
          drawn.push(read());
          expected.push({ label: pick(wanted.label), fill: pick(wanted.fill), edge: pick(wanted.edge) });
        }
        line?.style.removeProperty("color");
        line?.style.removeProperty("--tone");
        return { drawn, expected };
      }, wanted);
      assert.deepEqual(drawn.drawn, drawn.expected, `the button "${name}"`);
    }
  },
);

// A button's size is its height, a multiple of its text (§4.5), so it grows
// with the reader's text size (1.4.4). Half a pixel of rounding is allowed.
Then(
  "the button {string} is {float} times as tall as the page's text",
  async function (this: BrowserWorld, name: string, times: number) {
    const button = this.page.getByRole("button", { name, exact: true });
    await expect(button).toHaveCount(1);
    const { tall, text } = await button.evaluate((element) => ({
      tall: element.getBoundingClientRect().height,
      text: Number.parseFloat(getComputedStyle(document.body).fontSize),
    }));
    assert.ok(Math.abs(tall - times * text) < 0.5, `"${name}" is ${tall}px tall, the page's text ${text}px`);
  },
);

// An icon-only button is square at every size, and never under the 24 CSS
// pixels a side WCAG 2.5.8 (Target Size) asks of a target.
Then("the button {string} is square, at least 24 pixels a side", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("button", { name, exact: true });
  await expect(button).toHaveCount(1);
  const box = await button.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return { width: rect.width, height: rect.height };
  });
  assert.ok(Math.abs(box.width - box.height) < 0.5, `"${name}" is ${box.width}px by ${box.height}px`);
  assert.ok(box.height >= 24, `"${name}" is ${box.height}px tall`);
});

// A button's icon stands before its words in the page's direction, so at
// their right in a right-to-left page, and is as tall as them. Found
// page-wide, by the button's parts, as a pseudolocale renames the demo's
// heading and its buttons.
Then(
  /^every button with an icon and words shows its icon before its words, as tall as them, in a page that runs (left to right|right to left)$/,
  async function (this: BrowserWorld, direction: string) {
    // A visible one: a step-up field's Show password waits in a closed dialog.
    await this.page.locator(".button:visible").first().waitFor({ timeout: patienceMs() });
    const found = await this.page.evaluate(() => ({
      rtl: getComputedStyle(document.documentElement).direction === "rtl",
      // A drawn one only: the header's menu button waits, undrawn, for a
      // screen narrower than 41rem.
      buttons: Array.from(document.querySelectorAll(".button")).flatMap((button) => {
        if (button.getClientRects().length === 0) return [];
        const icon = button.querySelector(":scope > .icon")?.getBoundingClientRect();
        const label = button.querySelector(":scope > .button-label")?.getBoundingClientRect();
        if (!icon || !label) return [];
        return [
          {
            name: button.textContent?.trim() ?? "",
            icon: { left: icon.left, right: icon.right, height: icon.height },
            label: { left: label.left, right: label.right },
            text: Number.parseFloat(getComputedStyle(button).fontSize),
          },
        ];
      }),
    }));
    assert.equal(found.rtl, direction === "right to left", "the page's direction");
    assert.ok(found.buttons.length >= 2, `${found.buttons.length} buttons with an icon and words`);
    for (const button of found.buttons) {
      const before = found.rtl ? button.icon.left >= button.label.right : button.icon.right <= button.label.left;
      assert.ok(before, `the icon of "${button.name}" isn't before its words: ${JSON.stringify(button)}`);
      assert.ok(Math.abs(button.icon.height - button.text) < 0.5, `the icon of "${button.name}": ${JSON.stringify(button)}`);
    }
  },
);

// An unavailable button says so (`aria-disabled`) and keeps focus: never
// the `disabled` attribute, which would take it out of the Tab order (A1).
Then("the button {string} says it is unavailable, and is not disabled", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("button", { name, exact: true });
  await expect(button).toHaveAttribute("aria-disabled", "true");
  await expect(button).not.toHaveAttribute("disabled");
  await expect(button).toBeFocused();
});

// A busy button with an icon keeps its width as its spinner takes the
// icon's place, so nothing beside it moves: its width is noted before, and
// compared after, within half a pixel.
const notedButtonWidths = new WeakMap<BrowserWorld, Map<string, number>>();

async function buttonWidth(world: BrowserWorld, name: string): Promise<number> {
  const button = world.page.getByRole("button", { name, exact: true });
  await expect(button).toHaveCount(1);
  return button.evaluate((element) => element.getBoundingClientRect().width);
}

When("the widths of the buttons {string} and {string} are noted", async function (this: BrowserWorld, first: string, second: string) {
  notedButtonWidths.set(
    this,
    new Map([
      [first, await buttonWidth(this, first)],
      [second, await buttonWidth(this, second)],
    ]),
  );
});

Then("the buttons {string} and {string} keep the widths noted", async function (this: BrowserWorld, first: string, second: string) {
  const noted = notedButtonWidths.get(this);
  for (const name of [first, second]) {
    const before = noted?.get(name);
    assert.ok(before !== undefined, `the width of "${name}" was never noted`);
    const now = await buttonWidth(this, name);
    assert.ok(Math.abs(now - before) <= 0.5, `"${name}" was ${before}px wide, now ${now}px`);
  }
});

// A busy button (§4.5): unavailable (`aria-disabled`, never `disabled`, so it
// keeps focus), its name unchanged, and a spinner as tall as its text inside
// it, which a screen reader skips, so it hears the button's name alone.
async function busyButton(world: BrowserWorld, name: string) {
  const button = world.page.getByRole("button", { name: namePattern(name) });
  await expect(button).toHaveCount(1);
  await expect(button).toHaveAttribute("aria-disabled", "true");
  await expect(button).not.toHaveAttribute("disabled");
  await expect(button).toHaveAccessibleName(namePattern(name));
  await expect(button.getByRole("img")).toHaveCount(0);
  await expect(button.locator("svg")).toHaveCount(1);
  const parts = await button.evaluate((element) => {
    const spinner = element.querySelector(".spinner-graphic")?.getBoundingClientRect();
    const label = element.querySelector(":scope > .button-label")?.getBoundingClientRect();
    return {
      spinner: spinner ? { left: spinner.left, right: spinner.right, height: spinner.height } : null,
      label: label ? { left: label.left, right: label.right } : null,
      icon: element.querySelector(".icon") !== null,
      text: Number.parseFloat(getComputedStyle(element).fontSize),
      rtl: getComputedStyle(element).direction === "rtl",
    };
  });
  assert.ok(parts.spinner, `"${name}" holds no spinner`);
  assert.ok(Math.abs(parts.spinner.height - parts.text) < 0.5, `the spinner in "${name}": ${JSON.stringify(parts)}`);
  return { ...parts, spinner: parts.spinner };
}

Then(
  "the button {string} is busy, a spinner a screen reader skips before its label",
  async function (this: BrowserWorld, name: string) {
    const parts = await busyButton(this, name);
    assert.ok(parts.label, `"${name}" shows no label`);
    const before = parts.rtl ? parts.spinner.left >= parts.label.right : parts.spinner.right <= parts.label.left;
    assert.ok(before, `the spinner in "${name}" isn't before its label: ${JSON.stringify(parts)}`);
  },
);

Then(
  "the button {string} is busy, a spinner a screen reader skips in its icon's place",
  async function (this: BrowserWorld, name: string) {
    const parts = await busyButton(this, name);
    assert.equal(parts.icon, false, `"${name}" still shows its icon beside its spinner`);
  },
);

Then("the button {string} is not busy, its icon back in its place", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("button", { name, exact: true });
  await expect(button).toHaveCount(1);
  await expect(button).not.toHaveAttribute("aria-disabled");
  await expect(button.locator("svg")).toHaveCount(1);
  await expect(button.locator(".icon")).toHaveCount(1);
});

// Pressed is said by weight and an inset edge (The Weight Not Color Rule),
// never by color alone, and so still under forced colors.
async function weightAndEdge(world: BrowserWorld, name: string) {
  const button = world.page.getByRole("button", { name, exact: true });
  await expect(button).toHaveCount(1);
  return button.evaluate((element) => {
    const style = getComputedStyle(element);
    return { weight: Number(style.fontWeight), edge: style.borderTopStyle };
  });
}

Then("the button {string} is drawn at its resting weight and edge", async function (this: BrowserWorld, name: string) {
  const drawn = await weightAndEdge(this, name);
  assert.ok(drawn.weight < 600 && drawn.edge !== "inset", `"${name}": ${JSON.stringify(drawn)}`);
});

Then("the button {string} is drawn bolder, with an inset edge", async function (this: BrowserWorld, name: string) {
  const drawn = await weightAndEdge(this, name);
  assert.ok(drawn.weight >= 600 && drawn.edge === "inset", `"${name}": ${JSON.stringify(drawn)}`);
});

// Forced colors (the reader's own palette): every button in the reader's
// button colors, its label and edge `ButtonText` on `ButtonFace`, whatever
// its appearance and tone, and an unavailable one's label and edge
// `GrayText`. A busy one (its spinner showing) is `aria-disabled` too, but
// at work, not unavailable: it keeps `ButtonText`. A pressed one keeps it
// too, a Ghost one's edge included. Each system color is read through a
// probe.
async function buttonColors(world: BrowserWorld, component: string) {
  const section = demoSection(world.page, component);
  await section.waitFor({ timeout: patienceMs() });
  const found = await section.evaluate((element) => {
    const probe = document.createElement("span");
    document.body.append(probe);
    const system = (color: string) => {
      probe.style.color = color;
      return getComputedStyle(probe).color;
    };
    probe.style.backgroundColor = "ButtonFace";
    const face = getComputedStyle(probe).backgroundColor;
    const [text, gray] = [system("ButtonText"), system("GrayText")];
    probe.remove();
    return {
      face,
      text,
      gray,
      buttons: Array.from(element.querySelectorAll("button"), (button) => {
        const style = getComputedStyle(button);
        const busy = button.querySelector(".button-spinner") !== null;
        return {
          name: button.getAttribute("aria-label") || (button.textContent ?? "").trim(),
          busy,
          unavailable: button.getAttribute("aria-disabled") === "true" && !busy,
          pressed: button.getAttribute("aria-pressed") === "true",
          ghost: button.classList.contains("button-ghost"),
          drawn: { label: style.color, fill: style.backgroundColor, edge: style.borderTopColor },
        };
      }),
    };
  });
  assert.ok(found.buttons.length > 0, `the ${component} demo has no button`);
  assert.notEqual(found.text, found.gray, "the reader's button text and unavailable text are one color");
  return found;
}

Then(
  "every button of the {string} demo is drawn in the reader's button colors, an unavailable one in the color of unavailable text",
  async function (this: BrowserWorld, component: string) {
    const found = await buttonColors(this, component);
    assert.ok(
      found.buttons.some((button) => button.unavailable),
      `the ${component} demo has no unavailable button`,
    );
    for (const button of found.buttons) {
      const ink = button.unavailable ? found.gray : found.text;
      assert.deepEqual(button.drawn, { label: ink, fill: found.face, edge: ink }, button.name);
    }
  },
);

Then(
  "every busy button of the {string} demo is drawn in the reader's button colors, its label and edge in the button text, not the color of unavailable text",
  async function (this: BrowserWorld, component: string) {
    const found = await buttonColors(this, component);
    const busy = found.buttons.filter((button) => button.busy);
    const unavailable = found.buttons.filter((button) => button.unavailable);
    assert.ok(busy.length > 0 && unavailable.length > 0, `the ${component} demo shows no busy button beside an unavailable one`);
    for (const button of busy) {
      assert.deepEqual(button.drawn, { label: found.text, fill: found.face, edge: found.text }, button.name);
    }
    for (const button of unavailable) {
      assert.deepEqual(button.drawn, { label: found.gray, fill: found.face, edge: found.gray }, button.name);
    }
  },
);

Then(
  "every pressed button of the {string} demo is drawn in the reader's button colors, a Ghost one's edge too",
  async function (this: BrowserWorld, component: string) {
    const found = await buttonColors(this, component);
    const pressed = found.buttons.filter((button) => button.pressed);
    assert.ok(
      pressed.some((button) => button.ghost),
      `the ${component} demo shows no pressed Ghost button`,
    );
    for (const button of pressed) {
      assert.deepEqual(button.drawn, { label: found.text, fill: found.face, edge: found.text }, button.name);
    }
  },
);
