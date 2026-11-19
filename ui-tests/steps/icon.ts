// Journey steps for the shared icon in the gallery (technical design §4.5):
// decoration beside words, the icon-only button, its size and its tone.

import assert from "node:assert/strict";

import { type DataTable, Then } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import { patienceMs } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";
import { demoSection } from "../support/gallery.ts";

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
    assert.ok(Math.abs(icon.center - icon.middle) < 0.5, `the icon's middle is at ${icon.center}px, the text's at ${icon.middle}px`);
    assert.equal((await line.ariaSnapshot()).trim(), `- paragraph: ${text}`);
  },
);

// An icon-only control is named by a label of its own (`aria-label`), and
// the icon inside it stays decoration, so a screen reader reads the name
// once and never an image inside a button.
Then("the button {string} shows only an icon, which a screen reader skips", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("button", { name, exact: true });
  await expect(button).toHaveCount(1);
  await expect(button).toHaveAttribute("aria-label", name);
  await expect(button).toHaveText("");
  const icon = button.locator("svg");
  await expect(icon).toHaveCount(1);
  await expect(icon).toHaveAttribute("aria-hidden", "true");
  await expect(icon).toHaveAttribute("focusable", "false");
  await expect(button.getByRole("img")).toHaveCount(0);
});

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
// up. Each image is read alone, as `expect` finds it. Half a pixel of
// rounding is allowed.
Then("the image {string} is as tall as the image {string}", async function (this: BrowserWorld, name: string, other: string) {
  const tall = async (image: string) => {
    const found = this.page.getByRole("img", { name: image, exact: true });
    await expect(found).toHaveCount(1);
    return found.evaluate((element) => element.getBoundingClientRect().height);
  };
  const [one, two] = [await tall(name), await tall(other)];
  assert.ok(one > 0 && Math.abs(one - two) < 0.5, `"${name}" is ${one}px tall, "${other}" ${two}px`);
});

// A tone (technical design §4.5) colors the icon's glyph, and the words
// beside it name the tone, so the color never says it alone (1.4.1). A
// token is read through a probe, as the page resolves it. Inherit must
// follow the text around the icon, not a tone an outer part sets, so the
// step gives the line around it a color and a tone of its own and reads the
// glyph against each. Each tone is found by its words, the one element that
// holds them and its icon, and read alone with `evaluate`.
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
    await section.waitFor({ timeout: patienceMs() });
    const icons = await section.evaluate((element) =>
      Array.from(element.querySelectorAll("svg"), (svg) => ({
        name:
          svg.getAttribute("aria-label") ||
          svg.parentElement?.textContent?.trim() ||
          svg.parentElement?.getAttribute("aria-label") ||
          "",
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
