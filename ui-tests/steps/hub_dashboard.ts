// Steps of the hub's dashboard (features/ui/hub_dashboard.feature; #569):
// reports the hub took of its machines, written into the hub's database as
// its sync session writes them, since a scenario's machines run no
// operations, and what the page shows of them. Elements are found by role
// and accessible name, as in every journey.

import assert from "node:assert/strict";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { type DataTable, Given, Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";

import { expect } from "../support/expect.ts";
import { focusComesTo, ISOLATION, journey, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";
import { pressUntilFocusIsOn } from "./accessibility.ts";
import { askHub, listedMachine } from "./approving_machines.ts";
import { menuLiesInsideWindow } from "./disclosure_menu.ts";
import { attentionMark, cardIn, factOf, markedWith, standInOneColumn, standSideBySide, statusOf } from "./machine_card.ts";

/** The outbox epoch every report of a scenario's machine is under. */
const EPOCH = "018f0000-0000-7000-8000-0000000000e1";

/** Opens the hub's database, which its daemon has open too (WAL). */
function hubDatabase(world: BrowserWorld): DatabaseSync {
  assert.ok(world.ownServer, "no Given started a hub of the scenario's own");
  const database = new DatabaseSync(path.join(world.ownServer.scratch, "data", "ladderback.db"));
  database.exec("PRAGMA busy_timeout = 5000");
  return database;
}

/** The operation numbered `n` in a scenario, as an operation ID. */
function operationId(n: number): string {
  return `018f0000-0000-7000-8000-${n.toString().padStart(12, "0")}`;
}

Given("the hub took these reports of {string}:", async function (this: BrowserWorld, hostname: string, table: DataTable) {
  const agent = (await listedMachine(this, hostname))["machine-id"];
  const now = Math.floor(Date.now() / 1000);
  const database = hubDatabase(this);
  try {
    const take = database.prepare(
      "INSERT INTO sync_reports (machine_id, epoch, seq, op_id, happening, code, at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    );
    const rows = table.hashes();
    rows.forEach((row, place) => {
      take.run(
        agent,
        EPOCH,
        place + 1,
        operationId(Number(row.operation)),
        row.happening ?? "",
        row.code ? row.code : null,
        now - Number(row["minutes ago"]) * 60,
      );
    });
    database
      .prepare(
        "INSERT INTO sync_report_tallies (machine_id, held, dropped) VALUES (?, ?, 0) " +
          "ON CONFLICT (machine_id) DO UPDATE SET held = held + excluded.held",
      )
      .run(agent, rows.length);
  } finally {
    database.close();
  }
});

Given("the hub let go of {int} of the reports of {string}", async function (this: BrowserWorld, count: number, hostname: string) {
  const agent = (await listedMachine(this, hostname))["machine-id"];
  const database = hubDatabase(this);
  try {
    database
      .prepare(
        "INSERT INTO sync_report_tallies (machine_id, held, dropped) VALUES (?, 0, ?) " +
          "ON CONFLICT (machine_id) DO UPDATE SET dropped = excluded.dropped",
      )
      .run(agent, count);
  } finally {
    database.close();
  }
});

Given("the machine {string} runs in the time zone {string}", async function (this: BrowserWorld, hostname: string, zone: string) {
  const agent = (await listedMachine(this, hostname))["machine-id"];
  const database = hubDatabase(this);
  try {
    const changed = database.prepare("UPDATE machines SET zone = ? WHERE id = ?").run(zone, agent);
    assert.equal(changed.changes, 1, `the hub has no machine ${agent}`);
  } finally {
    database.close();
  }
});

// The page's first two reads of the hub's machines, the dashboard's and the
// Machines tab's each, fail as a hub that can't read them (503
// hub-unavailable): the read on opening and the one the events stream's
// opening asks for. Each read after them reaches the hub.
Given("the hub fails its first two answers about its machines", function (this: BrowserWorld) {
  for (const where of ["**/api/v1/hub/dashboard", "**/api/v1/hub/machines"]) {
    let reads = 0;
    this.contextRoutes.push(async (context) => {
      await context.route(where, async (route) => {
        if (route.request().method() !== "GET" || reads >= 2) {
          await route.fallback();
          return;
        }
        reads += 1;
        await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ code: "hub-unavailable" }) });
      });
    });
  }
});

// Each row by its result's badge and its error code, in the order the
// table shows them.
Then("the table {string} lists, newest first:", async function (this: BrowserWorld, name: string, expected: DataTable) {
  const rows = this.page.getByRole("table", { name: namePattern(name) }).getByRole("row");
  const wanted = expected.hashes();
  // The header's row, then one row for each operation.
  await expect(rows).toHaveCount(wanted.length + 1);
  for (const [place, row] of wanted.entries()) {
    const shown = rows.nth(place + 1);
    const code = row["error code"] ?? "";
    await expect(shown).toContainText(row.result ?? "");
    if (code !== "") await expect(shown).toContainText(code);
  }
});

Then("the page shows no machine named {string}", async function (this: BrowserWorld, hostname: string) {
  await expect(this.page.getByRole("main")).not.toContainText(hostname);
});

Then("the page shows the machine {string}", async function (this: BrowserWorld, hostname: string) {
  await expect(this.page.getByRole("heading", { level: 2, name: namePattern(hostname) })).toBeVisible();
});

// The start page's notices come before a hub's cards in the page's order,
// as a screen reader and the keyboard walk it (§16, decision 173).
Then("the region {string} comes before the cards", async function (this: BrowserWorld, name: string) {
  const region = this.page.getByRole("region", { name: namePattern(name) });
  await expect(region).toHaveCount(1);
  const first = cards(this).first();
  await expect(first).toBeVisible();
  const before = await region.evaluate(
    (earlier, card) => card !== null && (earlier.compareDocumentPosition(card) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0,
    await first.elementHandle(),
  );
  assert.ok(before, `the region "${name}" doesn't come before the cards`);
});

// On the machine's page, named by its hostname: the viewer's time, then the
// machine's in parentheses with its zone named, the zone isolated as
// fetched text is (§4.5).
Then(
  "the time {string} last connected reads in the zone {string} too",
  async function (this: BrowserWorld, hostname: string, zone: string) {
    await expect(this.page.getByRole("heading", { level: 1, name: namePattern(hostname) })).toBeVisible();
    const named = zone.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");
    await expect(this.page.getByRole("main").getByText(/^Last connected /)).toContainText(
      new RegExp(`\\(.+ ${ISOLATION}${named}${ISOLATION}\\)`),
    );
  },
);

// The machine's snapshot of its plans as the hub keeps it (migration
// 0025): each row in its place, a last run `minutes ago` with its status
// and a next run `in minutes`, each left out when its cell is empty, and
// how many more plans the machine left out of it.
Given(
  "{string} reported these plans to the hub, and {int} more it left out:",
  async function (this: BrowserWorld, hostname: string, leftOut: number, table: DataTable) {
    const agent = (await listedMachine(this, hostname))["machine-id"];
    const now = Math.floor(Date.now() / 1000);
    const database = hubDatabase(this);
    try {
      const take = database.prepare(
        "INSERT INTO sync_plans (machine_id, place, plan_id, last_at, last_outcome, next_run) VALUES (?, ?, ?, ?, ?, ?)",
      );
      table.hashes().forEach((row, place) => {
        const ago = row["last run minutes ago"] ?? "";
        const next = row["next run in minutes"] ?? "";
        take.run(
          agent,
          place,
          row.plan ?? "",
          ago === "" ? null : now - Number(ago) * 60,
          row["last run"] ? row["last run"] : null,
          next === "" ? null : now + Number(next) * 60,
        );
      });
      database.prepare("INSERT INTO sync_plan_snapshots (machine_id, left_out) VALUES (?, ?)").run(agent, leftOut);
    } finally {
      database.close();
    }
  },
);

// The machine's snapshot of its plans as the hub keeps it when the machine
// reported none (migration 0025): no plan, and none left out.
Given("{string} reported no plans to the hub", async function (this: BrowserWorld, hostname: string) {
  const agent = (await listedMachine(this, hostname))["machine-id"];
  const database = hubDatabase(this);
  try {
    database.prepare("INSERT INTO sync_plan_snapshots (machine_id, left_out) VALUES (?, 0)").run(agent);
  } finally {
    database.close();
  }
});

// The latest progress of the machine's operation numbered `n`, as the hub
// keeps it (migration 0025): a backup's bytes at the instant `at`.
Given(
  "{string} reported operation {int} backing up {int} of {int} bytes at {string}",
  async function (this: BrowserWorld, hostname: string, n: number, current: number, total: number, at: string) {
    const agent = (await listedMachine(this, hostname))["machine-id"];
    const database = hubDatabase(this);
    try {
      database
        .prepare("INSERT INTO sync_progress (machine_id, op_id, step, unit, current, total, at) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .run(agent, operationId(n), "backing-up", "bytes", current, total, Math.floor(Date.parse(at) / 1000));
    } finally {
      database.close();
    }
  },
);

// Each row by its plan, its last run's badge and, when the cell names one,
// its next run's words, in the order the table shows them.
Then("the table {string} lists, in order:", async function (this: BrowserWorld, name: string, expected: DataTable) {
  const rows = this.page.getByRole("table", { name: namePattern(name) }).getByRole("row");
  const wanted = expected.hashes();
  // The header's row, then one row for each plan.
  await expect(rows).toHaveCount(wanted.length + 1);
  for (const [place, row] of wanted.entries()) {
    const shown = rows.nth(place + 1);
    await expect(shown.getByRole("rowheader")).toHaveText(namePattern(row.plan ?? ""));
    await expect(shown).toContainText(row["last run"] ?? "");
    const next = row["next run"] ?? "";
    if (next !== "") await expect(shown).toContainText(next);
  }
});

/** The machines' cards on the page, in its order. */
function cards(world: BrowserWorld): Locator {
  return world.page.getByRole("main").getByRole("article");
}

/** Who the hub's own machine is, as the hub tells its page. */
async function hubItself(world: BrowserWorld): Promise<{ hostname: string; machineId: string }> {
  const answer = await askHub(world, "/api/v1/machine");
  assert.equal(answer.status, 200, `the hub didn't say who it is: ${answer.text}`);
  const { hostname, "machine-id": machineId } = JSON.parse(answer.text) as { hostname?: string; "machine-id"?: string };
  assert.ok(hostname && machineId, `the hub names no hostname or machine ID: ${answer.text}`);
  return { hostname, machineId };
}

/** The hub's own card, named by its hostname and the words "This hub". */
async function hubsCard(world: BrowserWorld): Promise<Locator> {
  const { hostname } = await hubItself(world);
  return cardIn(world.page.getByRole("main"), `${hostname} This hub`);
}

/** The card of the approved machine named `hostname`. */
function machineCard(world: BrowserWorld, hostname: string): Locator {
  return cardIn(world.page.getByRole("main"), hostname);
}

When("I open the page of the machine {string}", async function (this: BrowserWorld, hostname: string) {
  const { "machine-id": id } = await listedMachine(this, hostname);
  await this.open(`/machines/${id}`);
});

Then("the first card is the hub's own, marked {string}", async function (this: BrowserWorld, mark: string) {
  const { hostname } = await hubItself(this);
  await expect(cards(this).first()).toHaveAccessibleName(namePattern(`${hostname} ${mark}`));
});

Then("the cards after it are, in order:", async function (this: BrowserWorld, expected: DataTable) {
  const wanted = expected.hashes().map((row) => row.machine ?? "");
  await expect(cards(this)).toHaveCount(wanted.length + 1);
  for (const [place, hostname] of wanted.entries()) {
    await expect(cards(this).nth(place + 1)).toHaveAccessibleName(namePattern(hostname));
  }
});

Then("the card {string} tells its status {string}", async function (this: BrowserWorld, hostname: string, status: string) {
  await expect(statusOf(machineCard(this, hostname), status)).toBeVisible();
});

Then("the hub's own card tells its status {string}", async function (this: BrowserWorld, status: string) {
  await expect(statusOf(await hubsCard(this), status)).toBeVisible();
});

// A machine's page is titled by its hostname once its read answers (WCAG
// 2.4.2), the name isolated as fetched text is (§4.5).
Then("the page is titled after the machine: {string}", async function (this: BrowserWorld, title: string) {
  await expect(this.page).toHaveTitle(namePattern(title));
});

Then("the card {string} tells {string}: {string}", async function (this: BrowserWorld, hostname: string, term: string, value: string) {
  await expect(await factOf(machineCard(this, hostname), term)).toHaveText(namePattern(value));
});

// Each a time element whose `datetime` a machine can read (#115).
Then(
  "the card {string} tells its {string} and {string} as times",
  async function (this: BrowserWorld, hostname: string, first: string, second: string) {
    for (const term of [first, second]) {
      const time = (await factOf(machineCard(this, hostname), term)).getByRole("time");
      await expect(time).toHaveCount(1);
      await expect(time).toHaveAttribute("datetime", /\S/);
    }
  },
);

Then("the card {string} has no attention mark", async function (this: BrowserWorld, hostname: string) {
  const card = machineCard(this, hostname);
  await expect(card).toHaveCount(1);
  await expect(attentionMark(card)).toHaveCount(0);
});

Then(
  "the card {string} is marked as needing attention, its words starting {string}",
  async function (this: BrowserWorld, hostname: string, words: string) {
    await markedWith(machineCard(this, hostname), words);
  },
);

// The sentence's link to a machine that needs attention: words that start
// with its hostname, leading to its page.
Then(
  "the page links to the page of the machine {string} with words starting {string}",
  async function (this: BrowserWorld, hostname: string, words: string) {
    const { "machine-id": id } = await listedMachine(this, hostname);
    const start = [...words].map((ch) => ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(ISOLATION);
    const link = this.page.getByRole("main").getByRole("link", { name: new RegExp(`^${ISOLATION}${start}`) });
    await expect(link).toHaveCount(1);
    await expect(link).toHaveAttribute("href", `/machines/${id}`);
  },
);

Then("the page's address is the page of the machine {string}", async function (this: BrowserWorld, hostname: string) {
  const { "machine-id": id } = await listedMachine(this, hostname);
  await expect(this.page).toHaveURL((url) => url.pathname === `/machines/${id}`);
});

Then("the cards stand in one column, each inside the page", async function (this: BrowserWorld) {
  await standInOneColumn(cards(this));
});

Then("the card {string} stands beside the hub's own", async function (this: BrowserWorld, hostname: string) {
  await standSideBySide(await hubsCard(this), machineCard(this, hostname));
});

// Until the hub can ask a paired machine to back up (#1464), its card holds
// no Back up menu, not even one shown unavailable.
Then("the card {string} has the link {string} and no button", async function (this: BrowserWorld, hostname: string, link: string) {
  const card = machineCard(this, hostname);
  await expect(card.getByRole("link")).toHaveCount(1);
  await expect(card.getByRole("link", { name: namePattern(link) })).toHaveCount(1);
  await expect(card.getByRole("button")).toHaveCount(0);
});

// A hub with no plan has nothing to back up: its card offers no menu.
Then("the hub's own card has its Open link and no button", async function (this: BrowserWorld) {
  const { hostname } = await hubItself(this);
  const card = await hubsCard(this);
  await expect(card.getByRole("link", { name: namePattern(`Open ${hostname}`) })).toHaveCount(1);
  await expect(card.getByRole("button")).toHaveCount(0);
});

// The hub's own Back up menu is named by the machine it backs up, as its
// Open link is (The Summary Card Rule, DESIGN.md).
async function backUpName(world: BrowserWorld): Promise<string> {
  const { hostname } = await hubItself(world);
  return `Back up ${hostname}`;
}

When("I press Tab until focus is on the hub's own Back up button", async function (this: BrowserWorld) {
  await pressUntilFocusIsOn(this, "Tab", "button", await backUpName(this));
});

Then("focus is on the hub's own Back up button", async function (this: BrowserWorld) {
  const name = await backUpName(this);
  await focusComesTo(this.page, journey(this.page).byRole("button", { name: namePattern(name) }), `the button "${name}"`);
});

Then("the hub's own Back up button is collapsed", async function (this: BrowserWorld) {
  const button = this.page.getByRole("button", { name: namePattern(await backUpName(this)) });
  await expect(button).toHaveAttribute("aria-expanded", "false");
});

Then("the menu of the hub's own Back up button lies inside the window", async function (this: BrowserWorld) {
  await menuLiesInsideWindow(this.page, await backUpName(this));
});

// The Open link stands before the Back up menu, which is last on the card.
When("I press Shift+Tab until focus is on the hub's own Open link", async function (this: BrowserWorld) {
  const { hostname } = await hubItself(this);
  await pressUntilFocusIsOn(this, "Shift+Tab", "link", `Open ${hostname}`);
});

Then("focus is on the hub's own main heading", async function (this: BrowserWorld) {
  const { hostname, machineId } = await hubItself(this);
  const keys = journey(this.page);
  await keys.assertFocused(keys.byRole("heading", { level: 1, name: namePattern(hostname) }), `the main heading "${hostname}"`);
  await expect(this.page).toHaveURL((url) => url.pathname === `/machines/${machineId}`);
});

Then(
  "the hub's own table of plans lists {string} with its last run {string}",
  async function (this: BrowserWorld, plan: string, lastRun: string) {
    const { hostname } = await hubItself(this);
    const table = this.page.getByRole("table", { name: namePattern(`Plans of ${hostname}`) });
    const row = table.getByRole("row").filter({ has: this.page.getByRole("rowheader", { name: namePattern(plan) }) });
    await expect(row).toHaveCount(1);
    await expect(row).toContainText(lastRun);
  },
);

Then("the hub's own table of latest operations lists one, {string}", async function (this: BrowserWorld, outcome: string) {
  const { hostname } = await hubItself(this);
  const rows = this.page.getByRole("table", { name: namePattern(`Latest operations of ${hostname}`) }).getByRole("row");
  // The header's row, then the one operation.
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(1)).toContainText(outcome);
});
