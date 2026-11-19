// Journey steps for the shared toast in the gallery (technical design §4.5):
// the answer to what the reader did, an icon that names its tone in words,
// the words, then a Dismiss button. A toast has no role, so it has no name
// to find it by: it is found by its class inside <main>, which holds one at
// a time, or inside its demo, found by its heading, or by its id under a
// pseudolocale. Its Dismiss button is found by role and name and pressed by
// keyboard alone; the pointer only rests on the toast, as a reader reading
// it does (support/pointer.ts).

import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import { journey, pressUntilFocused } from "../support/keyboard.ts";
import { pointer } from "../support/pointer.ts";
import type { BrowserWorld } from "../support/world.ts";
import { demoSection } from "./components.ts";

// Half a pixel of rounding in a laid-out length.
const ROUNDING_PX = 0.5;

const near = (a: number, b: number, slack = ROUNDING_PX) => Math.abs(a - b) <= slack;

// A page's words isolate each name they hold (§4.5), as `\u2068…\u2069`;
// what a reader hears has no such marks.
const ISOLATION_MARKS = /[\u2066-\u2069]/g;

// The page's toast: one at a time, a new one taking the place of the one shown.
const toast = async (page: Page): Promise<Locator> => {
  const found = page.locator("main .toast");
  await expect(found).toHaveCount(1);
  await expect(found).toBeVisible();
  return found;
};

// What a screen reader finds in it: the icon, an image named for its tone,
// before the words, then the Dismiss button, whose tooltip only shows its
// name. Nothing is said by the toast itself (the announcer says it). Read
// with locator.ariaSnapshot(), since toMatchAriaSnapshot() only works inside
// a Playwright test; it quotes words that hold a colon before a space, as
// YAML must, and words that hold a name, as it does around any mark.
export async function toastReads(page: Page, words: string, name: string): Promise<void> {
  const found = await toast(page);
  const snapshot = (await found.ariaSnapshot()).trim().replace(ISOLATION_MARKS, "");
  const quoted = /:(\s|$)/.test(words) ? JSON.stringify(words) : words;
  const expected = (text: string) => `- img ${JSON.stringify(name)}
- paragraph: ${text}
- button "Dismiss"`;
  // The marks a name carries make the snapshot quote the words; once they
  // are taken out, the words are read either way.
  assert.ok(
    snapshot === expected(quoted) || snapshot === expected(JSON.stringify(words)),
    `the toast reads:\n${snapshot}\nnot:\n${expected(quoted)}`,
  );
}

Then("the toast reads {string} after its icon {string}", async function (this: BrowserWorld, words: string, name: string) {
  await toastReads(this.page, words, name);
});

// The toast's own Dismiss button: the alert demo further down has buttons
// of that name too, so the walk is to the one in the toast.
When("I press Tab until focus is on the toast's Dismiss button", async function (this: BrowserWorld) {
  const dismiss = (await toast(this.page)).getByRole("button", { name: "Dismiss", exact: true });
  await pressUntilFocused(this.page, "Tab", dismiss, "the toast's Dismiss button");
});

Then("the {string} demo shows no toast", async function (this: BrowserWorld, component: string) {
  await expect(demoSection(this.page, component).locator(".toast")).toHaveCount(0);
});

// A refusal is answered where it happened, never with a toast of success.
Then("the page shows no toast", async function (this: BrowserWorld) {
  await expect(this.page.locator("main .toast")).toHaveCount(0);
});

// Not a live region (§4.5): the announcer says a toast, once, so neither the
// toast nor anything in it is a region that would say it again.
Then("the toast is no live region, nor holds one", async function (this: BrowserWorld) {
  const found = await toast(this.page);
  const live = await found.evaluate((element) =>
    [element, ...element.querySelectorAll("*")].flatMap((part) => {
      const role = part.getAttribute("role") ?? "";
      const said = ["alert", "status", "log", "marquee", "timer"].includes(role) || part.hasAttribute("aria-live");
      return said ? [part.outerHTML.slice(0, 120)] : [];
    }),
  );
  assert.deepEqual(live, [], "a live region in the toast");
});

// A pointer resting on a toast holds it, as focus on it does (2.2.1): moving
// the pointer is pointer behaviour, so these steps move it, and never press.
// The pointer moves to what the window shows, so the toast is first
// scrolled into view, as a reader scrolls to what they read.
When("I rest the pointer on the toast", async function (this: BrowserWorld) {
  const found = await toast(this.page);
  await found.scrollIntoViewIfNeeded();
  await pointer(this.page).restOn(found);
});

When("I move the pointer off the toast", async function (this: BrowserWorld) {
  const box = await (await toast(this.page)).boundingBox();
  assert.ok(box !== null, "the toast isn't shown");
  const viewport = this.page.viewportSize();
  assert.ok(viewport !== null, "the window has no size");
  // The corner of the window farthest from the toast.
  const x = box.x > viewport.width / 2 ? 2 : viewport.width - 2;
  const y = box.y > viewport.height / 2 ? 2 : viewport.height - 2;
  await pointer(this.page).moveTo(x, y);
});

// Under a pseudolocale the buttons' names are the pseudolocale's, so the
// demo's first button is found by its role inside the demo, which the
// gallery gives an id.
When("I press Enter on the first button of the toast demo", async function (this: BrowserWorld) {
  const first = this.page.locator("#toast-demo").getByRole("button").first();
  await pressUntilFocused(this.page, "Tab", first, "the first button of the toast demo");
  await journey(this.page).press("Enter");
});

// Look (§4.5): a status tone's toast lies on its pair's fill with its words
// in the pair's words (decision 108), information's on Ledger Gray in the
// text's color; the icon and the Dismiss button take the words' color, so
// the pair's contrast covers them, and a 1px edge in its tone sets it off.
Then(
  "the toast lies on {word} with its words, its icon and its Dismiss button in {word}, edged {int} pixel(s) wide in {word}",
  async function (this: BrowserWorld, fill: string, words: string, width: number, edge: string) {
    const found = await toast(this.page);
    const drawn = await found.evaluate(
      (element, tokens) => {
        const resolve = (token: string) => {
          const probe = document.createElement("span");
          probe.style.color = `var(${token})`;
          document.body.append(probe);
          const color = getComputedStyle(probe).color;
          probe.remove();
          return color;
        };
        const style = getComputedStyle(element);
        const colorOf = (selector: string, property: "color" | "stroke") => {
          const part = element.querySelector(selector);
          return part === null ? "" : getComputedStyle(part)[property];
        };
        return {
          fill: resolve(tokens.fill),
          words: resolve(tokens.words),
          edge: resolve(tokens.edge),
          background: style.backgroundColor,
          text: colorOf(".toast-words", "color"),
          icon: colorOf(".toast-icon svg", "stroke"),
          dismiss: colorOf(".toast-dismiss button", "color"),
          edges: ["top", "right", "bottom", "left"].map((side) => ({
            side,
            width: Number.parseFloat(style.getPropertyValue(`border-${side}-width`)),
            style: style.getPropertyValue(`border-${side}-style`),
            color: style.getPropertyValue(`border-${side}-color`),
          })),
        };
      },
      { fill, words, edge },
    );
    assert.match(drawn.fill, /^rgb\(/, `${fill} is ${drawn.fill}, not an opaque color`);
    assert.equal(drawn.background, drawn.fill, "the toast's fill");
    assert.equal(drawn.text, drawn.words, "the toast's words");
    assert.equal(drawn.icon, drawn.words, "the toast's icon");
    assert.equal(drawn.dismiss, drawn.words, "the toast's Dismiss button");
    for (const side of drawn.edges) {
      assert.deepEqual([side.style, side.color], ["solid", drawn.edge], JSON.stringify(side));
      assert.ok(near(side.width, width), `an edge ${side.width}px wide, not ${width}px: ${JSON.stringify(side)}`);
    }
  },
);

// Forced colors (§4.5): the system drops the fill, so the edge, in the
// system's text color, keeps the toast's box in view; the icon takes the
// toast's text color, which a forced-colors mode makes CanvasText.
Then(
  "the toast is edged {int} pixel(s) wide in CanvasText, its icon in the text's color",
  async function (this: BrowserWorld, width: number) {
    const found = await toast(this.page);
    const drawn = await found.evaluate((element) => {
      const probe = document.createElement("span");
      probe.style.setProperty("forced-color-adjust", "none");
      probe.style.color = "CanvasText";
      document.body.append(probe);
      const text = getComputedStyle(probe).color;
      probe.remove();
      const style = getComputedStyle(element);
      const icon = element.querySelector(".toast-icon svg");
      return {
        text,
        words: style.color,
        icon: icon === null ? "" : getComputedStyle(icon).stroke,
        edges: ["top", "right", "bottom", "left"].map((side) => ({
          side,
          width: Number.parseFloat(style.getPropertyValue(`border-${side}-width`)),
          style: style.getPropertyValue(`border-${side}-style`),
          color: style.getPropertyValue(`border-${side}-color`),
        })),
      };
    });
    assert.equal(drawn.icon, drawn.words, "the icon");
    for (const side of drawn.edges) {
      assert.deepEqual([side.style, side.color], ["solid", drawn.text], JSON.stringify(side));
      assert.ok(near(side.width, width), `an edge ${side.width}px wide, not ${width}px: ${JSON.stringify(side)}`);
    }
  },
);

// Direction (§4.5, The Logical Rule): the icon stands at the start of the
// line, the left in English and the right in Arabic, and Dismiss at its end.
Then(
  "the toast has its icon before its words and its Dismiss button after them, in a page that runs {word} to {word}",
  async function (this: BrowserWorld, from: string, to: string) {
    const direction = from === "right" && to === "left" ? "rtl" : "ltr";
    const found = await toast(this.page);
    const drawn = await found.evaluate((element) => {
      const box = (selector: string) => {
        const rect = element.querySelector(selector)?.getBoundingClientRect();
        return rect === undefined ? null : { left: rect.left, right: rect.right };
      };
      return {
        direction: getComputedStyle(element).direction,
        icon: box(".toast-icon"),
        words: box(".toast-words"),
        dismiss: box(".toast-dismiss"),
      };
    });
    assert.equal(drawn.direction, direction, "the toast's direction");
    const { icon, words, dismiss } = drawn;
    assert.ok(icon !== null && words !== null && dismiss !== null, "the toast lacks its icon, its words or its Dismiss");
    if (direction === "ltr") {
      assert.ok(icon.right <= words.left + ROUNDING_PX, "the icon is after the words");
      assert.ok(dismiss.left >= words.right - ROUNDING_PX, "Dismiss is before the words");
    } else {
      assert.ok(icon.left >= words.right - ROUNDING_PX, "the icon is after the words");
      assert.ok(dismiss.right <= words.left + ROUNDING_PX, "Dismiss is before the words");
    }
  },
);

// The 320 Rule (DESIGN.md): on a small screen with larger text, the words
// wrap inside the toast, and its icon and Dismiss button stay beside them.
Then("the toast holds its icon, its words and its Dismiss button inside its edge", async function (this: BrowserWorld) {
  const found = await toast(this.page);
  const drawn = await found.evaluate((element) => {
    const outer = element.getBoundingClientRect();
    const words = element.querySelector(".toast-words");
    return {
      overflows: words === null ? true : words.scrollWidth > words.clientWidth + 1,
      parts: [".toast-icon", ".toast-words", ".toast-dismiss"].map((selector) => {
        const rect = element.querySelector(selector)?.getBoundingClientRect();
        return {
          selector,
          inside:
            rect !== undefined &&
            rect.left >= outer.left - 0.5 &&
            rect.right <= outer.right + 0.5 &&
            rect.top >= outer.top - 0.5 &&
            rect.bottom <= outer.bottom + 0.5,
        };
      }),
    };
  });
  assert.equal(drawn.overflows, false, "the toast's words run out of their room");
  for (const part of drawn.parts) assert.ok(part.inside, `${part.selector} lies outside the toast`);
});
