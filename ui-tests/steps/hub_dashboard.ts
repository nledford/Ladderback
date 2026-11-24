// Steps of the hub's dashboard (features/ui/hub_dashboard.feature; #569):
// reports the hub took of its machines, written into the hub's database as
// its sync session writes them, since a scenario's machines run no
// operations, and what the page shows of them. Elements are found by role
// and accessible name, as in every journey.

import assert from "node:assert/strict";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { type DataTable, Given, Then } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import { ISOLATION, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";
import { listedMachine } from "./approving_machines.ts";

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
  const agent = (await listedMachine(this, hostname))["agent-id"];
  const now = Math.floor(Date.now() / 1000);
  const database = hubDatabase(this);
  try {
    const take = database.prepare(
      "INSERT INTO sync_reports (agent_id, epoch, seq, op_id, happening, code, at) VALUES (?, ?, ?, ?, ?, ?, ?)",
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
        "INSERT INTO sync_report_tallies (agent_id, held, dropped) VALUES (?, ?, 0) " +
          "ON CONFLICT (agent_id) DO UPDATE SET held = held + excluded.held",
      )
      .run(agent, rows.length);
  } finally {
    database.close();
  }
});

Given("the hub let go of {int} of the reports of {string}", async function (this: BrowserWorld, count: number, hostname: string) {
  const agent = (await listedMachine(this, hostname))["agent-id"];
  const database = hubDatabase(this);
  try {
    database
      .prepare(
        "INSERT INTO sync_report_tallies (agent_id, held, dropped) VALUES (?, 0, ?) " +
          "ON CONFLICT (agent_id) DO UPDATE SET dropped = excluded.dropped",
      )
      .run(agent, count);
  } finally {
    database.close();
  }
});

Given("the machine {string} runs in the time zone {string}", async function (this: BrowserWorld, hostname: string, zone: string) {
  const agent = (await listedMachine(this, hostname))["agent-id"];
  const database = hubDatabase(this);
  try {
    const changed = database.prepare("UPDATE agents SET zone = ? WHERE id = ?").run(zone, agent);
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
  for (const where of ["**/api/v1/hub/machines", "**/api/v1/hub/agents"]) {
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

// The viewer's time, then the machine's in parentheses with its zone named,
// the zone isolated as fetched text is (§4.5).
Then(
  "the time {string} last connected reads in the zone {string} too",
  async function (this: BrowserWorld, hostname: string, zone: string) {
    const section = this.page.getByRole("region", { name: namePattern(hostname) });
    const named = zone.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");
    await expect(section.getByText(/^Last connected /)).toContainText(new RegExp(`\\(.+ ${ISOLATION}${named}${ISOLATION}\\)`));
  },
);

// The machine's snapshot of its plans as the hub keeps it (migration
// 0025): each row in its place, a last run `minutes ago` with its status
// and a next run `in minutes`, each left out when its cell is empty, and
// how many more plans the machine left out of it.
Given(
  "{string} reported these plans to the hub, and {int} more it left out:",
  async function (this: BrowserWorld, hostname: string, leftOut: number, table: DataTable) {
    const agent = (await listedMachine(this, hostname))["agent-id"];
    const now = Math.floor(Date.now() / 1000);
    const database = hubDatabase(this);
    try {
      const take = database.prepare(
        "INSERT INTO sync_plans (agent_id, place, plan_id, last_at, last_outcome, next_run) VALUES (?, ?, ?, ?, ?, ?)",
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
      database.prepare("INSERT INTO sync_plan_snapshots (agent_id, left_out) VALUES (?, ?)").run(agent, leftOut);
    } finally {
      database.close();
    }
  },
);

// The machine's snapshot of its plans as the hub keeps it when the machine
// reported none (migration 0025): no plan, and none left out.
Given("{string} reported no plans to the hub", async function (this: BrowserWorld, hostname: string) {
  const agent = (await listedMachine(this, hostname))["agent-id"];
  const database = hubDatabase(this);
  try {
    database.prepare("INSERT INTO sync_plan_snapshots (agent_id, left_out) VALUES (?, 0)").run(agent);
  } finally {
    database.close();
  }
});

// The latest progress of the machine's operation numbered `n`, as the hub
// keeps it (migration 0025): a backup's bytes at the instant `at`.
Given(
  "{string} reported operation {int} backing up {int} of {int} bytes at {string}",
  async function (this: BrowserWorld, hostname: string, n: number, current: number, total: number, at: string) {
    const agent = (await listedMachine(this, hostname))["agent-id"];
    const database = hubDatabase(this);
    try {
      database
        .prepare("INSERT INTO sync_progress (agent_id, op_id, step, unit, current, total, at) VALUES (?, ?, ?, ?, ?, ?, ?)")
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
