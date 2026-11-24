// Steps for the hub recovery-mode journey (features/ui/hub_recovery.feature;
// decision 137). The scenario seeds only the recovery marker, as a restored
// hub's database would, then exercises the person-facing page.

import assert from "node:assert/strict";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { Given } from "@cucumber/cucumber";

import { askHub } from "./approving_machines.ts";
import type { BrowserWorld } from "../support/world.ts";

/** Puts one proved floor in the hub's recovery table. */
Given("the hub is in recovery mode", async function (this: BrowserWorld) {
  assert.ok(this.ownServer, "no Given started a hub of the scenario's own");
  const database = new DatabaseSync(path.join(this.ownServer.scratch, "data", "ladderback.db"));
  try {
    database.exec("PRAGMA busy_timeout = 5000");
    database
      .prepare("INSERT INTO hub_recovery_floors (repo_id, holder, epoch, expires_at, held) VALUES (?, ?, ?, ?, ?)")
      .run("07".repeat(32), "018f0000-0000-7000-8000-0000000000e1", 2, Date.now() + 86_400_000, 1);
  } finally {
    database.close();
  }
  const answer = await askHub(this, "/api/v1/hub/recovery");
  assert.equal(answer.status, 200, `the hub's recovery state could not be read: ${answer.text}`);
  assert.deepEqual(JSON.parse(answer.text), { recovering: true });
});
