#!/usr/bin/env node
// Red-first check (technical design §13.1). For a pull request labelled
// `behaviour`, its first commit must be a `test:` commit. Every test and
// scenario that commit adds must fail *at run time* on that commit: a compile
// error doesn't count, and a test that already passes specifies nothing new.
// The commit may carry `todo!()` stubs in src so the tests compile; the check
// doesn't constrain which files it touches.
//
// The two trees (the merge base and the test: commit) are checked out at fixed
// paths, target/red-first/trees/base and trees/first, next to their target
// dirs, target/red-first/base and first. Paths that stay the same from run to
// run let Cargo reuse a target dir across runs: CI caches the base tree's. Run
// one at a time per checkout.
//
// Usage: node scripts/red-first.mjs <base-ref>
// Exit status: 0 when every new test fails at run time, 1 otherwise.
// Needs git, cargo, and, for @ui scenarios, trunk and npm.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, rmSync } from "node:fs";
import path from "node:path";

// Generous limits: builds from cold caches are slow, but a hung command must
// fail the check instead of holding the runner until the job timeout.
const BUILD_TIMEOUT_MS = 30 * 60 * 1000;
const TEST_TIMEOUT_MS = 10 * 60 * 1000;

const repo = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
const redFirst = path.join(repo, "target", "red-first");
// The worktrees' parent directory, canonical; set once the arguments check out.
let treesRoot;
// One target dir per worktree: with a shared one, Cargo kept the base tree's
// test binaries as fresh for the test: commit, so its new tests went unseen.
function targetDir(tree) {
  return path.join(redFirst, tree);
}

function git(...args) {
  return execFileSync("git", args, { cwd: repo, encoding: "utf8" }).trim();
}

/**
 * Runs a command in one of the worktrees, with that worktree's target dir. A
 * timeout or a command that can't start is an error, never a red result.
 */
function run(command, args, cwd, { env = {}, timeout = BUILD_TIMEOUT_MS } = {}) {
  const tree = path.relative(treesRoot, cwd).split(path.sep)[0];
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, CARGO_TARGET_DIR: targetDir(tree), ...env },
    maxBuffer: 256 * 1024 * 1024,
    timeout,
    killSignal: "SIGKILL",
  });
  if (result.error?.code === "ETIMEDOUT") {
    throw new Error(`${command} ${args.join(" ")} timed out after ${timeout / 60000} minutes`);
  }
  if (result.error) {
    throw new Error(`${command} ${args.join(" ")} didn't run: ${result.error.message}`);
  }
  return { status: result.status, output: `${result.stdout ?? ""}${result.stderr ?? ""}` };
}

/** The package name in a cargo `package_id`, in either of cargo's id formats. */
function packageName(id) {
  const [source, fragment] = id.split("#");
  if (fragment === undefined) return id.split(" ")[0];
  return fragment.includes("@") ? fragment.split("@")[0] : source.split("/").at(-1);
}

function fail(message) {
  console.error(`red-first: ${message}`);
  process.exitCode = 1;
}

// --- Worktrees -------------------------------------------------------------

/**
 * Removes the worktree at `dir`, and whatever a run that was killed left there:
 * the registration, the checkout, or both. The path is fixed, so a leftover
 * would make the next `git worktree add` fail.
 */
function removeTree(dir) {
  const registered = git("worktree", "list", "--porcelain").split("\n").includes(`worktree ${dir}`);
  if (registered) git("worktree", "remove", "--force", dir);
  rmSync(dir, { recursive: true, force: true });
  git("worktree", "prune");
}

// --- Rust tests ------------------------------------------------------------

/** Builds every test target and lists its tests, keyed by source file and name. */
function rustTests(dir) {
  if (!existsSync(path.join(dir, "Cargo.toml"))) {
    return { ok: true, tests: new Map() };
  }
  const build = run("cargo", ["test", "--workspace", "--no-run", "--message-format=json"], dir, {
    timeout: BUILD_TIMEOUT_MS,
  });
  if (build.status !== 0) {
    return { ok: false, output: build.output };
  }
  const tests = new Map();
  for (const line of build.output.split("\n")) {
    if (!line.startsWith("{")) continue;
    const message = JSON.parse(line);
    if (message.reason !== "compiler-artifact" || !message.profile?.test || !message.executable) {
      continue;
    }
    const source = path.relative(dir, message.target.src_path);
    // Run test binaries the way `cargo test` does: from the package's
    // directory, with the env vars it sets, so a test that reads fixtures or
    // CARGO_MANIFEST_DIR at run time doesn't fail for the wrong reason.
    const cwd = path.dirname(message.manifest_path);
    const env = { CARGO_MANIFEST_DIR: cwd, CARGO_PKG_NAME: packageName(message.package_id) };
    // Cucumber runners (harness = false) list nothing; their scenarios are
    // found from the feature files instead.
    const listed = run(message.executable, ["--list", "--format", "terse"], cwd, { env, timeout: TEST_TIMEOUT_MS });
    for (const entry of listed.output.split("\n")) {
      const match = entry.match(/^(.+): test$/);
      if (match) {
        tests.set(`${source}::${match[1]}`, { exe: message.executable, name: match[1], cwd, env });
      }
    }
  }
  return { ok: true, tests };
}

// --- Scenarios -------------------------------------------------------------

function featureFiles(dir) {
  const root = path.join(dir, "features");
  if (!existsSync(root)) return [];
  return readdirSync(root, { recursive: true })
    .filter((entry) => entry.endsWith(".feature"))
    .map((entry) => path.join(root, entry));
}

/** Scenarios keyed by file and name, with the runner their tags select. */
function scenarios(dir) {
  const found = new Map();
  for (const file of featureFiles(dir)) {
    let pending = [];
    let featureTags = [];
    let ruleTags = [];
    for (const raw of readFileSync(file, "utf8").split("\n")) {
      const line = raw.trim();
      if (line.startsWith("@")) {
        pending.push(...line.split(/\s+/).filter((t) => t.startsWith("@")));
      } else if (line.startsWith("Feature:")) {
        featureTags = pending;
        pending = [];
      } else if (line.startsWith("Rule:")) {
        ruleTags = pending;
        pending = [];
      } else {
        const match = line.match(/^(Scenario Outline|Scenario Template|Scenario|Example):\s*(.+)$/);
        if (match) {
          const tags = [...featureTags, ...ruleTags, ...pending];
          const runner = tags.includes("@ui") ? "ui" : tags.includes("@e2e") ? "e2e" : "acceptance";
          const name = match[2];
          found.set(`${path.relative(dir, file)}::${name}`, { name, runner });
          pending = [];
        }
      }
    }
  }
  return found;
}

/** An anchored regex for a scenario name; outline placeholders match anything. */
function nameRegex(name) {
  return `^${name
    .split(/<[^>]+>/)
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join(".*")}$`;
}

function scenarioFails(dir, scenario, uiReady) {
  const regex = nameRegex(scenario.name);
  let result;
  if (scenario.runner === "acceptance") {
    result = run("cargo", ["test", "-p", "app", "--test", "acceptance", "--", "--name", regex], dir, {
      timeout: TEST_TIMEOUT_MS,
    });
  } else if (scenario.runner === "e2e") {
    result = run("cargo", ["test", "--test", "e2e", "--", "--name", regex], dir, { timeout: TEST_TIMEOUT_MS });
  } else {
    uiReady();
    result = run("npx", ["cucumber-js", "--name", regex], path.join(dir, "ui-tests"), {
      env: { STEADING_BIN: path.join(targetDir("first"), "debug", "steading") },
      timeout: TEST_TIMEOUT_MS,
    });
  }
  // A filter that matched nothing passes, and an Outline can match several
  // rows: insist that the summary reports scenarios and that all of them failed.
  const summaries = [...result.output.matchAll(/(\d+) scenarios? \(([^)]*)\)/g)];
  const summary = summaries.at(-1);
  const total = summary ? Number(summary[1]) : 0;
  const failedCount = Number(summary?.[2].match(/(\d+) failed/)?.[1] ?? 0);
  return { failed: result.status !== 0 && total > 0 && failedCount === total, output: result.output };
}

// --- Main ------------------------------------------------------------------

const base = process.argv[2];
if (!base) {
  console.error("usage: node scripts/red-first.mjs <base-ref>");
  process.exit(2);
}
const first = git("rev-list", "--reverse", "--first-parent", `${base}..HEAD`).split("\n")[0];
if (!first) {
  console.error(`red-first: no commits between ${base} and HEAD`);
  process.exit(1);
}
const subject = git("log", "-1", "--format=%s", first);
if (!/^test(\([^)]*\))?:/.test(subject)) {
  fail(`the first commit must be a test: commit, got "${subject}" (${first.slice(0, 12)})`);
  process.exit(1);
}

// Cargo reports canonical source paths (macOS: /private/var, not /var), so
// the worktree roots must be canonical too for the paths to compare.
mkdirSync(path.join(redFirst, "trees"), { recursive: true });
treesRoot = realpathSync(path.join(redFirst, "trees"));
const trees = { base: path.join(treesRoot, "base"), first: path.join(treesRoot, "first") };
try {
  for (const dir of Object.values(trees)) removeTree(dir);
  git("worktree", "add", "--detach", trees.base, git("merge-base", base, first));
  git("worktree", "add", "--detach", trees.first, first);

  const before = rustTests(trees.base);
  const after = rustTests(trees.first);
  if (!before.ok) {
    throw new Error(`the base doesn't build:\n${before.output}`);
  }
  if (!after.ok) {
    throw new Error(`the test: commit doesn't compile; a compile error doesn't count as red:\n${after.output}`);
  }
  const newTests = [...after.tests].filter(([key]) => !before.tests.has(key));
  const oldScenarios = scenarios(trees.base);
  const newScenarios = [...scenarios(trees.first)].filter(([key]) => !oldScenarios.has(key));

  if (newTests.length === 0 && newScenarios.length === 0) {
    fail("the test: commit adds no tests or scenarios, so it specifies nothing new");
  }

  for (const [key, test] of newTests) {
    const result = run(test.exe, ["--exact", test.name], test.cwd, { env: test.env, timeout: TEST_TIMEOUT_MS });
    const failed = result.status !== 0 && /\b1 failed\b/.test(result.output);
    console.log(`${failed ? "red " : "GREEN"} ${key}`);
    if (!failed) fail(`${key} doesn't fail on the test: commit:\n${result.output}`);
  }

  let uiBuilt = false;
  const uiReady = () => {
    if (uiBuilt) return;
    for (const [command, args, cwd] of [
      ["trunk", ["build", "--release"], path.join(trees.first, "crates/web")],
      ["cargo", ["build"], trees.first],
      ["npm", ["ci"], path.join(trees.first, "ui-tests")],
    ]) {
      const step = run(command, args, cwd);
      if (step.status !== 0) throw new Error(`${command} ${args.join(" ")} failed:\n${step.output}`);
    }
    uiBuilt = true;
  };
  for (const [key, scenario] of newScenarios) {
    const result = scenarioFails(trees.first, scenario, uiReady);
    console.log(`${result.failed ? "red " : "GREEN"} ${key} (${scenario.runner})`);
    if (!result.failed) fail(`${key} doesn't fail on the test: commit:\n${result.output}`);
  }
} catch (error) {
  fail(error.message);
} finally {
  for (const dir of Object.values(trees)) removeTree(dir);
}
