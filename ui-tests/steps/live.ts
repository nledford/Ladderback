// The journeys over the daemon's live data (§4.4): the operations and
// snapshots pages. Their Givens start a daemon of the scenario's own, with a
// config.toml naming a demo plan whose repository is a scratch copy of the
// `demo-repo` fixture, so a backup the keyboard starts is real: real
// progress, a real snapshot, a real end.

import assert from "node:assert/strict";
import { once } from "node:events";
import { cp, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import { After, Given, Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";
import { expect } from "playwright/test";

import { ISOLATION, namePattern } from "../support/keyboard.ts";
import { ScriptedDaemon } from "../support/scripted-daemon.ts";
import { type BrowserWorld, fixtureRepo, startServer } from "../support/world.ts";

// The scratch repositories' password: the one the `demo-repo` fixture was
// initialized with. Not a real secret.
const PASSWORD = "demo-password";

// A real backup takes real time, and the steps that wait for one are patient:
// enough for a big scratch source to finish, in a debug build.
const ROW_PATIENCE_MS = 120_000;

// A page's answer to a key press, or to an event a step sent it, takes a
// moment at most: this long, not as long as a real backup.
const UPDATE_PATIENCE_MS = 5_000;

// How long a page that holds still is watched: past the moment an update the
// page heard would have shown, as long as a live region's silence is.
const STILL_MS = 2_500;

// The three sources a demo plan backs up: `tiny` ends at once, for journeys
// that need several real snapshots; `small` runs long enough that its
// Running row is there to be read (a second or two) and then ends; `big`
// outlives the moment a journey cancels it or reloads the page mid-run
// (about ten seconds). Each file's content is unique to its number, so the
// repository cannot collapse the source into one chunk and end at once.
const SOURCE_FILES = { tiny: 3, small: 10_000, big: 50_000 };
const FILE_BYTES = 2048;

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Polls `read` until it returns a value, or fails with what it last saw. */
async function until<T>(read: () => Promise<T | null>, ms: number, what: string): Promise<T> {
  const end = Date.now() + ms;
  let last: T | null = null;
  while (Date.now() < end) {
    last = await read();
    if (last !== null) return last;
    await sleep(250);
  }
  assert.fail(`${what} (last saw ${last === null ? "nothing" : JSON.stringify(last)})`);
}

/** The table a page shows, by its whole caption. A value in the caption is
 *  wrapped in bidi isolation marks, which a reader never hears (§4.5). */
function table(world: BrowserWorld, name: string): Locator {
  return world.page.getByRole("table", { name: namePattern(name) });
}

/** Writes the config of one or more demo plans and their repository into a
 *  scratch folder, starts a daemon on it, and points the scenario's pages at
 *  that daemon. */
export async function serveDemoPlans(world: BrowserWorld, plans: readonly string[], size: keyof typeof SOURCE_FILES): Promise<void> {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-plan-"));
  await cp(fixtureRepo, path.join(scratch, "repo"), { recursive: true });
  const source = path.join(scratch, "source");
  await mkdir(source);
  for (let n = 1; n <= SOURCE_FILES[size]; n += 1) {
    await writeFile(path.join(source, `demo-${n}.txt`), `${n}:` + "d".repeat(FILE_BYTES));
  }
  // The shape of the e2e `serving_with_plan` step: one repository, which this
  // agent maintains, and each plan backing the source up into it.
  const config = [
    "schema = 1",
    "",
    "[repositories.demo]",
    `repository = ${JSON.stringify(path.join(scratch, "repo"))}`,
    `password = ${JSON.stringify(PASSWORD)}`,
    'maintenance = "local"',
    "",
    ...plans.flatMap((plan) => [
      "[[plans]]",
      `id = ${JSON.stringify(plan)}`,
      'repository = "demo"',
      `sources = [${JSON.stringify(source)}]`,
      "",
    ]),
  ].join("\n");
  const configDir = path.join(scratch, "config");
  await mkdir(configDir);
  await writeFile(path.join(configDir, "config.toml"), config);
  const { child, url } = await startServer({
    LADDERBACK_CONFIG_DIR: configDir,
    LADDERBACK_DATA_DIR: path.join(scratch, "data"),
    LADDERBACK_CACHE_DIR: path.join(scratch, "cache"),
  });
  world.ownServer = { child, scratch };
  world.baseUrl = url;
}

After(async function (this: BrowserWorld) {
  const own = this.ownServer;
  if (own && own.child.exitCode === null) {
    own.child.kill("SIGTERM");
    await once(own.child, "exit");
  }
  if (own) {
    await rm(own.scratch, { recursive: true, force: true });
  }
});

Given("a demo plan {string} with a scratch repository", async function (this: BrowserWorld, plan: string) {
  await serveDemoPlans(this, [plan], "small");
});

Given("a demo plan {string} with a big scratch repository", async function (this: BrowserWorld, plan: string) {
  await serveDemoPlans(this, [plan], "big");
});

// Two plans that back up the same source into the same repository: only
// their names tell them apart.
Given(
  "two demo plans {string} and {string} with a scratch repository",
  async function (this: BrowserWorld, first: string, second: string) {
    await serveDemoPlans(this, [first, second], "small");
  },
);

// The scripted daemon (support/scripted-daemon.ts): a Given says what its
// history holds, and its Whens say what its events stream tells the page and
// when.
function scripted(world: BrowserWorld): ScriptedDaemon {
  assert.ok(world.scriptedDaemon, "this scenario's daemon is not scripted: a Given must say so");
  return world.scriptedDaemon;
}

Given("the daemon reports a queued backup of {string}", function (this: BrowserWorld, plan: string) {
  const daemon = new ScriptedDaemon();
  daemon.addBackup(plan, "queued", { queuedAt: "2026-11-09T10:00:00Z" });
  this.scriptedDaemon = daemon;
});

Given("the daemon reports a running backup of {string}", function (this: BrowserWorld, plan: string) {
  const daemon = new ScriptedDaemon();
  daemon.addBackup(plan, "running", { queuedAt: "2026-11-09T10:00:00Z", startedAt: "2026-11-09T10:00:01Z" });
  this.scriptedDaemon = daemon;
});

Given("the daemon reports running backups of {string} and {string}", function (this: BrowserWorld, first: string, second: string) {
  const daemon = new ScriptedDaemon();
  daemon.addBackup(first, "running", { queuedAt: "2026-11-09T10:00:00Z", startedAt: "2026-11-09T10:00:01Z" });
  daemon.addBackup(second, "running", { queuedAt: "2026-11-09T10:00:02Z", startedAt: "2026-11-09T10:00:03Z" });
  this.scriptedDaemon = daemon;
});

Given("the daemon reports two finished backups of {string}", function (this: BrowserWorld, plan: string) {
  const daemon = new ScriptedDaemon();
  this.scriptedDaemon = daemon;
  for (const second of ["01", "02"]) {
    daemon.addBackup(plan, "succeeded", {
      queuedAt: `2026-11-09T10:00:${second}Z`,
      startedAt: `2026-11-09T10:00:${second}Z`,
      finishedAt: `2026-11-09T10:01:${second}Z`,
    });
  }
});

When("the daemon reports that the backup of {string} has started", async function (this: BrowserWorld, plan: string) {
  await scripted(this).starts(plan, "2026-11-09T10:00:05Z");
});

When("the daemon holds back the opening of the page's events stream", function (this: BrowserWorld) {
  scripted(this).holdTheNextStream();
});

When("the backup of {string} starts before the page's events stream is open", async function (this: BrowserWorld, plan: string) {
  await scripted(this).startsUnheard(plan, "2026-11-09T10:00:05Z");
});

When("the daemon opens the events stream it held back", async function (this: BrowserWorld) {
  await scripted(this).openTheHeldStream();
});

When("the daemon reports {int} steps of progress of the backup of {string}", async function (this: BrowserWorld, steps: number, plan: string) {
  await scripted(this).progresses(plan, steps);
});

When(
  "the daemon reports that the backup of {string} has succeeded with {int} files",
  async function (this: BrowserWorld, plan: string, files: number) {
    await scripted(this).succeeds(plan, files, "2026-11-09T10:05:00Z");
  },
);

When("the daemon reports that the backup of {string} has failed", async function (this: BrowserWorld, plan: string) {
  await scripted(this).fails(plan, "2026-11-09T10:05:00Z");
});

When("the daemon holds back its answer to the next reading of the history", function (this: BrowserWorld) {
  scripted(this).holdTheNextRead();
});

Then("the page reads the history again", async function (this: BrowserWorld) {
  await scripted(this).untilAReadIsHeld();
});

When("the daemon's events stream drops", async function (this: BrowserWorld) {
  await scripted(this).dropTheStream();
});

// A page that read again beside the reading held back would have asked as it
// heard the end, before this step began; the moment it waits is for that
// request to reach the scripted daemon, not for the page.
Then("the page reads the history no more while that answer is held back", async function (this: BrowserWorld) {
  await sleep(1_000);
  assert.equal(scripted(this).readsBesideTheHeld, 0, "the page read the history beside the reading held back");
});

When("the daemon gives the answer it held back", async function (this: BrowserWorld) {
  await scripted(this).answerTheHeld();
});

When("the daemon has stopped", async function (this: BrowserWorld) {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  own.child.kill("SIGTERM");
  await once(own.child, "exit");
});

// The page's own loading state (§4.4): every answer the daemon owes the UI
// arrives late, so the page opens on "Loading…" alone.
When("the daemon's answers to the UI are delayed", function (this: BrowserWorld) {
  this.answerDelayMs = 10_000;
});

// A shorter wait, for a journey that has to read a wait and then act within
// it: every answer arrives this many seconds late.
When("the daemon's answers to the UI are delayed by {int} seconds", function (this: BrowserWorld, seconds: number) {
  this.answerDelayMs = seconds * 1_000;
});

When("I reload the page", async function (this: BrowserWorld) {
  await this.page.reload();
  await this.page.getByRole("heading", { level: 1 }).waitFor();
});

// A row is named by its plan; what it shows is its life so far, which a
// backup changes as it runs, so the step waits for the row to say it.
Then("the table {string} has a row {string} that shows {string}", async function (this: BrowserWorld, name: string, row: string, status: string) {
  const rows = table(this, name).getByRole("row", { name: new RegExp(escape(row)) });
  await until(
    async () => {
      const found = await rows.first().textContent().catch(() => null);
      return found?.includes(status) ? found : null;
    },
    ROW_PATIENCE_MS,
    `the row ${row} of the table ${name} never showed ${status}`,
  );
});

Then("the table {string} has {int} rows", async function (this: BrowserWorld, name: string, count: number) {
  const rows = table(this, name).locator("tbody tr");
  await until(
    async () => {
      const found = await rows.count();
      return found === count ? found : null;
    },
    ROW_PATIENCE_MS,
    `the table ${name} never had ${count} rows`,
  );
});

// The widths of a table's columns, as its column headers are laid out, noted
// by one step for a later one to compare with.
const notedWidths = new WeakMap<BrowserWorld, number[]>();

function columnWidths(world: BrowserWorld, name: string): Promise<number[]> {
  return table(world, name)
    .getByRole("columnheader")
    .evaluateAll((cells) => cells.map((cell) => cell.getBoundingClientRect().width));
}

When("the widths of the columns of the table {string} are noted", async function (this: BrowserWorld, name: string) {
  notedWidths.set(this, await columnWidths(this, name));
});

// A column that widens or narrows as a row's content changes moves every
// column beside it: the table jumps under the reader's eyes. Within half a
// pixel, the layout's own rounding, a column keeps its width.
Then("the columns of the table {string} keep the widths noted", async function (this: BrowserWorld, name: string) {
  const noted = notedWidths.get(this);
  assert.ok(noted, `the widths of the columns of the table ${name} were never noted`);
  const now = await columnWidths(this, name);
  assert.equal(now.length, noted.length, `the table ${name} has another number of columns`);
  const moved = now.flatMap((width, at) => {
    const before = noted[at] ?? 0;
    return Math.abs(width - before) > 0.5 ? [`column ${at + 1}: ${Math.round(before)}px, now ${Math.round(width)}px`] : [];
  });
  assert.deepEqual(moved, [], `the columns of the table ${name} changed width`);
});

// The gap a reader needs to see a header's text and its button as two things:
// a quarter of the root's text size, 4 px at the browser's default.
const APART_REM = 0.25;

// A header's sort button is inside the header's own cell and clear of the
// header's text: each line of the text is at least APART_REM away from the
// button, beside it or above it, and never under it.
Then(
  "the button {string} sits inside the column header {string}, apart from its text",
  async function (this: BrowserWorld, button: string, header: string) {
    const cell = this.page.getByRole("columnheader", { name: new RegExp(`^${ISOLATION}${escape(header)}\\b`) });
    assert.equal(await cell.count(), 1, `the column header "${header}" is not on the page once`);
    const control = cell.getByRole("button", { name: namePattern(button) });
    assert.equal(await control.count(), 1, `the column header "${header}" holds no button "${button}"`);
    const handle = await control.elementHandle();
    const problems = await cell.evaluate(
      (th, { target, apartRem }) => {
        const found: string[] = [];
        if (!(target instanceof Element)) return ["the button is gone"];
        const at = target.getBoundingClientRect();
        const box = th.getBoundingClientRect();
        if (at.left < box.left - 0.5 || at.right > box.right + 0.5 || at.top < box.top - 0.5 || at.bottom > box.bottom + 0.5) {
          found.push(`the button reaches ${Math.round(at.left)}..${Math.round(at.right)}px of a header at ${Math.round(box.left)}..${Math.round(box.right)}px`);
        }
        const apart = apartRem * parseFloat(getComputedStyle(document.documentElement).fontSize);
        const walker = document.createTreeWalker(th, NodeFilter.SHOW_TEXT);
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
          if (target.contains(node) || !(node.textContent ?? "").trim()) continue;
          const range = document.createRange();
          range.selectNodeContents(node);
          for (const line of range.getClientRects()) {
            if (line.width === 0) continue;
            const gap = Math.max(at.left - line.right, line.left - at.right, at.top - line.bottom, line.top - at.bottom);
            if (gap < apart - 0.5) {
              found.push(`"${(node.textContent ?? "").trim()}" is ${gap.toFixed(1)}px from the button, less than ${apart}px`);
            }
          }
        }
        return found;
      },
      { target: handle, apartRem: APART_REM },
    );
    assert.deepEqual(problems, [], `the button "${button}" is not apart from the header "${header}"`);
  },
);

// Two buttons side by side, or one under the other once they wrap, are two
// targets only with a gap between them: their boxes are at least APART_REM
// apart, beside each other or above each other, so a reader aiming at one
// doesn't press the other. The buttons may come from an answer of their own
// (the start buttons from the plans, not the history), so each is awaited.
Then("the buttons {string} and {string} stand apart", async function (this: BrowserWorld, first: string, second: string) {
  const box = async (name: string) => {
    const control = this.page.getByRole("button", { name: namePattern(name) });
    await expect(control, `the button "${name}" is not on the page once`).toHaveCount(1);
    const shown = await control.boundingBox();
    assert.ok(shown, `the button "${name}" is not shown`);
    return shown;
  };
  const a = await box(first);
  const b = await box(second);
  const gap = Math.max(
    b.x - (a.x + a.width),
    a.x - (b.x + b.width),
    b.y - (a.y + a.height),
    a.y - (b.y + b.height),
  );
  const root = await this.page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize));
  const apart = APART_REM * root;
  assert.ok(gap >= apart - 0.5, `the buttons "${first}" and "${second}" are ${gap.toFixed(1)}px apart, less than ${apart}px`);
});

// A table whose columns have fixed shares lets a cell's content run past the
// cell when it can't fit: it draws over the next column, which neither the
// page's width nor a clipped box shows. Every box and every line of text in a
// cell stays inside the cell, within half a pixel of rounding.
Then("nothing in the table {string} runs outside its cell", async function (this: BrowserWorld, name: string) {
  const outside = await table(this, name).evaluate(async (element) => {
    await document.fonts.ready;
    // Let layout settle after injected styles.
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    const found: string[] = [];
    const describe = (rect: DOMRect) =>
      `${Math.round(rect.left)}..${Math.round(rect.right)} x ${Math.round(rect.top)}..${Math.round(rect.bottom)}`;
    for (const cell of element.querySelectorAll("th, td")) {
      const box = cell.getBoundingClientRect();
      const check = (rect: DOMRect, what: string) => {
        if (rect.width === 0 && rect.height === 0) return;
        if (rect.left < box.left - 0.5 || rect.right > box.right + 0.5 || rect.top < box.top - 0.5 || rect.bottom > box.bottom + 0.5) {
          found.push(`${what} at ${describe(rect)}px runs outside its cell at ${describe(box)}px`);
        }
      };
      for (const inner of cell.querySelectorAll("*")) {
        if (getComputedStyle(inner).display === "none") continue;
        check(inner.getBoundingClientRect(), `<${inner.localName}> "${(inner.textContent ?? "").trim().slice(0, 30)}"`);
      }
      const walker = document.createTreeWalker(cell, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        if (!(node.textContent ?? "").trim()) continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        for (const line of range.getClientRects()) check(line, `the text "${(node.textContent ?? "").trim().slice(0, 30)}"`);
      }
    }
    return found;
  });
  assert.deepEqual(outside, [], `something in the table ${name} runs outside its cell`);
});

/** The row of a table that its row header names. A name the catalog wrote
 *  holds its arguments in bidi isolation marks, which a reader never hears. */
function rowNamed(world: BrowserWorld, name: string, row: string): Locator {
  return table(world, name)
    .getByRole("row")
    .filter({ has: world.page.getByRole("rowheader", { name: namePattern(row) }) });
}

Then("the table {string} has the row {string}", async function (this: BrowserWorld, name: string, row: string) {
  await rowNamed(this, name, row).first().waitFor({ state: "visible", timeout: ROW_PATIENCE_MS });
});

/** What a reader hears of the cell of `row` under the column header `column`,
 *  once the row is there: its text without bidi isolation marks. */
export async function cellText(world: BrowserWorld, name: string, row: string, column: string): Promise<string> {
  const header = table(world, name).getByRole("columnheader", { name: new RegExp(`^${escape(column)}`) });
  const index = await header.evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
  const found = rowNamed(world, name, row).first();
  await found.waitFor({ state: "visible", timeout: ROW_PATIENCE_MS });
  const text = await found.evaluate((tr: HTMLTableRowElement, at) => tr.cells[at]?.innerText ?? null, index);
  assert.ok(text !== null, `the row ${row} of the table ${name} has no cell under ${column}`);
  return text.replace(/[\u2066-\u2069]/g, "");
}

/** Waits until the cell of `row` under `column` holds `expected`: a row that
 *  is still running says what it did only once it has ended. */
export async function untilCellShows(world: BrowserWorld, name: string, row: string, column: string, expected: string): Promise<void> {
  const end = Date.now() + ROW_PATIENCE_MS;
  let seen = "";
  while (Date.now() < end) {
    seen = await cellText(world, name, row, column);
    if (seen.includes(expected)) return;
    await sleep(250);
  }
  assert.fail(`the row ${row} of the table ${name} never showed "${expected}" in its ${column} column; it showed "${seen}"`);
}

Then(
  "the row {string} of the table {string} shows {string} in its {string} column",
  async function (this: BrowserWorld, row: string, name: string, expected: string, column: string) {
    await untilCellShows(this, name, row, column, expected);
  },
);

// A paused page holds still (§4.5): the cell says what it said, and goes on
// saying it for as long as an update would have taken to reach it and more.
Then(
  "the row {string} of the table {string} keeps showing {string} in its {string} column",
  async function (this: BrowserWorld, row: string, name: string, expected: string, column: string) {
    const end = Date.now() + STILL_MS;
    do {
      const seen = await cellText(this, name, row, column);
      assert.ok(seen.includes(expected), `the row ${row} of the table ${name} showed "${seen}" in its ${column} column, not "${expected}"`);
      await sleep(100);
    } while (Date.now() < end);
  },
);

Then("the table {string} shows no progress bar", async function (this: BrowserWorld, name: string) {
  await expect(table(this, name).getByRole("progressbar")).toHaveCount(0);
});

/** Waits until the cell of `row` under `column` holds a list with an item
 *  that says `expected`: a reader hears what a run did as a list, one fact an
 *  item. */
export async function untilCellLists(world: BrowserWorld, name: string, row: string, column: string, expected: string): Promise<void> {
  const header = table(world, name).getByRole("columnheader", { name: new RegExp(`^${escape(column)}`) });
  const index = await header.evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
  const found = rowNamed(world, name, row).first();
  await found.waitFor({ state: "visible", timeout: ROW_PATIENCE_MS });
  // A cell's role leaves the row header out, so the cells count from after it.
  const headerAt = await found.getByRole("rowheader").evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
  assert.equal(headerAt, 0, `the row ${row} of the table ${name} is not named by its first cell`);
  const said = new RegExp([...expected].map(escape).join(ISOLATION));
  const item = found.getByRole("cell").nth(index - 1).getByRole("list").getByRole("listitem").filter({ hasText: said });
  await expect(item, `the ${column} of the row ${row} lists "${expected}"`).toHaveCount(1, { timeout: ROW_PATIENCE_MS });
}

Then(
  "the row {string} of the table {string} lists {string} in its {string} column",
  async function (this: BrowserWorld, row: string, name: string, expected: string, column: string) {
    await untilCellLists(this, name, row, column, expected);
  },
);

// The cells of the named column each carry the instant they show, so the
// order is read from those, not from localized text.
async function instants(world: BrowserWorld, name: string, column: string): Promise<string[]> {
  const header = table(world, name).getByRole("columnheader", { name: new RegExp(escape(column)) });
  const index = await header.evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
  return table(world, name)
    .locator("tbody tr")
    .evaluateAll(
      (rows: HTMLTableRowElement[], at) =>
        rows.map((row) => {
          const cell = row.cells[at];
          if (cell === undefined) throw new Error(`a row has no cell ${at}`);
          return cell.title;
        }),
      index,
    );
}

Then("the table {string} shows its {string} column newest first", async function (this: BrowserWorld, name: string, column: string) {
  const values = await instants(this, name, column);
  assert.ok(values.length > 0, `the table ${name} has no rows to be newest first`);
  const sorted = [...values].sort((a, b) => b.localeCompare(a));
  assert.deepEqual(values, sorted, `the ${column} column of ${name} is not newest first`);
});

Given("the browser is in the time zone {string}", function (this: BrowserWorld, zone: string) {
  this.contextOptions.timezoneId = zone;
});

// Each cell carries the instant it shows in its `title`, in UTC. What it shows
// is that instant read in the browser's own zone, as the browser's `Intl`
// writes it: the default of the formatter is UTC, so a page that didn't pass
// the zone would show the title's own hours.
Then("the table {string} shows its {string} column in the time zone {string}", async function (this: BrowserWorld, name: string, column: string, zone: string) {
  const header = table(this, name).getByRole("columnheader", { name: new RegExp(escape(column)) });
  const index = await header.evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
  const cells = await table(this, name)
    .locator("tbody tr")
    .evaluateAll(
      (rows: HTMLTableRowElement[], at) =>
        rows.map((row) => {
          const cell = row.cells[at];
          if (cell === undefined) throw new Error(`a row has no cell ${at}`);
          return [cell.title, cell.textContent ?? ""] as const;
        }),
      index,
    );
  assert.ok(cells.length > 0, `the table ${name} has no rows to read in ${zone}`);
  const fold = (text: string) => text.replace(/[\u202f\u00a0]/g, " ");
  const written = new Intl.DateTimeFormat("en-US", { dateStyle: "short", timeStyle: "medium", timeZone: zone });
  for (const [raw, shown] of cells) {
    assert.equal(fold(shown), fold(written.format(new Date(raw))), `the ${column} cell of ${raw} is not in ${zone}`);
  }
});

// The table answers a press of its sort button by reordering the rows, so the
// step waits for the order as it waits for any update.
Then("the table {string} shows its {string} column oldest first", async function (this: BrowserWorld, name: string, column: string) {
  const oldestFirst = (values: string[]) => [...values].sort((a, b) => a.localeCompare(b));
  const end = Date.now() + UPDATE_PATIENCE_MS;
  let values = await instants(this, name, column);
  while (Date.now() < end && values.join() !== oldestFirst(values).join()) {
    await sleep(100);
    values = await instants(this, name, column);
  }
  assert.ok(values.length > 0, `the table ${name} has no rows to be oldest first`);
  assert.deepEqual(values, oldestFirst(values), `the ${column} column of ${name} is not oldest first`);
});

// What the header says of the sort is what a screen reader tells (WCAG 1.3.1).
Then(
  "the table {string} announces its {string} column as sorted {word}",
  async function (this: BrowserWorld, name: string, column: string, direction: string) {
    const header = table(this, name).getByRole("columnheader", { name: new RegExp(escape(column)) });
    await expect(header).toHaveAttribute("aria-sort", direction);
  },
);
Then("the page shows the paragraph {string}", async function (this: BrowserWorld, text: string) {
  await this.page.getByText(text, { exact: true }).first().waitFor({ state: "visible" });
});

Then("the page shows the heading {string}", async function (this: BrowserWorld, name: string) {
  await this.page
    .getByRole("heading", { level: 1, name, exact: true })
    .waitFor({ state: "visible", timeout: 25_000 });
});
