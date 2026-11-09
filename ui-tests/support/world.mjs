// Starts one real `steading serve` for the run and a fresh browser page per
// scenario. The binary must already contain the built web UI.

import { spawn } from "node:child_process";
import { once } from "node:events";
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
const binary = process.env.STEADING_BIN ?? path.join(repoRoot, "target/debug/steading");

let server;
let baseUrl;
let browser;

function startServer() {
  const child = spawn(binary, ["serve", "--listen", "127.0.0.1:0"], {
    stdio: ["ignore", "inherit", "pipe"],
    // The banner below is read in English, whatever language the machine
    // speaks: `C` makes the daemon ignore LANGUAGE, which is cleared too.
    env: { ...process.env, LC_ALL: "C", LANGUAGE: "" },
  });
  return new Promise((resolve, reject) => {
    let output = "";
    let listening = false;
    child.on("error", (error) => reject(new Error(`cannot start ${binary}: ${error.message}`)));
    // "close" fires after stderr has been fully read, so `output` is complete.
    child.on("close", () => {
      if (!listening) reject(new Error(`steading exited before listening:\n${output}`));
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

BeforeAll(async function () {
  ({ child: server, url: baseUrl } = await startServer());
  browser = await chromium.launch();
});

AfterAll(async function () {
  await browser?.close();
  if (server && server.exitCode === null) {
    server.kill("SIGTERM");
    await once(server, "exit");
  }
});

class BrowserWorld extends World {
  /** Options for the next page, set by Given steps before it opens. */
  contextOptions = { viewport: { width: 1280, height: 800 } };
  /** Stylesheets injected after the page loads, as a reader's would be. */
  readerStyles = [];
  context;
  page;

  async open(pathname = "/") {
    this.context = await browser.newContext(this.contextOptions);
    this.page = await this.context.newPage();
    await this.page.goto(new URL(pathname, baseUrl).href);
    // The page is ready once the app has mounted its heading.
    await this.page.getByRole("heading", { level: 1 }).waitFor();
    for (const content of this.readerStyles) {
      await this.page.addStyleTag({ content });
    }
  }
}

setWorldConstructor(BrowserWorld);

Before(function () {
  this.contextOptions = { viewport: { width: 1280, height: 800 } };
  this.readerStyles = [];
});

After(async function () {
  await this.context?.close();
});
