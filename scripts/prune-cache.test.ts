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
      [
        entry(1, "linux-x64-mbx-v1-target-rust-aaa-1", "2026-11-15T10:00:00Z"),
        entry(3, "linux-x64-mbx-v1-target-rust-aaa-3", "2026-11-15T12:00:00Z"),
      ],
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
  const result = prune([[entry(1, "k-a", "2026-11-15T10:00:00Z"), entry(2, "mystery-1", "2026-11-15T10:00:00Z")]], ["^k-"]);
  assert.equal(result.status, 0);
  assert.deepEqual(result.deleted, []);
  assert.match(result.stdout, /belong to no family.*mystery-1/);
});

test("when every entry is claimed nothing is reported", () => {
  const result = prune([[entry(1, "k-a", "2026-11-15T10:00:00Z")]], ["^k-"]);
  assert.doesNotMatch(result.stdout, /no family/);
});

test("--keep N keeps the newest N entries of the families after it, until the next --keep", () => {
  const result = prune(
    [
      [
        entry(1, "a-1", "2026-11-11T10:00:00Z"),
        entry(2, "a-2", "2026-11-12T10:00:00Z"),
        entry(3, "a-3", "2026-11-13T10:00:00Z"),
        entry(4, "b-1", "2026-11-11T10:00:00Z"),
        entry(5, "b-2", "2026-11-12T10:00:00Z"),
        entry(6, "c-1", "2026-11-11T10:00:00Z"),
        entry(7, "c-2", "2026-11-12T10:00:00Z"),
      ],
    ],
    ["^a-", "--keep", "2", "^b-", "--keep", "1", "^c-"],
  );
  assert.deepEqual(result.deleted.sort(), [1, 2, 6]);
});

test("--keep takes a positive number", () => {
  for (const bad of [[], ["x"], ["0"]]) {
    const result = prune([[]], ["--keep", ...bad, "^a-"]);
    assert.equal(result.status, 2);
  }
});

test("an entry two families match fails the run before anything is deleted", () => {
  const result = prune(
    [[entry(1, "a-1", "2026-11-11T10:00:00Z"), entry(2, "a-2", "2026-11-12T10:00:00Z"), entry(3, "ab-1", "2026-11-12T10:00:00Z")]],
    ["^a-", "^a"],
  );
  assert.equal(result.status, 2);
  assert.deepEqual(result.deleted, []);
});

test("--keep with no family after it fails", () => {
  assert.equal(prune([[]], ["^a-", "--keep", "2"]).status, 0);
  assert.equal(prune([[]], ["--keep", "2"]).status, 2);
});

// The families ci.yml's `cache-prune` job passes, read from the workflow, each
// with the keys the repository has saved or will save under it, oldest first:
// the layouts of today and of before (the engine sets, the manifest hash in
// mbx's generation, the `target` payload before #1076's `objects` one). Run
// through the script, every family must keep its newest
// `keep` entries and delete the others, no key may match two families, and no
// key may be left unclaimed. A family added to ci.yml without its keys here
// fails, and so does a key that stops matching.
const sha = "ec58f954cd12bc72680a8052d009fc144a0201d3";
const lock = "ba36d31e3f5625414f85eb0bf8ce7c62f0b130a58ab02a6820f8513701ede9fb";
const real: Record<string, { keep: number; keys: string[] }> = {
  "^linux-x64-mbx-v1-((m[0-9a-f]+-)?target-|dir-)?rust-": {
    keep: 1,
    keys: [
      `linux-x64-mbx-v1-target-rust-897e5912e8f5-${sha}`,
      `linux-x64-mbx-v1-m3459b49abe91-target-rust-897e5912e8f5-${sha}`,
      `linux-x64-mbx-v1-m3459b49abe91-target-rust-897e5912e8f5-${sha.replace("e", "f")}`,
      `linux-x64-mbx-v1-rust-897e5912e8f5-${sha}`,
      `linux-x64-mbx-v1-dir-rust-897e5912e8f5-${sha}`,
    ],
  },
  "^linux-x64-mbx-v1-wasm-((m[0-9a-f]+-)?target-|dir-)?rust-": {
    keep: 1,
    keys: [
      `linux-x64-mbx-v1-wasm-target-rust-897e5912e8f5-${sha}`,
      `linux-x64-mbx-v1-wasm-m3459b49abe91-target-rust-897e5912e8f5-${sha}`,
      `linux-x64-mbx-v1-wasm-dir-rust-897e5912e8f5-${sha}`,
    ],
  },
  "^darwin-arm64-mbx-v1-((m[0-9a-f]+-)?target-|dir-)?rust-": {
    keep: 1,
    keys: [
      `darwin-arm64-mbx-v1-target-rust-4a6d2236aaf2-${sha}`,
      `darwin-arm64-mbx-v1-m3459b49abe91-target-rust-4a6d2236aaf2-${sha}`,
      `darwin-arm64-mbx-v1-dir-rust-4a6d2236aaf2-${sha}`,
    ],
  },
  "^trunk-tools-Linux-": { keep: 1, keys: [`trunk-tools-Linux-${lock}`, `trunk-tools-Linux-${lock.replace("a", "b")}`] },
  "^node-cache-Linux-x64-npm-": {
    keep: 1,
    keys: [`node-cache-Linux-x64-npm-${lock}`, `node-cache-Linux-x64-npm-${lock.replace("a", "b")}`],
  },
  "^playwright-(chromium-)?Linux-": {
    keep: 1,
    keys: [`playwright-Linux-${lock}`, `playwright-chromium-Linux-${lock}`, `playwright-chromium-Linux-${lock.replace("a", "b")}`],
  },
  "^playwright-chromium-firefox-webkit-Linux-": {
    keep: 1,
    keys: [`playwright-chromium-firefox-webkit-Linux-${lock}`, `playwright-chromium-firefox-webkit-Linux-${lock.replace("a", "b")}`],
  },
  "^playwright-debs-(chromium-)?ubuntu[0-9]+-": {
    keep: 2,
    keys: [
      `playwright-debs-ubuntu26-20261111.82.6-${lock}-43741`,
      `playwright-debs-chromium-ubuntu26-20261111.82.6-${lock}-43742`,
      `playwright-debs-chromium-ubuntu26-20261111.196.1-${lock}-43743-1`,
    ],
  },
  "^playwright-debs-chromium-firefox-webkit-ubuntu[0-9]+-": {
    keep: 2,
    keys: [
      `playwright-debs-chromium-firefox-webkit-ubuntu26-20261111.82.6-${lock}-43741-1`,
      `playwright-debs-chromium-firefox-webkit-ubuntu26-20261111.196.1-${lock}-43742-1`,
      `playwright-debs-chromium-firefox-webkit-ubuntu26-20261111.196.1-${lock}-43743-1`,
    ],
  },
};

test("the families ci.yml prunes by keep the newest entries of every key layout and claim every key", () => {
  const workflow = readFileSync(join(import.meta.dirname, "..", ".github", "workflows", "ci.yml"), "utf8");
  const call = workflow.slice(workflow.indexOf("scripts/prune-cache.sh \\\n"));
  const block = call.slice(0, call.indexOf("env:"));
  const args = [...block.matchAll(/--keep \d+|'([^']+)'/g)].map((m) => m[1] ?? m[0]);
  const families = args.filter((a) => !a.startsWith("--keep"));
  assert.deepEqual([...families].sort(), Object.keys(real).sort(), "ci.yml's families and this test's keys differ");

  const entries: Entry[] = [];
  const doomed: number[] = [];
  let id = 1;
  for (const family of families) {
    const layout = real[family];
    assert.ok(layout, `ci.yml prunes the family ${family}, which this test has no keys for`);
    const { keep, keys } = layout;
    keys.forEach((key, i) => {
      entries.push(entry(id, key, `2026-11-1${i + 1}T10:00:00Z`));
      if (i < keys.length - keep) doomed.push(id);
      id++;
    });
  }
  const result = prune(
    [entries],
    args.flatMap((a) => (a.startsWith("--keep") ? a.split(" ") : [a])),
  );
  assert.equal(result.status, 0, result.stdout);
  assert.doesNotMatch(result.stdout, /no family/);
  assert.deepEqual(
    result.deleted.sort((a, b) => a - b),
    doomed,
  );
});

test("it needs a family", () => {
  const result = prune([[]], []);
  assert.equal(result.status, 2);
});
