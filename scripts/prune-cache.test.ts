// Tests for scripts/prune-cache.sh, which ci.yml's `cache-prune` job runs. A
// stand-in `gh` serves a list of cache entries (two pages, as the API pages
// them) and records the deletes, so the tests need `jq` and nothing from
// GitHub. `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const script = join(import.meta.dirname, "prune-cache.sh");

type Entry = { id: number; key: string; created_at: string };

const entry = (id: number, key: string, created_at: string): Entry => ({ id, key, created_at });

// The stand-in prints each page of `pages` for the list and, for a delete,
// appends the ID to the log, failing for the IDs in `failing`. It records the
// list's URL too, so a test can check the ref filter.
function prune(pages: Entry[][], families: string[], failing: number[] = []) {
  const dir = mkdtempSync(join(tmpdir(), "prune-cache-"));
  try {
    const gh = join(dir, "gh");
    const log = join(dir, "log");
    writeFileSync(
      gh,
      `#!/usr/bin/env bash
if [ "$2" = "--paginate" ]; then
  echo "list $3" >> "${log}"
  cat "${join(dir, "pages")}"
elif [ "$2" = "--method" ] && [ "$3" = "DELETE" ]; then
  id="\${4##*/}"
  case " ${failing.join(" ")} " in *" $id "*) exit 1 ;; esac
  echo "delete $id" >> "${log}"
else
  echo "unexpected gh call: $*" >&2
  exit 3
fi
`,
    );
    chmodSync(gh, 0o755);
    writeFileSync(join(dir, "pages"), pages.map((p) => JSON.stringify({ total_count: 0, actions_caches: p })).join("\n"));
    writeFileSync(log, "");
    const result = spawnSync("bash", [script, ...families], {
      encoding: "utf8",
      env: { ...process.env, PATH: `${dir}:${process.env.PATH}`, GITHUB_REPOSITORY: "owner/repo" },
    });
    const lines = readFileSync(log, "utf8").split("\n").filter(Boolean);
    return {
      status: result.status,
      stdout: result.stdout,
      listed: lines.filter((l) => l.startsWith("list ")).map((l) => l.slice(5)),
      deleted: lines.filter((l) => l.startsWith("delete ")).map((l) => Number(l.slice(7))),
    };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const linux = "^linux-x64-mbx-v[0-9]+-target-rust-";

test("it keeps the newest entry of a family and deletes the rest, whichever page they are on", () => {
  const result = prune(
    [
      [entry(1, "linux-x64-mbx-v1-target-rust-aaa-1", "2026-11-15T10:00:00Z"), entry(3, "linux-x64-mbx-v1-target-rust-aaa-3", "2026-11-15T12:00:00Z")],
      [entry(2, "linux-x64-mbx-v1-target-rust-aaa-2", "2026-11-15T11:00:00Z")],
    ],
    [linux],
  );
  assert.equal(result.status, 0);
  assert.deepEqual(result.deleted.sort(), [1, 2]);
});

test("it lists only the entries of the default branch", () => {
  const result = prune([[entry(1, "a", "2026-11-15T10:00:00Z")]], ["^a$"]);
  assert.deepEqual(result.listed, ["repos/owner/repo/actions/caches?ref=refs/heads/main&per_page=100"]);
});

test("a family of one entry is left alone, and so is an empty one", () => {
  const result = prune([[entry(1, "linux-x64-mbx-v1-target-rust-aaa-1", "2026-11-15T10:00:00Z")]], [linux, "^nothing-"]);
  assert.equal(result.status, 0);
  assert.deepEqual(result.deleted, []);
});

test("an entry outside every family is never deleted", () => {
  const result = prune(
    [
      [
        entry(1, "linux-x64-mbx-v1-target-rust-aaa-1", "2026-11-15T10:00:00Z"),
        entry(2, "linux-x64-mbx-v1-target-rust-aaa-2", "2026-11-15T11:00:00Z"),
        entry(3, "darwin-arm64-mbx-v1-target-rust-bbb-1", "2026-11-15T09:00:00Z"),
        entry(4, "linux-x64-mbx-v1-wasm-target-rust-aaa-1", "2026-11-15T09:00:00Z"),
        entry(5, "unrelated", "2026-11-14T09:00:00Z"),
      ],
    ],
    [linux],
  );
  assert.deepEqual(result.deleted, [1]);
});

test("each family keeps its own newest entry", () => {
  const result = prune(
    [
      [
        entry(1, "playwright-chromium-Linux-h1", "2026-11-14T10:00:00Z"),
        entry(2, "playwright-chromium-Linux-h2", "2026-11-15T10:00:00Z"),
        entry(3, "playwright-chromium-firefox-webkit-Linux-h1", "2026-11-14T10:00:00Z"),
        entry(4, "playwright-chromium-firefox-webkit-Linux-h2", "2026-11-13T10:00:00Z"),
      ],
    ],
    ["^playwright-chromium-Linux-", "^playwright-chromium-firefox-webkit-Linux-"],
  );
  assert.deepEqual(result.deleted.sort(), [1, 4]);
});

test("entries saved in the same second go by ID, the larger being newer", () => {
  const result = prune(
    [[entry(7, "k-a", "2026-11-15T10:00:00Z"), entry(9, "k-b", "2026-11-15T10:00:00Z"), entry(8, "k-c", "2026-11-15T10:00:00Z")]],
    ["^k-"],
  );
  assert.deepEqual(result.deleted.sort(), [7, 8]);
});

test("a delete that fails is reported, fails the run, and the others still go", () => {
  const result = prune(
    [[entry(1, "k-a", "2026-11-15T10:00:00Z"), entry(2, "k-b", "2026-11-15T11:00:00Z"), entry(3, "k-c", "2026-11-15T12:00:00Z")]],
    ["^k-"],
    [1],
  );
  assert.equal(result.status, 1);
  assert.deepEqual(result.deleted, [2]);
  assert.match(result.stdout, /could not delete the cache entry k-a/);
});

test("an entry no family claims is reported, and kept", () => {
  const result = prune(
    [[entry(1, "k-a", "2026-11-15T10:00:00Z"), entry(2, "mystery-1", "2026-11-15T10:00:00Z")]],
    ["^k-"],
  );
  assert.equal(result.status, 0);
  assert.deepEqual(result.deleted, []);
  assert.match(result.stdout, /belong to no family.*mystery-1/);
});

test("when every entry is claimed nothing is reported", () => {
  const result = prune([[entry(1, "k-a", "2026-11-15T10:00:00Z")]], ["^k-"]);
  assert.doesNotMatch(result.stdout, /no family/);
});

test("it needs a family", () => {
  const result = prune([[]], []);
  assert.equal(result.status, 2);
});
