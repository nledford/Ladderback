// Steps of the restart journey (features/execution/restart_from_settings.feature):
// a daemon of the scenario's own started as its service file starts it, with
// `--service-managed`, and how it ended once the page asked it to restart.
// Nothing here starts it again: the journey reads what the page says while
// it waits. Elements are found by role and accessible name, as in every
// journey.

import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import { Given, Then } from "@cucumber/cucumber";

import { untilSettled } from "../support/patience.ts";
import { type BrowserWorld, startServer } from "../support/world.ts";

// The status a service manager starts the daemon again on (decision 94).
const RESTART_STATUS = 75;

// A daemon of the scenario's own, with sign-in off, as a service file starts
// it. The After hook of live.ts stops it, if it still runs, and removes the
// folder.
Given("a daemon run by a service manager", async function (this: BrowserWorld) {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-restart-"));
  const { child, url } = await startServer(
    scratch,
    {
      LADDERBACK_CONFIG_DIR: path.join(scratch, "config"),
      LADDERBACK_DATA_DIR: path.join(scratch, "data"),
      LADDERBACK_CACHE_DIR: path.join(scratch, "cache"),
    },
    ["--listen", "127.0.0.1:0", "--service-managed"],
  );
  this.ownServer = { child, scratch };
  this.baseUrl = url;
});

Then("the daemon has exited with the restart status", async function (this: BrowserWorld) {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  if (own.child.exitCode === null) {
    // It stops in order once the step before asked it to: until the step's
    // limit (#1104).
    await untilSettled(once(own.child, "exit"), "the daemon never exited");
  }
  assert.equal(own.child.exitCode, RESTART_STATUS);
});
