// Steps of the @ui scenarios of features/repositories/repository_locations.feature
// (decision 80): a daemon of the scenario's own whose repository has two
// locations, two scratch copies of the demo repository, so both hold the same
// repository ID. Elements are found by role and accessible name, as in every
// journey; texts are read without the bidi isolation marks a placeable
// carries (§4.5).

import assert from "node:assert/strict";
import { cp, mkdir, mkdtemp, readFile, rename, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { Given, Then, When } from "@cucumber/cucumber";

import { journey } from "../support/keyboard.ts";
import { type BrowserWorld, fixtureRepo, startServer } from "../support/world.ts";
import { api, ended } from "./maintenance.ts";

const PASSWORD = "demo-password";
// A Test probes each location, and a backup opens the repository: each takes
// the engine a moment, in a debug build more than one.
const PATIENCE_MS = 25_000;
const MAX_TABS = 80;
const ISOLATION_MARKS = /[\u2066-\u2069]/g;
const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** What the scenario knows of its repository's locations. */
interface Locations {
  scratch: string;
  alias: string;
  /** Each location's folder, by the name the scenario calls it, in order. */
  places: Array<[string, string]>;
}

const known = new WeakMap<BrowserWorld, Locations>();

function locations(world: BrowserWorld): Locations {
  const found = known.get(world);
  assert.ok(found, "the scenario has no repository with locations: a Given must make one");
  return found;
}

function placeOf(world: BrowserWorld, name: string): string {
  const found = locations(world).places.find(([known]) => known === name);
  assert.ok(found, `no location "${name}"`);
  return found[1];
}

const shown = async (text: Promise<string>) => (await text).replace(ISOLATION_MARKS, "");

async function until(check: () => Promise<boolean>, what: string): Promise<void> {
  const end = Date.now() + PATIENCE_MS;
  while (Date.now() < end) {
    if (await check()) return;
    await sleep(250);
  }
  assert.fail(`${what} within ${PATIENCE_MS / 1000} s`);
}

Given(
  "a repository {string} that records its repository ID, with the locations {string} and {string}, in that order",
  async function (this: BrowserWorld, alias: string, first: string, second: string) {
    const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-locations-"));
    for (const name of [first, second]) {
      await cp(fixtureRepo, path.join(scratch, name), { recursive: true });
    }
    known.set(this, {
      scratch,
      alias,
      places: [
        [first, path.join(scratch, first)],
        [second, path.join(scratch, second)],
      ],
    });
  },
);

// The plan's daemon starts with the first location alone, so the repository
// records its ID the way a person's does: a backup records it (decision 57).
// The further location is written after that, as a person adds one.
Given("the plan {string} backs up to {string}", async function (this: BrowserWorld, plan: string, alias: string) {
  const { scratch, places } = locations(this);
  assert.equal(alias, locations(this).alias);
  const source = path.join(scratch, "source");
  await mkdir(source);
  for (let n = 1; n <= 3; n += 1) {
    await writeFile(path.join(source, `demo-${n}.txt`), `demo file ${n}\n`);
  }
  const [head, ...rest] = places;
  assert.ok(head, "the repository has a first location");
  const first = head[1];
  const configDir = path.join(scratch, "config");
  await mkdir(configDir);
  const configFile = path.join(configDir, "config.toml");
  await writeFile(
    configFile,
    [
      "schema = 1",
      "",
      `[repositories.${alias}]`,
      `repository = ${JSON.stringify(first)}`,
      `password = ${JSON.stringify(PASSWORD)}`,
      'maintenance = "local"',
      "",
      "[[plans]]",
      `id = ${JSON.stringify(plan)}`,
      `repository = ${JSON.stringify(alias)}`,
      `sources = [${JSON.stringify(source)}]`,
      "",
    ].join("\n"),
  );
  const { child, url } = await startServer({
    LADDERBACK_CONFIG_DIR: configDir,
    LADDERBACK_DATA_DIR: path.join(scratch, "data"),
    LADDERBACK_CACHE_DIR: path.join(scratch, "cache"),
  });
  this.ownServer = { child, scratch };
  this.baseUrl = url;
  const firstRun = await ended(this, await api(this, "POST", "/api/v1/operations", { plan }), `the first backup of ${plan}`);
  assert.ok(firstRun.status.startsWith("succeeded"), `the first backup of ${plan} ended as ${firstRun.status}`);
  const recorded = await readFile(configFile, "utf8");
  assert.match(recorded, /^repository-id = /m, "the first backup recorded the repository's ID");
  const further = rest.flatMap(([, folder]) => ["", `[[repositories.${alias}.locations]]`, `repository = ${JSON.stringify(folder)}`]);
  await writeFile(configFile, [recorded.trimEnd(), ...further, ""].join("\n"));
});

// The first location is moved away, so it holds no repository: a location
// with no rule that can't be used is skipped, and the next one is used.
Given("the plan {string} ran and used the location {string}", async function (this: BrowserWorld, plan: string, name: string) {
  const { places } = locations(this);
  for (const [other, folder] of places) {
    if (other === name) break;
    await rename(folder, `${folder}-away`);
  }
  const run = await ended(this, await api(this, "POST", "/api/v1/operations", { plan }), `the backup of ${plan}`);
  assert.ok(run.status.startsWith("succeeded"), `the backup of ${plan} ended as ${run.status}`);
});

// The rule is written into the last location's table, the end of the file,
// as a person adds one; the page reads the configuration as it is listed.
Given("the location {string} is eligible only on a wired network", async function (this: BrowserWorld, name: string) {
  const { scratch, places } = locations(this);
  assert.equal(places.at(-1)?.[0], name, `the location ${name} is the last one, whose table ends the file`);
  const configFile = path.join(scratch, "config", "config.toml");
  const written = await readFile(configFile, "utf8");
  await writeFile(configFile, [written.trimEnd(), 'when = [ { network = "wired" } ]', ""].join("\n"));
});

When("I open the Repositories page", async function (this: BrowserWorld) {
  await this.open("/repositories");
});

When("I open the Operations page", async function (this: BrowserWorld) {
  await this.open("/operations");
});

function locationList(world: BrowserWorld, alias: string) {
  return world.page.getByRole("list", { name: new RegExp(`^Locations of \\W?${escape(alias)}\\W?$`) });
}

Then(
  "the repository {string} lists the location {string} first and the location {string} second",
  async function (this: BrowserWorld, alias: string, first: string, second: string) {
    const items = locationList(this, alias).getByRole("listitem");
    await until(async () => (await items.count()) === 2, `the repository ${alias} lists two locations`);
    const [one, two] = await Promise.all([0, 1].map((n) => shown(items.nth(n).innerText())));
    assert.ok(one?.includes(placeOf(this, first)), `the first location is ${first}: ${one}`);
    assert.ok(one?.includes("tried first"), `the first location says it is tried first: ${one}`);
    assert.ok(two?.includes(placeOf(this, second)), `the second location is ${second}: ${two}`);
  },
);

Then(
  "the location {string} of the repository {string} says it may be tried on a wired network",
  async function (this: BrowserWorld, name: string, alias: string) {
    const position = locations(this).places.findIndex(([known]) => known === name);
    assert.ok(position >= 0, `no location "${name}"`);
    const items = locationList(this, alias).getByRole("listitem");
    await until(async () => {
      if ((await items.count()) <= position) return false;
      return (await shown(items.nth(position).innerText())).includes("May be tried on a wired network");
    }, `the location ${name} says it may be tried on a wired network`);
  },
);

When("I press {string} for the repository {string}", async function (this: BrowserWorld, label: string, alias: string) {
  const keys = journey(this.page);
  const button = keys.byRole("button", { name: new RegExp(`^${escape(label)}\\b.*${escape(alias)}\\W?$`) });
  const focused = async () => (await button.count()) === 1 && (await button.evaluate((el) => el === document.activeElement));
  for (let presses = 0; presses < MAX_TABS && !(await focused()); presses += 1) {
    await keys.press("Tab");
  }
  assert.ok(await focused(), `focus did not reach the button "${label}" of ${alias} within ${MAX_TABS} presses`);
  await keys.press("Enter");
});

Then("each location shows whether it answers", async function (this: BrowserWorld) {
  const { alias } = locations(this);
  const items = locationList(this, alias).getByRole("listitem");
  await until(async () => {
    const count = await items.count();
    if (count !== 2) return false;
    const texts = await Promise.all([0, 1].map((n) => shown(items.nth(n).innerText())));
    return texts.every((text) => text.includes("Answers and holds the repository"));
  }, "each location shows that it answers");
});

Then("the row for the plan {string} says {string}", async function (this: BrowserWorld, plan: string, words: string) {
  const rows = this.page.getByRole("row").filter({ hasText: plan });
  await until(async () => {
    if ((await rows.count()) === 0) return false;
    return (await shown(rows.first().innerText())).includes(words);
  }, `the newest row for ${plan} says "${words}"`);
});
