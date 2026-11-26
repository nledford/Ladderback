// Journey steps for the shared spinner in the gallery (technical design
// §4.5): its label, its motion, its size, its place in its line and its tone.

import assert from "node:assert/strict";

import { type DataTable, Then } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import { patienceMs } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";
import { LIVE_REGION, demoSection } from "../support/gallery.ts";

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
  "the spinner labeled {string} stands in its line of text, its label on the line's baseline and its drawing centered on the text",
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
    assert.ok(first && placed.runs.length >= 2, `the spinner "${label}" stands in no line of text: ${JSON.stringify(placed.runs)}`);
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
  await section.waitFor({ timeout: patienceMs() });
  await expect(section.getByText(text, { exact: true })).toHaveCount(0);
});

// The page keeps one polite and one assertive region, the announcer's (§4.5,
// A6); a spinner adds none, so a page that waits says so through the
// announcer, and a wait that nobody asked for says nothing.
Then("the {string} demo has spinners but no live region", async function (this: BrowserWorld, component: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: patienceMs() });
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
    await section.waitFor({ timeout: patienceMs() });
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
  await section.waitFor({ timeout: patienceMs() });
  await expect
    .poll(async () => {
      const spinners = await spinnerMotions(section);
      assert.ok(spinners.length > 0, `the ${component} demo has no spinner`);
      return spinners.filter((spinner) => spinner.motions.length > 0).map((spinner) => spinner.text);
    })
    .toEqual([]);
  await expect(section.locator(".spinner svg").first()).toBeVisible();
});

// What each spinner of a demo draws, read where the browser lays it out: its
// still dots (`.spinner-still circle`) that are drawn, with their centers and
// fill, whether its arc and its track are drawn, its drawing's color and the
// color of the text around the spinner.
const stillForms = (section: Locator) =>
  section.evaluate((element) =>
    Array.from(element.querySelectorAll(".spinner"), (spinner) => {
      const graphic = spinner.querySelector("svg") as SVGSVGElement;
      const drawn = (part: Element | null) => {
        if (part === null) return false;
        const box = part.getBoundingClientRect();
        return getComputedStyle(part).display !== "none" && box.width > 0 && box.height > 0;
      };
      const dots = Array.from(spinner.querySelectorAll(".spinner-still circle"))
        .filter(drawn)
        .map((dot) => {
          const box = dot.getBoundingClientRect();
          return { x: box.x + box.width / 2, y: box.y + box.height / 2, fill: getComputedStyle(dot).fill };
        });
      return {
        name: spinner.textContent?.trim() || graphic.getAttribute("aria-label") || "",
        dots,
        arc: drawn(spinner.querySelector(".spinner-arc")),
        track: drawn(spinner.querySelector(".spinner-track")),
        color: getComputedStyle(graphic).color,
        text: spinner.parentElement ? getComputedStyle(spinner.parentElement).color : "",
      };
    }),
  );

Then("no spinner of the {string} demo draws its still dots", async function (this: BrowserWorld, component: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: patienceMs() });
  const spinners = await stillForms(section);
  assert.ok(spinners.length > 0, `the ${component} demo has no spinner`);
  for (const spinner of spinners) {
    assert.deepEqual(
      { dots: spinner.dots.length, arc: spinner.arc, track: spinner.track },
      { dots: 0, arc: true, track: true },
      spinner.name,
    );
  }
});

// Under reduced motion a spinner is three dots in a row across its drawing's
// middle, evenly spaced, and nothing of a circle, which still would read as a
// gauge (technical design §4.5). The dots are filled in the drawing's color,
// its tone, or under forced colors the text's own.
Then(
  /^every spinner of the "([^"]+)" demo stands still as a row of three dots in (its drawing's color|the color of the text around it), with no arc or track$/,
  async function (this: BrowserWorld, component: string, color: string) {
    const section = demoSection(this.page, component);
    await section.waitFor({ timeout: patienceMs() });
    await expect
      .poll(async () => (await stillForms(section)).filter((spinner) => spinner.dots.length !== 3).map((spinner) => spinner.name))
      .toEqual([]);
    const spinners = await stillForms(section);
    assert.ok(spinners.length > 0, `the ${component} demo has no spinner`);
    for (const { name, dots, arc, track, color: drawing, text } of spinners) {
      assert.deepEqual({ arc, track }, { arc: false, track: false }, `the arc and track of "${name}"`);
      const [first, middle, last] = dots;
      assert.ok(first && middle && last, name);
      assert.ok(
        dots.every((dot) => Math.abs(dot.y - first.y) < 0.5),
        `the dots of "${name}" stand in a row: ${JSON.stringify(dots)}`,
      );
      assert.ok(
        Math.abs(middle.x - first.x - (last.x - middle.x)) < 0.5 && last.x !== first.x,
        `the dots of "${name}" are evenly spaced: ${JSON.stringify(dots)}`,
      );
      const expected = color === "its drawing's color" ? drawing : text;
      assert.deepEqual(
        dots.map((dot) => dot.fill),
        [expected, expected, expected],
        `the dots of "${name}" in ${color}`,
      );
    }
  },
);

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
  await page.locator(".spinner").first().waitFor({ timeout: patienceMs() });
  return page.evaluate(() => ({
    rtl: getComputedStyle(document.documentElement).direction === "rtl",
    lines: Array.from(document.querySelectorAll<HTMLElement>(".spinner"))
      .filter((spinner) => {
        const line = spinner.parentElement;
        const row = line?.parentElement;
        return (
          line !== null && getComputedStyle(line).display === "flex" && !/flex|grid/.test(row ? getComputedStyle(row).display : "")
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
    await section.waitFor({ timeout: patienceMs() });
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
// finds it.
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
    await section.waitFor({ timeout: patienceMs() });
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
    await section.waitFor({ timeout: patienceMs() });
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
      assert.deepEqual({ arc: spinner.arc, track: spinner.track }, { arc: spinner.text, track: drawn.gray }, spinner.name);
    }
  },
);
