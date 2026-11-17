// Journey steps for the shared progress bar's look in the gallery (technical
// design §4.5): a fill in its tone on a Ledger Gray track as thick as half
// its size on the shared scale, its words in the text's color. A bar is a
// `progressbar` found by its name, or every one of the gallery's, which a
// pseudolocale renames, by its role alone; its track and fill have no role,
// so they are found by their classes inside it.

import assert from "node:assert/strict";

import type { DataTable } from "@cucumber/cucumber";
import { Then } from "@cucumber/cucumber";
import { expect } from "playwright/test";
import type { Locator, Page } from "playwright";

import { journey, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";
import { eachOf } from "./components.ts";

// The gallery's progress bars (`gallery.rs`): the one the reader advances,
// one of unknown size and one of a step, then one at each size and one in
// each tone.
const BARS = 13;

// Half a pixel of rounding in a laid-out length.
const ROUNDING_PX = 0.5;

const bar = (page: Page, name: string): Locator => page.getByRole("progressbar", { name: namePattern(name) });

// A bar is a stop for Tab at every size and tone (A1), and its focus ring,
// an outline around the bar, surrounds everything it draws, so a sighted
// keyboard user sees which bar a screen reader is reading.
Then(
  "pressing Tab moves focus to these progress bars in turn, each holding its track and its words inside its focus ring:",
  async function (this: BrowserWorld, table: DataTable) {
    const keys = journey(this.page);
    for (const { bar: name } of table.hashes() as { bar: string }[]) {
      await keys.press("Tab");
      const found = bar(this.page, name);
      await expect(found).toHaveCount(1);
      await keys.assertFocused(found, `the progressbar "${name}"`);
      const drawn = await found.evaluate((element) => {
        const rect = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        return {
          ring: { style: style.outlineStyle, width: Number.parseFloat(style.outlineWidth) },
          box: { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom },
          parts: Array.from(element.querySelectorAll(".progress-step, .progress-track, .progress-words"), (part) => {
            const inner = part.getBoundingClientRect();
            return { part: part.className, left: inner.left, right: inner.right, top: inner.top, bottom: inner.bottom };
          }),
        };
      });
      assert.notEqual(drawn.ring.style, "none", `"${name}" draws no focus ring`);
      assert.ok(drawn.ring.width > 0, `"${name}"'s focus ring is ${drawn.ring.width}px wide`);
      assert.ok(drawn.parts.some((part) => part.part.includes("progress-track")), `"${name}" draws no track`);
      for (const part of drawn.parts) {
        assert.ok(
          part.left >= drawn.box.left - ROUNDING_PX &&
            part.right <= drawn.box.right + ROUNDING_PX &&
            part.top >= drawn.box.top - ROUNDING_PX &&
            part.bottom <= drawn.box.bottom + ROUNDING_PX,
          `"${name}" draws ${JSON.stringify(part)} outside ${JSON.stringify(drawn.box)}`,
        );
      }
    }
  },
);

// A bar's size (technical design §4.5, the shared vocabulary) sets its
// track's thickness, half the shared scale's length, which is in `em`, so it
// grows with the reader's text (1.4.4): compared with the body's text, the
// size every page's text starts from.
Then(
  "the track of the progress bar {string} is {float} times as thick as the page's text",
  async function (this: BrowserWorld, name: string, times: number) {
    const found = bar(this.page, name);
    await expect(found).toHaveCount(1);
    const { thick, text } = await found.evaluate((element) => {
      const track = element.querySelector(".progress-track");
      return {
        thick: track === null ? Number.NaN : track.getBoundingClientRect().height,
        text: Number.parseFloat(getComputedStyle(document.body).fontSize),
      };
    });
    assert.ok(
      Math.abs(thick - times * text) < ROUNDING_PX,
      `the track of "${name}" is ${thick}px thick, the page's text ${text}px`,
    );
  },
);

// A bar's tone colors its fill only (technical design §4.5): the track stays
// Ledger Gray and the words the text's color. Each row names a bar and the
// token its tone fills in, or "the text around it" for Inherit, which must
// follow the text around the bar, so the step gives the bar's line two
// colors in turn and reads the fill against each. A token is read through a
// probe, as the page resolves it.
Then(
  "each progress bar fills its track in its tone, the track in {word} and its words in the color of the text around it:",
  async function (this: BrowserWorld, surface: string, table: DataTable) {
    for (const { bar: name, tone } of table.hashes() as { bar: string; tone: string }[]) {
      const found = bar(this.page, name);
      await expect(found).toHaveCount(1);
      const drawn = await found.evaluate(
        (element, { tone, surface }) => {
          const resolve = (token: string) => {
            const probe = document.createElement("span");
            probe.style.color = `var(${token})`;
            document.body.append(probe);
            const color = getComputedStyle(probe).color;
            probe.remove();
            return color;
          };
          const fill = element.querySelector(".progress-fill");
          const track = element.querySelector(".progress-track");
          const line = element.parentElement as HTMLElement | null;
          const filled = () => (fill === null ? "" : getComputedStyle(fill).backgroundColor);
          const words = Array.from(element.querySelectorAll(".progress-step, .progress-words"), (part) =>
            getComputedStyle(part).color,
          );
          let expected: string[];
          let fills: string[];
          if (tone.startsWith("--")) {
            expected = [resolve(tone)];
            fills = [filled()];
          } else {
            expected = ["rgb(1, 2, 3)", "rgb(200, 100, 50)"];
            fills = [];
            for (const color of expected) {
              line?.style.setProperty("color", color);
              fills.push(filled());
            }
            line?.style.removeProperty("color");
          }
          return {
            expected,
            fills,
            track: track === null ? "" : getComputedStyle(track).backgroundColor,
            surface: resolve(surface),
            words,
            around: line === null ? "" : getComputedStyle(line).color,
          };
        },
        { tone, surface },
      );
      assert.deepEqual(drawn.fills, drawn.expected, `the fill of "${name}"`);
      assert.equal(drawn.track, drawn.surface, `the track of "${name}"`);
      assert.ok(drawn.words.length > 0, `"${name}" shows no words`);
      for (const color of drawn.words) {
        assert.equal(color, drawn.around, `the words of "${name}"`);
      }
    }
  },
);

// Forced colors (the reader's own palette): the fill follows the tone rule
// of the shared vocabulary, every tone the text's own color, and the track
// takes the color of unavailable text, two of the reader's colors, which
// they choose to stand apart, as a spinner's arc and track do. A fill the
// mode turned into the page's background would leave the bar empty.
Then(
  "every progress bar of the gallery fills its track in the color of the text around it, and its track is GrayText",
  async function (this: BrowserWorld) {
    const bars = await eachOf(this.page.getByRole("progressbar"), BARS);
    const gray = await this.page.evaluate(() => {
      const probe = document.createElement("span");
      probe.style.setProperty("forced-color-adjust", "none");
      probe.style.color = "GrayText";
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    });
    let tracks = 0;
    for (const found of bars) {
      const drawn = await found.evaluate((element) => {
        const fill = element.querySelector(".progress-fill");
        const track = element.querySelector(".progress-track");
        return {
          name: element.getAttribute("aria-label") ?? "",
          text: getComputedStyle(element).color,
          fill: fill === null ? null : getComputedStyle(fill).backgroundColor,
          track: track === null ? null : getComputedStyle(track).backgroundColor,
        };
      });
      if (drawn.track === null) continue;
      tracks += 1;
      assert.equal(drawn.fill, drawn.text, `the fill of "${drawn.name}"`);
      assert.equal(drawn.track, gray, `the track of "${drawn.name}"`);
    }
    assert.ok(tracks > 0, "no progress bar of the gallery draws a track");
  },
);

// The 320 Rule and The Logical Rule (DESIGN.md): on a small screen with
// larger text, a bar's track and words stay inside it, and its fill grows
// from the start of the track, the left in English and the right in Arabic.
Then(
  "every progress bar of the gallery holds its track and its words inside it, its fill at the track's start, in a page that runs {word} to {word}",
  async function (this: BrowserWorld, from: string, to: string) {
    const direction = from === "right" && to === "left" ? "rtl" : "ltr";
    const bars = await eachOf(this.page.getByRole("progressbar"), BARS);
    let fills = 0;
    for (const found of bars) {
      const drawn = await found.evaluate((element) => {
        const box = (part: Element | null) => {
          const rect = part?.getBoundingClientRect();
          return rect === undefined ? null : { left: rect.left, right: rect.right, width: rect.width };
        };
        const rect = element.getBoundingClientRect();
        return {
          name: element.getAttribute("aria-label") ?? "",
          direction: getComputedStyle(element).direction,
          box: { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom },
          parts: Array.from(element.querySelectorAll(".progress-step, .progress-track, .progress-words"), (part) => {
            const inner = part.getBoundingClientRect();
            return { part: part.className, left: inner.left, right: inner.right, top: inner.top, bottom: inner.bottom };
          }),
          track: box(element.querySelector(".progress-track")),
          fill: box(element.querySelector(".progress-fill")),
        };
      });
      assert.equal(drawn.direction, direction, drawn.name);
      for (const part of drawn.parts) {
        assert.ok(
          part.left >= drawn.box.left - ROUNDING_PX &&
            part.right <= drawn.box.right + ROUNDING_PX &&
            part.top >= drawn.box.top - ROUNDING_PX &&
            part.bottom <= drawn.box.bottom + ROUNDING_PX,
          `"${drawn.name}" draws ${JSON.stringify(part)} outside ${JSON.stringify(drawn.box)}`,
        );
      }
      if (drawn.track === null || drawn.fill === null) continue;
      fills += 1;
      assert.ok(drawn.fill.width < drawn.track.width, `the fill of "${drawn.name}" is the whole track`);
      const start = direction === "ltr" ? "left" : "right";
      assert.ok(
        Math.abs(drawn.fill[start] - drawn.track[start]) < ROUNDING_PX,
        `the fill of "${drawn.name}" doesn't start at the track's ${start}: ${JSON.stringify(drawn)}`,
      );
    }
    assert.ok(fills > 0, "no progress bar of the gallery draws a fill");
  },
);
