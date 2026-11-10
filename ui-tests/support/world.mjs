// Starts one real `ladderback serve` for the run and a fresh browser page per
// scenario. The daemon must serve the web UI this checkout built, which the
// run checks before its first scenario.

import { spawn } from "node:child_process";
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
import { chromium } from "playwright";

setDefaultTimeout(30_000);

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const binary = process.env.LADDERBACK_BIN ?? path.join(repoRoot, "target/debug/ladderback");
const entryPage = path.join(repoRoot, "crates/web/dist/index.html");

// A real rustic repository, initialized once (see `fixtures/make-demo-repo.rs`)
// and copied into each scenario's scratch folder: the daemon backs up into it
// for real, so the journeys see real progress and real snapshots.
export const fixtureRepo = path.join(repoRoot, "ui-tests", "fixtures", "demo-repo");

let server;
let baseUrl;
let browser;
let serverScratch;

// Starts one `ladderback serve` in scratch folders of its own, so two
// daemons never contend on one database (the e2e suites hit that). `extraEnv`
// names what a caller has already prepared, such as the config folder of a
// scenario's demo plan.
export function startServer(extraEnv = {}) {
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
    child.stderr.on("data", (chunk) => {
      if (listening) {
        process.stderr.write(chunk);
        return;
      }
      output += chunk;
      // Match only a whole line: stderr arrives in chunks, and a chunk can end
      // inside the address ("http://127.0.0."), which `\S+` would accept.
      const match = output.match(/listening on (http:\/\/\S+)\r?\n/);
      if (match) {
        listening = true;
        resolve({ child, url: match[1] });
      }
    });
  });
}

// A development binary reads the UI from the folder it was compiled for, so one
// restored from another checkout's build cache serves that checkout's UI, or
// answers 503 where it has none (#47). Every scenario would then time out
// without saying why, so the run stops here and names the cause.
async function assertServesThisCheckoutsUi(url) {
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
  ({ child: server, url: baseUrl } = await startServer({
    LADDERBACK_CONFIG_DIR: path.join(serverScratch, "config"),
    LADDERBACK_DATA_DIR: path.join(serverScratch, "data"),
  }));
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

class BrowserWorld extends World {
  /** Options for the next page, set by Given steps before it opens. */
  contextOptions = { viewport: { width: 1280, height: 800 } };
  /** Stylesheets injected after the page loads, as a reader's would be. */
  readerStyles = [];
  /** Whether the daemon's API answers arrive late, so a page's loading state
   * can be tried (§4.4): every `/api/v1/` request waits before it continues. */
  slowNetwork = false;
  /** The daemon this scenario started, when its Given did; `null` shares the
   *  run's own daemon. */
  ownServer = null;
  /** Where this scenario's pages are served from; `null` is the run's daemon. */
  baseUrl = null;
  /** What the page's scripts threw, so a step can say it threw nothing. */
  scriptErrors = [];
  /** What the browser reports it refused under the page's content security
   *  policy. A journey that finds one fails, unless it set out to cause it. */
  policyBlocks = [];
  expectsPolicyBlocks = false;
  context;
  page;

  async open(pathname = "/") {
    this.context = await browser.newContext(this.contextOptions);
    if (this.slowNetwork) {
      await this.context.route("**/api/v1/**", async (route) => {
        await new Promise((done) => setTimeout(done, 10_000));
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

Before(function () {
  this.contextOptions = { viewport: { width: 1280, height: 800 } };
  this.readerStyles = [];
  this.slowNetwork = false;
  this.scriptErrors = [];
  this.policyBlocks = [];
  this.expectsPolicyBlocks = false;
  this.ownServer = null;
  this.baseUrl = null;
});

After(async function () {
  await this.context?.close();
  // The daemon's content security policy (§10.1) must let the UI do all it
  // does: any journey that has the browser refuse something fails.
  if (!this.expectsPolicyBlocks && this.policyBlocks.length > 0) {
    throw new Error(`the browser blocked the page under its content security policy:\n${this.policyBlocks.join("\n")}`);
  }
});
