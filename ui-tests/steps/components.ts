// Journey steps for the shared component set in the gallery (technical design
// §4.5): the dialog, the data table, the form field, the checkbox, the
// progress bar, the toast, the pause control and the status badge. Keyboard
// only, role-and-name locators, as everywhere in steps/.

import assert from "node:assert/strict";

import AxeBuilder from "@axe-core/playwright";
import { Then, When } from "@cucumber/cucumber";
import { expect } from "playwright/test";
import type { Locator, Page } from "playwright";

import { journey, namePattern } from "../support/keyboard.ts";
import { type BrowserWorld, evaluateAll } from "../support/world.ts";

// WCAG 2.2 A and AA, as in steps/start_page.ts.
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

/** The gallery section of a component, found by its heading, not by a test id. */
function demoSection(page: Page, component: string): Locator {
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

// A toggle button tells a screen reader its state with aria-pressed (WAI-ARIA
// button pattern), and its name stays the same whichever state it is in.
Then("the button {string} is pressed", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("button", { name, exact: true, pressed: true })).toHaveCount(1);
});

Then("the button {string} is not pressed", async function (this: BrowserWorld, name: string) {
  await expect(this.page.getByRole("button", { name, exact: true, pressed: false })).toHaveCount(1);
});

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

Then("the table {string} is sorted by repository ascending", async function (this: BrowserWorld, name: string) {
  const cells = await evaluateAll(
    this.page.getByRole("table", { name }).getByRole("cell"),
    (found) => found.map((cell) => cell.textContent ?? ""),
    undefined,
  );
  assert.deepEqual(
    cells,
    [...cells].sort((a, b) => a.localeCompare(b)),
    `the table ${JSON.stringify(name)} is not sorted ascending: ${cells.join(", ")}`,
  );
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

// The pinned structure of each demo (roles and names, not styling), compared
// with locator.ariaSnapshot(): toMatchAriaSnapshot() only works inside a
// Playwright test, and these run under Cucumber. Regenerate a snapshot by
// logging demoSection(page, name).ariaSnapshot() and pasting it here.
const SNAPSHOTS: Record<string, string> = {
  Dialog: `- heading "Dialog" [level=2]
- button "Open the demo dialog"`,
  "Data table": `- heading "Data table" [level=2]
- table "Demo repositories":
  - caption: Demo repositories
  - rowgroup:
    - row "Repository Sort by repository":
      - columnheader "Repository Sort by repository":
        - text: Repository
        - button "Sort by repository"
  - rowgroup:
    - row "moor":
      - cell "moor"
    - row "harbor":
      - cell "harbor"
    - row "anchor":
      - cell "anchor"`,
  "Form field": `- heading "Form field" [level=2]
- text: Repository name
- textbox "Repository name"
- paragraph: The name of the repository to check.
- paragraph:
  - button "Check the repository name"`,
  "Multi-line form field": `- heading "Multi-line form field" [level=2]
- text: Demo folders
- textbox "Demo folders"
- paragraph: One folder or file a line.
- paragraph:
  - button "Check the demo folders"`,
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
  - text: Every month`,
  "Status badge": `- heading "Status badge" [level=2]
- paragraph: Queued Running Succeeded Succeeded with warnings Failed Cancelled Empty`,
  Progress: `- heading "Progress" [level=2]
- progressbar "Demo backup progress": Backing up… 40%
- paragraph:
  - button "Advance the demo progress"
- progressbar "Demo progress of unknown size": Working…
- progressbar "Demo progress of a step": Reading the index…`,
  Toast: `- heading "Toast" [level=2]
- paragraph:
  - button "Save the demo settings"`,
  "Review dialog": `- heading "Review dialog" [level=2]
- paragraph:
  - button "Open a demo review"
- paragraph:
  - button "Open an empty demo review"
- paragraph:
  - button "Open a demo secret"`,
  "Pause control": `- heading "Pause control" [level=2]
- button "Pause the demo updates"
- paragraph:
  - button "Finish a demo run"`,
};
