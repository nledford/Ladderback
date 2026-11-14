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

import { After, type DataTable, Given, Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";
import { expect } from "playwright/test";

import { ISOLATION, namePattern } from "../support/keyboard.ts";
import { partsBrokenInside } from "../support/parts.ts";
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
export function table(world: BrowserWorld, name: string): Locator {
  return world.page.getByRole("table", { name: namePattern(name) });
}

/** Writes the config of one or more demo plans and their repository into a
 *  scratch folder, starts a daemon on it, and points the scenario's pages at
 *  that daemon. `env` is added to the daemon's environment, as `TZ` for the
 *  zone its clock runs in, which its workers inherit. A plan `schedules`
 *  names runs on that cron expression, and a plan in `iglobs` has those
 *  globs. */
export async function serveDemoPlans(
  world: BrowserWorld,
  plans: readonly string[],
  size: keyof typeof SOURCE_FILES,
  env: Record<string, string> = {},
  schedules: Readonly<Record<string, string>> = {},
  iglobs: Readonly<Record<string, readonly string[]>> = {},
): Promise<void> {
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
      ...(plan in schedules ? [`schedule = { cron = ${JSON.stringify(schedules[plan])} }`] : []),
      ...(plan in iglobs ? [`iglobs = ${JSON.stringify(iglobs[plan])}`] : []),
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
    ...env,
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

/** The run the daemon's plans list says `plan` has planned, if any. */
async function plannedRun(world: BrowserWorld, plan: string): Promise<string | null> {
  assert.ok(world.baseUrl, "this scenario's daemon was never started");
  const response = await fetch(new URL("/api/v1/plans", world.baseUrl), {
    headers: { Origin: new URL(world.baseUrl).origin },
  });
  if (!response.ok) return null;
  const { plans } = (await response.json()) as { plans: { id: string; "next-run": string | null }[] };
  return plans.find((listed) => listed.id === plan)?.["next-run"] ?? null;
}

// The scheduler plans a plan it first finds as the daemon starts; the Given
// waits until it has, so the page opened next has a planned run to show.
Given(
  "a demo plan {string} on the schedule {string} and a demo plan {string} without one",
  async function (this: BrowserWorld, scheduled: string, cron: string, unscheduled: string) {
    await serveDemoPlans(this, [scheduled, unscheduled], "tiny", {}, { [scheduled]: cron });
    await until(() => plannedRun(this, scheduled), UPDATE_PATIENCE_MS, `the daemon planned no run of ${scheduled}`);
  },
);

// A plan with globs, so that its row has something to say of them.
Given(
  "a demo plan {string} whose iglobs are {string} and {string}",
  async function (this: BrowserWorld, plan: string, first: string, second: string) {
    await serveDemoPlans(this, [plan], "tiny", {}, {}, { [plan]: [first, second] });
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

Given("the daemon reports a running check of {string}", function (this: BrowserWorld, repository: string) {
  const daemon = new ScriptedDaemon();
  daemon.addRunningCheck(repository, "2026-11-09T10:00:01Z");
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

// A backup refused before it ran because the password file its repository
// names could not be read: the failure's code and arguments as the daemon's
// wire has them, so the reason names the file as a placeable.
Given(
  "the daemon reports a backup of {string} that failed because it could not read {string}",
  function (this: BrowserWorld, plan: string, file: string) {
    const daemon = new ScriptedDaemon();
    daemon.addBackup(plan, "failed", {
      queuedAt: "2026-11-09T10:00:00Z",
      finishedAt: "2026-11-09T10:00:00Z",
      failure: {
        code: "secret-unreadable",
        args: { field: { type: "text", value: "password" }, source: { type: "text", value: file } },
      },
    });
    this.scriptedDaemon = daemon;
  },
);

// A backup that went through `gib` GiB and wrote a snapshot: the facts a
// finished backup's Result cell lists, as the daemon's wire has them.
Given(
  "the daemon reports a finished backup of {string} that went through {float} GiB",
  function (this: BrowserWorld, plan: string, gib: number) {
    const daemon = new ScriptedDaemon();
    daemon.addBackup(plan, "succeeded", {
      queuedAt: "2026-11-09T10:00:00Z",
      startedAt: "2026-11-09T10:00:00Z",
      finishedAt: "2026-11-09T10:42:00Z",
      result: {
        type: "backup",
        files: 1204,
        bytes: 1_288_490_189,
        "duration-ms": 2_520_000,
        snapshot: `ba5cc73e${"0".repeat(56)}`,
        "bytes-processed": Math.round(gib * 2 ** 30),
      },
    });
    this.scriptedDaemon = daemon;
  },
);

When("the daemon reports that the backup of {string} has started", async function (this: BrowserWorld, plan: string) {
  await scripted(this).starts(plan, "2026-11-09T10:00:05Z");
});

When("the daemon holds back the opening of the page's events stream", function (this: BrowserWorld) {
  scripted(this).holdTheNextStream();
});

When("the backup of {string} starts before the page's events stream is open", async function (this: BrowserWorld, plan: string) {
  await scripted(this).startsUnheard(plan, "2026-11-09T10:00:05Z");
});

When("a before-hook of the backup of {string} warns before the page's events stream is open", async function (this: BrowserWorld, plan: string) {
  await scripted(this).warnsUnheard(plan);
});

When("the daemon opens the events stream it held back", async function (this: BrowserWorld) {
  await scripted(this).openTheHeldStream();
});

When("the page has read the history again since its events stream opened", async function (this: BrowserWorld) {
  await scripted(this).untilReadSinceTheOpening();
});

When("the daemon reports {int} steps of progress of the backup of {string}", async function (this: BrowserWorld, steps: number, plan: string) {
  await scripted(this).progresses(plan, steps);
});

// A step as a person says it, and its code on the wire (decision 82).
const STEP_CODES: Record<string, string> = {
  "reading the index": "reading-index",
  "backing up": "backing-up",
  "checking trees": "checking-trees",
};

function stepCode(step: string): string {
  const code = STEP_CODES[step];
  assert.ok(code, `no code for the step "${step}"`);
  return code;
}

When(
  /^the daemon reports that the backup of "([^"]*)" is (reading the index|backing up), (\d+) of (\d+) done$/,
  async function (this: BrowserWorld, plan: string, step: string, current: string, total: string) {
    await scripted(this).progressesInStep(plan, stepCode(step), Number(current), Number(total));
  },
);

When(
  /^the daemon reports that the backup of "([^"]*)" is (reading the index|backing up), (\d+) done of a total not known yet$/,
  async function (this: BrowserWorld, plan: string, step: string, current: string) {
    await scripted(this).progressesInStep(plan, stepCode(step), Number(current), null);
  },
);

const GIB = 1024 ** 3;

When(
  "the daemon reports that the backup of {string} is backing up, {int} GiB of {int} GiB done",
  async function (this: BrowserWorld, plan: string, current: number, total: number) {
    await scripted(this).progressesInStep(plan, "backing-up", current * GIB, total * GIB, "bytes");
  },
);

When(
  "the daemon reports that the check of {string} is checking trees, {int} of {int} done",
  async function (this: BrowserWorld, repository: string, current: number, total: number) {
    await scripted(this).checkProgressesInStep(repository, stepCode("checking trees"), current, total);
  },
);

// The page's clock, which a Given installed, moves on: its timers that fall
// due meanwhile run, and what it reads as the time has moved on with them.
// Nothing else changes: the daemon tells the page nothing meanwhile.
When("the page's clock moves on {int} seconds", async function (this: BrowserWorld, seconds: number) {
  assert.ok(this.fakeClock, "the page's clock can't be moved on: no Given installed it");
  await this.page.clock.fastForward(seconds * 1000);
});

When(
  "the daemon reports {int} bytes processed of the backup of {string} and not yet its total",
  async function (this: BrowserWorld, bytes: number, plan: string) {
    await scripted(this).progressesWithoutTotal(plan, bytes);
  },
);

When(
  "the daemon reports that the backup of {string} has succeeded with {int} files",
  async function (this: BrowserWorld, plan: string, files: number) {
    await scripted(this).succeeds(plan, files, "2026-11-09T10:05:00Z");
  },
);

When("the daemon reports that the backup of {string} has failed", async function (this: BrowserWorld, plan: string) {
  await scripted(this).fails(plan, "2026-11-09T10:05:00Z");
});

When("the daemon reports that the backup of {string} has been cancelled", function (this: BrowserWorld, plan: string) {
  scripted(this).isCancelled(plan, "2026-11-09T10:05:00Z");
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

Given("the daemon queues each backup the page starts", function (this: BrowserWorld) {
  scripted(this).answerStarts();
});

Given("the daemon accepts each cancel the page asks for, and the run goes on", function (this: BrowserWorld) {
  scripted(this).acceptCancels();
});

When("the daemon has accepted the cancel of the backup of {string}", async function (this: BrowserWorld, plan: string) {
  await scripted(this).untilCancelAccepted(plan);
});

// A page the reader has left listens no more and reads nothing more: its
// events stream closes, and neither a reconnect nor a settle-poll asks the
// daemon anything after it (#300). A read or an opening asked for just before
// the page went may still be on its way, so the watch starts a moment later,
// and lasts longer than a first reconnect's back-off (one second) and many of
// a settle-poll's rounds (a quarter of a second each).
Then("the page stops listening to the daemon", async function (this: BrowserWorld) {
  const daemon = scripted(this);
  await daemon.untilStreamsOpen(0);
  await sleep(500);
  const [reads, opened] = [daemon.historyReads, daemon.streamsOpened];
  await sleep(2_000);
  assert.equal(daemon.historyReads - reads, 0, "the page read the history after the reader left it");
  assert.equal(daemon.streamsOpened - opened, 0, "the page opened an events stream after the reader left it");
});

// One stream, and still one a moment later: a page that kept an old stream,
// or opened a second, has more.
Then("the page has one events stream open", async function (this: BrowserWorld) {
  const daemon = scripted(this);
  await daemon.untilStreamsOpen(1);
  await sleep(1_000);
  assert.equal(daemon.openStreams, 1, `the page has ${daemon.openStreams} events streams open, not one`);
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

// A row is named by its plan, or a maintenance run by what ran and its
// repository, whose alias the page isolates (§4.5); what it shows is its
// life so far, which a run changes as it goes, so the step waits for the
// row to say it.
Then("the table {string} has a row {string} that shows {string}", async function (this: BrowserWorld, name: string, row: string, status: string) {
  const rows = table(this, name).getByRole("row", { name: new RegExp([...row].map(escape).join(ISOLATION)) });
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
  await hasRows(table(this, name), count, `the table ${name}`);
});

// The same, on a page with one table, whose name a pseudolocale transformed.
Then("the page's one table has {int} row(s)", async function (this: BrowserWorld, count: number) {
  await hasRows(this.page.getByRole("table"), count, "the table");
});

async function hasRows(grid: Locator, count: number, what: string): Promise<void> {
  const rows = grid.locator("tbody tr");
  await until(
    async () => {
      const found = await rows.count();
      return found === count ? found : null;
    },
    ROW_PATIENCE_MS,
    `${what} never had ${count} rows`,
  );
}

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
  assert.deepEqual(await outsideItsCells(table(this, name)), [], `something in the table ${name} runs outside its cell`);
});

// The same, on a page with one table, whose name a pseudolocale transformed.
Then("nothing in the page's one table runs outside its cell", async function (this: BrowserWorld) {
  assert.deepEqual(await outsideItsCells(this.page.getByRole("table")), [], "something in the table runs outside its cell");
});

async function outsideItsCells(grid: Locator): Promise<string[]> {
  return grid.evaluate(async (element) => {
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
}

/** The row of a table that its row header names. A name the catalog wrote
 *  holds its arguments in bidi isolation marks, which a reader never hears. */
export function rowNamed(world: BrowserWorld, name: string, row: string): Locator {
  return table(world, name)
    .getByRole("row")
    .filter({ has: world.page.getByRole("rowheader", { name: namePattern(row) }) });
}

Then("the table {string} has the row {string}", async function (this: BrowserWorld, name: string, row: string) {
  await rowNamed(this, name, row).first().waitFor({ state: "visible", timeout: ROW_PATIENCE_MS });
});

// For a page in a pseudolocale, whose tables' names the catalog transformed:
// the row is found by its header alone, which is data and stays as it is.
Then("the page shows the row {string}", async function (this: BrowserWorld, row: string) {
  await this.page
    .getByRole("row")
    .filter({ has: this.page.getByRole("rowheader", { name: namePattern(row) }) })
    .first()
    .waitFor({ state: "visible", timeout: ROW_PATIENCE_MS });
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

// What the cell shows, as a sighted reader sees it: the text a screen reader
// skips counts too, so a bar's hidden "0%" would fail it.
Then(
  "the row {string} of the table {string} shows no percentage in its {string} column",
  async function (this: BrowserWorld, row: string, name: string, column: string) {
    const seen = await cellText(this, name, row, column);
    assert.doesNotMatch(seen, /%/, `the row ${row} of the table ${name} showed "${seen}" in its ${column} column`);
  },
);

Then(
  "the row {string} of the table {string} doesn't show {string} in its {string} column",
  async function (this: BrowserWorld, row: string, name: string, unexpected: string, column: string) {
    const end = Date.now() + ROW_PATIENCE_MS;
    let seen = await cellText(this, name, row, column);
    while (seen.includes(unexpected) && Date.now() < end) {
      await sleep(250);
      seen = await cellText(this, name, row, column);
    }
    assert.ok(!seen.includes(unexpected), `the row ${row} of the table ${name} kept showing "${unexpected}" in its ${column} column: "${seen}"`);
  },
);

/** The cell of `row` under the column header `column`, once the row is
 *  there, found by its role. */
async function cellOf(world: BrowserWorld, name: string, row: string, column: string): Promise<Locator> {
  const header = table(world, name).getByRole("columnheader", { name: new RegExp(`^${escape(column)}`) });
  const index = await header.evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
  const found = rowNamed(world, name, row).first();
  await found.waitFor({ state: "visible", timeout: ROW_PATIENCE_MS });
  // A cell's role leaves the row header out, so the cells count from after it.
  const headerAt = await found.getByRole("rowheader").evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
  assert.equal(headerAt, 0, `the row ${row} of the table ${name} is not named by its first cell`);
  return found.getByRole("cell").nth(index - 1);
}

/** Waits until the cell of `row` under `column` holds a list with an item
 *  that says `expected`: a reader hears what a run did as a list, one fact an
 *  item. */
export async function untilCellLists(world: BrowserWorld, name: string, row: string, column: string, expected: string): Promise<void> {
  const said = new RegExp([...expected].map(escape).join(ISOLATION));
  const cell = await cellOf(world, name, row, column);
  const item = cell.getByRole("list").getByRole("listitem").filter({ hasText: said });
  await expect(item, `the ${column} of the row ${row} lists "${expected}"`).toHaveCount(1, { timeout: ROW_PATIENCE_MS });
}

Then(
  "the row {string} of the table {string} lists {string} in its {string} column",
  async function (this: BrowserWorld, row: string, name: string, expected: string, column: string) {
    await untilCellLists(this, name, row, column, expected);
  },
);

// One item alone is said as it is: a list of one is heard as a list and
// draws a marker that sets nothing apart (#430).
Then(
  "the row {string} of the table {string} says {string} in its {string} column, not as a list",
  async function (this: BrowserWorld, row: string, name: string, expected: string, column: string) {
    await untilCellShows(this, name, row, column, expected);
    const cell = await cellOf(this, name, row, column);
    await expect(cell.getByRole("list"), `the ${column} of the row ${row} says "${expected}" as a list`).toHaveCount(0);
  },
);

// Where the table has room, each item of a list in the cell reads on one
// line: it wraps only between items, not inside one (#430). Each
// character's box says which line it is on; a mark of no width is on none.
Then(
  "the row {string} of the table {string} lists each item on one line in its {string} column",
  async function (this: BrowserWorld, row: string, name: string, column: string) {
    const cell = await cellOf(this, name, row, column);
    const items = cell.getByRole("list").getByRole("listitem");
    await expect(items.first(), `the ${column} of the row ${row} lists nothing`).toBeVisible({ timeout: ROW_PATIENCE_MS });
    const split = await items.evaluateAll((elements) =>
      elements.flatMap((item) => {
        const tops = new Set<number>();
        const walker = document.createTreeWalker(item, NodeFilter.SHOW_TEXT);
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
          for (let at = 0; at < (node.textContent ?? "").length; at += 1) {
            const range = document.createRange();
            range.setStart(node, at);
            range.setEnd(node, at + 1);
            const box = [...range.getClientRects()].find((rect) => rect.width > 0);
            if (box !== undefined) tops.add(Math.round(box.top));
          }
        }
        return tops.size > 1 ? [`"${item.textContent}" takes ${tops.size} lines`] : [];
      }),
    );
    assert.deepEqual(split, [], `the ${column} of the row ${row} breaks inside an item`);
  },
);

// A path or an address the cell shows moves to a line of its own rather than
// breaking where a line runs out, when a line has room for it: its letters
// all sit on one line. The text is found across the cell's text nodes, a
// placeable's bidi isolation marks left out, as a reader doesn't see them.
Then(
  "the row {string} of the table {string} keeps {string} on one line in its {string} column",
  async function (this: BrowserWorld, row: string, name: string, expected: string, column: string) {
    await untilCellShows(this, name, row, column, expected);
    const cell = await cellOf(this, name, row, column);
    const lines = await cell.evaluate((td, expected) => {
      const chars: { node: Node; at: number }[] = [];
      let text = "";
      const walker = document.createTreeWalker(td, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const content = node.textContent ?? "";
        for (let at = 0; at < content.length; at += 1) {
          const char = content.charAt(at);
          if (/[\u2066-\u2069]/.test(char)) continue;
          chars.push({ node, at });
          text += char;
        }
      }
      const start = text.indexOf(expected);
      if (start < 0) return null;
      const tops = new Set<number>();
      for (const { node, at } of chars.slice(start, start + expected.length)) {
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + 1);
        const box = [...range.getClientRects()].find((rect) => rect.width > 0);
        if (box !== undefined) tops.add(Math.round(box.top));
      }
      return tops.size;
    }, expected);
    assert.ok(lines !== null, `the ${column} of the row ${row} doesn't show "${expected}"`);
    assert.equal(lines, 1, `"${expected}" takes ${lines} lines in the ${column} of the row ${row}`);
  },
);

// A path the cell shows breaks, where a line has no room for it whole, only
// between its parts, as an address does (`partsBrokenInside`, #482).
Then(
  "the row {string} of the table {string} keeps each part of {string} whole in its {string} column",
  async function (this: BrowserWorld, row: string, name: string, expected: string, column: string) {
    await untilCellShows(this, name, row, column, expected);
    const cell = await cellOf(this, name, row, column);
    const found = await cell.evaluate(partsBrokenInside, { text: expected, whole: true });
    assert.ok(found !== null, `the ${column} of the row ${row} doesn't show "${expected}"`);
    assert.deepEqual(found.inside, [], `"${expected}" breaks inside a part in the ${column} of the row ${row}`);
  },
);

// A column's header is a word or two: beside columns that take the room it
// keeps each word whole rather than breaking where its line runs out
// ("Actio" / "ns", #482).
Then(
  "the header {string} of the table {string} keeps its words whole",
  async function (this: BrowserWorld, column: string, name: string) {
    const header = table(this, name).getByRole("columnheader", { name: new RegExp(`^${escape(column)}`) });
    for (const word of column.split(" ")) {
      const found = await header.evaluate(partsBrokenInside, { text: word, whole: true });
      assert.ok(found !== null, `the header ${column} of the table ${name} doesn't show "${word}"`);
      assert.deepEqual(found.inside, [], `"${word}" breaks inside in the header ${column} of the table ${name}`);
    }
  },
);

// The column that says what a run did or why it failed has the room the
// others leave: it is wider than any of them.
Then(
  "the column {string} of the table {string} is wider than each of the others",
  async function (this: BrowserWorld, column: string, name: string) {
    const header = table(this, name).getByRole("columnheader", { name: new RegExp(`^${escape(column)}`) });
    const index = await header.evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
    const widths = await columnWidths(this, name);
    const own = widths[index] ?? 0;
    const wider = widths.flatMap((width, at) => (at !== index && width >= own ? [`column ${at + 1}: ${Math.round(width)}px`] : []));
    assert.deepEqual(wider, [], `the column ${column} of the table ${name} is ${Math.round(own)}px wide, no wider than these`);
  },
);

// A column of sentences keeps room enough to read them as sentences beside
// columns that hold paths: at least a quarter of the table, whatever those
// need (#482).
Then(
  "the column {string} of the table {string} takes at least a quarter of the table's width",
  async function (this: BrowserWorld, column: string, name: string) {
    const header = table(this, name).getByRole("columnheader", { name: new RegExp(`^${escape(column)}`) });
    const index = await header.evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
    const widths = await columnWidths(this, name);
    const own = widths[index] ?? 0;
    const whole = widths.reduce((sum, width) => sum + width, 0);
    assert.ok(own >= whole / 4, `the column ${column} of the table ${name} is ${Math.round(own)}px of ${Math.round(whole)}px`);
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

/** The cells of the named column, each as the instant its `title` carries (in
 *  UTC) and what a reader hears of it: its text, without bidi isolation marks
 *  and with the narrow and no-break spaces V8 and CLDR differ on folded. */
async function timeCells(world: BrowserWorld, name: string, column: string): Promise<Array<readonly [string, string]>> {
  const header = table(world, name).getByRole("columnheader", { name: new RegExp(escape(column)) });
  const index = await header.evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
  const cells = await table(world, name)
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
  return cells.map(([raw, shown]) => [raw, fold(shown)] as const);
}

const fold = (text: string) => text.replace(/[\u2066-\u2069]/g, "").replace(/[\u202f\u00a0]/g, " ");

/** An instant as the page's `ui-instant` writes it in en-US, read in `zone`. */
const writtenIn = (zone: string) => {
  const written = new Intl.DateTimeFormat("en-US", { dateStyle: "short", timeStyle: "medium", timeZone: zone });
  return (raw: string) => fold(written.format(new Date(raw)));
};

// Each cell carries the instant it shows in its `title`, in UTC. What it shows
// is that instant read in the browser's own zone, as the browser's `Intl`
// writes it: the default of the formatter is UTC, so a page that didn't pass
// the zone would show the title's own hours.
Then("the table {string} shows its {string} column in the time zone {string}", async function (this: BrowserWorld, name: string, column: string, zone: string) {
  const cells = await timeCells(this, name, column);
  assert.ok(cells.length > 0, `the table ${name} has no rows to read in ${zone}`);
  const written = writtenIn(zone);
  for (const [raw, shown] of cells) {
    assert.equal(shown, written(raw), `the ${column} cell of ${raw} is not in ${zone}`);
  }
});

// A snapshot time is what the clock that took it read (§16, decision 56): the
// cell's instant read in the zone that clock ran in, then that clock's offset,
// which a viewer whose own offset differs is told.
Then(
  "the table {string} shows its {string} column as a clock in {string} read it, with the offset {string}",
  async function (this: BrowserWorld, name: string, column: string, zone: string, offset: string) {
    const cells = await timeCells(this, name, column);
    assert.ok(cells.length > 0, `the table ${name} has no rows to read`);
    const written = writtenIn(zone);
    for (const [raw, shown] of cells) {
      assert.equal(shown, `${written(raw)} ${offset}`, `the ${column} cell of ${raw} is not as a clock in ${zone} read it`);
    }
  },
);

Then("the table {string} shows these times in its {string} column:", async function (this: BrowserWorld, name: string, column: string, expected: DataTable) {
  const shown = (await timeCells(this, name, column)).map(([, text]) => text);
  assert.deepEqual(shown, expected.raw().map(([text]) => text), `the ${column} column of ${name}`);
});

// The snapshots a plan's group lists, in place of the real daemon's: each is
// a time and the offset of the clock that took it, as a snapshot file records
// it (`2026-11-10 23:30:00-10:00`), newest first. The rest of the answer is
// the real daemon's.
Given("the plan {string} holds snapshots taken at:", function (this: BrowserWorld, plan: string, taken: DataTable) {
  const snapshots = taken.raw().map(([recorded], n) => {
    const parts = /^(\d{4}-\d{2}-\d{2}) (\d{2}:\d{2}:\d{2})([+-]\d{2}:\d{2})$/.exec(recorded ?? "");
    if (parts === null) throw new Error(`not a recorded time: ${recorded}`);
    const [, date, time, offset] = parts;
    return {
      id: (n + 1).toString(16).padStart(64, "0"),
      time: new Date(`${date}T${time}${offset}`).toISOString().replace(".000Z", "Z"),
      offset,
      hostname: "demo-host",
      label: "",
      tags: [`ladderback-plan:${plan}`],
      paths: ["/home/demo"],
    };
  });
  this.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/repositories/demo/groups", async (route) => {
      const answer = await route.fetch();
      const body = (await answer.json()) as { plans: Array<{ plan: string; count: number; snapshots: unknown[] }> };
      const group = { plan, count: snapshots.length, snapshots };
      body.plans = [...body.plans.filter((listed) => listed.plan !== plan), group];
      await route.fulfill({ response: answer, json: body });
    });
  });
});

// The cell of a plan's next run carries in its `title` the run the daemon
// planned, in UTC as the plans list sends it, and shows that instant read in
// the browser's own zone.
Then(
  "the row {string} of the table {string} shows its planned run in its {string} column in the time zone {string}",
  async function (this: BrowserWorld, row: string, name: string, column: string, zone: string) {
    const planned = await plannedRun(this, row);
    assert.ok(planned, `the daemon lists no planned run of ${row}`);
    const header = table(this, name).getByRole("columnheader", { name: new RegExp(`^${escape(column)}`) });
    const index = await header.evaluate((cell: HTMLTableCellElement) => cell.cellIndex);
    const found = rowNamed(this, name, row).first();
    await found.waitFor({ state: "visible", timeout: UPDATE_PATIENCE_MS });
    const [title, shown] = await found.evaluate((tr: HTMLTableRowElement, at) => {
      const cell = tr.cells[at];
      if (cell === undefined) throw new Error(`the row has no cell ${at}`);
      return [cell.title, cell.textContent ?? ""] as const;
    }, index);
    assert.equal(title, planned, `the ${column} cell of ${row} doesn't carry the run the daemon planned`);
    assert.equal(fold(shown), writtenIn(zone)(planned), `the ${column} cell of ${row} is not in ${zone}`);
  },
);

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

// The page's heading names what the page shows, and a table takes its name
// from it, rather than a caption saying it again under the heading (#431):
// the main content shows the words once, and that once is the heading.
Then("the main content shows {string} once, as its heading", async function (this: BrowserWorld, text: string) {
  const main = this.page.getByRole("main");
  await expect(main.getByRole("heading", { level: 1, name: text, exact: true })).toBeVisible();
  await expect(main.getByText(text, { exact: true }), `the main content shows "${text}" besides its heading`).toHaveCount(1);
});
