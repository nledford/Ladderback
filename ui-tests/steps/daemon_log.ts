// Steps of the Log page journeys (features/ui/daemon_log.feature): the
// recent log the daemon answers, as a Given sets it, the messages its table
// lists, and what "Copy for a bug report" puts on the clipboard. Elements are
// found by role and accessible name, as in every journey.

import { Given, Then } from "@cucumber/cucumber";
import type { DataTable } from "@cucumber/cucumber";
import { expect } from "playwright/test";

import type { BrowserWorld } from "../support/world.ts";
import { table } from "./live.ts";
import { runningBuild } from "./start_page.ts";

// How long the page takes to show what the daemon answered, with room.
const PAGE_PATIENCE_MS = 10_000;

const ISOLATION_MARKS = /[\u2066-\u2069]/g;

// The daemon answers the recent log with these lines, which the Given lists
// oldest first, as a log file reads; the answer lists them newest first.
Given("the recent log holds:", function (this: BrowserWorld, lines: DataTable) {
  const newestFirst = lines.hashes().reverse();
  this.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/log", (route) => route.fulfill({ json: { lines: newestFirst } }));
  });
});

// The daemon refuses the read of the recent log as it does when it can't read
// the sessions in its database (503 sessions-unavailable), whichever daemon
// serves the page. Registered after the Background's answer, so it wins.
Given("the daemon can't read its sessions when the recent log is read", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/log", (route) =>
      route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ code: "sessions-unavailable" }),
      }),
    );
  });
});

// The read of the recent log is lost on its way, as when the daemon has
// stopped; every other request reaches the daemon.
Given("the read of the recent log reaches no daemon", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/log", (route) => route.abort("connectionrefused"));
  });
});

// The text of each row's cell under the header "Message", from the top, with
// no bidi isolation marks, which a reader never hears (§4.5). Until the table
// shows that column (the page lists the lines once the daemon answers), it
// says what it found instead, so the poll keeps asking and a failure names it:
// a throw here would end the poll at its first try. Each read takes what is
// there and never waits, so a row drawn again meanwhile can't throw either.
async function messages(world: BrowserWorld, name: string): Promise<string[] | string> {
  const found = table(world, name);
  const headers = (await found.getByRole("columnheader").allTextContents()).map((text) => text.trim());
  const column = headers.indexOf("Message");
  if (column === -1) return `the table ${name} has no column "Message" (its headers: ${headers.join(", ")})`;
  const shown: string[] = [];
  for (const row of await found.getByRole("row").all()) {
    const cells = await row.getByRole("cell").or(row.getByRole("rowheader")).allTextContents();
    if (cells.length === 0) continue;
    shown.push((cells[column] ?? "").replace(ISOLATION_MARKS, "").trim());
  }
  return shown;
}

Then("the table {string} lists, from the top, the messages:", async function (this: BrowserWorld, name: string, expected: DataTable) {
  const listed = expected.raw().map(([message]) => message);
  await expect.poll(() => messages(this, name), { timeout: PAGE_PATIENCE_MS }).toEqual(listed);
});

// The program and its build as `--version` prints them, then each line as the
// log writes it, one to a line.
Then("the clipboard holds the running version, then the lines:", async function (this: BrowserWorld, lines: string) {
  const expected = `ladderback ${await runningBuild()}\n${lines}\n`;
  await expect.poll(() => this.page.evaluate(() => navigator.clipboard.readText()), { timeout: PAGE_PATIENCE_MS }).toBe(expected);
});
