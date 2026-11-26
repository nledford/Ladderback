// Tests for scripts/copy-to-baseline.ts (decision 165): the logs it refuses,
// left as they were, and its check that every row of the old log was copied
// or dropped as junk. The copy itself, opened by the store afterwards, runs
// in crates/adapters/store/tests/baseline.rs.
// `just lint` runs these with `node --test`.

import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";

import { checkCounts, copyToBaseline, Refusal } from "./copy-to-baseline.ts";

const AT_0029 = path.join(import.meta.dirname, "..", "crates", "adapters", "store", "tests", "fixtures", "at-0029.sql");

/** A log at migration 29 (the store tests' fixture), changed by `change`; its path. */
function logAt0029(change: string, t: { after: (fn: () => void) => void }): string {
  const dir = mkdtempSync(path.join(tmpdir(), "copy-to-baseline-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const log = path.join(dir, "ladderback.db");
  const db = new DatabaseSync(log);
  // The dump's own `CREATE TABLE sqlite_sequence` is the shell's; SQLite
  // makes that table itself for the AUTOINCREMENT tables before it.
  db.exec(readFileSync(AT_0029, "utf8").replace("CREATE TABLE IF NOT EXISTS sqlite_sequence(name,seq);", ""));
  db.exec(change);
  db.close();
  return log;
}

/** The script refuses `log`, saying `why`, and leaves it as it was with no file beside it. */
function refused(log: string, why: RegExp) {
  const before = readFileSync(log);
  assert.throws(
    () => copyToBaseline(log),
    (error: unknown) => error instanceof Refusal && why.test(error.message),
  );
  assert.deepEqual(readFileSync(log), before, "the refused log is left as it was");
  assert.equal(existsSync(`${log}.before-baseline`), false, "no copy is kept");
  assert.equal(existsSync(`${log}.baseline-new`), false, "no new log is left");
}

test("a log at migration 28, as the build before the last left it, is refused", (t) => {
  const log = logAt0029("DELETE FROM _sqlx_migrations WHERE version = 29", t);
  refused(log, /records migrations 1, .*, 28, not 1 to 29/);
});

test("a log with a migration that didn't finish is refused", (t) => {
  const log = logAt0029("UPDATE _sqlx_migrations SET success = 0 WHERE version = 17", t);
  refused(log, /a migration it records did not finish/);
});

test("a log whose migrations have another checksum is refused", (t) => {
  const log = logAt0029("UPDATE _sqlx_migrations SET checksum = x'00' WHERE version = 29", t);
  refused(log, /not the ones Ladderback shipped/);
});

test("a log at migration 29 is copied, its junk counted", (t) => {
  const log = logAt0029("", t);
  const { copied, dropped } = copyToBaseline(log);
  assert.equal(copied.get("operations"), 4);
  assert.deepEqual(dropped.map(({ table, rows }) => [table, rows]).sort(), [
    ["ownership_reports", 1],
    ["sync_plan_snapshots", 1],
    ["sync_plans", 1],
    ["sync_progress", 1],
    ["sync_reports", 1],
  ]);
  assert.ok(existsSync(`${log}.before-baseline`), "the old log is kept");
});

test("rows neither copied nor dropped stop the copy", () => {
  assert.doesNotThrow(() => checkCounts("sync_reports", 4, 3, 1));
  assert.throws(
    () => checkCounts("sync_reports", 4, 2, 1),
    (error: unknown) => error instanceof Refusal && /holds 4 rows, but 2 were copied and 1 dropped/.test(error.message),
  );
  assert.throws(() => checkCounts("operations", 4, 5, 0), Refusal);
});
