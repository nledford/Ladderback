// Tests for scripts/changed-paths.sh, which ci.yml's `changes` job runs to
// sort a pull request's paths into CI's lanes. Each test builds a small
// repository, commits a change on a branch and reads the lanes the script
// answers. `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

const script = join(import.meta.dirname, "changed-paths.sh");

type Files = Record<string, string | null>;

// What the script printed, one entry per `name=value` line.
type Lanes = Record<string, string>;

// Commits `files` (null deletes one) on a branch off a base that holds
// `setup`, and answers the script's exit status, lanes and message.
function lanes(files: Files, setup: Files = {}, ...args: string[]) {
  const dir = mkdtempSync(join(tmpdir(), "changed-paths-"));
  const git = (...gitArgs: string[]) => {
    const result = spawnSync("git", ["-C", dir, "-c", "user.name=t", "-c", "user.email=t@example.com", ...gitArgs], {
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
  };
  const write = (changes: Files) => {
    for (const [path, content] of Object.entries(changes)) {
      if (content === null) {
        unlinkSync(join(dir, path));
      } else {
        mkdirSync(dirname(join(dir, path)), { recursive: true });
        writeFileSync(join(dir, path), content);
      }
    }
  };
  try {
    mkdirSync(join(dir, "scripts"));
    copyFileSync(script, join(dir, "scripts", "changed-paths.sh"));
    git("init", "-q", "-b", "main");
    write({ "README.md": "base\n", ...setup });
    git("add", "-A");
    git("commit", "-q", "-m", "base");
    git("checkout", "-q", "-b", "change");
    write(files);
    git("add", "-A");
    git("commit", "-q", "--allow-empty", "-m", "change");
    const result = spawnSync("bash", [join(dir, "scripts", "changed-paths.sh"), "main", "HEAD", ...args], {
      encoding: "utf8",
    });
    const out: Lanes = {};
    for (const line of result.stdout.split("\n")) {
      const at = line.indexOf("=");
      if (at > 0) out[line.slice(0, at)] = line.slice(at + 1);
    }
    return { status: result.status, lanes: out, why: result.stderr.trim() };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("a change to docs and Markdown alone is docs-only and reaches neither the services nor the UI", () => {
  const result = lanes({
    "docs/design/x.md": "text\n",
    "README.md": "changed\n",
    "LICENSE-MIT": "text\n",
    ".github/ISSUE_TEMPLATE/bug.yml": "name: bug\n",
  });
  assert.equal(result.status, 0);
  assert.deepEqual(result.lanes, { "docs-only": "true", guide: "false", service: "false", ui: "false" });
});

test("a change to the guide alone, or with other docs, is docs-only and runs the guide's example tests", () => {
  const alone = lanes({ "docs/guide/first-plan.md": "text\n" });
  assert.deepEqual(alone.lanes, { "docs-only": "true", guide: "true", service: "false", ui: "false" });
  const withDocs = lanes({ "docs/guide/book.toml": "[book]\n", "docs/design/x.md": "text\n", "README.md": "changed\n" });
  assert.equal(withDocs.lanes["docs-only"], "true");
  assert.equal(withDocs.lanes.guide, "true");
});

test("a change to the guide and to code is no docs-only change", () => {
  const result = lanes({ "docs/guide/first-plan.md": "text\n", "crates/adapters/config/src/lib.rs": "fn f() {}\n" });
  assert.equal(result.lanes["docs-only"], "false");
});

test("a path that only starts like the guide's folder is no guide change", () => {
  assert.equal(lanes({ "docs/guide.md": "text\n", "docs/guides/x.md": "text\n" }).lanes.guide, "false");
});

test("Markdown under crates/ counts as code", () => {
  assert.equal(lanes({ "crates/domain/README.md": "text\n" }).lanes["docs-only"], "false");
});

test("a path the script doesn't know counts as code and reaches the services", () => {
  const result = lanes({ "tools/new.py": "print()\n" });
  assert.equal(result.lanes["docs-only"], "false");
  assert.equal(result.lanes.service, "true");
});

test("the web UI turns on the engines and stays out of the services' reach", () => {
  const result = lanes({ "crates/web/src/lib.rs": "fn f() {}\n" });
  assert.deepEqual(result.lanes, { "docs-only": "false", guide: "false", service: "false", ui: "true" });
});

test("the platform adapter reaches the services, not the engines", () => {
  const result = lanes({ "crates/adapters/platform/src/lib.rs": "fn f() {}\n" });
  assert.equal(result.lanes.service, "true");
  assert.equal(result.lanes.ui, "false");
});

test("a feature file outside features/ui/ turns on the engines only when it holds an @ui scenario", () => {
  assert.equal(lanes({ "features/a/x.feature": "@ui\nFeature: x\n" }).lanes.ui, "true");
  assert.equal(lanes({ "features/a/x.feature": "@e2e\nFeature: x\n" }).lanes.ui, "false");
  assert.equal(lanes({ "features/a/x.feature": null }, { "features/a/x.feature": "@ui\nFeature: x\n" }).lanes.ui, "false");
});

test("a change with no files runs every job", () => {
  assert.deepEqual(lanes({}).lanes, { "docs-only": "false", guide: "false", service: "true", ui: "true" });
});

test("a wrong number of arguments is refused", () => {
  const result = spawnSync("bash", [script, "main"], { encoding: "utf8" });
  assert.equal(result.status, 2);
});
