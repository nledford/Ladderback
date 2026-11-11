#!/usr/bin/env node
// Red-first check (technical design §13.1). For a pull request labelled
// `behaviour`, its first commit must be a `test:` commit. Every test and
// scenario that commit adds must fail *at run time* on that commit: a compile
// error doesn't count, and a test that already passes specifies nothing new.
// The commit may carry `todo!()` stubs in src so the tests compile; the check
// doesn't constrain which files it touches. Each red scenario prints its first
// failing step and message under its `red` line (scripts/failure-reason.ts).
//
// The two trees (the merge base and the test: commit) are checked out at fixed
// paths, target/red-first/trees/base and trees/first, each with its own target
// dir, target/red-first/base and first. Paths that stay the same from run to
// run let a target dir outlive its run. The base tree can start from a copy of
// another build (RED_FIRST_SEED, below; CI passes it main's), and the first
// tree starts from a copy of the base tree's. Run one at a time per checkout.
//
// Usage: node scripts/red-first.ts <base-ref>
// Exit status: 0 when every new test fails at run time, 1 otherwise.
// Needs git, cargo, and, for @ui scenarios, trunk and npm.
//
// Wasm tests (`#[wasm_bindgen_test]`, run under Node by wasm-bindgen-test-runner,
// as `just test-wasm` does) count too: when the commit adds one, both trees are
// built for wasm32-unknown-unknown, and each new wasm test must fail at run time.
// That needs the wasm32 target and wasm-bindgen-cli at Cargo.lock's version
// (`just setup`), and Node.
// @ui scenarios build the UI with the tree's `just web-test` when it has one.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, renameSync, rmSync } from "node:fs";
import { failureReason } from "./failure-reason.ts";
import path from "node:path";
import { parseMetadata, wasmTestPackages } from "./wasm-test-packages.ts";

const WASM_TARGET = "wasm32-unknown-unknown";
const WASM_RUNNER = "wasm-bindgen-test-runner";
// Cargo's runner for the target, for the wasm calls only (as `just test-wasm`).
const WASM_ENV = { CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER: WASM_RUNNER };

// Generous limits: builds from cold caches are slow, but a hung command must
// fail the check instead of holding the runner until the job timeout.
const BUILD_TIMEOUT_MS = 30 * 60 * 1000;
const TEST_TIMEOUT_MS = 10 * 60 * 1000;

const repo = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
const redFirst = path.join(repo, "target", "red-first");
// The worktrees' parent directory, canonical; set once the arguments check out.
let treesRoot: string;
// One target dir per worktree: with a shared one, Cargo kept the base tree's
// test binaries as fresh for the test: commit, so its new tests went unseen.
function targetDir(tree: string): string {
  return path.join(redFirst, tree);
}

function git(...args: string[]): string {
  return execFileSync("git", args, { cwd: repo, encoding: "utf8" }).trim();
}

type Env = Record<string, string>;

/** A test a test binary lists, and how to run it alone. */
interface Test {
  exe: string;
  name: string;
  cwd: string;
  env: Env;
}

/** A tree's tests keyed by source file and name, or why they didn't build. */
type Tests = { ok: true; tests: Map<string, Test> } | { ok: false; output: string };

/** The part of a `cargo --message-format=json` line this reads. */
interface CargoMessage {
  reason: string;
  profile?: { test: boolean };
  executable?: string | null;
  target: { src_path: string };
  manifest_path: string;
  package_id: string;
}

/** A test binary built for a test target, from one line of cargo's JSON output. */
function testArtifact(line: string): (CargoMessage & { executable: string }) | null {
  if (!line.startsWith("{")) return null;
  // Cargo's own JSON, trusted to have the shape `CargoMessage` reads.
  const message = JSON.parse(line) as CargoMessage;
  if (message.reason !== "compiler-artifact" || !message.profile?.test || !message.executable) return null;
  return { ...message, executable: message.executable };
}

/**
 * Runs a command in one of the worktrees, with that worktree's target dir. A
 * timeout or a command that can't start is an error, never a red result.
 */
function run(
  command: string,
  args: string[],
  cwd: string,
  { env = {}, timeout = BUILD_TIMEOUT_MS }: { env?: Env; timeout?: number } = {},
): { status: number | null; output: string } {
  const tree = path.relative(treesRoot, cwd).split(path.sep)[0] ?? "";
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, CARGO_TARGET_DIR: targetDir(tree), ...env },
    maxBuffer: 256 * 1024 * 1024,
    timeout,
    killSignal: "SIGKILL",
  });
  if ((result.error as NodeJS.ErrnoException | undefined)?.code === "ETIMEDOUT") {
    throw new Error(`${command} ${args.join(" ")} timed out after ${timeout / 60000} minutes`);
  }
  if (result.error) {
    throw new Error(`${command} ${args.join(" ")} didn't run: ${result.error.message}`);
  }
  return { status: result.status, output: `${result.stdout ?? ""}${result.stderr ?? ""}` };
}

/** The package name in a cargo `package_id`, in either of cargo's id formats. */
function packageName(id: string): string {
  const [source = "", fragment] = id.split("#");
  if (fragment === undefined) return id.split(" ")[0] ?? id;
  return (fragment.includes("@") ? fragment.split("@")[0] : source.split("/").at(-1)) ?? id;
}

function fail(message: string): void {
  console.error(`red-first: ${message}`);
  process.exitCode = 1;
}

// --- Worktrees -------------------------------------------------------------

/**
 * Removes the worktree at `dir`, and whatever a run that was killed left there:
 * the registration, the checkout, or both. The path is fixed, so a leftover
 * would make the next `git worktree add` fail.
 */
function removeTree(dir: string): void {
  const registered = git("worktree", "list", "--porcelain").split("\n").includes(`worktree ${dir}`);
  if (registered) git("worktree", "remove", "--force", dir);
  rmSync(dir, { recursive: true, force: true });
  git("worktree", "prune");
}

/**
 * Copies a target dir to `to` unless `to` exists, beside it first and then
 * renamed, so a copy that died half way never passes for a target dir. Entries
 * named in `skip` stay behind.
 *
 * Order matters: copy before the tree that builds into `to` is checked out.
 * Cargo treats a workspace crate as fresh when no source file is newer than its
 * outputs, so files checked out before the copy would look older than the
 * copied outputs and their changes would go unseen, the very failure that
 * separate target dirs exist to prevent. `cp -a` keeps the outputs' own times,
 * and the checkout that follows is newer than all of them.
 */
function seedTarget(from: string, to: string, skip: string[] = []): void {
  if (existsSync(to) || !existsSync(from)) return;
  const partial = `${to}.partial`;
  rmSync(partial, { recursive: true, force: true });
  mkdirSync(partial, { recursive: true });
  for (const entry of readdirSync(from)) {
    if (!skip.includes(entry)) execFileSync("cp", ["-a", path.join(from, entry), partial]);
  }
  renameSync(partial, to);
  console.log(`red-first: ${path.relative(repo, to)} starts from a copy of ${path.relative(repo, from)}`);
}

/**
 * Gives the base tree a copy of the build that RED_FIRST_SEED names to start
 * from, unless it already has a target dir. CI names the build of main it
 * restored, so only what changed since then builds again, the workspace crates
 * at least. Opt-in: a local target dir can be many gigabytes, and locally the
 * base tree's own target dir outlives the run anyway.
 */
function seedBaseTarget(): void {
  const seed = process.env.RED_FIRST_SEED;
  if (!seed) return;
  // The seed may hold target/red-first itself.
  seedTarget(path.resolve(repo, seed), targetDir("base"), ["red-first"]);
}

/**
 * Gives the first tree a copy of the base tree's build to start from, unless it
 * already has a target dir. Both trees compile the same dependencies, so after
 * the copy only the crates the test: commit touches build again. Call it after
 * the base tree is built and before the first tree is checked out.
 */
function seedFirstTarget(): void {
  seedTarget(targetDir("base"), targetDir("first"));
}

// --- Rust tests ------------------------------------------------------------

/** Builds every test target and lists its tests, keyed by source file and name. */
function rustTests(dir: string): Tests {
  if (!existsSync(path.join(dir, "Cargo.toml"))) {
    return { ok: true, tests: new Map() };
  }
  const build = run("cargo", ["test", "--workspace", "--no-run", "--message-format=json"], dir, {
    timeout: BUILD_TIMEOUT_MS,
  });
  if (build.status !== 0) {
    return { ok: false, output: build.output };
  }
  const tests = new Map<string, Test>();
  for (const line of build.output.split("\n")) {
    const message = testArtifact(line);
    if (message === null) continue;
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
      const name = entry.match(/^(.+): test$/)?.[1];
      if (name !== undefined) {
        tests.set(`${source}::${name}`, { exe: message.executable, name, cwd, env });
      }
    }
  }
  return { ok: true, tests };
}

// --- Wasm tests ------------------------------------------------------------

/**
 * Whether the test: commit adds a wasm-bindgen test. The wasm lane costs two
 * builds, so it runs only for a commit that shows one in its added lines. A
 * test that this misses shows up in the host or scenario count, or not at all.
 */
function addsWasmTests(mergeBase: string, first: string): boolean {
  const diff = spawnSync("git", ["diff", "-U0", "--no-color", mergeBase, first, "--", "*.rs"], {
    cwd: repo,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  if (diff.status !== 0) throw new Error(`git diff failed:\n${diff.stderr}`);
  return diff.stdout.split("\n").some((line) => line.startsWith("+") && /#\[(\w+::)*wasm_bindgen_test\b/.test(line));
}

/**
 * Builds the wasm-bindgen test binaries of the packages that depend on
 * `wasm-bindgen-test` (`scripts/wasm-test-packages.ts`, which `just test-wasm`
 * asks too) and lists their tests, as `rustTests` does. A package
 * gets everything the Justfile's `test-wasm` gives it: all its features, and
 * the runner as Cargo's for the target (set per call, no `.cargo/config.toml`).
 */
function wasmTests(dir: string): Tests {
  if (!existsSync(path.join(dir, "Cargo.toml"))) {
    return { ok: true, tests: new Map() };
  }
  const metadata = run("cargo", ["metadata", "--no-deps", "--format-version", "1"], dir);
  if (metadata.status !== 0) {
    return { ok: false, output: metadata.output };
  }
  const packages = wasmTestPackages(parseMetadata(metadata.output)).flatMap((name) => ["--package", name]);
  if (packages.length === 0) {
    return { ok: true, tests: new Map() };
  }
  const build = run(
    "cargo",
    ["test", ...packages, "--lib", "--bins", "--tests", "--all-features"]
      .concat(["--target", WASM_TARGET, "--no-run", "--message-format=json"]),
    dir,
    { env: WASM_ENV },
  );
  if (build.status !== 0) {
    return { ok: false, output: build.output };
  }
  const tests = new Map<string, Test>();
  for (const line of build.output.split("\n")) {
    const message = testArtifact(line);
    if (message === null) continue;
    const source = path.relative(dir, message.target.src_path);
    const cwd = path.dirname(message.manifest_path);
    const env = { CARGO_MANIFEST_DIR: cwd, CARGO_PKG_NAME: packageName(message.package_id) };
    const listed = run(WASM_RUNNER, [message.executable, "--list", "--format", "terse"], cwd, {
      env,
      timeout: TEST_TIMEOUT_MS,
    });
    // A runner that can't list (its version must equal Cargo.lock's
    // wasm-bindgen: `just setup`) must stop the check, not read as no tests.
    if (listed.status !== 0) {
      throw new Error(`${WASM_RUNNER} --list ${message.executable} failed:\n${listed.output}`);
    }
    for (const entry of listed.output.split("\n")) {
      const name = entry.match(/^(.+): test$/)?.[1];
      if (name !== undefined) {
        tests.set(`${source}::${name}`, { exe: message.executable, name, cwd, env });
      }
    }
  }
  return { ok: true, tests };
}

// --- Scenarios -------------------------------------------------------------

function featureFiles(dir: string): string[] {
  const root = path.join(dir, "features");
  if (!existsSync(root)) return [];
  return readdirSync(root, { recursive: true, encoding: "utf8" })
    .filter((entry) => entry.endsWith(".feature"))
    .map((entry) => path.join(root, entry));
}

type Runner = "ui" | "e2e" | "acceptance";

interface Scenario {
  name: string;
  runner: Runner;
}

/** Scenarios keyed by file and name, with the runner their tags select. */
function scenarios(dir: string): Map<string, Scenario> {
  const found = new Map<string, Scenario>();
  for (const file of featureFiles(dir)) {
    let pending: string[] = [];
    let featureTags: string[] = [];
    let ruleTags: string[] = [];
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
        const name = line.match(/^(Scenario Outline|Scenario Template|Scenario|Example):\s*(.+)$/)?.[2];
        if (name !== undefined) {
          const tags = [...featureTags, ...ruleTags, ...pending];
          const runner = tags.includes("@ui") ? "ui" : tags.includes("@e2e") ? "e2e" : "acceptance";
          found.set(`${path.relative(dir, file)}::${name}`, { name, runner });
          pending = [];
        }
      }
    }
  }
  return found;
}

/** An anchored regex for a scenario name; outline placeholders match anything. */
function nameRegex(name: string): string {
  return `^${name
    .split(/<[^>]+>/)
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join(".*")}$`;
}

function scenarioFails(dir: string, scenario: Scenario, uiReady: () => void): { failed: boolean; output: string } {
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
    // The local binary by path, never `npx`: see the Justfile's test-ui. A
    // tree whose cucumber config is TypeScript names it: cucumber-js finds
    // only a .js, .cjs, .mjs, .json or YAML config by itself.
    const ui = path.join(dir, "ui-tests");
    const config = existsSync(path.join(ui, "cucumber.ts")) ? ["--config", "cucumber.ts"] : [];
    result = run("node_modules/.bin/cucumber-js", [...config, "--name", regex], ui, {
      env: { LADDERBACK_BIN: path.join(targetDir("first"), "debug", "ladderback") },
      timeout: TEST_TIMEOUT_MS,
    });
  }
  // A filter that matched nothing passes, and an Outline can match several
  // rows: insist that the summary reports scenarios and that all of them failed.
  const summaries = [...result.output.matchAll(/(\d+) scenarios? \(([^)]*)\)/g)];
  const summary = summaries.at(-1);
  const total = summary ? Number(summary[1]) : 0;
  const failedCount = Number(summary?.[2]?.match(/(\d+) failed/)?.[1] ?? 0);
  return { failed: result.status !== 0 && total > 0 && failedCount === total, output: result.output };
}

// --- Main ------------------------------------------------------------------

const base = process.argv[2];
if (!base) {
  console.error("usage: node scripts/red-first.ts <base-ref>");
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
  const mergeBase = git("merge-base", base, first);
  seedBaseTarget();
  git("worktree", "add", "--detach", trees.base, mergeBase);
  const before = rustTests(trees.base);
  if (!before.ok) {
    throw new Error(`the base doesn't build:\n${before.output}`);
  }
  // The wasm lane is built only for a test: commit that adds a wasm test, so
  // that other pull requests don't pay for two more builds. It goes before the
  // seed, which copies the base tree's target dir.
  const wasm = addsWasmTests(mergeBase, first);
  const beforeWasm: Tests = wasm ? wasmTests(trees.base) : { ok: true, tests: new Map() };
  if (!beforeWasm.ok) {
    throw new Error(`the base doesn't build for ${WASM_TARGET}:\n${beforeWasm.output}`);
  }
  seedFirstTarget();
  git("worktree", "add", "--detach", trees.first, first);
  const after = rustTests(trees.first);
  if (!after.ok) {
    throw new Error(`the test: commit doesn't compile; a compile error doesn't count as red:\n${after.output}`);
  }
  const afterWasm: Tests = wasm ? wasmTests(trees.first) : { ok: true, tests: new Map() };
  if (!afterWasm.ok) {
    throw new Error(
      `the test: commit doesn't compile for ${WASM_TARGET}; a compile error doesn't count as red:\n${afterWasm.output}`,
    );
  }
  const newTests = [...after.tests].filter(([key]) => !before.tests.has(key));
  const newWasmTests = [...afterWasm.tests].filter(([key]) => !beforeWasm.tests.has(key));
  const oldScenarios = scenarios(trees.base);
  const newScenarios = [...scenarios(trees.first)].filter(([key]) => !oldScenarios.has(key));

  if (newTests.length === 0 && newWasmTests.length === 0 && newScenarios.length === 0) {
    fail("the test: commit adds no tests or scenarios, so it specifies nothing new");
  }

  for (const [key, test] of newTests) {
    const result = run(test.exe, ["--exact", test.name], test.cwd, { env: test.env, timeout: TEST_TIMEOUT_MS });
    const failed = result.status !== 0 && /\b1 failed\b/.test(result.output);
    console.log(`${failed ? "red " : "GREEN"} ${key}`);
    if (!failed) fail(`${key} doesn't fail on the test: commit:\n${result.output}`);
  }
  for (const [key, test] of newWasmTests) {
    const result = run(WASM_RUNNER, [test.exe, "--exact", test.name], test.cwd, {
      env: test.env,
      timeout: TEST_TIMEOUT_MS,
    });
    const failed = result.status !== 0 && /\b1 failed\b/.test(result.output);
    console.log(`${failed ? "red " : "GREEN"} ${key} (wasm32)`);
    if (!failed) fail(`${key} doesn't fail on the test: commit (wasm32):\n${result.output}`);
  }

  let uiBuilt = false;
  const uiReady = () => {
    if (uiBuilt) return;
    // A tree that defines a `web-test` recipe builds its UI for the scenarios
    // with it, as `just test-ui` does (it may need features a release build
    // leaves out). A tree without one gets the plain release build.
    const webTest =
      existsSync(path.join(trees.first, "Justfile")) && run("just", ["--show", "web-test"], trees.first).status === 0;
    const steps: [string, string[], string][] = [
      webTest
        ? ["just", ["web-test"], trees.first]
        : ["trunk", ["build", "--release"], path.join(trees.first, "crates/web")],
      ["cargo", ["build"], trees.first],
      ["npm", ["ci"], path.join(trees.first, "ui-tests")],
    ];
    for (const [command, args, cwd] of steps) {
      const step = run(command, args, cwd);
      if (step.status !== 0) throw new Error(`${command} ${args.join(" ")} failed:\n${step.output}`);
    }
    uiBuilt = true;
  };
  for (const [key, scenario] of newScenarios) {
    const result = scenarioFails(trees.first, scenario, uiReady);
    console.log(`${result.failed ? "red " : "GREEN"} ${key} (${scenario.runner})`);
    if (result.failed) {
      // Why it is red: a scenario whose page never loaded and one whose
      // assertion failed are both red, and the log should say which this is.
      const reason = failureReason(scenario.runner, result.output) ?? "no failing step in the runner's output";
      console.log(`     failed at: ${reason}`);
    } else {
      fail(`${key} doesn't fail on the test: commit:\n${result.output}`);
    }
  }
} catch (error) {
  fail(error instanceof Error ? error.message : String(error));
} finally {
  for (const dir of Object.values(trees)) removeTree(dir);
}
