// Journey steps for the shared component set in the gallery (technical design
// §4.5): the dialog, the data table, the text field, the checkbox, the
// progress bar, the toast, the pause control, the status badge and the
// shared vocabulary of tones, sizes and placements, the spinner, the icon
// and the button. Keyboard only, role-and-name locators, as everywhere in
// steps/.

import assert from "node:assert/strict";

import AxeBuilder from "@axe-core/playwright";
import { type DataTable, Then, When } from "@cucumber/cucumber";
import { expect } from "playwright/test";
import type { Locator, Page } from "playwright";

import { journey, namePattern } from "../support/keyboard.ts";
import { pointer } from "../support/pointer.ts";
import { type BrowserWorld, evaluateAll } from "../support/world.ts";

// WCAG 2.2 A and AA, as in steps/start_page.ts.
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

/** What makes an element a live region, by role or by attribute. */
export const LIVE_REGION = "[aria-live], [role~='status'], [role~='alert'], [role~='log'], [role~='marquee'], [role~='timer'], output";

/** The gallery section of a component, found by its heading, not by a test id. */
export function demoSection(page: Page, component: string): Locator {
  return page.locator("section", {
    has: page.getByRole("heading", { level: 2, name: component, exact: true }),
  });
}

// The button's whole name, read past the isolation marks around a name the
// reader gave, such as a token's label.
Then("focus is on the button {string}", async function (this: BrowserWorld, name: string) {
  await journey(this.page).assertFocused(
    journey(this.page).byRole("button", { name: namePattern(name) }),
    `the button ${JSON.stringify(name)}`,
  );
});

// One of several buttons of one name, told apart by the whole of what it is
// described by: each field's Show password button by its field's label.
Then(
  "focus is on the button {string} described by {string}",
  async function (this: BrowserWorld, name: string, description: string) {
    await journey(this.page).assertFocused(
      journey(this.page).byRole("button", { name: namePattern(name), description: namePattern(description) }),
      `the button ${JSON.stringify(name)} described by ${JSON.stringify(description)}`,
    );
  },
);

// A review says in its own polite region what the page's announcer can't
// while the modal is open (§4.5). The region has no role of its own (a second
// `status` would hide the page's one), so it is found as the live region
// holding the text.
Then("the dialog {string} politely says {string}", async function (this: BrowserWorld, name: string, text: string) {
  const dialog = journey(this.page).byRole("dialog", { name: namePattern(name) });
  await expect
    .poll(
      () =>
        dialog.evaluate((el, said) => {
          const marks = /[\u2066-\u2069]/g;
          return [...el.querySelectorAll('[aria-live="polite"]')].some((region) =>
            (region.textContent ?? "").replace(marks, "").includes(said),
          );
        }, text),
      { message: `the dialog "${name}" never said "${text}" in its polite region`, timeout: 5_000 },
    )
    .toBe(true);
});

// The counterpart of a button held with `aria-disabled`: it can be pressed.
Then("the dialog {string} has its button {string} available", async function (this: BrowserWorld, name: string, button: string) {
  const found = journey(this.page)
    .byRole("dialog", { name: namePattern(name) })
    .getByRole("button", { name: namePattern(button) });
  await expect(found).toHaveCount(1);
  await expect(found).not.toHaveAttribute("aria-disabled", "true");
  await expect(found).toHaveAccessibleDescription("");
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

// The banner sits beside the focused button, so an edge in the focus ring's
// color would read as a second focus ring (2.4.7): its edge has the color of a
// rule, not of focus.
Then("the banner describing the button {string} is edged unlike its focus ring", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("button", { name, exact: true });
  await expect(button).toBeFocused();
  const [ring, edge] = await button.evaluate((el) => {
    const banner = document.getElementById(el.getAttribute("aria-describedby") ?? "");
    if (banner === null) {
      throw new Error("the button is described by no banner");
    }
    const outline = getComputedStyle(el);
    const border = getComputedStyle(banner);
    return [
      outline.outlineStyle === "none" ? null : outline.outlineColor,
      border.borderTopStyle === "none" ? null : border.borderTopColor,
    ];
  });
  assert.notEqual(ring, null, `the button ${JSON.stringify(name)} shows no focus ring`);
  assert.notEqual(edge, null, "the banner has no edge");
  assert.notEqual(edge, ring, `the banner's edge is ${edge}, the focus ring's color`);
});

Then("the button {string} has no description", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("button", { name, exact: true })).toHaveAccessibleDescription("");
});

// The wrap the dialog's own trap does: Tab on its last control puts focus on
// its first, not just anywhere inside.
Then(
  "focus is on the first control of the dialog {string}",
  async function (this: BrowserWorld, name: string) {
    const first = this.page
      .getByRole("dialog", { name })
      .locator(
        'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      )
      .first();
    await journey(this.page).assertFocused(
      first,
      `the first control of the dialog ${JSON.stringify(name)}`,
    );
  },
);
// The field's own error is on screen and in its description; a form's error
// summary may repeat the words as a link, so they are looked for among what
// the field points at, not anywhere on the page.
Then("the field {string} shows the error {string}", async function (this: BrowserWorld, name: string, error: string) {
  const input = this.page.getByRole("textbox", { name });
  const ids = ((await input.getAttribute("aria-describedby")) ?? "").split(/\s+/).filter(Boolean);
  await expect(
    this.page.locator(ids.map((id) => `#${id}`).join(", ") || ":not(*)").filter({ hasText: error }),
  ).toBeVisible();
});

Then("the field {string} is described by {string}", async function (this: BrowserWorld, name: string, description: string) {
  const describedBy = await this.page
    .getByRole("textbox", { name })
    .getAttribute("aria-describedby");
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
  await expect(this.page.getByRole("textbox", { name })).toHaveAttribute(
    "aria-invalid",
    "true",
  );
});

Then("the field {string} is not marked invalid", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("textbox", { name })).not.toHaveAttribute(
    "aria-invalid",
    "true",
  );
});

Then("the field {string} shows no error", async function (this: BrowserWorld, name: string) {
  const describedBy = await this.page
    .getByRole("textbox", { name })
    .getAttribute("aria-describedby");
  const shown = await Promise.all(
    (describedBy ?? "")
      .split(/\s+/)
      .filter(Boolean)
      .map((id) => this.page.locator(`#${id}`).textContent()),
  );
  assert.ok(
    !shown.includes("A repository name is required."),
    `the field ${JSON.stringify(name)} still shows the error`,
  );
});

Then("the field {string} is described only by {string}", async function (this: BrowserWorld, name: string, description: string) {
  const describedBy = await this.page
    .getByRole("textbox", { name })
    .getAttribute("aria-describedby");
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

Then("the checkbox {string} is checked", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("checkbox", { name, exact: true })).toBeChecked();
});

Then("the checkbox {string} is not checked", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("checkbox", { name, exact: true })).not.toBeChecked();
});

// What a screen reader reads after the name: the hint, wired with
// aria-describedby.
Then("the checkbox {string} is described by {string}", async function (this: BrowserWorld, name: string, description: string) {
  await expect(this.page.getByRole("checkbox", { name, exact: true })).toHaveAccessibleDescription(description);
});

// A radio button found within its group, which names what it chooses.
function radioOfGroup(world: BrowserWorld, name: string, group: string) {
  return world.page.getByRole("group", { name: group, exact: true }).getByRole("radio", { name, exact: true });
}

Then("the radio {string} of the group {string} is checked", async function (this: BrowserWorld, name: string, group: string) {
  await expect(radioOfGroup(this, name, group)).toBeChecked();
});

Then("the radio {string} of the group {string} is not checked", async function (this: BrowserWorld, name: string, group: string) {
  await expect(radioOfGroup(this, name, group)).not.toBeChecked();
});

// What a screen reader reads after the group's name as it enters the group.
Then("the group {string} is described by {string}", async function (this: BrowserWorld, name: string, description: string) {
  await expect(this.page.getByRole("group", { name, exact: true })).toHaveAccessibleDescription(description);
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
Then("the button {string} shows {string}, which its name begins with", async function (this: BrowserWorld, name: string, label: string) {
  const button = this.page.getByRole("button", { name: namePattern(name) });
  await expect(button, `the button "${name}" shows "${label}"`).toHaveText(label);
  assert.ok(name.startsWith(label), `the name "${name}" doesn't begin with "${label}"`);
});

// What a screen reader is told of the bar's value: `aria-valuenow`, which
// changes at most every 30 s while the step stays the same (§4.5).
Then("the progress bar {string} shows {int} percent", async function (this: BrowserWorld, name: string, percent: number) {
  await expect(this.page.getByRole("progressbar", { name: namePattern(name) })).toHaveAttribute(
    "aria-valuenow",
    String(percent),
  );
});

// What a screen reader reads as the bar's value, `aria-valuetext`, without
// the isolation marks around the values the page put in it, which a reader
// never hears.
Then("the progress bar {string} reads {string}", async function (this: BrowserWorld, name: string, text: string) {
  const bar = this.page.getByRole("progressbar", { name: namePattern(name) });
  const heard = async () => ((await bar.getAttribute("aria-valuetext")) ?? "").replace(/[\u2066-\u2069]/g, "");
  await expect.poll(heard).toBe(text);
});

// The fill a sighted reader sees, drawn to the percentage at every report,
// whatever the bar has told a screen reader so far.
const filled = (bar: Locator) =>
  bar.evaluate((element) => (element.querySelector(".progress-fill") as HTMLElement | null)?.style.inlineSize ?? "");

Then("the progress bar {string} is filled to {int} percent", async function (this: BrowserWorld, name: string, percent: number) {
  await expect.poll(() => filled(this.page.getByRole("progressbar", { name: namePattern(name) }))).toBe(`${percent}%`);
});

// What a sighted reader sees: the fill drawn to the percentage, and the
// percentage written inside the bar beside it. Both follow every report.
Then("the progress bar {string} is drawn at {int} percent", async function (this: BrowserWorld, name: string, percent: number) {
  const bar = this.page.getByRole("progressbar", { name: namePattern(name) });
  await expect.poll(() => filled(bar)).toBe(`${percent}%`);
  await expect.poll(() => bar.evaluate((element) => (element as HTMLElement).innerText)).toContain(`${percent}%`);
});

// How long the fill takes to move to a new report, as the browser computes it
// under the reader's settings: "0s" is a jump.
const moves = (bar: Locator) =>
  bar.evaluate((element) => {
    const fill = element.querySelector(".progress-fill");
    return fill === null ? "" : getComputedStyle(fill).transitionDuration;
  });

Then("the fill of the progress bar {string} moves smoothly to each report", async function (this: BrowserWorld, name: string) {
  const bar = this.page.getByRole("progressbar", { name: namePattern(name) });
  await expect.poll(() => moves(bar)).toMatch(/^(?!0s$)\d/);
});

Then("the fill of the progress bar {string} jumps to each report", async function (this: BrowserWorld, name: string) {
  const bar = this.page.getByRole("progressbar", { name: namePattern(name) });
  await expect.poll(() => moves(bar)).toBe("0s");
});

// The reader asks her system for less motion while the page is open, and the
// page follows at once (WCAG 2.3.3).
When("the reader asks for less motion", async function (this: BrowserWorld) {
  await this.page.emulateMedia({ reducedMotion: "reduce" });
});

// A status badge's icon (technical design §4.5) is decoration beside its
// text, so a screen reader reads the label once and Tab never stops on it; it
// is drawn in the text's color, so the contrast gate's badge pairs cover it;
// and its shape is its state's own, so the state reads without the color.
Then(
  "each of the {int} badges of the {string} demo has an icon of its own, in its text's color, which a screen reader skips",
  async function (this: BrowserWorld, count: number, component: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: 5_000 });
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

/**
 * The items of one of the vocabulary demo's lists, found by the list's name.
 * Each list is named by its heading through aria-labelledby, so its items are
 * read one at a time with `evaluate`, which finds them in Playwright's
 * utility world as `expect` does, and never with `evaluateAll`, which finds
 * them in the page's main world: Firefox crashes the page when a name read
 * through aria-labelledby is computed in both worlds (microsoft/playwright
 * #43967, issue #774).
 */
function vocabularyItems(page: Page, component: string, list: string): Locator {
  return demoSection(page, component).getByRole("list", { name: list, exact: true }).getByRole("listitem");
}

/** The `count` items a locator finds, each its own locator, found as `expect` finds them. */
export async function eachOf(items: Locator, count: number): Promise<Locator[]> {
  await expect(items).toHaveCount(count);
  return items.all();
}

// A tone (technical design §4.5, the shared vocabulary) is said in words, so
// its color never carries the meaning alone (1.4.1). Its swatch is
// decoration beside the words, so a screen reader reads each name once, and
// it is stroked in the words' color, so the contrast gate's tone pairs cover
// it as well as the text.
Then(
  "each of the {int} tones of the {string} demo is named in words, its swatch drawn in its text's color and skipped by a screen reader",
  async function (this: BrowserWorld, count: number, component: string) {
    const items = await eachOf(vocabularyItems(this.page, component, "Tones"), count);
    const tones = await Promise.all(
      items.map((locator) =>
        locator.evaluate((item) => {
          const swatch = item.querySelector("svg");
          return {
            text: item.textContent?.trim() ?? "",
            hidden: swatch?.getAttribute("aria-hidden") ?? null,
            focusable: swatch?.getAttribute("focusable") ?? null,
            stroke: swatch === null ? "" : getComputedStyle(swatch).stroke,
            color: getComputedStyle(item).color,
          };
        }),
      ),
    );
    for (const tone of tones) {
      assert.notEqual(tone.text, "", "a tone without its name");
      assert.equal(tone.hidden, "true", tone.text);
      assert.equal(tone.focusable, "false", tone.text);
      assert.equal(tone.stroke, tone.color, tone.text);
    }
  },
);

// Every tone but Inherit draws in a token of its own: Primary the accent,
// Secondary the muted text, and the three status hues. None is the list's own
// text color either, so a tone whose rule is lost, and which falls back to
// the text around it, fails here.
Then(
  "the {int} tones of the {string} demo other than Inherit each have a color of their own",
  async function (this: BrowserWorld, count: number, component: string) {
    const list = demoSection(this.page, component).getByRole("list", { name: "Tones", exact: true });
    const items = list.getByRole("listitem").filter({
      hasNot: this.page.getByText("Inherit", { exact: true }),
    });
    const colors = await Promise.all(
      (await eachOf(items, count)).map((item) => item.evaluate((element) => getComputedStyle(element).color)),
    );
    const around = await list.evaluate((element) => getComputedStyle(element).color);
    assert.equal(new Set([...colors, around]).size, count + 1, `the tones' colors: ${colors.join(", ")}; the text's: ${around}`);
  },
);

// Under forced colors (1.4.11, §4.5) every tone is the system's text color,
// as the text around it is, so the tones' words, not their colors, tell them
// apart, and no author color is left where the reader chose her own.
Then(
  "the {int} tones of the {string} demo all draw in the color of the text around them",
  async function (this: BrowserWorld, count: number, component: string) {
    const list = demoSection(this.page, component).getByRole("list", { name: "Tones", exact: true });
    const around = await list.evaluate((element) => getComputedStyle(element).color);
    for (const item of await eachOf(list.getByRole("listitem"), count)) {
      const tone = await item.evaluate((element) => ({
        text: element.textContent?.trim() ?? "",
        color: getComputedStyle(element).color,
      }));
      assert.equal(tone.color, around, tone.text);
    }
  },
);

// Inherit is the color of the text around a part, as a spinner inside a
// button takes the button's: the step colors the list around the tone, and
// gives it a tone of its own as an outer part would, and reads the tone's
// color, which must follow the text rather than name a token or take the
// outer part's tone.
Then(
  "the tone {string} of the {string} demo takes the color of the text around it",
  async function (this: BrowserWorld, tone: string, component: string) {
    const list = demoSection(this.page, component).getByRole("list", { name: "Tones", exact: true });
    const item = list.getByRole("listitem").filter({ hasText: tone });
    await expect(item).toHaveCount(1);
    for (const around of ["rgb(1, 2, 3)", "rgb(200, 100, 50)"]) {
      await list.evaluate((element, color) => {
        (element as HTMLElement).style.color = color;
        (element as HTMLElement).style.setProperty("--tone", "rgb(9, 9, 9)");
      }, around);
      assert.equal(await item.evaluate((element) => getComputedStyle(element).color), around, tone);
    }
    await list.evaluate((element) => {
      (element as HTMLElement).style.color = "";
      (element as HTMLElement).style.removeProperty("--tone");
    });
  },
);

// A status tone's pair (technical design §4.5, the shared vocabulary): a part
// tinted in it draws its words in the pair's words token on its fill token.
// Each token is read through a probe, as the page resolves it, with a
// fallback no token has, so a token style.css lacks fails here rather than
// matching whatever color the probe inherits. The part is read alone with
// `evaluate`, as `expect` finds it (issue #774).
async function drawnOnItsFill(part: Locator, name: string, words: string, fill: string) {
  await expect(part).toHaveCount(1);
  const missing = "rgb(1, 2, 3)";
  const drawn = await part.evaluate(
    (element, { tokens, missing }) => {
      const probe = document.createElement("span");
      document.body.append(probe);
      const resolve = (token: string) => {
        probe.style.color = `var(${token}, ${missing})`;
        return getComputedStyle(probe).color;
      };
      const expected = { words: resolve(tokens.words), fill: resolve(tokens.fill) };
      probe.remove();
      const style = getComputedStyle(element);
      return { expected, found: { words: style.color, fill: style.backgroundColor } };
    },
    { tokens: { words, fill }, missing },
  );
  assert.notEqual(drawn.expected.words, missing, `${words} is not defined`);
  assert.notEqual(drawn.expected.fill, missing, `${fill} is not defined`);
  assert.deepEqual(drawn.found, drawn.expected, name);
}

// The vocabulary demo's status pairs, each named in words drawn in its pair,
// so the axe row reads each pair's words on its fill too.
Then(
  "each status pair of the {string} demo is named in words, drawn on its fill:",
  async function (this: BrowserWorld, component: string, pairs: DataTable) {
    const list = demoSection(this.page, component).getByRole("list", { name: "Status pairs", exact: true });
    const rows = pairs.hashes() as { pair: string; words: string; fill: string }[];
    await expect(list.getByRole("listitem")).toHaveCount(rows.length);
    for (const { pair, words, fill } of rows) {
      await drawnOnItsFill(list.getByRole("listitem").filter({ hasText: wholly(pair) }), pair, words, fill);
    }
  },
);

// A status badge (§4.5) whose operation ended draws its words and its pill in
// the pair of its status tone, found by its words, the whole of them
// ("Succeeded" is never "Succeeded with warnings").
Then(
  "each of these badges of the {string} demo is drawn on its fill:",
  async function (this: BrowserWorld, component: string, badges: DataTable) {
    const section = demoSection(this.page, component);
    for (const { badge, words, fill } of badges.hashes() as { badge: string; words: string; fill: string }[]) {
      const pill = section.getByText(badge, { exact: true });
      await drawnOnItsFill(pill, badge, words, fill);
    }
  },
);

/** Text that is the whole of an element's, never a part of it. */
function wholly(text: string): RegExp {
  return new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`);
}

// A placement (technical design §4.5) is in logical terms: Start is the
// line's start in the page's direction, the left in English and the right in
// Arabic. Each row of the demo's last list is a part placed in its line (the
// list's content box), its swatch and its name; Inline keeps the part in the
// line of text, as wide as what it holds. The vocabulary demo holds the
// gallery's only lists, the placements last; it is found by order, since a
// pseudolocale changes the names. A pixel of rounding is allowed.
Then(
  /^the placements of the vocabulary demo put each part where they say, in a page that runs (left to right|right to left)$/,
  async function (this: BrowserWorld, direction: string) {
    const placements = this.page.getByRole("main").getByRole("list").last();
    const items = placements.getByRole("listitem");
    await expect(items).toHaveCount(4);
    const rows = await placements.evaluate((list) => {
      const frame = list.getBoundingClientRect();
      const style = getComputedStyle(list);
      const inset = (side: "left" | "right") =>
        Number.parseFloat(style.getPropertyValue(`border-${side}-width`)) +
        Number.parseFloat(style.getPropertyValue(`padding-${side}`));
      return {
        direction: style.direction,
        line: { left: frame.left + inset("left"), right: frame.right - inset("right") },
        parts: Array.from(list.children, (item) => {
          const box = item.getBoundingClientRect();
          const held = Array.from(item.children, (child) => child.getBoundingClientRect());
          return {
            box: { left: box.left, right: box.right },
            left: Math.min(...held.map((rect) => rect.left)),
            right: Math.max(...held.map((rect) => rect.right)),
          };
        }),
      };
    });
    assert.equal(rows.direction, direction === "right to left" ? "rtl" : "ltr", "the page's direction");
    const [inline, start, center, end] = rows.parts;
    assert.ok(inline && start && center && end, "four placements");
    // The gaps between a part and its line's two sides, in reading order.
    const gaps = (part: { left: number; right: number }) => {
      const [left, right] = [part.left - rows.line.left, rows.line.right - part.right];
      return direction === "right to left" ? { before: right, after: left } : { before: left, after: right };
    };
    assert.ok(Math.abs(inline.box.right - inline.box.left - (inline.right - inline.left)) < 1, "Inline is as wide as its part");
    assert.ok(gaps(inline.box).after > 1, "Inline leaves the rest of its line");
    assert.ok(gaps(start).before < 1 && gaps(start).after > 1, `Start: ${JSON.stringify(gaps(start))}`);
    assert.ok(Math.abs(gaps(center).before - gaps(center).after) < 1 && gaps(center).before > 1, `Center: ${JSON.stringify(gaps(center))}`);
    assert.ok(gaps(end).after < 1 && gaps(end).before > 1, `End: ${JSON.stringify(gaps(end))}`);
  },
);

// No badge moves (technical design §4.5): a running operation can run for
// hours while the page stays in use, and nothing on the page could stop a
// turning icon or a pulsing pill (WCAG 2.2.2), so the visible text tells the
// state. A badge's count is every animation the browser runs on its pill or
// inside it (CSS animations and transitions, the Web Animations API) and
// every SVG animation element it holds. A badge without an icon fails first,
// so the check can't pass on a badge with nothing in it to move.
Then(
  "none of the {int} badges of the {string} demo moves",
  async function (this: BrowserWorld, count: number, component: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: 5_000 });
    const badges = await section.evaluate((element) =>
      Array.from(element.querySelectorAll(".badge"), (badge) => ({
        text: badge.textContent?.trim() ?? "",
        icon: badge.querySelector("svg") !== null,
        motions:
          badge.getAnimations({ subtree: true }).length +
          badge.querySelectorAll("animate, animateMotion, animateTransform, set").length,
      })),
    );
    assert.equal(badges.length, count);
    for (const badge of badges) {
      assert.ok(badge.icon, `the badge "${badge.text}" has no icon`);
      assert.equal(badge.motions, 0, badge.text);
    }
  },
);

// A badge's icon is as tall as its text, `1em` (technical design §4.5), so it
// grows with the reader's text size (WCAG 1.4.4) rather than staying a small
// mark beside large words. Half a pixel of rounding is allowed.
Then(
  "the icon of each badge of the {string} demo is as tall as its text",
  async function (this: BrowserWorld, component: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: 5_000 });
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
  },
);

// A bar whose size isn't known is indeterminate (WAI-ARIA `progressbar`): it
// has no value to give, so it gives no `aria-valuenow`, rather than a value
// of nothing, and its value text says what the operation is doing, never a
// percentage. What it shows, where a known bar's percentage stands, says the
// same, so a sighted reader sees no "0%" either.
Then("the progress bar {string} shows no percentage", async function (this: BrowserWorld, name: string) {
  const bar = this.page.getByRole("progressbar", { name: namePattern(name) });
  await expect(bar).toHaveCount(1);
  await expect(bar).not.toHaveAttribute("aria-valuenow");
  await expect(bar).toHaveAttribute("aria-valuetext", /^[^%]+$/);
  const shown = () => bar.evaluate((element) => (element as HTMLElement).innerText.trim());
  await expect.poll(shown).not.toBe("");
  await expect.poll(shown).not.toMatch(/%/);
});

Then("a toast reads {string}", async function (this: BrowserWorld, text: string) {
  await expect(this.page.locator("main").getByText(text)).toBeVisible();
});

// A toast is transient; ten seconds is generous for however long it shows.
// Scoped to main: the polite live region carries the same text.
When("the toast {string} is gone", async function (this: BrowserWorld, text: string) {
  await expect(this.page.locator("main").getByText(text)).toBeHidden({
    timeout: 10_000,
  });
});

Then("the {string} demo has no accessibility violations", async function (this: BrowserWorld, component: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: 5_000 });
  // `analyze` takes no argument: this scans the whole gallery page, the demo
  // among it, as it always has.
  const { violations } = await new AxeBuilder({ page: this.page }).withTags(WCAG_TAGS).analyze();
  const summary = violations.map((v) => `${v.id}: ${v.help} (${v.nodes.length} nodes)`);
  assert.deepEqual(summary, [], `${component} demo violations: ${summary.join("; ")}`);
});

Then("the {string} demo matches its ARIA snapshot", async function (this: BrowserWorld, component: string) {
  const pinned = SNAPSHOTS[component];
  assert.ok(pinned !== undefined, `no ARIA snapshot is pinned for the ${component} demo`);
  const snapshot = await demoSection(this.page, component).ariaSnapshot();
  assert.equal(
    snapshot.trim(),
    pinned.trim(),
    `the ${component} demo's ARIA snapshot`,
  );
});

// A spinner beside a visible label (technical design §4.5): the label is
// ordinary text, read once, and the drawing beside it is hidden from a screen
// reader, so what the reader hears of the spinner is its label alone.
Then(
  "the {string} demo shows the text {string} beside a spinner a screen reader skips",
  async function (this: BrowserWorld, component: string, label: string) {
    const text = demoSection(this.page, component).getByText(label, { exact: true });
    await expect(text).toBeVisible();
    const spinner = text.locator("xpath=..");
    await expect(spinner.locator("svg")).toHaveCount(1);
    await expect(spinner.getByRole("img")).toHaveCount(0);
    assert.equal((await spinner.ariaSnapshot()).trim(), `- text: ${label}`);
  },
);

// A spinner inside a button: the button is named by the spinner's label, and
// its drawing adds nothing to what a screen reader reads.
Then("the button {string} holds a spinner a screen reader skips", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("button", { name, exact: true });
  await expect(button).toHaveCount(1);
  await expect(button.locator("svg")).toHaveCount(1);
  await expect(button.getByRole("img")).toHaveCount(0);
  await expect(button).toHaveAccessibleName(name);
});

// A spinner in a line of text (technical design §4.5): its label is more of
// the line's text, on the same baseline as the words before it, and its
// drawing is centered on that text as `vertical-align: middle` centers an
// inline image, on the baseline raised by half the letters' x-height. A probe
// set in the same line finds that middle; every run of text in the line is
// in one font, so runs on one baseline share their top and bottom.
Then(
  "the spinner labelled {string} stands in its line of text, its label on the line's baseline and its drawing centered on the text",
  async function (this: BrowserWorld, label: string) {
    const text = demoSection(this.page, "Spinner").getByText(label, { exact: true });
    await expect(text).toBeVisible();
    const placed = await text.locator("xpath=ancestor::p[1]").evaluate((line) => {
      const runs: { text: string; top: number; bottom: number }[] = [];
      const walker = document.createTreeWalker(line, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const words = node.textContent?.trim() ?? "";
        if (words === "") continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        const box = range.getBoundingClientRect();
        runs.push({ text: words, top: box.top, bottom: box.bottom });
      }
      const probe = document.createElement("span");
      probe.style.cssText = "display: inline-block; block-size: 0; vertical-align: middle";
      line.append(probe);
      const middle = probe.getBoundingClientRect().top;
      probe.remove();
      const drawing = line.querySelector("svg")?.getBoundingClientRect();
      return { runs, middle, drawing: drawing ? (drawing.top + drawing.bottom) / 2 : Number.NaN };
    });
    const first = placed.runs[0];
    assert.ok(
      first && placed.runs.length >= 2,
      `the spinner "${label}" stands in no line of text: ${JSON.stringify(placed.runs)}`,
    );
    for (const run of placed.runs) {
      assert.ok(
        Math.abs(run.top - first.top) < 0.5 && Math.abs(run.bottom - first.bottom) < 0.5,
        `"${run.text}" stands on another baseline than "${first.text}": ${JSON.stringify(placed.runs)}`,
      );
    }
    assert.ok(
      Math.abs(placed.drawing - placed.middle) < 0.5,
      `the drawing's middle is at ${placed.drawing}px, the text's at ${placed.middle}px`,
    );
  },
);

// A spinner whose label isn't shown is an image (`role="img"`) named by it.
Then("the {string} demo has the image {string}", async function (this: BrowserWorld, component: string, name: string) {
  await expect(demoSection(this.page, component).getByRole("img", { name, exact: true })).toHaveCount(1);
});

Then("the {string} demo shows no text {string}", async function (this: BrowserWorld, component: string, text: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: 5_000 });
  await expect(section.getByText(text, { exact: true })).toHaveCount(0);
});

// The page keeps one polite and one assertive region, the announcer's (§4.5,
// A6); a spinner adds none, so a page that waits says so through the
// announcer, and a wait that nobody asked for says nothing.
Then("the {string} demo has spinners but no live region", async function (this: BrowserWorld, component: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: 5_000 });
  const found = await section.evaluate(
    (element, live) => ({
      spinners: element.querySelectorAll(".spinner").length,
      live: Array.from(element.querySelectorAll(live), (region) => region.outerHTML),
    }),
    LIVE_REGION,
  );
  assert.ok(found.spinners > 0, `the ${component} demo has no spinner`);
  assert.deepEqual(found.live, []);
});

// Every animation the browser runs on a spinner or inside it, with how it
// turns: its timing function, its period and how often it repeats.
const spinnerMotions = (section: Locator) =>
  section.evaluate((element) =>
    Array.from(element.querySelectorAll(".spinner"), (spinner) => ({
      text: spinner.textContent?.trim() || spinner.querySelector("svg")?.getAttribute("aria-label") || "",
      motions: spinner.getAnimations({ subtree: true }).map((animation) => {
        const target = (animation.effect as KeyframeEffect | null)?.target;
        const style = target ? getComputedStyle(target) : null;
        return {
          state: animation.playState,
          period: Number(animation.effect?.getComputedTiming().duration ?? 0),
          repeats: style?.animationIterationCount ?? "",
          easing: style?.animationTimingFunction ?? "",
        };
      }),
    })),
  );

// A steady turn (technical design §4.5): one linear animation that repeats
// without end, once in 0.8 to 1 s, so the wait reads as work going on rather
// than as a pulse.
Then(
  "every spinner of the {string} demo turns steadily, once in 0.8 to 1 second",
  async function (this: BrowserWorld, component: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: 5_000 });
    const spinners = await spinnerMotions(section);
    assert.ok(spinners.length > 0, `the ${component} demo has no spinner`);
    for (const spinner of spinners) {
      assert.equal(spinner.motions.length, 1, `the spinner "${spinner.text}" has ${spinner.motions.length} animations`);
      const [motion] = spinner.motions;
      assert.equal(motion?.state, "running", spinner.text);
      assert.equal(motion?.easing, "linear", spinner.text);
      assert.equal(motion?.repeats, "infinite", spinner.text);
      const period = motion?.period ?? 0;
      assert.ok(period >= 800 && period <= 1000, `the spinner "${spinner.text}" turns once in ${period} ms`);
    }
  },
);

// Under reduced motion the arc stands still and the label carries the
// meaning (WCAG 2.3.3); a spinner whose drawing went missing fails first.
Then("no spinner of the {string} demo moves", async function (this: BrowserWorld, component: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: 5_000 });
  await expect
    .poll(async () => {
      const spinners = await spinnerMotions(section);
      assert.ok(spinners.length > 0, `the ${component} demo has no spinner`);
      return spinners.filter((spinner) => spinner.motions.length > 0).map((spinner) => spinner.text);
    })
    .toEqual([]);
  await expect(section.locator(".spinner svg").first()).toBeVisible();
});

// A spinner's size is set in `em` (technical design §4.5), so it grows with
// the reader's text (1.4.4): compared with the body's text, the size every
// page's text starts from. Its laid-out height, as the stylesheet sets it.
// Half a pixel of rounding is allowed.
Then(
  "the image {string} is {float} times as tall as the page's text",
  async function (this: BrowserWorld, name: string, times: number) {
    const image = this.page.getByRole("img", { name, exact: true });
    await expect(image).toHaveCount(1);
    const { tall, text } = await image.evaluate((element) => ({
      tall: Number.parseFloat(getComputedStyle(element).blockSize),
      text: Number.parseFloat(getComputedStyle(document.body).fontSize),
    }));
    assert.ok(Math.abs(tall - times * text) < 0.5, `"${name}" is ${tall}px tall, the page's text ${text}px`);
  },
);

// The page's spinners on lines of their own, in the page's order: a spinner
// whose line (the element its placement goes on, around the spinner) is a
// block in the page's flow, a line of the page's width, rather than an inline
// box in a line of text or an item of a row of spinners (which a flex row
// makes a block too). Found page-wide, as a pseudolocale renames the demo's
// heading. Each gives its label, or its drawing's name, how far the spinner
// stands from its line's left and right edges, its drawing's box, which
// keeps its length as the arc turns inside it (a step below checks that),
// and the box of each line its label takes. A label's line is the span of
// the label's text on one line, its pieces on that line joined.
const linesOfTheirOwn = async (page: Page) => {
  await page.locator(".spinner").first().waitFor({ timeout: 5_000 });
  return page.evaluate(() => ({
    rtl: getComputedStyle(document.documentElement).direction === "rtl",
    lines: Array.from(document.querySelectorAll<HTMLElement>(".spinner"))
      .filter((spinner) => {
        const line = spinner.parentElement;
        const row = line?.parentElement;
        return (
          line !== null &&
          getComputedStyle(line).display === "flex" &&
          !/flex|grid/.test(row ? getComputedStyle(row).display : "")
        );
      })
      .map((spinner) => {
        const line = (spinner.parentElement as HTMLElement).getBoundingClientRect();
        const box = spinner.getBoundingClientRect();
        const graphic = spinner.querySelector("svg") as SVGSVGElement;
        const drawing = graphic.getBoundingClientRect();
        const label = spinner.querySelector(".spinner-label");
        const labelLines: { left: number; right: number; top: number; bottom: number }[] = [];
        if (label !== null) {
          const range = document.createRange();
          range.selectNodeContents(label);
          for (const piece of Array.from(range.getClientRects()).filter((rect) => rect.width > 0)) {
            const same = labelLines.find((known) => Math.abs(known.top - piece.top) < 1);
            if (same) {
              same.left = Math.min(same.left, piece.left);
              same.right = Math.max(same.right, piece.right);
            } else {
              labelLines.push({ left: piece.left, right: piece.right, top: piece.top, bottom: piece.bottom });
            }
          }
          labelLines.sort((one, other) => one.top - other.top);
        }
        return {
          label: spinner.textContent?.trim() || graphic.getAttribute("aria-label") || "",
          left: box.left - line.left,
          right: line.right - box.right,
          drawing: { left: drawing.left, right: drawing.right, top: drawing.top, bottom: drawing.bottom },
          labelLines,
        };
      }),
  }));
};

// A spinner on a line of its own sits at its line's start, middle or end, as
// the page's direction runs (logical, §8.4): the spinner is measured against
// its line. Each line is wider than the spinner, so a spinner that sat
// anywhere else would be seen.
Then(
  "the spinners on lines of their own sit at the start, the middle and the end of lines that run {word} to {word}",
  async function (this: BrowserWorld, from: string, to: string) {
    const direction = `${from} to ${to}`;
    assert.ok(direction === "left to right" || direction === "right to left", `unknown direction "${direction}"`);
    const { rtl, lines } = await linesOfTheirOwn(this.page);
    assert.equal(rtl, direction === "right to left", "the page's direction");
    assert.equal(lines.length, 3, `the spinners on lines of their own: ${JSON.stringify(lines)}`);
    const [atStart, inMiddle, atEnd] = lines.map((line) => {
      const [start, end] = rtl ? [line.right, line.left] : [line.left, line.right];
      assert.ok(start + end > 10, `the line of "${line.label}" is no wider than the spinner`);
      return { ...line, start, end };
    });
    assert.ok(atStart && atStart.start < 1, `"${atStart?.label}" is ${atStart?.start}px from its line's start`);
    assert.ok(
      inMiddle && Math.abs(inMiddle.start - inMiddle.end) < 1,
      `"${inMiddle?.label}" is ${inMiddle?.start}px from its line's start and ${inMiddle?.end}px from its end`,
    );
    assert.ok(atEnd && atEnd.end < 1, `"${atEnd?.label}" is ${atEnd?.end}px from its line's end`);
  },
);

// Which spinner stands on which of those lines, by what a reader reads.
Then(
  "the spinners on lines of their own read {string}, {string} and {string}, in that order",
  async function (this: BrowserWorld, first: string, second: string, third: string) {
    const { lines } = await linesOfTheirOwn(this.page);
    assert.deepEqual(
      lines.map((line) => line.label),
      [first, second, third],
    );
  },
);

// A long label on a line of its own wraps beside its drawing, not under it
// (technical design §4.5): the label takes more than one line, every one of
// them starts after the drawing's end, as the page's direction runs, and the
// drawing stands centered on the first line, which the gallery checks with a
// drawing shorter than the line, one as tall and one taller. A line's middle
// is its text's, as the line's leading is shared above and below the text.
// Half a pixel of rounding is allowed in the direction, a pixel in the
// middle.
Then(
  "the label of each spinner on a line of its own wraps onto more lines beside its drawing, which stands centered on the first, in a page that runs {word} to {word}",
  async function (this: BrowserWorld, from: string, to: string) {
    const direction = `${from} to ${to}`;
    assert.ok(direction === "left to right" || direction === "right to left", `unknown direction "${direction}"`);
    const { rtl, lines } = await linesOfTheirOwn(this.page);
    assert.equal(rtl, direction === "right to left", "the page's direction");
    assert.equal(lines.length, 3, `the spinners on lines of their own: ${JSON.stringify(lines)}`);
    for (const { label, drawing, labelLines } of lines) {
      const [first, second] = labelLines;
      assert.ok(first && second, `"${label}" takes ${labelLines.length} line(s), not more than one`);
      for (const [index, line] of labelLines.entries()) {
        const after = rtl ? drawing.left - line.right : line.left - drawing.right;
        assert.ok(
          after > -0.5,
          `line ${index + 1} of "${label}" starts ${-after}px before the drawing's end: ${JSON.stringify({ drawing, labelLines })}`,
        );
      }
      const middle = (drawing.top + drawing.bottom) / 2;
      const firstMiddle = (first.top + first.bottom) / 2;
      assert.ok(
        Math.abs(middle - firstMiddle) < 1,
        `the drawing of "${label}" stands at ${middle}px, its first line at ${firstMiddle}px: ${JSON.stringify({ drawing, labelLines })}`,
      );
    }
  },
);

// A spinner turns its drawing inside the drawing's box (technical design
// §4.5): the box a page lays out, and scrolls by, keeps its length at every
// angle. A drawing that turned whole would stand out of its box by a fifth
// of its length at an eighth of a turn, so a spinner at the edge of a table
// that scrolls would change how far it scrolls as it turned. Every animation
// is paused at an eighth of its turn before the boxes are read, and a
// spinner that doesn't turn at all fails first.
Then(
  "no spinner of the {string} demo reaches out of its drawing's box as it turns",
  async function (this: BrowserWorld, component: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: 5_000 });
    const drawings = await section.evaluate((element) =>
      Array.from(element.querySelectorAll(".spinner"), (spinner) => {
        const animations = spinner.getAnimations({ subtree: true });
        for (const animation of animations) {
          animation.pause();
          animation.currentTime = Number(animation.effect?.getComputedTiming().duration ?? 0) / 8;
        }
        const graphic = spinner.querySelector("svg") as SVGSVGElement;
        const box = graphic.getBoundingClientRect();
        const style = getComputedStyle(graphic);
        return {
          text: spinner.textContent?.trim() || graphic.getAttribute("aria-label") || "",
          turning: animations.length,
          drawn: [box.width, box.height],
          laidOut: [Number.parseFloat(style.inlineSize), Number.parseFloat(style.blockSize)],
        };
      }),
    );
    assert.ok(drawings.length > 0, `the ${component} demo has no spinner`);
    for (const { text, turning, drawn, laidOut } of drawings) {
      assert.ok(turning > 0, `the spinner "${text}" doesn't turn`);
      assert.ok(
        drawn.every((length, axis) => Math.abs(length - (laidOut[axis] ?? Number.NaN)) < 0.5),
        `the drawing of "${text}" reaches ${JSON.stringify(drawn)} at an eighth of a turn, laid out ${JSON.stringify(laidOut)}`,
      );
    }
  },
);

// A spinner's tone (technical design §4.5, the shared vocabulary) colors its
// drawing: the arc in the tone's token, the track in the same color, faint.
// Each row names a spinner whose label is hidden, so it is an image named by
// it, and the token its tone draws in, or "the text around it" for Inherit.
// A token is read through a probe, as the page resolves it. Inherit must
// follow the text around the spinner, not a tone an outer part sets, so the
// step gives the spinner's line a color and a tone of its own and reads the
// arc against each. Each image is read alone with `evaluate`, as `expect`
// finds it (issue #774).
Then(
  "each spinner of the {string} demo draws its arc in its tone and its track in a faint version of it:",
  async function (this: BrowserWorld, component: string, tones: DataTable) {
    const section = demoSection(this.page, component);
    for (const { spinner: name, tone } of tones.hashes() as { spinner: string; tone: string }[]) {
      const image = section.getByRole("img", { name, exact: true });
      await expect(image).toHaveCount(1);
      const drawn = await image.evaluate((svg, token) => {
        const stroke = (selector: string) => {
          const part = svg.querySelector(selector);
          return part ? getComputedStyle(part) : null;
        };
        const line = svg.closest(".spinner")?.parentElement as HTMLElement | null | undefined;
        const read = () => ({
          arc: stroke(".spinner-arc")?.stroke ?? "",
          track: stroke(".spinner-track")?.stroke ?? "",
          faint: Number(stroke(".spinner-track")?.strokeOpacity ?? 1),
        });
        if (token.startsWith("--")) {
          const probe = document.createElement("span");
          probe.style.color = `var(${token})`;
          document.body.append(probe);
          const color = getComputedStyle(probe).color;
          probe.remove();
          return { ...read(), expected: [color], arcs: [read().arc] };
        }
        if (!line) return { ...read(), expected: ["a line around the spinner"], arcs: [] };
        const arcs: string[] = [];
        const expected = ["rgb(1, 2, 3)", "rgb(200, 100, 50)"];
        for (const color of expected) {
          line.style.color = color;
          line.style.setProperty("--tone", "rgb(9, 9, 9)");
          arcs.push(read().arc);
        }
        line.style.color = "";
        line.style.removeProperty("--tone");
        return { ...read(), expected, arcs };
      }, tone);
      assert.deepEqual(drawn.arcs, drawn.expected, `the arc of "${name}"`);
      assert.equal(drawn.track, drawn.arc, `the track of "${name}" is drawn in its arc's color`);
      assert.ok(drawn.faint > 0 && drawn.faint < 0.5, `the track of "${name}" is ${drawn.faint} opaque`);
    }
  },
);

// A tone colors the drawing only (technical design §4.5): every shown label
// stays in the color of the text around its spinner, so it reads as the
// text does and keeps the text's contrast.
Then(
  "the label of every spinner of the {string} demo is in the color of the text around it",
  async function (this: BrowserWorld, component: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: 5_000 });
    const labels = await section.evaluate((element) =>
      Array.from(element.querySelectorAll(".spinner"), (spinner) => {
        const label = Array.from(spinner.children).find((child) => child.localName !== "svg");
        return {
          text: label?.textContent?.trim() ?? "",
          color: label ? getComputedStyle(label).color : "",
          around: spinner.parentElement ? getComputedStyle(spinner.parentElement).color : "",
        };
      }).filter((label) => label.text !== ""),
    );
    assert.ok(labels.length > 0, `the ${component} demo shows no spinner's label`);
    for (const label of labels) {
      assert.equal(label.color, label.around, label.text);
    }
  },
);

// Forced colors (the reader's own palette): the arc follows the tone rule of
// the shared vocabulary, every tone the text's own color, and the track takes
// the color of unavailable text, two of the reader's colors, which they
// choose to stand apart.
Then(
  "every spinner of the {string} demo draws its arc in the color of the text around it and its track in GrayText",
  async function (this: BrowserWorld, component: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: 5_000 });
    const drawn = await section.evaluate((element) => {
      const probe = document.createElement("span");
      probe.style.setProperty("forced-color-adjust", "none");
      probe.style.color = "GrayText";
      document.body.append(probe);
      const gray = getComputedStyle(probe).color;
      probe.remove();
      return {
        gray,
        spinners: Array.from(element.querySelectorAll(".spinner"), (spinner) => {
          const stroke = (selector: string) => {
            const part = spinner.querySelector(selector);
            return part ? getComputedStyle(part).stroke : "";
          };
          return {
            name: spinner.textContent?.trim() || spinner.querySelector("svg")?.getAttribute("aria-label") || "",
            arc: stroke(".spinner-arc"),
            track: stroke(".spinner-track"),
            text: spinner.parentElement ? getComputedStyle(spinner.parentElement).color : "",
          };
        }),
      };
    });
    assert.ok(drawn.spinners.length > 0, `the ${component} demo has no spinner`);
    for (const spinner of drawn.spinners) {
      assert.notEqual(spinner.text, drawn.gray, `the text around "${spinner.name}" is GrayText`);
      assert.deepEqual(
        { arc: spinner.arc, track: spinner.track },
        { arc: spinner.text, track: drawn.gray },
        spinner.name,
      );
    }
  },
);

// An icon beside words (technical design §4.5) is decoration: hidden from a
// screen reader, so the words are read once, never a tab stop, stroked in
// the words' color (Inherit, its default tone), as tall as them (Small, its
// default size, 1em) and centered on them as `vertical-align: middle`
// centers an inline image, on the baseline raised by half the letters'
// x-height. A probe set right after the icon, so on its line however the
// words wrap, finds that middle. Half a pixel of rounding is allowed.
Then(
  "the {string} demo shows the text {string} beside an icon a screen reader skips, in the text's color, as tall as it and centered on it",
  async function (this: BrowserWorld, component: string, text: string) {
    const line = demoSection(this.page, component).getByText(text, { exact: true });
    await expect(line).toBeVisible();
    const icon = await line.evaluate((element) => {
      const svg = element.querySelector(":scope > svg");
      const probe = document.createElement("span");
      probe.style.cssText = "display: inline-block; block-size: 0; vertical-align: middle";
      svg?.after(probe);
      const middle = probe.getBoundingClientRect().top;
      probe.remove();
      const box = svg?.getBoundingClientRect();
      return {
        found: svg !== null,
        role: svg?.getAttribute("role") ?? null,
        hidden: svg?.getAttribute("aria-hidden") ?? null,
        focusable: svg?.getAttribute("focusable") ?? null,
        stroke: svg === null ? "" : getComputedStyle(svg).stroke,
        tall: box?.height ?? 0,
        center: box ? (box.top + box.bottom) / 2 : Number.NaN,
        middle,
        color: getComputedStyle(element).color,
        font: Number.parseFloat(getComputedStyle(element).fontSize),
      };
    });
    assert.ok(icon.found, `no icon beside "${text}"`);
    assert.deepEqual(
      { role: icon.role, hidden: icon.hidden, focusable: icon.focusable, stroke: icon.stroke },
      { role: null, hidden: "true", focusable: "false", stroke: icon.color },
    );
    assert.ok(Math.abs(icon.tall - icon.font) < 0.5, `the icon is ${icon.tall}px tall, its text ${icon.font}px`);
    assert.ok(
      Math.abs(icon.center - icon.middle) < 0.5,
      `the icon's middle is at ${icon.center}px, the text's at ${icon.middle}px`,
    );
    assert.equal((await line.ariaSnapshot()).trim(), `- paragraph: ${text}`);
  },
);

// An icon-only control is named by a label of its own (`aria-label`), and
// the icon inside it stays decoration, so a screen reader reads the name
// once and never an image inside a button.
Then(
  "the button {string} shows only an icon, which a screen reader skips",
  async function (this: BrowserWorld, name: string) {
    const button = this.page.getByRole("button", { name, exact: true });
    await expect(button).toHaveCount(1);
    await expect(button).toHaveAttribute("aria-label", name);
    await expect(button).toHaveText("");
    const icon = button.locator("svg");
    await expect(icon).toHaveCount(1);
    await expect(icon).toHaveAttribute("aria-hidden", "true");
    await expect(icon).toHaveAttribute("focusable", "false");
    await expect(button.getByRole("img")).toHaveCount(0);
  },
);

// A toggle button that shows only an icon shows its state in the icon's
// shape, besides `aria-pressed`, so a sighted reader sees what a screen
// reader hears: Lucide's bell (its clapper, which both bells draw) and, on
// the crossed-out one, the slash from corner to corner of its 24-unit box.
const bellClapper = 'path[d="M10.26 21a2 2 0 0 0 3.46 0"]';
const bellSlash = 'path[d="m2 2 20 20"]';

Then("the button {string} shows a crossed-out bell", async function (this: BrowserWorld, name: string) {
  const icon = this.page.getByRole("button", { name, exact: true }).locator("svg");
  await expect(icon.locator(bellClapper)).toHaveCount(1);
  await expect(icon.locator(bellSlash)).toHaveCount(1);
});

Then("the button {string} shows a bell, not crossed out", async function (this: BrowserWorld, name: string) {
  const icon = this.page.getByRole("button", { name, exact: true }).locator("svg");
  await expect(icon.locator(bellClapper)).toHaveCount(1);
  await expect(icon.locator(bellSlash)).toHaveCount(0);
});

// Two components of one size share one length (technical design §4.5, the
// shared vocabulary's scale), so an icon and a spinner of the same size line
// up. Each image is read alone, as `expect` finds it (issue #774). Half a
// pixel of rounding is allowed.
Then(
  "the image {string} is as tall as the image {string}",
  async function (this: BrowserWorld, name: string, other: string) {
    const tall = async (image: string) => {
      const found = this.page.getByRole("img", { name: image, exact: true });
      await expect(found).toHaveCount(1);
      return found.evaluate((element) => element.getBoundingClientRect().height);
    };
    const [one, two] = [await tall(name), await tall(other)];
    assert.ok(one > 0 && Math.abs(one - two) < 0.5, `"${name}" is ${one}px tall, "${other}" ${two}px`);
  },
);

// A tone (technical design §4.5) colors the icon's glyph, and the words
// beside it name the tone, so the color never says it alone (1.4.1). A
// token is read through a probe, as the page resolves it. Inherit must
// follow the text around the icon, not a tone an outer part sets, so the
// step gives the line around it a color and a tone of its own and reads the
// glyph against each. Each tone is found by its words, the one element that
// holds them and its icon, and read alone with `evaluate` (issue #774).
Then(
  "each tone of the {string} demo, named in words, draws its icon in it:",
  async function (this: BrowserWorld, component: string, tones: DataTable) {
    const section = demoSection(this.page, component);
    for (const { tone, color: token } of tones.hashes() as { tone: string; color: string }[]) {
      const named = section.getByText(tone, { exact: true });
      await expect(named).toHaveCount(1);
      const drawn = await named.evaluate((element, token) => {
        const svg = element.querySelector(":scope > svg");
        const stroke = () => (svg ? getComputedStyle(svg).stroke : "");
        const decoration = svg?.getAttribute("aria-hidden") ?? null;
        if (token.startsWith("--")) {
          const probe = document.createElement("span");
          probe.style.color = `var(${token})`;
          document.body.append(probe);
          const color = getComputedStyle(probe).color;
          probe.remove();
          return { decoration, expected: [color], drawn: [stroke()] };
        }
        const line = element.parentElement;
        if (!line) return { decoration, expected: ["a line around the icon"], drawn: [] };
        const drawn: string[] = [];
        const expected = ["rgb(1, 2, 3)", "rgb(200, 100, 50)"];
        for (const color of expected) {
          line.style.color = color;
          line.style.setProperty("--tone", "rgb(9, 9, 9)");
          drawn.push(stroke());
        }
        line.style.color = "";
        line.style.removeProperty("--tone");
        return { decoration, expected, drawn };
      }, token);
      assert.equal(drawn.decoration, "true", `the icon beside "${tone}" is decoration`);
      assert.deepEqual(drawn.drawn, drawn.expected, `the icon beside "${tone}"`);
    }
  },
);

// Forced colors (the reader's own palette): every icon follows the tone rule
// of the shared vocabulary, the text's own color whatever its tone, as the
// text around it is drawn.
Then(
  "every icon of the {string} demo is drawn in the color of the text around it",
  async function (this: BrowserWorld, component: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: 5_000 });
    const icons = await section.evaluate((element) =>
      Array.from(element.querySelectorAll("svg"), (svg) => ({
        name: svg.getAttribute("aria-label") || svg.parentElement?.textContent?.trim() || svg.parentElement?.getAttribute("aria-label") || "",
        stroke: getComputedStyle(svg).stroke,
        text: svg.parentElement ? getComputedStyle(svg.parentElement).color : "",
      })),
    );
    assert.ok(icons.length > 0, `the ${component} demo has no icon`);
    for (const icon of icons) {
      assert.equal(icon.stroke, icon.text, icon.name);
    }
  },
);

// A sentence of a demo, one of its paragraphs, read without the bidi
// isolation marks a number sits in (§8.4), which a reader never sees. The
// button demo's counts its buttons' presses, so a press that reached the
// page's handler shows in it.
Then("the {string} demo shows the sentence {string}", async function (this: BrowserWorld, component: string, sentence: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: 5_000 });
  await expect
    .poll(
      () =>
        section.evaluate((element) =>
          Array.from(element.querySelectorAll(":scope > p"), (line) =>
            (line.textContent ?? "").replace(/[\u2066-\u2069]/g, "").trim(),
          ),
        ),
      { message: `the ${component} demo never showed "${sentence}"`, timeout: 5_000 },
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
// by its name and read alone with `evaluate` (issue #774).
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
    await this.page.locator(".button:visible").first().waitFor({ timeout: 5_000 });
    const found = await this.page.evaluate(() => ({
      rtl: getComputedStyle(document.documentElement).direction === "rtl",
      buttons: Array.from(document.querySelectorAll(".button")).flatMap((button) => {
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
Then(
  "the button {string} says it is unavailable, and is not disabled",
  async function (this: BrowserWorld, name: string) {
    const button = this.page.getByRole("button", { name, exact: true });
    await expect(button).toHaveAttribute("aria-disabled", "true");
    await expect(button).not.toHaveAttribute("disabled");
    await expect(button).toBeFocused();
  },
);

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
  notedButtonWidths.set(this, new Map([
    [first, await buttonWidth(this, first)],
    [second, await buttonWidth(this, second)],
  ]));
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
  await section.waitFor({ timeout: 5_000 });
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

// A text field (§4.5), found by its label as a screen reader finds it, the
// whole of it ("New password" is never "New password again"): its input,
// and the parts the input's id names, the label above it, its description
// and its error.
const textField = (world: BrowserWorld, name: string): Locator =>
  world.page.getByRole("textbox", { name: namePattern(name) });

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
    tops.forEach((rect, at) => assert.ok(rect, `"${name}" shows no ${parts[at]}`));
    for (let at = 1; at < tops.length; at++) {
      const [above, below] = [tops[at - 1]!, tops[at]!];
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
Then(
  "the textbox {string} asks for the keyboard of digits, and is a text input",
  async function (this: BrowserWorld, name: string) {
    const field = textField(this, name);
    await expect(field).toHaveAttribute("inputmode", "numeric");
    await expect(field).toHaveAttribute("type", "text");
  },
);

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
    const [a, b] = await Promise.all(
      [field, pressed].map((locator) =>
        locator.evaluate((element) => {
          const rect = element.getBoundingClientRect();
          return { top: rect.top, bottom: rect.bottom, left: rect.left, right: rect.right };
        }),
      ),
    );
    assert.ok(Math.abs(a!.bottom - a!.top - (b!.bottom - b!.top)) < 0.5, `"${name}" ${JSON.stringify(a)}, "${button}" ${JSON.stringify(b)}`);
    assert.ok(Math.abs(a!.bottom - b!.bottom) < 0.5, `"${name}" and "${button}" don't stand level`);
    assert.ok(a!.right <= b!.left || b!.right <= a!.left, `"${button}" isn't beside "${name}"`);
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
    await section.waitFor({ timeout: 5_000 });
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
    assert.ok(found.inputs.some((input) => input.disabled), `the ${component} demo has no disabled field`);
    for (const input of found.inputs) {
      const ink = input.disabled ? found.gray : found.text;
      assert.deepEqual(input.drawn, { text: ink, fill: found.field, edge: ink }, input.id);
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

// A link (technical design §4.5): a page of the app is a route change, the
// page the reader is on is marked and bold, a link to another site says so
// in its name and with an icon a screen reader skips, and every link is
// underlined, in Ladder Blue or the color of the text around it, and in the
// system's link color under forced colors. Each link is read alone with
// `evaluate`, never with `evaluateAll` (issue #774).

// The links of the gallery's "Link" demo.
const LINK_DEMO_LINKS = 6;

function linkNamed(page: Page, name: string): Locator {
  return page.getByRole("link", { name: namePattern(name) });
}

type Marked = { ladderbackMark?: string };

When("the page carries a mark that a reload would lose", async function (this: BrowserWorld) {
  await this.page.evaluate(() => {
    (window as unknown as Marked).ladderbackMark = "kept";
  });
});

Then("the page still carries its mark", async function (this: BrowserWorld) {
  const mark = await this.page.evaluate(() => (window as unknown as Marked).ladderbackMark);
  assert.equal(mark, "kept", "the page was loaded again: the link was not a route change");
});

Then("the link {string} is the current page, drawn bold", async function (this: BrowserWorld, name: string) {
  const link = linkNamed(this.page, name);
  await expect(link).toHaveAttribute("aria-current", "page");
  const weight = await link.evaluate((el) => Number(getComputedStyle(el).fontWeight));
  assert.ok(weight >= 700, `the link "${name}" is drawn at weight ${weight}, not bold`);
});

Then(
  "the link {string} is not the current page, drawn in the weight of the text around it",
  async function (this: BrowserWorld, name: string) {
    const link = linkNamed(this.page, name);
    await expect(link).not.toHaveAttribute("aria-current");
    const weights = await link.evaluate((el) => ({
      own: getComputedStyle(el).fontWeight,
      around: getComputedStyle(el.parentElement ?? el).fontWeight,
    }));
    assert.equal(weights.own, weights.around, `the link "${name}" is drawn at another weight than its text`);
  },
);

// The link's words come first, then its one icon, at the inline end of the
// words' last line in the line's direction, hidden from a screen reader;
// what the name adds after the words is in a box no sighted reader sees.
Then(
  "the link {string} shows the words {string}, then an icon a screen reader skips",
  async function (this: BrowserWorld, name: string, words: string) {
    const link = linkNamed(this.page, name);
    const found = await link.evaluate((el) => {
      const icons = el.querySelectorAll("svg");
      const icon = icons.item(0);
      if (icons.length !== 1 || icon === null) return { icons: icons.length };
      const range = document.createRange();
      range.setStart(el, 0);
      range.setEndBefore(icon);
      const last = [...range.getClientRects()].filter((rect) => rect.width > 0).at(-1);
      const box = icon.getBoundingClientRect();
      const rtl = getComputedStyle(el).direction === "rtl";
      const note = icon.nextElementSibling;
      return {
        icons: 1,
        words: range.toString().trim(),
        hidden: icon.getAttribute("aria-hidden"),
        after: last !== undefined && (rtl ? box.right <= last.left + 1 : box.left >= last.right - 1),
        noteUnseen: note !== null && getComputedStyle(note).clipPath === "inset(50%)" && note.nextSibling === null,
      };
    });
    assert.equal(found.icons, 1, `the link "${name}" has ${found.icons} icons`);
    assert.equal(found.words, words, `the words the link "${name}" shows`);
    assert.equal(found.hidden, "true", `the icon of the link "${name}" is not hidden from a screen reader`);
    assert.ok(found.after, `the icon of the link "${name}" doesn't stand after its words in the line's direction`);
    assert.ok(found.noteUnseen, `the rest of the link "${name}"'s name is not in a box no sighted reader sees`);
  },
);

// An outside address is never routed, even one that turns out to be the
// app's own: `external` keeps the router off it.
Then(
  "the link {string} is marked external, sends no referrer and opens where the reader is",
  async function (this: BrowserWorld, name: string) {
    const found = await linkNamed(this.page, name).evaluate((el) => ({
      rel: (el.getAttribute("rel") ?? "").split(/\s+/),
      target: el.getAttribute("target"),
    }));
    assert.ok(found.rel.includes("external"), `the link "${name}" has rel ${JSON.stringify(found.rel)}`);
    assert.ok(found.rel.includes("noreferrer"), `the link "${name}" has rel ${JSON.stringify(found.rel)}`);
    assert.equal(found.target, null, `the link "${name}" opens in ${found.target}`);
  },
);

// A link's underline thickness, as the page draws it now.
function underlineOf(link: Locator): Promise<number> {
  return link.evaluate((el) => Number.parseFloat(getComputedStyle(el).textDecorationThickness));
}

/** A link's underline before the pointer came to rest on it, per page. */
const underlineAtRest = new WeakMap<Page, number>();

// The link may sit below the window's edge, where a pointer can't reach; the
// page scrolls it into view first, as a reader would, and its underline is
// read before the pointer arrives.
When("I rest the pointer on the link {string}", async function (this: BrowserWorld, name: string) {
  const link = linkNamed(this.page, name);
  await link.scrollIntoViewIfNeeded();
  underlineAtRest.set(this.page, await underlineOf(link));
  await pointer(this.page).restOn(link);
});

Then("the underline of the link {string} is thicker than at rest", async function (this: BrowserWorld, name: string) {
  const atRest = underlineAtRest.get(this.page);
  assert.ok(atRest !== undefined, "the pointer never came to rest on a link");
  await expect.poll(() => underlineOf(linkNamed(this.page, name))).toBeGreaterThan(atRest);
});

// Underlined at rest, its line at least a pixel thick and far enough under
// the baseline (a fifth of the text's size) to clear the letters' descenders,
// which ink skipping keeps it off as well.
Then("every link of the {string} demo is underlined below its letters", async function (this: BrowserWorld, component: string) {
  const links = demoSection(this.page, component).getByRole("link");
  for (const link of await eachOf(links, LINK_DEMO_LINKS)) {
    const drawn = await link.evaluate((el) => {
      const style = getComputedStyle(el);
      return {
        name: (el.textContent ?? "").trim(),
        line: style.textDecorationLine,
        thickness: Number.parseFloat(style.textDecorationThickness),
        offset: Number.parseFloat(style.textUnderlineOffset) / Number.parseFloat(style.fontSize),
        skip: style.textDecorationSkipInk,
      };
    });
    assert.ok(drawn.line.includes("underline"), `the link "${drawn.name}" is not underlined: ${drawn.line}`);
    assert.ok(drawn.thickness >= 1, `the link "${drawn.name}"'s underline is ${drawn.thickness}px thick`);
    assert.ok(drawn.offset >= 0.2, `the link "${drawn.name}"'s underline is ${drawn.offset}em under its baseline`);
    assert.notEqual(drawn.skip, "none", `the link "${drawn.name}"'s underline crosses its letters`);
  }
});

Then(
  "each link of the {string} demo is drawn in its tone:",
  async function (this: BrowserWorld, component: string, table: DataTable) {
    const rows = table.hashes() as { link: string; color: string }[];
    const section = demoSection(this.page, component);
    await eachOf(section.getByRole("link"), rows.length);
    for (const { link: name, color } of rows) {
      const drawn = await section.getByRole("link", { name: namePattern(name) }).evaluate((el, token) => {
        let want = getComputedStyle(el.parentElement ?? el).color;
        if (token.startsWith("--")) {
          const probe = document.createElement("span");
          probe.style.color = `var(${token})`;
          document.body.append(probe);
          want = getComputedStyle(probe).color;
          probe.remove();
        }
        const style = getComputedStyle(el);
        return { want, text: style.color, underline: style.textDecorationColor };
      }, color);
      assert.equal(drawn.text, drawn.want, `the link "${name}" is not drawn in ${color}`);
      assert.equal(drawn.underline, drawn.want, `the link "${name}"'s underline is not drawn in ${color}`);
    }
  },
);

Then("every link of the {string} demo is drawn in the system's link color", async function (this: BrowserWorld, component: string) {
  const links = demoSection(this.page, component).getByRole("link");
  for (const link of await eachOf(links, LINK_DEMO_LINKS)) {
    const drawn = await link.evaluate((el) => {
      const probe = document.createElement("span");
      probe.style.forcedColorAdjust = "none";
      probe.style.color = "LinkText";
      document.body.append(probe);
      const want = getComputedStyle(probe).color;
      probe.remove();
      const style = getComputedStyle(el);
      return { name: (el.textContent ?? "").trim(), want, text: style.color, underline: style.textDecorationColor };
    });
    assert.equal(drawn.text, drawn.want, `the link "${drawn.name}" is not drawn in LinkText`);
    assert.equal(drawn.underline, drawn.want, `the link "${drawn.name}"'s underline is not drawn in LinkText`);
  }
});

Then("the link {string} wraps onto more than one line", async function (this: BrowserWorld, name: string) {
  const lines = await linkNamed(this.page, name).evaluate((el) => {
    const tops = [...el.getClientRects()].filter((rect) => rect.width > 0).map((rect) => Math.round(rect.top));
    return new Set(tops).size;
  });
  assert.ok(lines > 1, `the link "${name}" stands on ${lines} line`);
});

// The pinned structure of each demo (roles and names, not styling), compared
// with locator.ariaSnapshot(): toMatchAriaSnapshot() only works inside a
// Playwright test, and these run under Cucumber. Regenerate a snapshot by
// logging demoSection(page, name).ariaSnapshot() and pasting it here.
const SNAPSHOTS: Record<string, string> = {
  Dialog: `- heading "Dialog" [level=2]
- button "Open the demo dialog"`,
  Table: `- heading "Table" [level=2]
- group "Demo table shows":
  - text: Demo table shows
  - radio "Demo rows" [checked]
  - text: Demo rows
  - radio "Demo loading"
  - text: Demo loading
  - radio "Demo empty"
  - text: Demo empty
  - radio "Demo unreachable"
  - text: Demo unreachable
- region "Demo folders":
  - table "Demo folders":
    - caption: Demo folders
    - rowgroup:
      - row "Name Sort by name Size Sort by size Changed Sort by changed Files Sort by files Actions":
        - columnheader "Name Sort by name":
          - text: Name
          - button "Sort by name"
        - columnheader "Size Sort by size":
          - text: Size
          - button "Sort by size"
        - columnheader "Changed Sort by changed":
          - text: Changed
          - button "Sort by changed"
        - columnheader "Files Sort by files":
          - text: Files
          - button "Sort by files"
        - columnheader "Actions"
    - rowgroup:
      - row "photos 4.2 GiB Oct 4, 2026 1,204 Add a file to ⁨photos⁩":
        - rowheader "photos"
        - cell "4.2 GiB"
        - cell "Oct 4, 2026"
        - cell "1,204"
        - cell "Add a file to ⁨photos⁩":
          - button "Add a file to ⁨photos⁩"
      - row "music 12.6 GiB Aug 21, 2026 12 Add a file to ⁨music⁩":
        - rowheader "music"
        - cell "12.6 GiB"
        - cell "Aug 21, 2026"
        - cell "12"
        - cell "Add a file to ⁨music⁩":
          - button "Add a file to ⁨music⁩"
      - row "documents 820 MiB Nov 2, 2026 312 Add a file to ⁨documents⁩":
        - rowheader "documents"
        - cell "820 MiB"
        - cell "Nov 2, 2026"
        - cell "312"
        - cell "Add a file to ⁨documents⁩":
          - button "Add a file to ⁨documents⁩"
      - row "notes 1.5 MiB Nov 12, 2026 12 Add a file to ⁨notes⁩":
        - rowheader "notes"
        - cell "1.5 MiB"
        - cell "Nov 12, 2026"
        - cell "12"
        - cell "Add a file to ⁨notes⁩":
          - button "Add a file to ⁨notes⁩"
    - rowgroup:
      - row "Total 17.6 GiB 1,540":
        - rowheader "Total"
        - cell "17.6 GiB"
        - cell
        - cell "1,540"
        - cell
- region "Demo folders in a narrow frame":
  - table "Demo folders in a narrow frame":
    - caption: Demo folders in a narrow frame
    - rowgroup:
      - row "Name Size Changed Files":
        - columnheader "Name"
        - columnheader "Size"
        - columnheader "Changed"
        - columnheader "Files"
    - rowgroup:
      - row "photos 4.2 GiB Oct 4, 2026 1,204":
        - rowheader "photos"
        - cell "4.2 GiB"
        - cell "Oct 4, 2026"
        - cell "1,204"
      - row "music 12.6 GiB Aug 21, 2026 12":
        - rowheader "music"
        - cell "12.6 GiB"
        - cell "Aug 21, 2026"
        - cell "12"
      - row "documents 820 MiB Nov 2, 2026 312":
        - rowheader "documents"
        - cell "820 MiB"
        - cell "Nov 2, 2026"
        - cell "312"
      - row "notes 1.5 MiB Nov 12, 2026 12":
        - rowheader "notes"
        - cell "1.5 MiB"
        - cell "Nov 12, 2026"
        - cell "12"
- region "Demo folders, right to left":
  - table "Demo folders, right to left":
    - caption: Demo folders, right to left
    - rowgroup:
      - row "Name Size Changed Files":
        - columnheader "Name"
        - columnheader "Size"
        - columnheader "Changed"
        - columnheader "Files"
    - rowgroup:
      - row "photos 4.2 GiB Oct 4, 2026 1,204":
        - rowheader "photos"
        - cell "4.2 GiB"
        - cell "Oct 4, 2026"
        - cell "1,204"
      - row "music 12.6 GiB Aug 21, 2026 12":
        - rowheader "music"
        - cell "12.6 GiB"
        - cell "Aug 21, 2026"
        - cell "12"
      - row "documents 820 MiB Nov 2, 2026 312":
        - rowheader "documents"
        - cell "820 MiB"
        - cell "Nov 2, 2026"
        - cell "312"
      - row "notes 1.5 MiB Nov 12, 2026 12":
        - rowheader "notes"
        - cell "1.5 MiB"
        - cell "Nov 12, 2026"
        - cell "12"`,
  "Text field": `- heading "Text field" [level=2]
- text: Repository name
- paragraph: The name of the repository to check.
- textbox "Repository name"
- paragraph:
  - button "Check the repository name"
- text: Demo folders
- paragraph: One folder or file a line.
- textbox "Demo folders"
- paragraph:
  - button "Check the demo folders"
- text: Demo password
- paragraph: "A demo field: what is typed here goes nowhere."
- textbox "Demo password"
- button "Show password"
- text: Demo copies to keep
- paragraph: A whole number, such as 7.
- textbox "Demo copies to keep"
- text: Demo backup time
- paragraph: When the demo backup starts, such as 02:00.
- textbox "Demo backup time"
- text: Demo plan name
- paragraph: A demo field that must be filled in.
- textbox "Demo plan name"
- text: Demo host
- paragraph: The demo machine's host name.
- paragraph: Enter a host name, such as backup.example.
- textbox "Demo host" [invalid]
- text: Demo snapshot ID
- paragraph: The demo snapshot's short ID.
- paragraph: "Read only: you can select and copy it, but not change it."
- textbox "Demo snapshot ID": 1a2b3c4d
- text: Demo repository location
- paragraph: Set when the demo repository was made.
- paragraph: "Unavailable: it can't be changed now."
- textbox "Demo repository location" [disabled]: /srv/demo
- text: Demo keyring password
- paragraph: Kept in the demo keyring, and changed there.
- paragraph: "Unavailable: it can't be changed now."
- textbox "Demo keyring password" [disabled]
- text: Demo tag at the Small size
- textbox "Demo tag at the Small size"
- button "Add the demo tag at the Small size"
- text: Demo tag at the Medium size
- textbox "Demo tag at the Medium size"
- button "Add the demo tag at the Medium size"
- text: Demo tag at the Large size
- textbox "Demo tag at the Large size"
- button "Add the demo tag at the Large size"
- text: Demo tag at the Extra large size
- textbox "Demo tag at the Extra large size"
- button "Add the demo tag at the Extra large size"`,
  Checkbox: `- heading "Checkbox" [level=2]
- checkbox "Send a demo notification"
- text: Send a demo notification
- paragraph: "A demo choice: turning it on changes nothing."
- checkbox "Keep the demo log" [checked]
- text: Keep the demo log`,
  "Radio group": `- heading "Radio group" [level=2]
- group "Demo check frequency":
  - text: Demo check frequency
  - paragraph: "A demo choice: choosing one changes nothing."
  - radio "Every day"
  - text: Every day
  - radio "Every week" [checked]
  - text: Every week
  - radio "Every month"
  - text: Every month
- group "Demo language":
  - text: Demo language
  - radio "English" [checked]
  - text: English
  - radio "日本語"
  - text: 日本語
  - radio "العربية"
  - text: العربية`,
  "Status badge": `- heading "Status badge" [level=2]
- paragraph: Queued Running Succeeded Succeeded with warnings Failed Cancelled Empty`,
  Progress: `- heading "Progress" [level=2]
- progressbar "Demo backup progress": Backing up… 40%
- paragraph:
  - button "Advance the demo progress"
- progressbar "Demo progress of unknown size": Working…
- progressbar "Demo progress of a step": Reading the index…
- progressbar "Demo progress at the Small size": Backing up… 60%
- progressbar "Demo progress at the Medium size": Backing up… 60%
- progressbar "Demo progress at the Large size": Backing up… 60%
- progressbar "Demo progress at the Extra large size": Backing up… 60%
- progressbar "Demo progress in the Primary tone": Backing up… 60%
- progressbar "Demo progress in the Secondary tone": Backing up… 60%
- progressbar "Demo progress in the Success tone": Backing up… 60%
- progressbar "Demo progress in the Warning tone": Backing up… 60%
- progressbar "Demo progress in the Error tone": Backing up… 60%
- progressbar "Demo progress in the Inherit tone": Backing up… 60%`,
  Toast: `- heading "Toast" [level=2]
- paragraph:
  - button "Save the demo settings"`,
  "Review dialog": `- heading "Review dialog" [level=2]
- paragraph:
  - button "Open a demo review"
- paragraph:
  - button "Open an empty demo review"
- paragraph:
  - button "Open a demo secret"
- paragraph:
  - button "Open a held demo review"`,
  "Pause control": `- heading "Pause control" [level=2]
- button "Pause the demo updates"
- paragraph:
  - button "Finish a demo run"`,
  Tree: `- heading "Tree" [level=2]
- tree "Demo snapshot":
  - treeitem "documents" [level=1] [selected]
  - treeitem "empty" [level=1]
  - treeitem "many" [level=1]
  - treeitem "slow" [level=1]
  - treeitem "unreadable" [level=1]
  - treeitem "latest" [level=1]
  - treeitem "pipe" [level=1]
  - treeitem "abc[U+202E]txt.exe" [level=1]
  - treeitem "הערות.txt" [level=1]
  - treeitem "readme.txt" [level=1]`,
  Tabs: `- heading "Tabs" [level=2]
- heading "Demo repository" [level=3]
- tablist "Demo repository":
  - tab "Demo overview" [selected]
  - tab "Demo note"
  - tab "Demo history"
- tabpanel "Demo overview":
  - paragraph: The demo repository holds 12 snapshots of two demo plans.
- paragraph: These demo tabs are chosen with Enter or Space. The arrow keys only move between them.
- tablist "Demo agent":
  - tab "Demo log" [selected]
  - tab "Demo stats"
  - tab "Demo plans"
- tabpanel "Demo log":
  - paragraph: A demo panel that a page would read from the daemon once its tab is chosen.
- paragraph: "Chosen demo tab: \u2068Demo log\u2069"
- tablist "Demo tabs, right to left":
  - tab "First demo tab" [selected]
  - tab "Second demo tab"
  - tab "Third demo tab"
- tabpanel "First demo tab":
  - paragraph: The first demo panel.`,
  "Vertical tabs": `- heading "Vertical tabs" [level=2]
- tablist "Demo plan":
  - tab "Demo sources" [selected]
  - tab "Demo times"
  - tab "Demo hooks"
- tabpanel "Demo sources":
  - paragraph: The demo plan backs up two demo folders.
- tablist "Demo plan, right to left":
  - tab "First vertical tab" [selected]
  - tab "Second vertical tab"
  - tab "Third vertical tab"
- tabpanel "First vertical tab":
  - paragraph: The first vertical panel.`,
  "Responsive tabs": `- heading "Responsive tabs" [level=2]
- tablist "Demo settings":
  - tab "Demo general" [selected]
  - tab "Demo address"
  - tab "Demo about"
- tabpanel "Demo general":
  - paragraph: The demo settings shown here change nothing.
- tablist "Demo settings in a narrow column, right to left":
  - tab "First column tab" [selected]
  - tab "Second column tab"
  - tab "Third column tab"
- tabpanel "First column tab":
  - paragraph: The first column panel.`,
  "Shared vocabulary": `- heading "Shared vocabulary" [level=2]
- heading "Tones" [level=3]
- list "Tones":
  - listitem: Primary
  - listitem: Secondary
  - listitem: Success
  - listitem: Warning
  - listitem: Error
  - listitem: Inherit
- heading "Status pairs" [level=3]
- list "Status pairs":
  - listitem: Success
  - listitem: Warning
  - listitem: Error
- heading "Sizes" [level=3]
- list "Sizes":
  - listitem: Small
  - listitem: Medium
  - listitem: Large
  - listitem: Extra large
- heading "Placements" [level=3]
- list "Placements":
  - listitem: Inline
  - listitem: Start
  - listitem: Center
  - listitem: End`,
  Spinner: `- heading "Spinner" [level=2]
- paragraph: "Step 2 of 3: Loading the demo snapshots…"
- paragraph:
  - img "Demo spinner at the Small size"
  - img "Demo spinner at the Medium size"
  - img "Demo spinner at the Large size"
  - img "Demo spinner at the Extra large size"
- paragraph:
  - img "Demo spinner in the Primary tone"
  - img "Demo spinner in the Secondary tone"
  - img "Demo spinner in the Success tone"
  - img "Demo spinner in the Warning tone"
  - img "Demo spinner in the Error tone"
  - img "Demo spinner in the Inherit tone"
- text: Loading at the start of the line… Loading in the middle of the line… Loading at the end of the line…
- paragraph:
  - button "Save the demo notes" [disabled]`,
  Icon: `- heading "Icon" [level=2]
- paragraph: The next demo backup starts at 02:00 every night.
- paragraph:
  - img "Demo icon at the Small size"
  - img "Demo icon at the Medium size"
  - img "Demo icon at the Large size"
  - img "Demo icon at the Extra large size"
- paragraph: Primary Secondary Success Warning Error Inherit
- paragraph:
  - button "Demo notifications"`,
  Button: `- heading "Button" [level=2]
- paragraph:
  - button "Filled Primary"
  - button "Filled Secondary"
  - button "Filled Success"
  - button "Filled Warning"
  - button "Filled Error"
- paragraph:
  - button "Outlined Primary"
  - button "Outlined Secondary"
  - button "Outlined Success"
  - button "Outlined Warning"
  - button "Outlined Error"
  - button "Outlined Inherit"
- paragraph:
  - button "Ghost Primary"
  - button "Ghost Secondary"
  - button "Ghost Success"
  - button "Ghost Warning"
  - button "Ghost Error"
  - button "Ghost Inherit"
- paragraph:
  - button "Demo button at the Small size"
  - button "Demo button at the Medium size"
  - button "Demo button at the Large size"
  - button "Demo button at the Extra large size"
- paragraph:
  - button "Schedule a demo backup"
  - button "Demo details at the Small size"
  - button "Demo details at the Medium size"
  - button "Demo details at the Large size"
  - button "Demo details at the Extra large size"
- paragraph:
  - button "Demo work in progress"
  - button "Save the demo plan"
  - button "Check the demo repository"
  - button "Refresh the demo"
- paragraph:
  - button "Delete the demo snapshot" [disabled]
  - button "Show the demo's hidden files"
- paragraph:
  - button "Send the demo form"
- paragraph: The demo buttons haven't been pressed yet.`,
  Tooltip: `- heading "Tooltip" [level=2]
- paragraph:
  - button "Prune the demo repository"
- paragraph:
  - button "Demo schedule"
- paragraph:
  - button "Demo note above"
  - button "Demo note below"
  - button "Demo note at the start"
  - button "Demo note at the end"
- paragraph:
  - button "Demo note at the start, right to left"
  - button "Demo note at the end, right to left"
- paragraph:
  - button "Demo long note"
- paragraph:
  - button "Demo note at the foot of the page"`,
  // A link inside a sentence stands between the isolation marks (U+2068,
  // U+2069) the catalog puts around its words, which stay in the text
  // beside it.
  Link: `- heading "Link" [level=2]
- paragraph:
  - link "Demo link to the start page":
    - /url: /
- paragraph:
  - link "Demo link to this page":
    - /url: /_gallery
- paragraph:
  - link "Demo rustic documentation (goes to another site)":
    - /url: https://rustic.cli.rs/
- paragraph:
  - text: Read \u2068
  - link "the demo plan format":
    - /url: /plans
  - text: \u2069 before you write a demo plan.
- paragraph:
  - text: "The demo check failed: \u2068"
  - link "the demo log":
    - /url: /log
  - text: \u2069 says why.
- paragraph:
  - link "Demo rustic documentation, right to left (goes to another site)":
    - /url: https://rustic.cli.rs/`,
  "Step-up field": `- heading "Step-up field" [level=2]
- paragraph:
  - button "Open the demo step-up"
- paragraph:
  - button "Open the demo short-ID step-up"`,
  // The skeletons are hidden from a screen reader, so only the busy
  // region's words and the control that brings its content are here.
  Skeleton: `- heading "Skeleton" [level=2]
- paragraph: Loading the demo plan…
- paragraph:
  - button "Show the demo plan"`,
  // Each alert is its icon, an image named for its tone, then its words,
  // with no role of its own: no live region. The notice ends with its
  // Dismiss button, whose tooltip is hidden from a screen reader; the
  // button that brings it back is unavailable while it shows.
  Alert: `- heading "Alert" [level=2]
- img "Information"
- text: The demo plan backs up the demo folder every night.
- img "Success"
- text: The demo backup finished and added 12 files.
- img "Warning"
- text: "The demo repository hasn't been checked in 30 days: \u2068"
- link "the demo operations":
  - /url: /operations
- text: \u2069 lists its last runs.
- img "Error"
- text: The demo repository at /srv/demo-backups/ladderback-demo-repository couldn't be read, so nothing was backed up.
- img "Information"
- text: This demo notice goes when you dismiss it.
- button "Dismiss"
- paragraph:
  - button "Show the demo notice again" [disabled]`,
};
