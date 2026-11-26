#!/usr/bin/env node
// Copies an operation log the migrations before the baseline schema left
// (versions 1 to 29) into the baseline schema (`migrations/0001_baseline.sql`;
// §16, decision 165), once. A build with the baseline refuses the old log
// (`OpenError::OldSchema`) and names this script; it can't upgrade it in
// place, because the baseline replaced the migrations an upgrade would run.
//
//   node scripts/copy-to-baseline.ts <data folder>/ladderback.db
//
// Stop Ladderback first (the service, `ladderback serve` and any hub
// command): the script can't tell whether a daemon still has the log open.
//
//   1. The log must hold exactly migrations 1 to 29 of the old set, each
//      applied, the first and the last by their recorded checksums. Any
//      other log is refused and left as it was.
//   2. A copy of the whole log, its write-ahead log included, goes to
//      `<log>.before-baseline` (`VACUUM INTO`); the script refuses to run
//      when that file is already there, so a second run never overwrites the
//      first copy.
//   3. A new log, `<log>.baseline-new`, gets the baseline schema and sqlx's
//      record of it, then every row of the copy, table by table and in one
//      transaction, with each row's rowid, so the order the store reads in
//      stays. `operations.machine_id`, which the baseline dropped, is left
//      out. Only rows the baseline's rules prove are junk are dropped, each
//      kind counted on the way: the plans, progress and ownership reports of
//      machines the hub had removed, which the baseline's cascades would have
//      deleted with them, and a second report of an operation's happening
//      the hub took under another machine ID (#1513), the first one kept;
//      the tally of a machine that lost one counts what it still holds. Any
//      other row the baseline refuses stops the script with the database's
//      message, and the new log is removed.
//   4. The new log must hold as many rows of each table as the copy, less
//      those dropped, pass `foreign_key_check` and `integrity_check`.
//   5. The old log's `-wal` and `-shm` files are removed and the new log is
//      renamed over the old one. The copy from step 2 stays.

import { createHash } from "node:crypto";
import { existsSync, readFileSync, realpathSync, renameSync, rmSync, statSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { pathToFileURL } from "node:url";

const ROOT = path.resolve(import.meta.dirname, "..");
const BASELINE = path.join(ROOT, "migrations", "0001_baseline.sql");

/** The old set's last version, and the checksums sqlx recorded for its first and last migrations. */
export const OLD_LAST = 29;
const OLD_FIRST_CHECKSUM = "27AD56A7416EE4E55FDBC6C7671A82D8783984EE9FA2A95253EF23BD3DB95F0388E1A69B1048FD42ADB5467C8D808A5F";
const OLD_LAST_CHECKSUM = "9F8D1ADA8704978A98C24D7F1E4CFA9FE8EEE7521A548F522B1B103ACFAB240B125E6B9A67302DFB919FFC69A4D23E67";

/** sqlx 0.9's record of the migrations it ran, as it creates it. */
const SQLX_MIGRATIONS = `CREATE TABLE IF NOT EXISTS _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
)`;

/** The tables that hold a machine's current state, which go with the machine (the baseline's cascades). */
const CASCADED = ["sync_plans", "sync_plan_snapshots", "sync_progress", "ownership_reports"];

/** The old columns the baseline dropped, by table. */
const DROPPED_COLUMNS: Record<string, string[]> = { operations: ["machine_id"] };

/** Why the script stopped, said as it is. */
export class Refusal extends Error {}

/** Each migration the log records: version, whether it applied, its checksum in upper-case hex. */
type Recorded = { version: number; success: number; checksum: string };

/** Why `recorded` is not exactly the old set, 1 to 29, each applied; null when it is. */
export function notTheOldSet(recorded: Recorded[]): string | null {
  if (recorded.length === 0) {
    return "it records no migration";
  }
  const versions = recorded.map((row) => row.version);
  const expected = Array.from({ length: OLD_LAST }, (_, index) => index + 1);
  if (versions.join(",") !== expected.join(",")) {
    return `it records migrations ${versions.join(", ")}, not 1 to ${OLD_LAST}`;
  }
  if (recorded.some((row) => row.success !== 1)) {
    return "a migration it records did not finish";
  }
  if (recorded[0]?.checksum !== OLD_FIRST_CHECKSUM || recorded[OLD_LAST - 1]?.checksum !== OLD_LAST_CHECKSUM) {
    return "its migrations are not the ones Ladderback shipped";
  }
  return null;
}

type Column = { name: string; type: string; pk: number };

function columns(db: DatabaseSync, schema: string, table: string): Column[] {
  return db.prepare(`PRAGMA ${schema}.table_info("${table}")`).all() as Column[];
}

function count(db: DatabaseSync, sql: string): number {
  const row = db.prepare(sql).get() as { n: number } | undefined;
  return Number(row?.n ?? 0);
}

/** What the copy did: rows copied and dropped, by table. */
export type Copied = { copied: Map<string, number>; dropped: { kind: string; table: string; rows: number }[] };

/** Refuses unless the new log's `copied` rows of `table` and the `dropped` junk make up the old log's `source` rows. */
export function checkCounts(table: string, source: number, copied: number, dropped: number): void {
  if (copied + dropped !== source) {
    throw new Refusal(`${table}: the old log holds ${source} rows, but ${copied} were copied and ${dropped} dropped`);
  }
}

/** Copies `log` into the baseline schema, as the header says. */
export function copyToBaseline(log: string): Copied {
  if (!existsSync(log) || !statSync(log).isFile()) {
    throw new Refusal(`${log} is not a file`);
  }
  const before = `${log}.before-baseline`;
  const fresh = `${log}.baseline-new`;
  for (const left of [before, fresh]) {
    if (existsSync(left)) {
      throw new Refusal(`${left} is already there: move it away first (an earlier run left it)`);
    }
  }

  const old = new DatabaseSync(log, { readOnly: true });
  try {
    const recorded = old
      .prepare("SELECT version, success, hex(checksum) AS checksum FROM _sqlx_migrations ORDER BY version")
      .all() as Recorded[];
    const why = notTheOldSet(recorded);
    if (why !== null) {
      throw new Refusal(`${log} is not an operation log at migration ${OLD_LAST}: ${why}`);
    }
    old.exec(`VACUUM INTO '${before.replaceAll("'", "''")}'`);
  } catch (error) {
    old.close();
    if (error instanceof Refusal) {
      throw error;
    }
    throw new Refusal(`${log} can't be read as an operation log: ${(error as Error).message}`);
  }
  old.close();

  let result: Copied;
  try {
    result = build(before, fresh);
  } catch (error) {
    // The old log is as it was, so its copy is of no use: a second run
    // starts over.
    rmSync(fresh, { force: true });
    rmSync(before, { force: true });
    throw error;
  }

  for (const suffix of ["-wal", "-shm"]) {
    rmSync(`${log}${suffix}`, { force: true });
  }
  renameSync(fresh, log);
  return result;
}

/** Builds the baseline log `fresh` from the old log's copy `from`. */
function build(from: string, fresh: string): Copied {
  const baseline = readFileSync(BASELINE);
  const db = new DatabaseSync(fresh);
  try {
    db.exec("PRAGMA foreign_keys = ON");
    db.exec(baseline.toString("utf8"));
    db.exec(SQLX_MIGRATIONS);
    db.prepare(
      "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (1, 'baseline', TRUE, ?, -1)",
    ).run(createHash("sha384").update(baseline).digest());
    db.exec(`ATTACH DATABASE '${from.replaceAll("'", "''")}' AS old`);

    const tables = (
      db
        .prepare(
          "SELECT name FROM main.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations' ORDER BY name",
        )
        .all() as { name: string }[]
    ).map((row) => row.name);
    const oldTables = (
      db
        .prepare(
          "SELECT name FROM old.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations' ORDER BY name",
        )
        .all() as { name: string }[]
    ).map((row) => row.name);
    if (tables.join(",") !== oldTables.join(",")) {
      throw new Refusal(`the old log's tables (${oldTables.join(", ")}) are not the baseline's (${tables.join(", ")})`);
    }

    const copied = new Map<string, number>();
    const dropped: Copied["dropped"] = [];
    db.exec("BEGIN");
    try {
      // The tables a foreign key refers to first.
      const parents = ["machines", "operations"];
      const order = [...parents, ...tables.filter((table) => !parents.includes(table))];
      for (const table of order) {
        const kept = columns(db, "main", table);
        const had = new Set(columns(db, "old", table).map((column) => column.name));
        const missing = kept.filter((column) => !had.has(column.name)).map((column) => column.name);
        const extra = [...had].filter(
          (name) => !kept.some((column) => column.name === name) && !(DROPPED_COLUMNS[table] ?? []).includes(name),
        );
        if (missing.length > 0 || extra.length > 0) {
          throw new Refusal(`${table}'s columns are not the ones at migration ${OLD_LAST}: ${[...missing, ...extra].join(", ")}`);
        }
        const keys = kept.filter((column) => column.pk > 0);
        const rowidAlias = keys.length === 1 && keys[0]?.type.toUpperCase() === "INTEGER";
        const names = [...(rowidAlias ? [] : ["rowid"]), ...kept.map((column) => `"${column.name}"`)];
        // The rows the baseline's rules keep; those of the old log's that
        // don't match are the junk the script drops.
        let keep: string | null = null;
        if (CASCADED.includes(table)) {
          keep = "r.machine_id IN (SELECT id FROM old.machines)";
        } else if (table === "sync_reports") {
          keep =
            "r.id = (SELECT MIN(first.id) FROM old.sync_reports AS first WHERE first.op_id = r.op_id AND first.happening = r.happening)";
        }
        const list = names.join(", ");
        const selected = names.map((name) => `r.${name}`).join(", ");
        const source = count(db, `SELECT COUNT(*) AS n FROM old."${table}"`);
        const junk = keep === null ? 0 : count(db, `SELECT COUNT(*) AS n FROM old."${table}" AS r WHERE NOT (${keep})`);
        const where = keep === null ? "" : `WHERE ${keep}`;
        db.exec(`INSERT INTO main."${table}" (${list}) SELECT ${selected} FROM old."${table}" AS r ${where} ORDER BY r.rowid`);
        const now = count(db, `SELECT COUNT(*) AS n FROM main."${table}"`);
        checkCounts(table, source, now, junk);
        copied.set(table, now);
        if (junk > 0) {
          const kind =
            table === "sync_reports" ? "a report another machine ID had sent first (#1513)" : "a removed machine's current state";
          dropped.push({ kind, table, rows: junk });
        }
      }
      // A machine's tally counts the reports it still holds.
      db.exec(`UPDATE main.sync_report_tallies SET held = (
          SELECT COUNT(*) FROM main.sync_reports AS r WHERE r.machine_id = sync_report_tallies.machine_id)
        WHERE machine_id IN (SELECT machine_id FROM old.sync_reports AS r
          WHERE r.id NOT IN (SELECT id FROM main.sync_reports))`);
      // An ID the old log gave is never given again.
      db.exec(`UPDATE main.sqlite_sequence
          SET seq = MAX(seq, (SELECT seq FROM old.sqlite_sequence WHERE name = 'sync_reports'))
          WHERE name = 'sync_reports'`);
      db.exec(`INSERT INTO main.sqlite_sequence (name, seq)
          SELECT name, seq FROM old.sqlite_sequence WHERE name = 'sync_reports'
            AND NOT EXISTS (SELECT 1 FROM main.sqlite_sequence WHERE name = 'sync_reports')`);

      const broken = db.prepare("PRAGMA main.foreign_key_check").all();
      if (broken.length > 0) {
        throw new Refusal(`the new log breaks its foreign keys: ${JSON.stringify(broken)}`);
      }
      db.exec("COMMIT");
    } catch (error) {
      db.exec("ROLLBACK");
      if (error instanceof Refusal) {
        throw error;
      }
      throw new Refusal(`the old log holds a row the baseline refuses: ${(error as Error).message}`);
    }
    db.exec("DETACH DATABASE old");
    const integrity = db.prepare("PRAGMA integrity_check").get() as { integrity_check: string } | undefined;
    if (integrity?.integrity_check !== "ok") {
      throw new Refusal(`the new log fails its integrity check: ${JSON.stringify(integrity)}`);
    }
    return { copied, dropped };
  } finally {
    db.close();
  }
}

function main(args: string[]): number {
  const [log, ...rest] = args;
  if (log === undefined || rest.length > 0) {
    process.stderr.write("usage: node scripts/copy-to-baseline.ts <data folder>/ladderback.db\n");
    return 2;
  }
  try {
    const { copied, dropped } = copyToBaseline(log);
    for (const [table, rows] of copied) {
      process.stdout.write(`copied ${rows} rows of ${table}\n`);
    }
    for (const { kind, table, rows } of dropped) {
      process.stdout.write(`dropped ${rows} rows of ${table}: ${kind}\n`);
    }
    process.stdout.write(`${log} is in the baseline schema; the old log is kept at ${log}.before-baseline\n`);
    return 0;
  } catch (error) {
    if (error instanceof Refusal) {
      process.stderr.write(`copy-to-baseline: ${error.message}\n`);
      return 1;
    }
    throw error;
  }
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  process.exitCode = main(process.argv.slice(2));
}
