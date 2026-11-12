// The journeys of the Snapshots page's maintenance (features/ui/maintenance.feature):
// history grouped by owner, and a forget, an adoption, a prune and a check
// that are reviewed in a dialog before they run. Their Givens prepare the
// scenario's own daemon through its HTTP API, which is not a pointer action;
// everything the journeys do to the page is a key press, and every element is
// found by role and accessible name.

import assert from "node:assert/strict";
import { once } from "node:events";
import { copyFile, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";

import { Given, Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { ISOLATION, journey, namePattern } from "../support/keyboard.ts";
import { type BrowserWorld, startServer } from "../support/world.ts";
import { serveDemoPlans, untilCellLists } from "./live.ts";

// The alias of the scratch repository every demo plan backs up to.
const ALIAS = "demo";

// The ID the stray pack is given: one no index lists.
const STRAY_PACK = "5ca1ab1e".repeat(8);

// How rustic names that pack in its words: by the first 8 hex digits of its
// ID, as its own `check` prints it.
const STRAY_PACK_NAMED = STRAY_PACK.slice(0, 8);

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

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** What these steps read of the daemon's answers: a started operation's id,
 *  or an operation with its status and, when it failed, its code. */
export interface Answer {
  status: number;
  json: { operation?: string | { status?: string; "failure-code"?: string } } | null;
}

/** How an operation ended: its status and, when it failed, its code. */
export interface Ended {
  status: string;
  failureCode: string | undefined;
}

/** One request to the scenario's daemon, as its own page would send it. */
export async function api(world: Pick<BrowserWorld, "baseUrl">, method: string, route: string, body?: unknown): Promise<Answer> {
  assert.ok(world.baseUrl, "this scenario's daemon was never started");
  const headers: Record<string, string> = { Origin: new URL(world.baseUrl).origin };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(new URL(route, world.baseUrl), {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  // The daemon's own JSON, trusted to have the shape `Answer` reads.
  return { status: response.status, json: text === "" ? null : (JSON.parse(text) as Answer["json"]) };
}

/** Waits for the started operation to end and says how it ended. */
export async function ended(world: Pick<BrowserWorld, "baseUrl">, started: Answer, what: string): Promise<Ended> {
  assert.equal(started.status, 202, `${what} was refused: ${JSON.stringify(started.json)}`);
  const id = started.json?.operation;
  assert.ok(typeof id === "string", `${what} was started with no operation id: ${JSON.stringify(started.json)}`);
  const end = Date.now() + RUN_PATIENCE_MS;
  let status = "queued";
  let failureCode: string | undefined;
  while (Date.now() < end) {
    const operation = (await api(world, "GET", `/api/v1/operations/${id}`)).json?.operation;
    status = (typeof operation === "object" ? operation.status : undefined) ?? status;
    failureCode = typeof operation === "object" ? operation["failure-code"] : undefined;
    if (!["queued", "running"].includes(status)) break;
    await sleep(250);
  }
  return { status, failureCode };
}

/** Waits for the operation to end and asserts that it succeeded. */
async function succeeds(world: Pick<BrowserWorld, "baseUrl">, started: Answer, what: string): Promise<void> {
  const { status } = await ended(world, started, what);
  assert.ok(status.startsWith("succeeded"), `${what} ended as ${status}`);
}

async function backUp(world: Pick<BrowserWorld, "baseUrl">, plan: string): Promise<void> {
  await succeeds(world, await api(world, "POST", "/api/v1/operations", { plan }), `the backup of ${plan}`);
}

/** The scenario's own config.toml. */
function configPath(world: BrowserWorld): string {
  assert.ok(world.ownServer, "this scenario's daemon was never started");
  return path.join(world.ownServer.scratch, "config", "config.toml");
}

/** The source folder serveDemoPlans filled. */
function sourcePath(world: BrowserWorld): string {
  assert.ok(world.ownServer, "this scenario's daemon was never started");
  return path.join(world.ownServer.scratch, "source");
}

/** The id of the one plan the config names. */
async function planOf(world: BrowserWorld): Promise<string> {
  const found = (await readFile(configPath(world), "utf8")).match(/^id = ("(?:[^"\\]|\\.)*")$/m)?.[1];
  assert.ok(found, "the config names no plan");
  // A TOML basic string the config was written with, which is JSON too.
  return JSON.parse(found) as string;
}

/** Gives the plan, which must be the config's last, a retention policy of its own.
 *  The daemon reads its configuration afresh for every request. */
async function keepLast(world: BrowserWorld, plan: string, count: number): Promise<void> {
  const file = configPath(world);
  const config = await readFile(file, "utf8");
  assert.ok(config.includes(`id = ${JSON.stringify(plan)}`), `the config has no plan ${plan}`);
  const without = config.replace(/\n?\[plans\.retention\]\n(?:[a-z-]+ = .*\n)*/, "\n");
  await writeFile(file, `${without.trimEnd()}\n\n[plans.retention]\nkeep-last = ${count}\n`);
}

/** The dialog of this name, by role and accessible name. */
function dialogOf(world: BrowserWorld, name: string): Locator {
  return world.page.getByRole("dialog", { name: namePattern(name) });
}

/** What a reader gets from a dialog: its text without bidi isolation marks. */
async function textOfDialog(world: BrowserWorld, name: string): Promise<string> {
  const dialog = dialogOf(world, name);
  await dialog.waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  return (await dialog.innerText()).replace(ISOLATION_MARKS, "");
}

Given("a demo plan {string} with a tiny scratch repository", async function (this: BrowserWorld, plan: string) {
  await serveDemoPlans(this, [plan], "tiny");
});

Given("the plan {string} has been backed up {int} times", async function (this: BrowserWorld, plan: string, times: number) {
  for (let done = 0; done < times; done += 1) {
    await backUp(this, plan);
  }
});

Given("the plan {string} keeps the last {int} snapshots", async function (this: BrowserWorld, plan: string, count: number) {
  await keepLast(this, plan, count);
});

// Another machine's agent: a daemon of its own, whose own data folder gives it
// its own agent ID, and whose plan of the same name, recording the host `host`
// (§7.3), backs the same source up into the same repository. It maintains
// nothing, and stops once its backup is done.
Given(
  "another machine called {string} has backed up the plan {string} once",
  async function (this: BrowserWorld, host: string, plan: string) {
    assert.ok(this.ownServer, "this scenario's daemon was never started");
    const config = await readFile(configPath(this), "utf8");
    const id = `id = ${JSON.stringify(plan)}`;
    assert.ok(config.includes(id), `the config has no plan ${plan}`);
    const local = 'maintenance = "local"';
    assert.ok(config.includes(local), "the config's repository is not maintained here");
    const machine = path.join(this.ownServer.scratch, "other-machine");
    const configDir = path.join(machine, "config");
    await mkdir(configDir, { recursive: true });
    await writeFile(
      path.join(configDir, "config.toml"),
      config.replace(local, 'maintenance = "none"').replace(id, `${id}\nhost = ${JSON.stringify(host)}`),
    );
    const { child, url } = await startServer({
      LADDERBACK_CONFIG_DIR: configDir,
      LADDERBACK_DATA_DIR: path.join(machine, "data"),
      LADDERBACK_CACHE_DIR: path.join(machine, "cache"),
    });
    try {
      await backUp({ baseUrl: url }, plan);
    } finally {
      if (child.exitCode === null) {
        child.kill("SIGTERM");
        await once(child, "exit");
      }
    }
  },
);

// The history the plan made is now tagged with a plan this configuration
// does not have: orphaned.
Given("the plan {string} is now called {string}", async function (this: BrowserWorld, plan: string, renamed: string) {
  const file = configPath(this);
  const config = await readFile(file, "utf8");
  const before = `id = ${JSON.stringify(plan)}`;
  assert.ok(config.includes(before), `the config has no plan ${plan}`);
  await writeFile(file, config.replace(before, `id = ${JSON.stringify(renamed)}`));
});

// Two snapshots, the second without a file the first has, then a forget of
// the first: the repository keeps that file's data, and no snapshot uses it.
Given("the repository holds data no snapshot uses", async function (this: BrowserWorld) {
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

// A check the API starts, as the Snapshots page's review would: it runs to
// its end before the scenario goes on.
Given("the repository {string} has been checked", async function (this: BrowserWorld, alias: string) {
  assert.equal(alias, ALIAS, `only the repository ${ALIAS} is served`);
  await succeeds(
    this,
    await api(this, "POST", `/api/v1/repositories/${ALIAS}/maintenance`, { kind: "check" }),
    "the check",
  );
});

/** The ID of the newest snapshot of `plan` in the scenario's repository, as
 *  the daemon lists the repository: newest first, each tagged with its plan. */
async function latestSnapshotOf(world: BrowserWorld, plan: string): Promise<string> {
  assert.ok(world.baseUrl, "this scenario's daemon was never started");
  const response = await fetch(new URL(`/api/v1/repositories/${ALIAS}/snapshots`, world.baseUrl), {
    headers: { Origin: new URL(world.baseUrl).origin },
  });
  const listing: unknown = await response.json();
  assert.equal(response.status, 200, `the snapshots of ${ALIAS} could not be listed: ${JSON.stringify(listing)}`);
  const snapshots = listing !== null && typeof listing === "object" && "snapshots" in listing ? listing.snapshots : null;
  assert.ok(Array.isArray(snapshots), `the listing of ${ALIAS} holds no snapshots: ${JSON.stringify(listing)}`);
  for (const snapshot of snapshots) {
    if (snapshot === null || typeof snapshot !== "object" || !("id" in snapshot) || !("tags" in snapshot)) continue;
    const { id, tags } = snapshot;
    if (typeof id === "string" && Array.isArray(tags) && tags.some((tag) => typeof tag === "string" && tag.includes(plan))) {
      return id;
    }
  }
  assert.fail(`the repository ${ALIAS} holds no snapshot of the plan ${plan}`);
}

// The snapshot is read from the repository, through the daemon's listing of
// it, not from the operation the row shows. A row names a snapshot by the
// first 8 hex digits of its ID, as rustic does.
Then(
  "the row {string} of the table {string} lists the latest snapshot of the plan {string} in its {string} column",
  async function (this: BrowserWorld, row: string, name: string, plan: string, column: string) {
    const latest = await latestSnapshotOf(this, plan);
    await untilCellLists(this, name, row, column, latest.slice(0, 8));
  },
);

// The daemon asks the engine for the repository's folder, which is gone, and
// says so with the engine's own words.
Given("the repository has been removed", async function (this: BrowserWorld) {
  assert.ok(this.ownServer, "this scenario's daemon was never started");
  await rm(path.join(this.ownServer.scratch, "repo"), { recursive: true, force: true });
});

// A backup killed before it saved its index has written packs that no index
// lists. A copy of a pack of the repository, under an ID of its own, is one.
Given(
  "a backup killed before it saved its index left a pack in the repository that no index lists",
  async function (this: BrowserWorld) {
    assert.ok(this.ownServer, "this scenario's daemon was never started");
    const data = path.join(this.ownServer.scratch, "repo", "data");
    const pack = (await readdir(data, { recursive: true, withFileTypes: true })).find((entry) => entry.isFile());
    assert.ok(pack, "the repository holds no pack to copy");
    const folder = path.join(data, STRAY_PACK.slice(0, 2));
    await mkdir(folder, { recursive: true });
    await copyFile(path.join(pack.parentPath, pack.name), path.join(folder, STRAY_PACK));
  },
);

// The demo plan is the config's last table: cut it off, and the repository is
// one no plan backs up to.
Given("the config names no plan", async function (this: BrowserWorld) {
  const file = configPath(this);
  const config = await readFile(file, "utf8");
  const at = config.indexOf("[[plans]]");
  assert.ok(at >= 0, "the config names no plan to cut off");
  await writeFile(file, config.slice(0, at));
});

// A repository with no `maintenance` key asks this agent for no maintenance
// (§7.3). The daemon reads its configuration afresh for every request.
Given("the repository {string} has no maintenance setting", async function (this: BrowserWorld, alias: string) {
  assert.equal(alias, ALIAS, `only the repository ${ALIAS} is served`);
  const file = configPath(this);
  const config = await readFile(file, "utf8");
  const setting = /^maintenance = .*\n/m;
  assert.match(config, setting, "the config sets no maintenance to take away");
  await writeFile(file, config.replace(setting, ""));
});

/** Presses Tab until the button the locator finds, once, has focus. The
 *  page reads the history it lists from the daemon, so a button made from it
 *  is not there when the presses would begin: they begin when it is. */
async function tabUntilFocused(world: BrowserWorld, target: Locator, description: string): Promise<void> {
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
async function readAgain(world: BrowserWorld): Promise<void> {
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
function adoptionButton(world: BrowserWorld, plan: string): Locator {
  return journey(world.page).byRole("button", {
    name: new RegExp(`^Adopt history of plan .+ from agent .+ into plan ${ISOLATION}${escape(plan)}${ISOLATION}$`),
  });
}

When("I press Tab until focus is on the adoption button into the plan {string}", async function (this: BrowserWorld, plan: string) {
  await tabUntilFocused(this, adoptionButton(this, plan), `the button that adopts history into the plan "${plan}"`);
});

// The button of a repository by the repository it picks. Its name is a
// sentence of the catalog around the alias, so in a pseudolocale the alias is
// what is left to find it by.
When("I press Tab until focus is on the button of the repository {string}", async function (this: BrowserWorld, alias: string) {
  const target = journey(this.page).byRole("button", { name: new RegExp(escape(alias)) });
  await tabUntilFocused(this, target, `the button of the repository "${alias}"`);
});

// Every snapshot row of the page together, in whatever language it is in: the
// headings that name the sections are the catalog's words then.
Then("the page lists {int} snapshots", async function (this: BrowserWorld, count: number) {
  const rows = this.page.getByRole("main").locator("tbody tr");
  const end = Date.now() + LINE_PATIENCE_MS;
  let found = -1;
  while (Date.now() < end) {
    found = await rows.count();
    if (found === count) return;
    await sleep(100);
  }
  assert.equal(found, count, "the snapshot rows of the page");
});

// A header or an ID that breaks in the middle of a word, with room beside it,
// is a layout that squeezed its column. A table's words stay whole: each is
// laid out on one line.
Then("no word of a table is broken across lines", async function (this: BrowserWorld) {
  const broken = await this.page.evaluate(async () => {
    await document.fonts.ready;
    // Let layout settle after injected styles.
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    const found: string[] = [];
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const cell = node.parentElement?.closest("th, td");
      const text = node.textContent ?? "";
      if (!cell || !cell.checkVisibility() || !text.trim()) continue;
      for (const word of text.matchAll(/\S+/g)) {
        const range = document.createRange();
        range.setStart(node, word.index);
        range.setEnd(node, word.index + word[0].length);
        const lines = new Set(
          [...range.getClientRects()].filter((rect) => rect.width > 0).map((rect) => Math.round(rect.top)),
        );
        if (lines.size > 1) found.push(`"${word[0]}" in <${cell.localName}> is broken across ${lines.size} lines`);
      }
    }
    return found;
  });
  assert.deepEqual(broken, [], "words of tables broken across lines");
});

// The review opens once the preview has come back, and focus moves into it a
// moment later.
Then("the dialog {string} opens with focus inside it", async function (this: BrowserWorld, name: string) {
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

Then("the dialog {string} says {string}", async function (this: BrowserWorld, name: string, text: string) {
  const read = await textOfDialog(this, name);
  assert.ok(
    read.toLowerCase().includes(text.toLowerCase()),
    `the dialog "${name}" does not say "${text}" (it reads ${JSON.stringify(read)})`,
  );
});

// What a screen reader reads as the dialog opens, after its name: the text of
// the elements its aria-describedby names, without bidi isolation marks.
Then("the dialog {string} is described as saying {string}", async function (this: BrowserWorld, name: string, text: string) {
  const dialog = dialogOf(this, name);
  await dialog.waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  const described = await dialog.evaluate((el) =>
    (el.getAttribute("aria-describedby") ?? "")
      .split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent ?? "")
      .join(" "),
  );
  const read = described.replace(ISOLATION_MARKS, "").replace(/\s+/g, " ");
  assert.ok(
    read.includes(text),
    `the dialog "${name}" is not described as saying "${text}" (its description reads ${JSON.stringify(read)})`,
  );
});

Then("the dialog {string} offers the button {string}", async function (this: BrowserWorld, name: string, button: string) {
  await dialogOf(this, name)
    .getByRole("button", { name: namePattern(button) })
    .waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});

Then("the dialog {string} offers only the button {string}", async function (this: BrowserWorld, name: string, button: string) {
  const dialog = dialogOf(this, name);
  await dialog.waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  const names = (await dialog.getByRole("button").allTextContents()).map((text) =>
    text.replace(ISOLATION_MARKS, "").trim(),
  );
  assert.deepEqual(names, [button], `the buttons of the dialog "${name}"`);
});

Then("the dialog {string} is closed", async function (this: BrowserWorld, name: string) {
  await dialogOf(this, name).waitFor({ state: "hidden", timeout: PAGE_PATIENCE_MS });
});

// A control that cannot be used for now is still on the page, and says so
// with `aria-disabled`, which keeps it where a keyboard and a screen reader
// find it.
Then("the dialog {string} has its button {string} unavailable", async function (this: BrowserWorld, name: string, button: string) {
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
Then("the dialog {string} stays open", async function (this: BrowserWorld, name: string) {
  await sleep(1_000);
  assert.ok(await dialogOf(this, name).isVisible(), `the dialog "${name}" was dismissed`);
});

// A review is read as a column of sentences and a short table: a dialog that
// takes the whole of a wide screen puts the table's two columns a screen apart.
Then("the dialog {string} is no wider than {int} pixels", async function (this: BrowserWorld, name: string, limit: number) {
  const box = await dialogOf(this, name).boundingBox();
  assert.ok(box, `the dialog "${name}" is not on the page`);
  assert.ok(
    box.width <= limit,
    `the dialog "${name}" is ${Math.round(box.width)} pixels wide, wider than ${limit}`,
  );
});

Then("the page shows the button {string}", async function (this: BrowserWorld, name: string) {
  await this.page
    .getByRole("button", { name: namePattern(name) })
    .first()
    .waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});

// Any level: the groups' headings are not the page's main heading.
Then("the page shows the section heading {string}", async function (this: BrowserWorld, name: string) {
  await this.page
    .getByRole("heading", { name: namePattern(name) })
    .first()
    .waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});

Then("the page shows no section heading {string}", async function (this: BrowserWorld, name: string) {
  await this.page
    .getByRole("heading", { name: namePattern(name) })
    .first()
    .waitFor({ state: "hidden", timeout: PAGE_PATIENCE_MS });
});

// The plan's table is named after the plan; what the page fills in carries
// bidi isolation marks, which the name pattern allows for.
async function waitForRows(world: BrowserWorld, plan: string, count: number): Promise<void> {
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
Then("the plan {string} shows {int} snapshot rows", async function (this: BrowserWorld, plan: string, count: number) {
  await waitForRows(this, plan, count);
  await readAgain(this);
  await waitForRows(this, plan, count);
});

// The live regions repeat what the page says, so a line of the page is looked
// for in its main content.
Then("the main content shows the line {string}", async function (this: BrowserWorld, text: string) {
  await this.page
    .getByRole("main")
    .getByText(text, { exact: true })
    .first()
    .waitFor({ state: "visible", timeout: LINE_PATIENCE_MS });
});

Then("the main content shows no line {string}", async function (this: BrowserWorld, text: string) {
  await this.page
    .getByRole("main")
    .getByText(text, { exact: true })
    .first()
    .waitFor({ state: "hidden", timeout: PAGE_PATIENCE_MS });
});

// A warning is a line of the main content, its data set apart by bidi
// isolation marks.
Then("the main content warns {string}", async function (this: BrowserWorld, text: string) {
  await this.page
    .getByRole("main")
    .getByText(namePattern(text))
    .first()
    .waitFor({ state: "visible", timeout: LINE_PATIENCE_MS });
});

// Asked once the page has drawn what it waits for, such as a plan's rows: a
// button that was never drawn is not one still on its way.
Then("the page offers no button {string}", async function (this: BrowserWorld, name: string) {
  const buttons = this.page.getByRole("button", { name: namePattern(name) });
  assert.equal(await buttons.count(), 0, `the page offers the button "${name}"`);
});

// Asked, like the step above, once the page has drawn the history.
Then("the page offers no adoption button into the plan {string}", async function (this: BrowserWorld, plan: string) {
  const buttons = adoptionButton(this, plan);
  assert.equal(await buttons.count(), 0, `the page offers a button that adopts history into the plan "${plan}"`);
});

// Waits, as a page draws its buttons once its reads come back.
Then("the page offers the button {string}", async function (this: BrowserWorld, name: string) {
  await this.page
    .getByRole("button", { name: namePattern(name) })
    .first()
    .waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});

// A line of the page that says more than the words a scenario names.
Then("the main content shows a line containing {string}", async function (this: BrowserWorld, text: string) {
  await this.page
    .getByRole("main")
    .getByText(text)
    .first()
    .waitFor({ state: "visible", timeout: LINE_PATIENCE_MS });
});

// What the check found is in rustic's words, which name the pack; they are
// English whatever the language of the page, and the page marks them so (§4.5).
Then(
  "the main content names the pack no index lists in the engine's words, marked as English",
  async function (this: BrowserWorld) {
    await this.page
      .getByRole("main")
      .locator('[lang="en"]')
      .filter({ hasText: STRAY_PACK_NAMED })
      .first()
      .waitFor({ state: "visible", timeout: LINE_PATIENCE_MS });
  },
);

// What the engine says is English whatever the language of the page, and the
// page marks it so (§4.5).
Then("the alert marks the engine's words as English", async function (this: BrowserWorld) {
  const words = this.page.getByRole("alert").locator('[lang="en"]');
  await words.first().waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  assert.notEqual((await words.first().innerText()).trim(), "", "the engine's words in the alert");
});

// A refusal of the preview is told where a reader is looking, as an alert.
Then("the page shows an alert containing {string}", async function (this: BrowserWorld, needle: string) {
  await this.page
    .getByRole("alert")
    .filter({ hasText: new RegExp(escape(needle)) })
    .first()
    .waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});
