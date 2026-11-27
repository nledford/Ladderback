// Journey steps for the shared table (technical design §4.5): the order of
// its rows and what its headers say of it, a row's action keeping its focus,
// its numbers' alignment, what shows in its place, and its frame, which
// scrolls sideways on a small screen with each row's header pinned at its
// start. Keyboard only, role-and-name locators, as everywhere in steps/.
//
// The table is named by its caption, and its frame, a region, by the same
// caption through aria-labelledby.

import assert from "node:assert/strict";
import { setTimeout as sleep } from "node:timers/promises";

import { Then, When } from "@cucumber/cucumber";
import type { Locator, Page } from "playwright";

import { expect } from "../support/expect.ts";
import { demoSection } from "../support/gallery.ts";
import { journey, namePattern, scrollKey } from "../support/keyboard.ts";
import { patienceMs } from "../support/patience.ts";
import { dragHandleOf, pointer } from "../support/pointer.ts";
import type { BrowserWorld } from "../support/world.ts";
import { rowNamed } from "./live.ts";

// More presses than a frame needs to scroll from one end to the other.
const MAX_SCROLL_PRESSES = 40;

function table(page: Page, name: string): Locator {
  return page.getByRole("table", { name, exact: true });
}

/** A column header's own words: its first line, before its sort button's. */
function headerWords(text: string): string {
  const [words] = text.split("\n", 1);
  assert.ok(words !== undefined, `a column header reads nothing: ${JSON.stringify(text)}`);
  return words.trim();
}

/** The cell of `row` (its row header's words) in the column headed `column`. */
async function cellOf(page: Page, name: string, row: string, column: string): Promise<Locator> {
  const found = table(page, name);
  const headers = (await found.locator("thead th").allInnerTexts()).map(headerWords);
  const index = headers.indexOf(column);
  assert.ok(index >= 0, `the table "${name}" has no column "${column}": ${headers.join(", ")}`);
  const line = found.locator("tr", { has: page.getByRole("rowheader", { name: row, exact: true }) });
  return line.locator("th, td").nth(index);
}

/** Each cell of the column headed `column`, its header's and footer's too. */
async function columnOf(page: Page, name: string, column: string): Promise<Locator[]> {
  const found = table(page, name);
  const headers = (await found.locator("thead th").allInnerTexts()).map(headerWords);
  const index = headers.indexOf(column);
  assert.ok(index >= 0, `the table "${name}" has no column "${column}": ${headers.join(", ")}`);
  const rows = found.locator("tr");
  await expect(rows.first()).toBeVisible();
  const cells: Locator[] = [];
  for (const row of await rows.all()) {
    cells.push(row.locator(":scope > th, :scope > td").nth(index));
  }
  return cells;
}

Then("the rows of the table {string} are {string}", async function (this: BrowserWorld, name: string, rows: string) {
  await expect(table(this.page, name).locator("tbody th")).toHaveText(rows.split(", "));
});

Then("no column header of the table {string} says it is sorted", async function (this: BrowserWorld, name: string) {
  await expect(table(this.page, name).locator("thead th[aria-sort]")).toHaveCount(0);
});

// One sorted column at a time (the WAI-ARIA table pattern): only its header
// carries aria-sort.
Then(
  "the column header {string} alone of the table {string} says it is sorted {string}",
  async function (this: BrowserWorld, header: string, name: string, direction: string) {
    const sorted = table(this.page, name).locator("thead th[aria-sort]");
    await expect(sorted).toHaveCount(1);
    await expect(sorted).toHaveAttribute("aria-sort", direction);
    expect(headerWords(await sorted.innerText())).toBe(header);
  },
);

Then(
  "the row {string} of the table {string} reads {string} in the column {string}",
  async function (this: BrowserWorld, row: string, name: string, text: string, column: string) {
    await expect(await cellOf(this.page, name, row, column)).toHaveText(text);
  },
);

// A row that moved is moved, not built again, so its control is the one the
// reader was on; focus goes back to it once the move is made (WCAG 2.4.3).
Then("focus is kept on the button {string}", async function (this: BrowserWorld, name: string) {
  const button = journey(this.page).byRole("button", { name: namePattern(name) });
  await expect(button).toHaveCount(1);
  await expect(button).toBeFocused();
});

// Numbers line up by their last digit: every cell's text ends at the cell's
// end, and its figures are all one width (`tabular-nums`).
Then(
  "the column {string} of the table {string} is aligned to its end in figures of one width",
  async function (this: BrowserWorld, column: string, name: string) {
    for (const cell of await columnOf(this.page, name, column)) {
      const found = await cell.evaluate((el) => {
        const style = getComputedStyle(el);
        const range = document.createRange();
        range.selectNodeContents(el);
        const text = [...range.getClientRects()].filter((rect) => rect.width > 0);
        const box = el.getBoundingClientRect();
        const end = box.right - parseFloat(style.paddingRight) - parseFloat(style.borderRightWidth);
        return {
          text: (el.textContent ?? "").trim(),
          align: style.textAlign,
          figures: style.fontVariantNumeric,
          gap: text.length === 0 ? 0 : end - Math.max(...text.map((rect) => rect.right)),
        };
      });
      assert.ok(["end", "right"].includes(found.align), `"${found.text}" is aligned ${found.align}`);
      assert.match(found.figures, /tabular-nums/, `"${found.text}" is in figures of ${found.figures}`);
      assert.ok(Math.abs(found.gap) <= 1, `"${found.text}" ends ${found.gap.toFixed(1)}px before its cell's end`);
    }
  },
);

Then(
  "the column {string} of the table {string} is aligned to its start",
  async function (this: BrowserWorld, column: string, name: string) {
    for (const cell of await columnOf(this.page, name, column)) {
      const align = await cell.evaluate((el) => getComputedStyle(el).textAlign);
      assert.ok(["start", "left"].includes(align), `a cell of "${column}" is aligned ${align}`);
    }
  },
);

// What shows in the table's place: the sentence, and no table of that name.
Then(
  "the {string} demo says {string} in the place of the table {string}",
  async function (this: BrowserWorld, component: string, sentence: string, name: string) {
    const section = this.page.locator("section", {
      has: this.page.getByRole("heading", { level: 2, name: component, exact: true }),
    });
    await expect(section.getByText(sentence, { exact: true })).toBeVisible();
    await expect(table(this.page, name)).toHaveCount(0);
  },
);

// The frame is a focusable region, so the arrow keys scroll it (Option and
// the arrow in WebKit on macOS: `scrollKey`); its end is the right in a
// left-to-right page and the left in a right-to-left one, where
// `scrollLeft` counts down from 0. Each press waits for its scroll to end,
// as a reader's next press does: WebKit animates a key's scroll, and a press
// during the animation cuts it short (12 presses without a wait moved the
// gallery's frame 187 px of its 838; with 100 ms between them, all of it).
async function scrollSettles(frame: Locator): Promise<void> {
  await frame.evaluate(async (el) => {
    let last = el.scrollLeft;
    let still = 0;
    for (let frames = 0; still < 6 && frames < 120; frames += 1) {
      await new Promise(requestAnimationFrame);
      still = el.scrollLeft === last ? still + 1 : 0;
      last = el.scrollLeft;
    }
  });
}

When(
  "I press {word} until the region {string} has scrolled to its end",
  async function (this: BrowserWorld, key: string, name: string) {
    const frame = journey(this.page).byRole("region", { name: namePattern(name) });
    const press = scrollKey(this.page, key);
    await expect(frame).toBeFocused();
    const atEnd = () => frame.evaluate((el) => el.scrollWidth - el.clientWidth - Math.abs(el.scrollLeft) < 1);
    const scrolls = await frame.evaluate((el) => el.scrollWidth > el.clientWidth + 1);
    assert.ok(scrolls, `the region "${name}" has nothing to scroll`);
    for (let presses = 0; presses < MAX_SCROLL_PRESSES && !(await atEnd()); presses += 1) {
      await this.page.keyboard.press(press);
      await scrollSettles(frame);
    }
    await expect.poll(atEnd, { message: `the region "${name}" didn't scroll to its end by ${press}` }).toBe(true);
  },
);

// A row header is pinned at the frame's start (`position: sticky`), so a
// reader scrolled sideways still sees whose row it is.
Then(
  "each row header of the table {string} stands whole at the start of its frame",
  async function (this: BrowserWorld, name: string) {
    const headers = table(this.page, name).locator("tbody th");
    await expect(headers.first()).toBeVisible();
    for (const header of await headers.all()) {
      const found = await header.evaluate((el) => {
        const frame = el.closest('[role="region"]');
        if (frame === null) return { problem: "the row header is in no frame" };
        const box = el.getBoundingClientRect();
        const room = frame.getBoundingClientRect();
        const rtl = getComputedStyle(frame).direction === "rtl";
        const start = rtl ? room.right - frame.clientLeft : room.left + frame.clientLeft;
        const at = rtl ? box.right : box.left;
        return {
          problem:
            Math.abs(at - start) > 1 || box.left < room.left - 0.5 || box.right > room.right + 0.5
              ? `"${el.textContent}" stands at ${Math.round(box.left)}..${Math.round(box.right)}px in a frame at ${Math.round(room.left)}..${Math.round(room.right)}px`
              : null,
        };
      });
      assert.equal(found.problem, null, found.problem ?? "");
    }
  },
);

// A caption is page text, not table data (The 320 Rule, #1695): where the
// columns are wider than the frame, its lines still lie inside the frame's
// visible width, wrapped there and kept at its start as the frame scrolls,
// so the frame's clip cuts off none of its words.
Then(
  "the caption of the table {string} wraps within its frame, every word of it in view",
  async function (this: BrowserWorld, name: string) {
    const caption = table(this.page, name).locator("caption");
    await expect(caption).toHaveText(name);
    const found = await caption.evaluate((el) => {
      const frame = el.closest('[role="region"]');
      if (frame === null) return { problem: "the caption is in no frame" };
      if (frame.scrollWidth <= frame.clientWidth + 1) return { problem: "the table is no wider than its frame" };
      const start = frame.getBoundingClientRect().left + frame.clientLeft;
      const end = start + frame.clientWidth;
      const words = document.createRange();
      words.selectNodeContents(el);
      const lines = [...words.getClientRects()].filter((line) => line.width > 0);
      if (lines.length === 0) return { problem: "the caption shows no words" };
      const cut = lines.find((line) => line.left < start - 0.5 || line.right > end + 0.5);
      return {
        problem:
          cut === undefined
            ? null
            : `a line of "${el.textContent}" stands at ${Math.round(cut.left)}..${Math.round(cut.right)}px in a frame showing ${Math.round(start)}..${Math.round(end)}px`,
      };
    });
    assert.equal(found.problem, null, found.problem ?? "");
  },
);

// What the cells scrolled under a pinned row header would show through: the
// header's fill is the page's own background, the system's in forced colors.
Then(
  "each row header of the table {string} is filled with the page's background color",
  async function (this: BrowserWorld, name: string) {
    const headers = table(this.page, name).locator("tbody th");
    await expect(headers.first()).toBeVisible();
    for (const header of await headers.all()) {
      const [fill, page] = await header.evaluate((el) => [
        getComputedStyle(el).backgroundColor,
        getComputedStyle(document.body).backgroundColor,
      ]);
      assert.notEqual(fill, "rgba(0, 0, 0, 0)", "a row header has no fill");
      assert.equal(fill, page, "a row header's fill is not the page's background");
    }
  },
);

// A table the shared table draws sits in a frame of its own: a region named
// as the table is, which Tab stops on, so a keyboard scrolls it sideways
// (§4.5, #876). The frame is the table's own parent: a section named by the
// same heading may hold it, a region of that name too. A value in the name is
// wrapped in bidi isolation marks, which a reader never hears.
Then(
  "the table {string} sits in a frame of its own that Tab stops on, named as the table is",
  async function (this: BrowserWorld, name: string) {
    const found = this.page.getByRole("table", { name: namePattern(name) });
    await expect(found).toBeVisible();
    const frame = found.locator("xpath=..");
    await expect(frame).toHaveRole("region");
    await expect(frame).toHaveAccessibleName(namePattern(name));
    await expect(frame).toHaveAttribute("tabindex", "0");
  },
);

// An instant in a cell is the cell's own words, in a `<time datetime>` that
// gives it to a program (§4.5, "Time"), so a keyboard or a screen reader meets
// it where a sighted reader does. A `title`, which only a pointer resting on
// the cell shows, carries none of it (#851).
Then(
  "the table {string} gives each instant of its {string} column in its words, in a time element, with no title",
  async function (this: BrowserWorld, name: string, column: string) {
    const found = this.page.getByRole("table", { name: namePattern(name) });
    await expect(found.locator("tbody tr").first()).toBeVisible();
    const headers = (await found.locator("thead th").allInnerTexts()).map(headerWords);
    const index = headers.indexOf(column);
    assert.ok(index >= 0, `the table "${name}" has no column "${column}": ${headers.join(", ")}`);
    const problems = await found.evaluate((el: HTMLTableElement, at) => {
      const said: string[] = [];
      for (const titled of el.querySelectorAll("[title]")) {
        said.push(`<${titled.localName}> carries the title "${titled.getAttribute("title")}"`);
      }
      let times = 0;
      for (const row of el.tBodies[0]?.rows ?? []) {
        const cell = row.cells[at];
        const time = cell?.querySelector("time");
        if (cell === undefined || time === null || time === undefined) continue;
        times += 1;
        const instant = time.getAttribute("datetime") ?? "";
        if (Number.isNaN(Date.parse(instant))) said.push(`the time "${time.textContent}" gives no instant: "${instant}"`);
        if (!time.textContent?.trim()) said.push(`the time of "${instant}" has no words`);
        if (time.textContent?.trim() !== cell.textContent?.trim()) {
          said.push(`the cell "${cell.textContent}" says more than its time "${time.textContent}"`);
        }
      }
      if (times === 0) said.push("no cell of the column holds a time");
      return said;
    }, index);
    assert.deepEqual(problems, [], `the "${column}" column of the table "${name}"`);
  },
);

// A table's frame scrolls rather than break a word (#524): no word of a cell
// or a header breaks inside, wherever its line ends. An address or a path
// still breaks between its parts, after a slash, a dot, an at sign or a colon
// that a letter follows (#482), and a word after its hyphen or at a soft
// hyphen. Each character's box says which line it is on; a bidi isolation
// mark isn't text a reader sees, and a character of no width is on no line.
// A word ends where its box does: text in another block or flex item, such as
// the next line of a cell or a header's sort button, starts a word of its own.
// A column keeps its width in rem however narrow the frame (the frame
// scrolls), but a pinned header is at most 40 % of the frame (60 % where its
// words must read whole): a word wider than its cell's room breaks rather
// than run over the cells beside it, and only such a word.
Then("no word of a table breaks inside, an address only between its parts", async function (this: BrowserWorld) {
  const broken = await this.page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    // The box a text node's line is laid out in: its nearest ancestor that is
    // not inline, the cell at the furthest.
    const boxOf = (node: Node, cell: Element): Element => {
      let element = node.parentElement ?? cell;
      while (element !== cell) {
        const display = getComputedStyle(element).display;
        if (!display.startsWith("inline") && display !== "contents") return element;
        element = element.parentElement ?? cell;
      }
      return cell;
    };
    const found: string[] = [];
    for (const cell of document.querySelectorAll("main table th, main table td")) {
      if (!cell.checkVisibility()) continue;
      const style = getComputedStyle(cell);
      const room = cell.clientWidth - parseFloat(style.paddingInlineStart) - parseFloat(style.paddingInlineEnd);
      const words: { char: string; top: number; width: number }[][] = [[]];
      let box: Element | null = null;
      const walker = document.createTreeWalker(cell, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const holder = boxOf(node, cell);
        if (holder !== box) {
          words.push([]);
          box = holder;
        }
        const content = node.textContent ?? "";
        for (let at = 0; at < content.length; at += 1) {
          const char = content.charAt(at);
          if (/[\u2066-\u2069]/.test(char)) continue;
          if (/[\s\u200B]/.test(char)) {
            words.push([]);
            continue;
          }
          const range = document.createRange();
          range.setStart(node, at);
          range.setEnd(node, at + 1);
          const rect = [...range.getClientRects()].find((rect) => rect.width > 0);
          if (rect !== undefined) words[words.length - 1]?.push({ char, top: Math.round(rect.top), width: rect.width });
        }
      }
      for (const letters of words) {
        const wide = letters.reduce((sum, letter) => sum + letter.width, 0);
        if (wide > room) continue;
        letters.forEach(({ char, top }, at) => {
          const before = letters[at - 1];
          if (before === undefined || top <= before.top) return;
          if (/[-\u2010\u00AD]/.test(before.char)) return;
          if (/[/.@:]/.test(before.char) && /\p{L}/u.test(char)) return;
          found.push(`"${letters.map((letter) => letter.char).join("")}" in <${cell.localName}> breaks at ${at}`);
        });
      }
    }
    return found;
  });
  assert.deepEqual(broken, [], "words of tables broken inside");
});

// A pinned row header covers the cells that scroll beneath it, so however
// wide its words make it, it leaves the cells beside it two fifths of its
// frame to be read in, at 320 px with 200 % text too (#1129). Each frame is
// the region around its table.
Then("each table's row header leaves two fifths of its frame to the cells beside it", async function (this: BrowserWorld) {
  const covering = await this.page.evaluate(() => {
    const found: string[] = [];
    for (const header of document.querySelectorAll("table th[scope=row]")) {
      const frame = header.closest('[role="region"]');
      if (frame === null || !header.checkVisibility()) continue;
      const left = frame.clientWidth - header.getBoundingClientRect().width;
      if (left < frame.clientWidth * 0.4 - 1) {
        found.push(`"${header.textContent?.trim()}" leaves ${left.toFixed(1)} of its frame's ${frame.clientWidth} px`);
      }
    }
    return found;
  });
  assert.deepEqual(covering, [], "row headers that leave the cells beside them less than two fifths of their frame");
});

// The change mark (decision 129): a row a live change marked "changed, not
// yet seen" reads its watched column in bold and has an edge at its start,
// on its row header, which stands at the frame's start. The edge is the
// `--cue` token, or the system's text color under forced colors, at least
// 2px wide and the row's full height. Bold is every element of the column's
// cell that holds words of its own, and the cell itself when it does.
interface Mark {
  bold: string[];
  notBold: string[];
  edge: { content: string; color: string; width: number; start: number; top: number; bottom: number };
  cue: string;
}

/** Where `column` stands among the table's columns, its row header's first. */
async function columnAt(world: BrowserWorld, name: string, column: string): Promise<number> {
  const headers = (await table(world.page, name).locator("thead th").allInnerTexts()).map(headerWords);
  const index = headers.indexOf(column);
  assert.ok(index >= 0, `the table "${name}" has no column "${column}": ${headers.join(", ")}`);
  return index;
}

async function markOf(world: BrowserWorld, name: string, row: string, column: string): Promise<Mark> {
  const found = rowNamed(world, name, row).first();
  await found.waitFor({ state: "visible", timeout: patienceMs() });
  // A hidden column has no cell, so nothing of it is bold: only the edge
  // can mark its row.
  const index = (await table(world.page, name).locator("thead th").allInnerTexts()).map(headerWords).indexOf(column);
  return found.evaluate((element, at) => {
    const probe = document.createElement("span");
    probe.style.setProperty("forced-color-adjust", "none");
    probe.style.color = matchMedia("(forced-colors: active)").matches ? "CanvasText" : "var(--cue)";
    document.body.append(probe);
    const cue = getComputedStyle(probe).color;
    probe.remove();
    const cell = element.querySelectorAll(":scope > :is(th, td)")[at];
    const bold: string[] = [];
    const notBold: string[] = [];
    for (const part of cell === undefined ? [] : [cell, ...cell.querySelectorAll("*")]) {
      const words = [...part.childNodes]
        .filter((node) => node.nodeType === Node.TEXT_NODE)
        .map((node) => node.textContent ?? "")
        .join("")
        .trim();
      if (words === "") continue;
      (Number.parseInt(getComputedStyle(part).fontWeight, 10) >= 700 ? bold : notBold).push(words);
    }
    const header = element.querySelector(':scope > th[scope="row"]');
    const edge = header === null ? null : getComputedStyle(header, "::before");
    const start = edge?.direction === "rtl" ? "right" : "left";
    return {
      bold,
      notBold,
      edge: {
        content: edge?.content ?? "none",
        color: edge?.backgroundColor ?? "",
        width: Number.parseFloat(edge?.width ?? "0") || 0,
        start: Number.parseFloat(edge?.getPropertyValue(start) ?? "") || 0,
        top: Number.parseFloat(edge?.top ?? "") || 0,
        bottom: Number.parseFloat(edge?.bottom ?? "") || 0,
      },
      cue,
    };
  }, index);
}

/** Whether `mark` draws a row as changed and not yet seen. */
function marked(mark: Mark): boolean {
  const edge = mark.edge;
  return (
    mark.bold.length > 0 &&
    mark.notBold.length === 0 &&
    edge.content !== "none" &&
    edge.width >= 2 &&
    edge.color === mark.cue &&
    edge.start === 0 &&
    edge.top === 0 &&
    edge.bottom === 0
  );
}

/** Whether `mark` draws a row with no mark: no edge, no bold. */
function unmarked(mark: Mark): boolean {
  return mark.bold.length === 0 && (mark.edge.content === "none" || mark.edge.width === 0);
}

/** Waits, up to the patience, until the row's mark is `wanted`. */
async function waitForMark(
  world: BrowserWorld,
  name: string,
  row: string,
  column: string,
  wanted: (mark: Mark) => boolean,
): Promise<Mark> {
  const until = Date.now() + patienceMs();
  let mark = await markOf(world, name, row, column);
  while (!wanted(mark) && Date.now() < until) {
    await sleep(50);
    mark = await markOf(world, name, row, column);
  }
  return mark;
}

Then(
  "the row {string} of the table {string} is marked as changed, its {string} in bold and an edge at its start",
  async function (this: BrowserWorld, row: string, name: string, column: string) {
    const mark = await waitForMark(this, name, row, column, marked);
    assert.ok(marked(mark), `the row ${row} isn't marked as changed: ${JSON.stringify(mark)}`);
  },
);

Then("the row {string} of the table {string} is not marked", async function (this: BrowserWorld, row: string, name: string) {
  const mark = await waitForMark(this, name, row, "Status", unmarked);
  assert.ok(unmarked(mark), `the row ${row} is still marked as changed: ${JSON.stringify(mark)}`);
});

// What a screen reader hears of a marked row (decision 129): the mark's word
// in the name of its cell, so the mark is not drawn alone (1.3.1). A cell
// after the row header, found by its column's place among the cells, in a
// row whose header may be fetched text, isolated as such (`rowNamed`).
async function cellOfLive(world: BrowserWorld, name: string, row: string, column: string): Promise<Locator> {
  const found = rowNamed(world, name, row).first();
  await found.waitFor({ state: "visible", timeout: patienceMs() });
  const index = await columnAt(world, name, column);
  assert.ok(index > 0, `the column "${column}" holds the row headers, not cells`);
  return found.getByRole("cell").nth(index - 1);
}

/** `words` anywhere in a name, bidi isolation marks and all. */
const saying = (words: string): RegExp => new RegExp(namePattern(words).source.slice(1, -1));

Then(
  "the row {string} of the table {string} says {string} in its {string} column to a screen reader",
  async function (this: BrowserWorld, row: string, name: string, words: string, column: string) {
    const cell = await cellOfLive(this, name, row, column);
    await expect(cell).toHaveAccessibleName(saying(words), { timeout: patienceMs() });
  },
);

Then(
  "the row {string} of the table {string} doesn't say {string} in its {string} column to a screen reader",
  async function (this: BrowserWorld, row: string, name: string, words: string, column: string) {
    const cell = await cellOfLive(this, name, row, column);
    await expect(cell).not.toHaveAccessibleName(saying(words), { timeout: patienceMs() });
  },
);

// Where a table's frame starts on the page, not in the viewport, so a scroll
// that brings a control into view isn't counted as a move: a control that
// comes and goes before it, in a line kept for it, moves nothing (decision
// 129).
const starts = new WeakMap<BrowserWorld, { x: number; y: number }>();

async function startOf(world: BrowserWorld, name: string): Promise<{ x: number; y: number }> {
  const region = world.page.getByRole("region", { name: namePattern(name) });
  await region.waitFor({ state: "visible", timeout: patienceMs() });
  return region.evaluate((frame) => {
    const box = frame.getBoundingClientRect();
    return { x: box.left + scrollX, y: box.top + scrollY };
  });
}

When("I note where the table {string} starts", async function (this: BrowserWorld, name: string) {
  starts.set(this, await startOf(this, name));
});

Then("the table {string} starts where it did", async function (this: BrowserWorld, name: string) {
  const noted = starts.get(this);
  assert.ok(noted, "no step noted where the table started");
  const now = await startOf(this, name);
  assert.ok(
    Math.abs(now.x - noted.x) < 0.5 && Math.abs(now.y - noted.y) < 0.5,
    `the table "${name}" started at ${JSON.stringify(noted)} and now starts at ${JSON.stringify(now)}`,
  );
});

// The region a table's frame is, named as its table: where focus goes when
// the control that had it leaves the page with nothing to take its place.
Then("focus is on the region {string}", async function (this: BrowserWorld, name: string) {
  const keys = journey(this.page);
  const region = keys.byRole("region", { name: namePattern(name) });
  assert.equal(await region.count(), 1, `the region "${name}" is not on the page once`);
  await keys.assertFocused(region, `the region "${name}"`);
});

// Waits, as a control goes once what it was for is done: off the page, or
// hidden on it (out of the accessibility tree), as a control whose line
// stays so nothing moves.
Then("the page doesn't offer the button {string}", async function (this: BrowserWorld, name: string) {
  await this.page
    .getByRole("button", { name: namePattern(name) })
    .first()
    .waitFor({ state: "hidden", timeout: patienceMs() });
});

Then("the column headers of the table {string} are {string}", async function (this: BrowserWorld, name: string, headers: string) {
  const found = table(this.page, name).locator("thead th");
  await expect.poll(async () => (await found.allInnerTexts()).map(headerWords).join(", "), { timeout: patienceMs() }).toBe(headers);
});

// The button says whether its panel shows (`aria-expanded`), and the panel
// it controls (`aria-controls`) shows or is hidden with it.
for (const [state, expanded] of [
  ["open", "true"],
  ["closed", "false"],
] as const) {
  Then(`the button {string} says its panel is ${state}`, async function (this: BrowserWorld, name: string) {
    const button = this.page.getByRole("button", { name: namePattern(name) });
    await expect(button).toHaveAttribute("aria-expanded", expanded);
    const controls = await button.getAttribute("aria-controls");
    assert.ok(controls !== null, `the button "${name}" names no panel it controls`);
    const panel = this.page.locator(`[id="${controls}"]`);
    if (expanded === "true") await expect(panel).toBeVisible();
    else await expect(panel).toBeHidden();
  });
}

/** The Columns panel's list `list` holds a checkbox for each of `columns`, in order, each checked. */
async function listsEachShown(list: Locator, columns: string): Promise<void> {
  const boxes = list.getByRole("listitem").getByRole("checkbox");
  await expect(boxes).toHaveCount(columns.split(", ").length);
  for (const [at, column] of columns.split(", ").entries()) {
    await expect(boxes.nth(at)).toHaveAccessibleName(column);
    await expect(boxes.nth(at)).toBeChecked();
  }
}

Then(
  "the Columns panel of the {string} demo lists {string}, each shown",
  async function (this: BrowserWorld, demo: string, columns: string) {
    await listsEachShown(demoSection(this.page, demo).getByRole("list", { name: "Columns", exact: true }), columns);
  },
);

// A page's one Columns panel: a column it doesn't list is one the reader
// can't hide (decision 140).
Then("the Columns panel lists {string}, each shown", async function (this: BrowserWorld, columns: string) {
  await listsEachShown(this.page.getByRole("list", { name: "Columns", exact: true }), columns);
});

// A Move button at its end of the list says it is unavailable
// (`aria-disabled`) and stays in the Tab order, never `disabled` nor
// taken out of it (A1). Focus may be elsewhere: on the button that opened
// the panel, or on the other Move button it went to. Its name carries the
// column's words in bidi isolation marks, so it is matched by
// `namePattern`.
Then(
  "the button {string} says it is unavailable, and is not disabled, wherever focus is",
  async function (this: BrowserWorld, name: string) {
    const button = this.page.getByRole("button", { name: namePattern(name) });
    await expect(button).toHaveCount(1);
    await expect(button).toHaveAttribute("aria-disabled", "true");
    await expect(button).not.toHaveAttribute("disabled");
    assert.ok(await button.evaluate((element) => (element as HTMLElement).tabIndex >= 0), `the button "${name}" left the Tab order`);
  },
);

/** The item of the Columns panel's list that holds the column `column`. */
function panelItem(page: Page, column: string): Locator {
  return page
    .getByRole("list", { name: "Columns", exact: true })
    .getByRole("listitem")
    .filter({ has: page.getByRole("checkbox", { name: column, exact: true }) });
}

When(
  "I drag the column {string} by its handle onto the top half of the column {string} in the Columns panel",
  async function (this: BrowserWorld, dragged: string, onto: string) {
    await pointer(this.page).dragOntoTopHalf(dragHandleOf(panelItem(this.page, dragged)), panelItem(this.page, onto));
  },
);

When(
  "I hold the column {string} by its handle over the top half of the column {string} in the Columns panel",
  async function (this: BrowserWorld, dragged: string, onto: string) {
    await pointer(this.page).holdOverTopHalf(dragHandleOf(panelItem(this.page, dragged)), panelItem(this.page, onto));
  },
);

When("I let go of the pointer", async function (this: BrowserWorld) {
  await pointer(this.page).letGo();
});

// Where a dragged column would land is a line across the list's item: at
// its top before it, at its bottom after the last. Every line the panel
// draws is counted, a border at least 2px wide across nine tenths of an
// item, of a color that shows: so a border kept transparent elsewhere,
// which forced colors paints in the text's color, counts too (1.4.11).
Then("the Columns panel draws one line, before the column {string}", async function (this: BrowserWorld, column: string) {
  const list = this.page.getByRole("list", { name: "Columns", exact: true });
  const lines = await list.evaluate((element) =>
    [...element.children].flatMap((item) => {
      const width = item.getBoundingClientRect().width;
      const label = item.querySelector("label")?.textContent?.trim() ?? "";
      const drawn: string[] = [];
      for (const part of [item, ...item.querySelectorAll("*")]) {
        const style = getComputedStyle(part);
        if (style.display === "none" || style.visibility !== "visible") continue;
        if (part.getBoundingClientRect().width < width * 0.9) continue;
        for (const side of ["top", "bottom"] as const) {
          const shows =
            Number.parseFloat(style.getPropertyValue(`border-${side}-width`)) >= 2 &&
            style.getPropertyValue(`border-${side}-style`) !== "none" &&
            !/^(transparent|rgba\(.*,\s*0\))$/.test(style.getPropertyValue(`border-${side}-color`));
          if (shows) drawn.push(`${side === "top" ? "before" : "after"} ${label}`);
        }
      }
      return drawn;
    }),
  );
  assert.deepEqual(lines, [`before ${column}`], "the lines the Columns panel draws");
});
