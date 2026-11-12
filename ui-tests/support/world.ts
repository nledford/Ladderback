// Starts one real `ladderback serve` for the run and a fresh browser page per
// scenario. The daemon must serve the web UI this checkout built, which the
// run checks before its first scenario.

import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  After,
  AfterAll,
  Before,
  BeforeAll,
  setDefaultTimeout,
  setWorldConstructor,
  World,
} from "@cucumber/cucumber";
import { type Browser, type BrowserContext, type BrowserContextOptions, chromium, type Page } from "playwright";

import type { ScriptedDaemon } from "./scripted-daemon.ts";

setDefaultTimeout(30_000);

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

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

let server: ChildProcess | undefined;
let baseUrl: string;
let browser: Browser | undefined;
let serverScratch: string;

/** A daemon started by `startServer`, and where it listens. */
export interface Server {
  child: ChildProcess;
  url: string;
}

// Starts one `ladderback serve` in scratch folders of its own, so two
// daemons never contend on one database (the e2e suites hit that). `extraEnv`
// names what a caller has already prepared, such as the config folder of a
// scenario's demo plan.
export function startServer(extraEnv: Record<string, string> = {}): Promise<Server> {
  const child = spawn(binary, ["serve", "--listen", "127.0.0.1:0"], {
    stdio: ["ignore", "inherit", "pipe"],
    // The banner below is read in English, whatever language the machine
    // speaks: `C` makes the daemon ignore LANGUAGE, which is cleared too.
    env: { ...process.env, LC_ALL: "C", LANGUAGE: "", ...extraEnv },
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

BeforeAll(async function () {
  serverScratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-shared-"));
  const started = await startServer({
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
  browser = await chromium.launch();
});

AfterAll(async function () {
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
   *  run's own daemon. */
  ownServer: OwnServer | null = null;
  /** Where this scenario's pages are served from; `null` is the run's daemon. */
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
  // Set by `open`, which every scenario's first page goes through before a
  // step reads them.
  context!: BrowserContext;
  page!: Page;

  async open(pathname = "/"): Promise<void> {
    if (browser === undefined) throw new Error("the browser is not running: BeforeAll failed");
    this.context = await browser.newContext(this.contextOptions);
    await this.scriptedDaemon?.install(this.context);
    for (const install of this.contextRoutes) await install(this.context);
    if (this.answerDelayMs > 0) {
      await this.context.route("**/api/v1/**", async (route) => {
        await new Promise((done) => setTimeout(done, this.answerDelayMs));
        await route.continue();
      });
    }
    this.page = await this.context.newPage();
    this.page.on("pageerror", (error) => this.scriptErrors.push(error.message));
    this.page.on("console", (message) => {
      if (/Content Security Policy/i.test(message.text())) this.policyBlocks.push(message.text());
    });
    await this.page.goto(new URL(pathname, this.baseUrl ?? baseUrl).href);
    // The page is ready once the app has mounted its heading.
    await this.page.getByRole("heading", { level: 1 }).waitFor();
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
  this.ownServer = null;
  this.baseUrl = null;
});

After(async function (this: BrowserWorld) {
  // A scenario that failed before its first page opened has no context.
  await (this.context as BrowserContext | undefined)?.close();
  // The daemon's content security policy (§10.1) must let the UI do all it
  // does: any journey that has the browser refuse something fails.
  if (!this.expectsPolicyBlocks && this.policyBlocks.length > 0) {
    throw new Error(`the browser blocked the page under its content security policy:\n${this.policyBlocks.join("\n")}`);
  }
});
