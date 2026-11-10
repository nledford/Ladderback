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
import { expect } from "playwright/test";

import { namePattern } from "../support/keyboard.mjs";
import { ScriptedDaemon } from "../support/scripted-daemon.mjs";
import { fixtureRepo, startServer } from "../support/world.mjs";

// The scratch repositories' password: the one the `demo-repo` fixture was
// initialized with. Not a real secret.
const PASSWORD = "demo-password";

// A real backup takes real time, and the steps that wait for one are patient:
// enough for a big scratch source to finish, in a debug build.
const ROW_PATIENCE_MS = 120_000;

// A page's answer to a key press, or to an event a step sent it, takes a
// moment at most: this long, not as long as a real backup.
const UPDATE_PATIENCE_MS = 5_000;

// The three sources a demo plan backs up: `tiny` ends at once, for journeys
// that need several real snapshots; `small` runs long enough that its
// Running row is there to be read (a second or two) and then ends; `big`
// outlives the moment a journey cancels it or reloads the page mid-run
// (about ten seconds). Each file's content is unique to its number, so the
// repository cannot collapse the source into one chunk and end at once.
const SOURCE_FILES = { tiny: 3, small: 10_000, big: 50_000 };
const FILE_BYTES = 2048;

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

const escape = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Polls `read` until it returns a string, or fails with what it last saw. */
async function until(read, ms, what) {
  const end = Date.now() + ms;
  let last;
  while (Date.now() < end) {
    last = await read();
    if (last !== null) return last;
    await sleep(250);
  }
  assert.fail(`${what} (last saw ${last === null ? "nothing" : JSON.stringify(last)})`);
}

/** The table a page shows, by its whole caption. A value in the caption is
 *  wrapped in bidi isolation marks, which a reader never hears (§4.5). */
function table(world, name) {
  return world.page.getByRole("table", { name: namePattern(name) });
}

/** Writes a demo plan's config and repository into a scratch folder, starts
 *  a daemon on it, and points the scenario's pages at that daemon. */
export async function serveDemoPlan(world, plan, size) {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-plan-"));
  await cp(fixtureRepo, path.join(scratch, "repo"), { recursive: true });
  const source = path.join(scratch, "source");
  await mkdir(source);
  for (let n = 1; n <= SOURCE_FILES[size]; n += 1) {
    await writeFile(path.join(source, `demo-${n}.txt`), `${n}:` + "d".repeat(FILE_BYTES));
  }
  // The shape of the e2e `serving_with_plan` step: one repository, one plan
  // backing the source up into it, nothing else set.
  const config = [
    "schema = 1",
    "",
    "[repositories.demo]",
    `repository = ${JSON.stringify(path.join(scratch, "repo"))}`,
    `password = ${JSON.stringify(PASSWORD)}`,
    "",
    "[[plans]]",
    `id = ${JSON.stringify(plan)}`,
    'repository = "demo"',
    `sources = [${JSON.stringify(source)}]`,
    "",
  ].join("\n");
  const configDir = path.join(scratch, "config");
  await mkdir(configDir);
  await writeFile(path.join(configDir, "config.toml"), config);
  const { child, url } = await startServer({
    LADDERBACK_CONFIG_DIR: configDir,
    LADDERBACK_DATA_DIR: path.join(scratch, "data"),
  });
  world.ownServer = { child, scratch };
  world.baseUrl = url;
}

After(async function () {
  const own = this.ownServer;
  if (own && own.child.exitCode === null) {
    own.child.kill("SIGTERM");
    await once(own.child, "exit");
  }
  if (own) {
    await rm(own.scratch, { recursive: true, force: true });
  }
});

Given("a demo plan {string} with a scratch repository", async function (plan) {
  await serveDemoPlan(this, plan, "small");
});

Given("a demo plan {string} with a big scratch repository", async function (plan) {
  await serveDemoPlan(this, plan, "big");
});

// The scripted daemon (support/scripted-daemon.mjs): a Given says what its
// history holds, and its Whens say what its events stream tells the page and
// when.
function scripted(world) {
  assert.ok(world.scriptedDaemon, "this scenario's daemon is not scripted: a Given must say so");
  return world.scriptedDaemon;
}

Given("the daemon reports a queued backup of {string}", function (plan) {
  this.scriptedDaemon = new ScriptedDaemon();
  this.scriptedDaemon.addBackup(plan, "queued", { queuedAt: "2026-11-09T10:00:00Z" });
});

Given("the daemon reports two finished backups of {string}", function (plan) {
  this.scriptedDaemon = new ScriptedDaemon();
  for (const second of ["01", "02"]) {
    this.scriptedDaemon.addBackup(plan, "succeeded", {
      queuedAt: `2026-11-09T10:00:${second}Z`,
      startedAt: `2026-11-09T10:00:${second}Z`,
      finishedAt: `2026-11-09T10:01:${second}Z`,
    });
  }
});

When("the daemon reports that the backup of {string} has started", async function (plan) {
  await scripted(this).starts(plan, "2026-11-09T10:00:05Z");
});

When("the daemon reports {int} steps of progress of the backup of {string}", async function (steps, plan) {
  await scripted(this).progresses(plan, steps);
});

When("the daemon has stopped", async function () {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  own.child.kill("SIGTERM");
  await once(own.child, "exit");
});

// The page's own loading state (§4.4): every answer the daemon owes the UI
// arrives late, so the page opens on "Loading…" alone.
When("the daemon's answers to the UI are delayed", function () {
  this.answerDelayMs = 10_000;
});

// A shorter wait, for a journey that has to read a wait and then act within
// it: every answer arrives this many seconds late.
When("the daemon's answers to the UI are delayed by {int} seconds", function (seconds) {
  this.answerDelayMs = seconds * 1_000;
});

When("I reload the page", async function () {
  await this.page.reload();
  await this.page.getByRole("heading", { level: 1 }).waitFor();
});

// A row is named by its plan; what it shows is its life so far, which a
// backup changes as it runs, so the step waits for the row to say it.
Then("the table {string} has a row {string} that shows {string}", async function (name, row, status) {
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

Then("the table {string} has {int} rows", async function (name, count) {
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

// The cells of the named column each carry the instant they show, so the
// order is read from those, not from localized text.
async function instants(world, name, column) {
  const header = table(world, name).getByRole("columnheader", { name: new RegExp(escape(column)) });
  const index = await header.evaluate((cell) => cell.cellIndex);
  return table(world, name)
    .locator("tbody tr")
    .evaluateAll((rows, at) => rows.map((row) => row.cells[at].title), index);
}

Then("the table {string} shows its {string} column newest first", async function (name, column) {
  const values = await instants(this, name, column);
  assert.ok(values.length > 0, `the table ${name} has no rows to be newest first`);
  const sorted = [...values].sort((a, b) => b.localeCompare(a));
  assert.deepEqual(values, sorted, `the ${column} column of ${name} is not newest first`);
});

Given("the browser is in the time zone {string}", function (zone) {
  this.contextOptions.timezoneId = zone;
});

// Each cell carries the instant it shows in its `title`, in UTC. What it shows
// is that instant read in the browser's own zone, as the browser's `Intl`
// writes it: the default of the formatter is UTC, so a page that didn't pass
// the zone would show the title's own hours.
Then("the table {string} shows its {string} column in the time zone {string}", async function (name, column, zone) {
  const header = table(this, name).getByRole("columnheader", { name: new RegExp(escape(column)) });
  const index = await header.evaluate((cell) => cell.cellIndex);
  const cells = await table(this, name)
    .locator("tbody tr")
    .evaluateAll((rows, at) => rows.map((row) => [row.cells[at].title, row.cells[at].textContent]), index);
  assert.ok(cells.length > 0, `the table ${name} has no rows to read in ${zone}`);
  const fold = (text) => text.replace(/[\u202f\u00a0]/g, " ");
  const written = new Intl.DateTimeFormat("en-US", { dateStyle: "short", timeStyle: "medium", timeZone: zone });
  for (const [raw, shown] of cells) {
    assert.equal(fold(shown), fold(written.format(new Date(raw))), `the ${column} cell of ${raw} is not in ${zone}`);
  }
});

// The table answers a press of its sort button by reordering the rows, so the
// step waits for the order as it waits for any update.
Then("the table {string} shows its {string} column oldest first", async function (name, column) {
  const oldestFirst = (values) => [...values].sort((a, b) => a.localeCompare(b));
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
  async function (name, column, direction) {
    const header = table(this, name).getByRole("columnheader", { name: new RegExp(escape(column)) });
    await expect(header).toHaveAttribute("aria-sort", direction);
  },
);
Then("the page shows the paragraph {string}", async function (text) {
  await this.page.getByText(text, { exact: true }).first().waitFor({ state: "visible" });
});

Then("the page shows the heading {string}", async function (name) {
  await this.page
    .getByRole("heading", { level: 1, name, exact: true })
    .waitFor({ state: "visible", timeout: 25_000 });
});
