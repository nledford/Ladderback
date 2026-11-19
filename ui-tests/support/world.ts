// Starts one real `ladderback serve` for each worker of the run (one for the
// run when its scenarios run one at a time; `cucumber.ts`) and a fresh browser
// page per scenario. The daemon must serve the web UI this checkout built,
// which each worker checks before its first scenario.

import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { mkdirSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  After,
  AfterAll,
  AfterStep,
  Before,
  BeforeAll,
  type ITestCaseHookParameter,
  setDefaultTimeout,
  setDefinitionFunctionWrapper,
  setWorldConstructor,
  Status,
  World,
} from "@cucumber/cucumber";
import {
  type Browser,
  type BrowserContext,
  type BrowserContextOptions,
  type BrowserType,
  chromium,
  type Download,
  firefox,
  type Page,
  webkit,
} from "playwright";

import { avoidLabelledByCrash } from "./labelledby-crash.ts";
import { patienceMs, STEP_LIMIT_MS, timed } from "./patience.ts";
import type { ScriptedDaemon } from "./scripted-daemon.ts";

setDefaultTimeout(STEP_LIMIT_MS);
// Each step and hook starts its clock, which its waits run until (#1104).
setDefinitionFunctionWrapper(timed);

export const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

/** Where cargo builds this workspace: `target/`, or the `CARGO_TARGET_DIR`
 *  each worktree sets its own. */
function cargoTargetDir(): string {
  const metadata = execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  const parsed: unknown = JSON.parse(metadata);
  if (typeof parsed === "object" && parsed !== null && "target_directory" in parsed) {
    const dir = parsed.target_directory;
    if (typeof dir === "string") return dir;
  }
  throw new Error("cargo metadata named no target directory");
}

// The binary cargo built last, unless `LADDERBACK_BIN` names another. Steps
// that run one of its commands before the daemon starts run this one too.
export const binary = process.env.LADDERBACK_BIN ?? path.join(cargoTargetDir(), "debug", "ladderback");
const entryPage = path.join(repoRoot, "crates/web/dist/index.html");

// A real rustic repository, initialized once (see `fixtures/make-demo-repo.rs`)
// and copied into each scenario's scratch folder: the daemon backs up into it
// for real, so the journeys see real progress and real snapshots.
export const fixtureRepo = path.join(repoRoot, "ui-tests", "fixtures", "demo-repo");

const engines = { chromium, firefox, webkit } satisfies Record<string, BrowserType>;

/** One of Playwright's engines; a table keyed by it must name all three. */
export type Engine = keyof typeof engines;

// Own keys only: `constructor` or `toString` would find a method of Object.
const isEngine = (name: string): name is Engine => Object.hasOwn(engines, name);

function chosenEngine(): Engine {
  const name = process.env.LADDERBACK_UI_BROWSER || "chromium";
  if (!isEngine(name)) {
    throw new Error(`LADDERBACK_UI_BROWSER is "${name}"; it must be one of ${Object.keys(engines).join(", ")}`);
  }
  return name;
}

/** The browser engine the run drives: `LADDERBACK_UI_BROWSER` (`chromium`,
 *  `firefox` or `webkit`), Chromium when unset. `just test-ui <engine>` sets
 *  it, and the red-first check (`scripts/red-first.ts`) runs a new scenario
 *  under each. A step whose browser behaviour differs by engine branches on
 *  this. */
export const engine: Engine = chosenEngine();
const browserType: BrowserType = engines[engine];

/** Where a run's reports go: the JUnit report (`cucumber.ts`) and, under
 *  `failures/<engine>/`, a failed scenario's trace and screenshot. `just
 *  test-ui` empties it first, and CI uploads it when the run fails. */
const failuresDir = path.join(repoRoot, "ui-tests", "reports", "failures", engine);

/** A failed scenario's files are named after its feature file, its name and
 *  the start of its pickle's ID, which tells apart an outline's rows and two
 *  scenarios of one name. */
function evidenceName(pickle: ITestCaseHookParameter["pickle"]): string {
  const slug = (text: string) =>
    text
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "")
      .slice(0, 80);
  return `${slug(path.basename(pickle.uri, ".feature"))}--${slug(pickle.name)}--${pickle.id.slice(0, 8)}`;
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

let server: ChildProcess | undefined;
let baseUrl: string;
let browser: Browser | undefined;
let serverScratch: string;

/** A daemon started by `startServer`, and where it listens. */
export interface Server {
  child: ChildProcess;
  url: string;
}

// Where every daemon the scenarios start asks for Ladderback's newest
// release: an address nothing answers at, so no scenario asks GitHub. A
// scenario that shows an available update gives the page one itself.
export const NO_RELEASE_FEED = "http://127.0.0.1:9/";

/** The environment a command of the binary runs in: the run's own, read in
 *  English, with `HOME` and the XDG folders in `scratch/home`. A daemon that
 *  falls back on the platform's folders, or a restore to a new folder under
 *  the home, then writes into the scenario's scratch folder, never into the
 *  developer's real Ladderback folders or home (as scripts/with-test-home.sh
 *  does for the Rust tests). `extra` names what the caller has prepared, such
 *  as Ladderback's own folders. */
export function binaryEnv(scratch: string, extra: Record<string, string> = {}): NodeJS.ProcessEnv {
  const home = path.join(scratch, "home");
  mkdirSync(home, { recursive: true });
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    // The banner `startServer` reads is in English, whatever language the
    // machine speaks: `C` makes the binary ignore LANGUAGE, which is cleared too.
    LC_ALL: "C",
    LANGUAGE: "",
    HOME: home,
    XDG_DATA_HOME: path.join(home, ".local", "share"),
    XDG_CONFIG_HOME: path.join(home, ".config"),
    XDG_CACHE_HOME: path.join(home, ".cache"),
    XDG_STATE_HOME: path.join(home, ".local", "state"),
  };
  // A caller's own would name the developer's folders where a step names none.
  delete env.LADDERBACK_CONFIG_DIR;
  delete env.LADDERBACK_DATA_DIR;
  delete env.LADDERBACK_CACHE_DIR;
  return { ...env, ...extra };
}

// Starts one `ladderback serve` in scratch folders of its own, so two
// daemons never contend on one database (the e2e suites hit that), with its
// home in `scratch` (`binaryEnv`). `extraEnv` names what a caller has already
// prepared, such as the config folder of a scenario's demo plan; `args`
// follow `serve`, a loopback address by default.
export function startServer(
  scratch: string,
  extraEnv: Record<string, string> = {},
  args: string[] = ["--listen", "127.0.0.1:0"],
): Promise<Server> {
  const child = spawn(binary, ["serve", ...args], {
    stdio: ["ignore", "inherit", "pipe"],
    env: binaryEnv(scratch, { LADDERBACK_RELEASES_URL: NO_RELEASE_FEED, ...extraEnv }),
  });
  return new Promise((resolve, reject) => {
    let output = "";
    let listening = false;
    child.on("error", (error) => reject(new Error(`cannot start ${binary}: ${error.message}`)));
    // "close" fires after stderr has been fully read, so `output` is complete.
    child.on("close", () => {
      if (!listening) reject(new Error(`ladderback exited before listening:\n${output}`));
    });
    // A `for await` that returns early destroys the stream, and the server's
    // later stderr writes would then fail with EPIPE. Keep draining instead.
    child.stderr.on("data", (chunk: Buffer) => {
      if (listening) {
        process.stderr.write(chunk);
        return;
      }
      output += chunk;
      // Match only a whole line: stderr arrives in chunks, and a chunk can end
      // inside the address ("http://127.0.0."), which `\S+` would accept.
      const url = output.match(/listening on (http:\/\/\S+)\r?\n/)?.[1];
      if (url !== undefined) {
        listening = true;
        resolve({ child, url });
      }
    });
  });
}

// A development binary reads the UI from the folder it was compiled for, so one
// restored from another checkout's build cache serves that checkout's UI, or
// answers 503 where it has none (#47). Every scenario would then time out
// without saying why, so the run stops here and names the cause.
async function assertServesThisCheckoutsUi(url: string): Promise<void> {
  const shown = path.relative(repoRoot, entryPage);
  const built = await readFile(entryPage, "utf8").catch(() => undefined);
  if (built === undefined) {
    throw new Error(`the web UI is not built: ${shown} does not exist. Run \`just web\`.`);
  }
  const response = await fetch(url);
  const served = await response.text();
  if (response.status !== 200 || served !== built) {
    throw new Error(
      `the daemon at ${url} does not serve this checkout's ${shown} (HTTP ${response.status}). ` +
        "Its binary was built for another checkout, or embeds an older UI: rebuild it with `just build`.",
    );
  }
}

BeforeAll(async () => {
  serverScratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-shared-"));
  const started = await startServer(serverScratch, {
    LADDERBACK_CONFIG_DIR: path.join(serverScratch, "config"),
    LADDERBACK_DATA_DIR: path.join(serverScratch, "data"),
    LADDERBACK_CACHE_DIR: path.join(serverScratch, "cache"),
  });
  server = started.child;
  baseUrl = started.url;
  try {
    await assertServesThisCheckoutsUi(baseUrl);
  } catch (error) {
    server.kill("SIGTERM");
    throw error;
  }
  browser = await browserType.launch();
});

AfterAll(async () => {
  await browser?.close();
  if (server && server.exitCode === null) {
    server.kill("SIGTERM");
    await once(server, "exit");
  }
  await rm(serverScratch, { recursive: true, force: true });
});

/** A daemon a scenario started for itself, and its scratch folder. */
export interface OwnServer {
  child: ChildProcess;
  scratch: string;
}

export class BrowserWorld extends World {
  /** Options for the next page, set by Given steps before it opens. */
  contextOptions: BrowserContextOptions = { viewport: { width: 1280, height: 800 } };
  /** Stylesheets injected after the page loads, as a reader's would be. */
  readerStyles: string[] = [];
  /** How long the daemon's API answers wait before they arrive, so a page's
   * loading state and its waits can be tried (§4.4): every `/api/v1/` request
   * waits this long before it continues. `0` is no wait. */
  answerDelayMs = 0;
  /** A daemon the scenario plays itself, whose answers and events the page
   *  gets in place of the real daemon's; `null` is the real daemon's. */
  scriptedDaemon: ScriptedDaemon | null = null;
  /** Routes a Given sets on the next page's context, before it opens: an
   *  answer of the real daemon's that the step changes on its way. */
  contextRoutes: Array<(context: BrowserContext) => Promise<void>> = [];
  /** The daemon this scenario started, when its Given did; `null` shares the
   *  worker's shared daemon. */
  ownServer: OwnServer | null = null;
  /** Where this scenario's pages are served from; `null` is the worker's
   *  shared daemon. */
  baseUrl: string | null = null;
  /** What the page's scripts threw, so a step can say it threw nothing. */
  scriptErrors: string[] = [];
  /** What the browser reports it refused under the page's content security
   *  policy. A journey that finds one fails, unless it set out to cause it. */
  policyBlocks: string[] = [];
  expectsPolicyBlocks = false;
  /** Whether a script the page doesn't carry ran once added; set by the step
   *  that adds one. */
  injectedScriptRan: boolean | undefined;
  /** Whether the page's timers run on a clock the steps can move on, set by
   *  the Given that asks for one before the page opens. */
  fakeClock = false;
  // Set by `open`, which every scenario's first page goes through before a
  // step reads them.
  context!: BrowserContext;
  page!: Page;
  /** The scenario's first page while it works in a second tab of the same
   *  browser, which shares its cookies; `null` while there is one tab. */
  firstTab: Page | null = null;
  /** The files the browser saved, oldest first: a download's event comes
   *  before the step that checks it starts, so every page keeps them. */
  downloads: Download[] = [];
  /** Whether the context records a Playwright trace that is still to be kept
   *  or thrown away: `open` starts one, and the hooks below end it. */
  tracing = false;

  async open(pathname = "/"): Promise<void> {
    if (browser === undefined) throw new Error("the browser is not running: BeforeAll failed");
    // A second page of the scenario gets a context of its own; the first
    // one's trace ends here rather than record for the rest of the run, and
    // the first context closes, so contexts don't pile up until AfterAll's
    // `browser.close()` has to close them all at once within its limit.
    await this.discardTrace();
    await (this.context as BrowserContext | undefined)?.close();
    this.context = await browser.newContext(this.contextOptions);
    // Screenshots and DOM snapshots of every action, kept only when the
    // scenario fails (`keepFailureEvidence`), so a failure that shows only on
    // CI can be read from its run.
    await this.context.tracing.start({ screenshots: true, snapshots: true });
    this.tracing = true;
    await avoidLabelledByCrash(this.context, engine);
    await this.scriptedDaemon?.install(this.context);
    for (const install of this.contextRoutes) await install(this.context);
    if (this.answerDelayMs > 0) {
      await this.context.route("**/api/v1/**", async (route) => {
        await new Promise((done) => setTimeout(done, this.answerDelayMs));
        await route.continue();
      });
    }
    this.page = await this.context.newPage();
    // Before the page loads, so every timer it sets runs on this clock. It
    // keeps time as a real clock does until a step moves it on.
    if (this.fakeClock) await this.page.clock.install();
    this.#watch(this.page);
    await this.page.goto(new URL(pathname, this.baseUrl ?? baseUrl).href);
    // The page is ready once the app has mounted its heading.
    await this.page.getByRole("heading", { level: 1 }).waitFor({ timeout: patienceMs() });
    // As a constructed sheet, not a <style> element: the page's content
    // security policy refuses an inline one, and a reader's own styles are not
    // the page's to refuse.
    for (const content of this.readerStyles) {
      await this.page.evaluate((css) => {
        const sheet = new CSSStyleSheet();
        sheet.replaceSync(css);
        document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
      }, content);
    }
  }

  /** Opens `pathname` in a second tab beside the first, which the steps then
   *  act in until `backToFirstTab`. */
  async openSecondTab(pathname: string): Promise<void> {
    this.firstTab = this.page;
    this.page = await this.context.newPage();
    this.#watch(this.page);
    await this.page.goto(new URL(pathname, this.baseUrl ?? baseUrl).href);
    await this.page.getByRole("heading", { level: 1 }).waitFor({ timeout: patienceMs() });
  }

  /** The steps act in the first tab again, brought to the front. */
  async backToFirstTab(): Promise<void> {
    if (this.firstTab === null) throw new Error("there is no second tab to leave: no step opened one");
    this.page = this.firstTab;
    this.firstTab = null;
    await this.page.bringToFront();
  }

  /** Saves the trace the context recorded and a screenshot of the page, as
   *  `<name>.trace.zip` and `<name>.png` under `ui-tests/reports/failures/
   *  <engine>/`, and names them in the scenario's report. Once per scenario:
   *  at the failed step, before any After hook has stopped a daemon, or in
   *  the After hook for a scenario that failed in a hook or is about to. The
   *  trace first, and the screenshot within 5 s, so a page that hangs can't
   *  hold the hook past its own limit and lose both. A page that crashed has
   *  no screenshot, and a capture that fails is reported, never thrown, so it
   *  never hides the scenario's own failure. */
  async keepFailureEvidence(name: string): Promise<void> {
    if (!this.tracing) return;
    const base = path.join(failuresDir, name);
    const shown = path.relative(repoRoot, base);
    try {
      await mkdir(failuresDir, { recursive: true });
    } catch (error) {
      this.attach(`no trace or screenshot kept: ${message(error)}`);
      await this.discardTrace();
      return;
    }
    this.tracing = false;
    try {
      await this.context.tracing.stop({ path: `${base}.trace.zip` });
      this.attach(`trace: ${shown}.trace.zip (ui-tests/node_modules/.bin/playwright show-trace, or trace.playwright.dev)`);
    } catch (error) {
      this.attach(`no trace: ${message(error)}`);
    }
    try {
      // Not a wait for the page but a picture of a failed one, capped so that a
      // page that won't draw leaves the hook its time for the rest.
      await this.page.screenshot({ path: `${base}.png`, timeout: 5_000 });
      this.attach(`screenshot: ${shown}.png`);
    } catch (error) {
      this.attach(`no screenshot: ${message(error)}`);
    }
  }

  /** Ends a passing scenario's trace without writing it. */
  async discardTrace(): Promise<void> {
    if (!this.tracing) return;
    this.tracing = false;
    await this.context.tracing.stop().catch(() => undefined);
  }

  /** Keeps what `page`'s scripts throw and what its content security policy
   *  refuses, for the steps and the After hook to check. */
  #watch(page: Page): void {
    page.on("pageerror", (error) => this.scriptErrors.push(error.message));
    page.on("download", (download) => this.downloads.push(download));
    // Chromium and WebKit write "Content Security Policy", Firefox
    // "Content-Security-Policy".
    page.on("console", (message) => {
      if (/Content[- ]Security[- ]Policy/i.test(message.text())) this.policyBlocks.push(message.text());
    });
  }
}

setWorldConstructor(BrowserWorld);

Before(function (this: BrowserWorld) {
  this.contextOptions = { viewport: { width: 1280, height: 800 } };
  this.readerStyles = [];
  this.answerDelayMs = 0;
  this.scriptedDaemon = null;
  this.contextRoutes = [];
  this.scriptErrors = [];
  this.policyBlocks = [];
  this.expectsPolicyBlocks = false;
  this.downloads = [];
  this.ownServer = null;
  this.baseUrl = null;
  this.fakeClock = false;
  this.firstTab = null;
  this.tracing = false;
});

// At the step that failed, so the screenshot and the trace's end show the
// page as the step left it.
AfterStep(async function (this: BrowserWorld, { pickle, result }) {
  if (result.status === Status.FAILED) await this.keepFailureEvidence(evidenceName(pickle));
});

After(async function (this: BrowserWorld, { pickle, result }) {
  // The daemon's content security policy (§10.1) must let the UI do all it
  // does: any journey that has the browser refuse something fails.
  const blocked = !this.expectsPolicyBlocks && this.policyBlocks.length > 0;
  // A scenario that failed in a hook, or fails on a refusal below, keeps its
  // trace and screenshot here, unless a failed step kept them already.
  if (blocked || result?.status === Status.FAILED) await this.keepFailureEvidence(evidenceName(pickle));
  else await this.discardTrace();
  // A scenario that failed before its first page opened has no context.
  await (this.context as BrowserContext | undefined)?.close();
  if (blocked) {
    throw new Error(`the browser blocked the page under its content security policy:\n${this.policyBlocks.join("\n")}`);
  }
});
