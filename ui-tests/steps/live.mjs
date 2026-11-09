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

import { fixtureRepo, startServer } from "../support/world.mjs";

// The scratch repositories' password: the one the `demo-repo` fixture was
// initialized with. Not a real secret.
const PASSWORD = "demo-password";

// A real backup takes real time, and the steps that wait for one are patient:
// enough for a big scratch source to finish, in a debug build.
const ROW_PATIENCE_MS = 120_000;

// The two sources a demo plan backs up: `small` ends quickly, so a journey
// sees a whole operation through; `big` outlives the moment a journey
// cancels it or reloads the page mid-run.
const SOURCE_FILES = { small: 3, big: 2500 };
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

/** The table a page shows, by its caption. */
function table(world, name) {
  return world.page.getByRole("table", { name });
}

/** Writes a demo plan's config and repository into a scratch folder, starts
 *  a daemon on it, and points the scenario's pages at that daemon. */
async function serveDemoPlan(world, plan, size) {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-plan-"));
  await cp(fixtureRepo, path.join(scratch, "repo"), { recursive: true });
  const source = path.join(scratch, "source");
  await mkdir(source);
  const contents = "d".repeat(FILE_BYTES);
  for (let n = 1; n <= SOURCE_FILES[size]; n += 1) {
    await writeFile(path.join(source, `demo-${n}.txt`), contents);
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
When("the daemon has stopped", async function () {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  own.child.kill("SIGTERM");
  await once(own.child, "exit");
});

// The page's own loading state (§4.4): every answer the daemon owes the UI
// arrives late, so the page opens on "Loading…" alone.
When("the daemon's answers to the UI are delayed", function () {
  this.slowNetwork = true;
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
Then("the table {string} shows its {string} column newest first", async function (name, column) {
  const header = table(this, name).getByRole("columnheader", { name: new RegExp(escape(column)) });
  const index = await header.evaluate((cell) => cell.cellIndex);
  const values = await table(this, name)
    .locator("tbody tr")
    .evaluateAll((rows, at) => rows.map((row) => row.cells[at].title), index);
  assert.ok(values.length > 0, `the table ${name} has no rows to be newest first`);
  const sorted = [...values].sort((a, b) => b.localeCompare(a));
  assert.deepEqual(values, sorted, `the ${column} column of ${name} is not newest first`);
});

Then("the page shows the paragraph {string}", async function (text) {
  await this.page.getByText(text, { exact: true }).first().waitFor({ state: "visible" });
});

Then("the page shows the heading {string}", async function (name) {
  await this.page
    .getByRole("heading", { level: 1, name, exact: true })
    .waitFor({ state: "visible", timeout: 25_000 });
});
