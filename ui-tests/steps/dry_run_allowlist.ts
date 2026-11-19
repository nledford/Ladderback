// Steps of the dry-run allowlist journeys in Settings
// (features/ui/dry_run_allowlist.feature): the folders set before the page
// opens and read back afterwards through the daemon's own route, on a daemon
// of the scenario's own with sign-in off, which needs no session. Elements
// are found by role and accessible name, as in every journey.

import assert from "node:assert/strict";

import { Given, Then } from "@cucumber/cucumber";

import { namePattern } from "../support/keyboard.ts";
import { type BrowserWorld } from "../support/world.ts";
import { api } from "./maintenance.ts";
import { rowNamed } from "./live.ts";

/** The route that reads and sets the dry-run allowlist. */
const ALLOWLIST = "/api/v1/settings/dry-run-allowlist";

// How long the row the page adds takes to show, with room.
const PAGE_PATIENCE_MS = 10_000;

/** The allowlist as the daemon answers it: its folders and its home folder. */
interface Allowlist {
  folders: string[];
  home: string | null;
}

/** The scenario's daemon's allowlist, read as a person's page reads it. */
async function allowlist(world: BrowserWorld): Promise<Allowlist> {
  const answer = await api(world, "GET", ALLOWLIST);
  assert.equal(answer.status, 200, `the allowlist was not read: ${JSON.stringify(answer.json)}`);
  const read: unknown = answer.json;
  const folders: unknown = typeof read === "object" && read !== null ? Reflect.get(read, "folders") : undefined;
  const home: unknown = typeof read === "object" && read !== null ? Reflect.get(read, "home") : undefined;
  assert.ok(Array.isArray(folders), `the allowlist holds no folders: ${JSON.stringify(read)}`);
  return {
    folders: folders.map(String),
    home: typeof home === "string" ? home : null,
  };
}

/** Sets the scenario's daemon's allowlist to `folders`. */
async function setAllowlist(world: BrowserWorld, folders: string[]): Promise<void> {
  const answer = await api(world, "PUT", ALLOWLIST, { folders });
  assert.equal(answer.status, 200, `the allowlist was not set: ${JSON.stringify(answer.json)}`);
}

Given("the dry-run allowlist holds {string}", async function (this: BrowserWorld, folder: string) {
  await setAllowlist(this, [folder]);
});

Given("the dry-run allowlist holds {string} and {string}", async function (this: BrowserWorld, first: string, second: string) {
  await setAllowlist(this, [first, second]);
});

// The daemon answers the page's read of the allowlist as it does when it
// can't read the configuration file (503 config-unavailable).
Given("the daemon can't read its dry-run allowlist", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context) => {
    await context.route(
      (url) => url.pathname === ALLOWLIST,
      async (route) => {
        if (route.request().method() !== "GET") return route.continue();
        await route.fulfill({
          status: 503,
          contentType: "application/json",
          body: JSON.stringify({
            code: "config-unavailable",
            args: { file: { type: "text", value: "/home/demo/.config/ladderback/config.toml" } },
          }),
        });
      },
    );
  });
});

Then("the daemon's dry-run allowlist holds no folder", async function (this: BrowserWorld) {
  assert.deepEqual((await allowlist(this)).folders, []);
});

Then("the daemon's dry-run allowlist holds its home folder alone", async function (this: BrowserWorld) {
  const { folders, home } = await allowlist(this);
  assert.ok(home, "the daemon names no home folder");
  assert.deepEqual(folders, [home]);
});

Then("the table {string} has the row of the daemon's home folder", async function (this: BrowserWorld, name: string) {
  const { home } = await allowlist(this);
  assert.ok(home, "the daemon names no home folder");
  await rowNamed(this, name, home).first().waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
});

// A folder's row, found by its header, holds the button named for it.
Then(
  "the row {string} of the table {string} holds the button {string}",
  async function (this: BrowserWorld, row: string, name: string, button: string) {
    await rowNamed(this, name, row)
      .getByRole("button", { name: namePattern(button) })
      .waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  },
);
