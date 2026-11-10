// The journeys of the Snapshots page's maintenance (features/ui/maintenance.feature):
// history grouped by owner, and a forget, an adoption, a prune and a check
// that are reviewed in a dialog before they run. Their Givens prepare the
// scenario's own daemon through its HTTP API, which is not a pointer action;
// everything the journeys do to the page is a key press, and every element is
// found by role and accessible name.

import assert from "node:assert/strict";
import { readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";

import { Given, Then, When } from "@cucumber/cucumber";

import { ISOLATION, journey, namePattern } from "../support/keyboard.mjs";
import { serveDemoPlan } from "./live.mjs";

// The alias of the scratch repository every demo plan backs up to.
const ALIAS = "demo";

// A backup, a forget or a prune of the tiny source ends in a moment, even in
// a debug build; this is the room a slow machine gets.
const RUN_PATIENCE_MS = 120_000;

// What the page owes a keyboard once an answer is in: a dialog, a heading, a
// row. Short, so a page that never shows them fails a journey quickly.
const PAGE_PATIENCE_MS = 10_000;
const ROWS_PATIENCE_MS = 30_000;
// A line of the page that a run's wait brings: a run behind answers that come
// seconds late is on the page seconds after it was started. Under the
// timeout of a step, so that a line that never comes fails by saying so.
const LINE_PATIENCE_MS = 20_000;

// More Tab presses than the Snapshots page has stops, with its groups.
const MAX_TABS = 40;

const ISOLATION_MARKS = /[\u2066-\u2069]/g;

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

const escape = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** One request to the scenario's daemon, as its own page would send it. */
async function api(world, method, route, body) {
  assert.ok(world.baseUrl, "this scenario's daemon was never started");
  const headers = { Origin: new URL(world.baseUrl).origin };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(new URL(route, world.baseUrl), {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  return { status: response.status, json: text === "" ? null : JSON.parse(text) };
}

/** Waits for the operation to end and asserts that it succeeded. */
async function succeeds(world, started, what) {
  assert.equal(started.status, 202, `${what} was refused: ${JSON.stringify(started.json)}`);
  const id = started.json.operation;
  const end = Date.now() + RUN_PATIENCE_MS;
  let status = "queued";
  while (Date.now() < end) {
    const { json } = await api(world, "GET", `/api/v1/operations/${id}`);
    status = json?.operation?.status ?? status;
    if (!["queued", "running"].includes(status)) break;
    await sleep(250);
  }
  assert.ok(status.startsWith("succeeded"), `${what} ended as ${status}`);
}

async function backUp(world, plan) {
  await succeeds(world, await api(world, "POST", "/api/v1/operations", { plan }), `the backup of ${plan}`);
}

/** The scenario's own config.toml. */
function configPath(world) {
  assert.ok(world.ownServer, "this scenario's daemon was never started");
  return path.join(world.ownServer.scratch, "config", "config.toml");
}

/** The source folder serveDemoPlan filled. */
function sourcePath(world) {
  return path.join(world.ownServer.scratch, "source");
}

/** The id of the one plan the config names. */
async function planOf(world) {
  const found = (await readFile(configPath(world), "utf8")).match(/^id = ("(?:[^"\\]|\\.)*")$/m);
  assert.ok(found, "the config names no plan");
  return JSON.parse(found[1]);
}

/** Gives the plan, which must be the config's last, a retention policy of its own.
 *  The daemon reads its configuration afresh for every request. */
async function keepLast(world, plan, count) {
  const file = configPath(world);
  const config = await readFile(file, "utf8");
  assert.ok(config.includes(`id = ${JSON.stringify(plan)}`), `the config has no plan ${plan}`);
  const without = config.replace(/\n?\[plans\.retention\]\n(?:[a-z-]+ = .*\n)*/, "\n");
  await writeFile(file, `${without.trimEnd()}\n\n[plans.retention]\nkeep-last = ${count}\n`);
}

/** The dialog of this name, by role and accessible name. */
function dialogOf(world, name) {
  return world.page.getByRole("dialog", { name: namePattern(name) });
}

/** What a reader gets from a dialog: its text without bidi isolation marks. */
async function textOfDialog(world, name) {
  const dialog = dialogOf(world, name);
  await dialog.waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  return (await dialog.innerText()).replace(ISOLATION_MARKS, "");
}

Given("a demo plan {string} with a tiny scratch repository", async function (plan) {
  await serveDemoPlan(this, plan, "tiny");
});

Given("the plan {string} has been backed up {int} times", async function (plan, times) {
  for (let done = 0; done < times; done += 1) {
    await backUp(this, plan);
  }
});

Given("the plan {string} keeps the last {int} snapshots", async function (plan, count) {
  await keepLast(this, plan, count);
});

// The history the plan made is now tagged with a plan this configuration
// does not have: orphaned.
Given("the plan {string} is now called {string}", async function (plan, renamed) {
  const file = configPath(this);
  const config = await readFile(file, "utf8");
  const before = `id = ${JSON.stringify(plan)}`;
  assert.ok(config.includes(before), `the config has no plan ${plan}`);
  await writeFile(file, config.replace(before, `id = ${JSON.stringify(renamed)}`));
});

// Two snapshots, the second without a file the first has, then a forget of
// the first: the repository keeps that file's data, and no snapshot uses it.
Given("the repository holds data no snapshot uses", async function () {
  const plan = await planOf(this);
  await backUp(this, plan);
  await rm(path.join(sourcePath(this), "demo-1.txt"));
  await backUp(this, plan);
  await keepLast(this, plan, 1);
  await succeeds(
    this,
    await api(this, "POST", `/api/v1/repositories/${ALIAS}/maintenance`, { kind: "forget" }),
    "the forget",
  );
});

// The daemon asks the engine for the repository's folder, which is gone, and
// says so with the engine's own words.
Given("the repository has been removed", async function () {
  assert.ok(this.ownServer, "this scenario's daemon was never started");
  await rm(path.join(this.ownServer.scratch, "repo"), { recursive: true, force: true });
});

// The demo plan is the config's last table: cut it off, and the repository is
// one no plan backs up to.
Given("the config names no plan", async function () {
  const file = configPath(this);
  const config = await readFile(file, "utf8");
  const at = config.indexOf("[[plans]]");
  assert.ok(at >= 0, "the config names no plan to cut off");
  await writeFile(file, config.slice(0, at));
});

/** Presses Tab until the button the locator finds, once, has focus. The
 *  page reads the history it lists from the daemon, so a button made from it
 *  is not there when the presses would begin: they begin when it is. */
async function tabUntilFocused(world, target, description) {
  const keys = journey(world.page);
  await target.waitFor({ state: "attached", timeout: PAGE_PATIENCE_MS });
  const focused = async () =>
    (await target.count()) === 1 && (await target.evaluate((el) => el === document.activeElement));
  for (let presses = 0; presses < MAX_TABS && !(await focused()); presses += 1) {
    await keys.press("Tab");
  }
  assert.ok(await focused(), `focus did not reach ${description} within ${MAX_TABS} Tab presses`);
}

/** Reads the repository's history afresh, as a person does: back to the
 *  repository's button, and Enter on it. */
async function readAgain(world) {
  const keys = journey(world.page);
  const picker = keys.byRole("button", { name: namePattern(`Snapshots of ${ALIAS}`) });
  const focused = async () =>
    (await picker.count()) === 1 && (await picker.evaluate((el) => el === document.activeElement));
  for (let presses = 0; presses < MAX_TABS && !(await focused()); presses += 1) {
    await keys.press("Shift+Tab");
  }
  assert.ok(await focused(), `focus did not reach the repository's button within ${MAX_TABS} Shift+Tab presses`);
  await keys.press("Enter");
}

// An adoption button names the agent too, which a journey cannot know.
When("I press Tab until focus is on the adoption button into the plan {string}", async function (plan) {
  const target = journey(this.page).byRole("button", {
    name: new RegExp(`^Adopt history of plan .+ from agent .+ into plan ${ISOLATION}${escape(plan)}${ISOLATION}$`),
  });
  await tabUntilFocused(this, target, `the button that adopts history into the plan "${plan}"`);
});

// The review opens once the preview has come back, and focus moves into it a
// moment later.
Then("the dialog {string} opens with focus inside it", async function (name) {
  const dialog = dialogOf(this, name);
  await dialog.waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  const end = Date.now() + PAGE_PATIENCE_MS;
  let inside = false;
  while (!inside && Date.now() < end) {
    inside = await dialog.evaluate((el) => el.contains(document.activeElement));
    if (!inside) await sleep(100);
  }
  assert.ok(inside, `focus did not come inside the dialog "${name}"`);
});

Then("the dialog {string} says {string}", async function (name, text) {
  const read = await textOfDialog(this, name);
  assert.ok(
    read.toLowerCase().includes(text.toLowerCase()),
    `the dialog "${name}" does not say "${text}" (it reads ${JSON.stringify(read)})`,
  );
});

Then("the dialog {string} offers the button {string}", async function (name, button) {
  await dialogOf(this, name)
    .getByRole("button", { name: namePattern(button) })
    .waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});

Then("the dialog {string} offers only the button {string}", async function (name, button) {
  const dialog = dialogOf(this, name);
  await dialog.waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  const names = (await dialog.getByRole("button").allTextContents()).map((text) =>
    text.replace(ISOLATION_MARKS, "").trim(),
  );
  assert.deepEqual(names, [button], `the buttons of the dialog "${name}"`);
});

Then("the dialog {string} is closed", async function (name) {
  await dialogOf(this, name).waitFor({ state: "hidden", timeout: PAGE_PATIENCE_MS });
});

// A control that cannot be used for now is still on the page, and says so
// with `aria-disabled`, which keeps it where a keyboard and a screen reader
// find it.
Then("the dialog {string} has its button {string} unavailable", async function (name, button) {
  const found = dialogOf(this, name).getByRole("button", { name: namePattern(button) });
  await found.waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  assert.equal(
    await found.getAttribute("aria-disabled"),
    "true",
    `the button "${button}" of the dialog "${name}" can be used`,
  );
});

// Silence is shown by waiting: a dialog that was going to close has done so
// by the end of a beat.
Then("the dialog {string} stays open", async function (name) {
  await sleep(1_000);
  assert.ok(await dialogOf(this, name).isVisible(), `the dialog "${name}" was dismissed`);
});

Then("the page shows the button {string}", async function (name) {
  await this.page
    .getByRole("button", { name: namePattern(name) })
    .first()
    .waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});

// Any level: the groups' headings are not the page's main heading.
Then("the page shows the section heading {string}", async function (name) {
  await this.page
    .getByRole("heading", { name: namePattern(name) })
    .first()
    .waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});

Then("the page shows no section heading {string}", async function (name) {
  await this.page
    .getByRole("heading", { name: namePattern(name) })
    .first()
    .waitFor({ state: "hidden", timeout: PAGE_PATIENCE_MS });
});

// The plan's table is named after the plan; what the page fills in carries
// bidi isolation marks, which the name pattern allows for.
async function waitForRows(world, plan, count) {
  const rows = world.page
    .getByRole("table", { name: namePattern(`Snapshots of plan ${plan}`) })
    .locator("tbody tr");
  const end = Date.now() + ROWS_PATIENCE_MS;
  let found = -1;
  while (Date.now() < end) {
    found = await rows.count();
    if (found === count) return;
    await sleep(250);
  }
  assert.equal(found, count, `the snapshot rows of plan ${plan}`);
}

// What the page drew is not what the repository holds: once the table shows
// the count, the history is read again, and the count must hold.
Then("the plan {string} shows {int} snapshot rows", async function (plan, count) {
  await waitForRows(this, plan, count);
  await readAgain(this);
  await waitForRows(this, plan, count);
});

// The live regions repeat what the page says, so a line of the page is looked
// for in its main content.
Then("the main content shows the line {string}", async function (text) {
  await this.page
    .getByRole("main")
    .getByText(text, { exact: true })
    .first()
    .waitFor({ state: "visible", timeout: LINE_PATIENCE_MS });
});

Then("the main content shows no line {string}", async function (text) {
  await this.page
    .getByRole("main")
    .getByText(text, { exact: true })
    .first()
    .waitFor({ state: "hidden", timeout: PAGE_PATIENCE_MS });
});

// What the engine says is English whatever the language of the page, and the
// page marks it so (§4.5).
Then("the alert marks the engine's words as English", async function () {
  const words = this.page.getByRole("alert").locator('[lang="en"]');
  await words.first().waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  assert.notEqual((await words.first().innerText()).trim(), "", "the engine's words in the alert");
});

// A refusal of the preview is told where a reader is looking, as an alert.
Then("the page shows an alert containing {string}", async function (needle) {
  await this.page
    .getByRole("alert")
    .filter({ hasText: new RegExp(escape(needle)) })
    .first()
    .waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});
