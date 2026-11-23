// axe-core over the user docs site (docs/guide/, §16 decision 128), which
// `just docs-site` builds into docs/guide/book/: the check that keeps the site
// at WCAG 2.2 AA, as axe-core keeps the product's pages (§4.5). Every page is
// scanned with the A and AA rules the @ui scenarios scan with
// (steps/gallery.ts), in each of mdBook's five themes, at a wide and a narrow
// window, but the sidebar's page for a reader without JavaScript, which takes
// only its own; then the states a page load doesn't show: the landing page
// with its theme list open and with search results showing, and the page the
// first result opens, with its matches marked. A theme a page doesn't take
// fails the run, so a scan can't pass in the wrong colors.
//
// The pages are served over HTTP under /Ladderback/, the path GitHub Pages
// serves them at (book.toml's `site-url`), so the search index and the 404
// page's links load as they do there. The server listens on a free port of
// 127.0.0.1 and serves only files inside the built site.
//
// Usage: node ui-tests/scripts/docs-site-a11y.ts (`just docs-site-a11y`, which
// builds the site first). Runs in Chromium, which `just setup` installs.
// Exits 0 when axe-core finds nothing, 1 with one line per violation on
// standard error otherwise.

import { once } from "node:events";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import AxeBuilder from "@axe-core/playwright";
import { chromium, type Page } from "playwright";

/** The built site, with a trailing separator, so a prefix test keeps a path inside it. */
const SITE = fileURLToPath(new URL("../../docs/guide/book/", import.meta.url));

/** Where GitHub Pages serves the site (book.toml's `site-url`). */
const BASE = "/Ladderback/";

// WCAG 2.2 A and AA, as in steps/gallery.ts and steps/start_page.ts.
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

// mdBook's themes, by the class each puts on <html>.
const THEMES = ["light", "rust", "coal", "navy", "ayu"];

// The sidebar as a page of its own, which mdBook frames in each page only for
// a reader with JavaScript off. It carries no script, so it never takes the
// reader's theme, only the site's default (book.toml's `default-theme`), the
// theme every page has without JavaScript: it is scanned in that one alone.
const NO_SCRIPT_TOC = "toc.html";
const NO_SCRIPT_THEME = "light";

// Wide enough for the sidebar to show beside the page, and narrow enough
// that it folds away behind its toggle and the menu bar's buttons crowd.
const WINDOWS = [
  { name: "1280 px", width: 1280, height: 800 },
  { name: "400 px", width: 400, height: 800 },
];

// A word every guide uses, so the search lists several results.
const SEARCH_TERM = "repository";

// A scenario step's limit (support/world.ts), for each wait.
const LIMIT_MS = 30_000;

const TYPES: Record<string, string> = {
  ".css": "text/css; charset=utf-8",
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".txt": "text/plain; charset=utf-8",
  ".woff2": "font/woff2",
};

/** The file a request's path names inside the site, or `undefined` when it names none. */
function fileFor(pathname: string): string | undefined {
  if (!pathname.startsWith(BASE)) return undefined;
  let rest: string;
  try {
    rest = decodeURIComponent(pathname.slice(BASE.length));
  } catch {
    return undefined;
  }
  const path = resolve(SITE, rest === "" || rest.endsWith("/") ? `${rest}index.html` : rest);
  if (!path.startsWith(SITE)) return undefined;
  try {
    return statSync(path).isFile() ? path : undefined;
  } catch {
    return undefined;
  }
}

/** Every page of the built site, as paths relative to it with `/`. */
function sitePages(): string[] {
  return readdirSync(SITE, { recursive: true, encoding: "utf8" })
    .filter((path) => path.endsWith(".html"))
    .map((path) => path.split(sep).join("/"))
    .sort();
}

/** One line per violation axe-core finds on `page` as it stands, each led by `where`. */
async function scan(page: Page, where: string): Promise<string[]> {
  const { violations } = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  return violations.map((violation) => {
    const targets = violation.nodes.map((node) => node.target.join(" ")).join(", ");
    return `${where}: ${violation.id}: ${violation.help} (${violation.nodes.length} nodes: ${targets})`;
  });
}

/** Fails unless the page on show took `theme`. */
async function checkTheme(page: Page, theme: string): Promise<void> {
  const classes = await page.evaluate(() => document.documentElement.className);
  if (!classes.split(/\s+/).includes(theme)) {
    throw new Error(`${page.url()} opened without the ${theme} theme (its <html> classes: "${classes}")`);
  }
}

/** Opens `file` and fails unless the page took `theme`. */
async function open(page: Page, origin: string, file: string, theme: string): Promise<void> {
  await page.goto(`${origin}${BASE}${file}`);
  await checkTheme(page, theme);
}

if (!existsSync(`${SITE}index.html`)) {
  console.error(`docs-site-a11y: ${SITE} holds no built site; run just docs-site`);
  process.exit(1);
}
const pages = sitePages();

const server = createServer((request, response) => {
  const file = fileFor(new URL(request.url ?? "/", "http://127.0.0.1").pathname);
  // A missing page gets the site's 404 page, as on GitHub Pages.
  const status = file === undefined ? 404 : 200;
  const path = file ?? `${SITE}404.html`;
  response.writeHead(status, { "content-type": TYPES[extname(path)] ?? "application/octet-stream" });
  response.end(readFileSync(path));
});
server.listen(0, "127.0.0.1");
await once(server, "listening");
const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;

const problems: string[] = [];
let scans = 0;
const browser = await chromium.launch();
try {
  for (const theme of THEMES) {
    for (const window of WINDOWS) {
      const context = await browser.newContext({ viewport: { width: window.width, height: window.height } });
      // mdBook reads the reader's theme from local storage before it paints.
      await context.addInitScript((name) => localStorage.setItem("mdbook-theme", name), theme);
      const page = await context.newPage();
      page.setDefaultTimeout(LIMIT_MS);
      for (const file of pages) {
        if (file === NO_SCRIPT_TOC && theme !== NO_SCRIPT_THEME) continue;
        await open(page, origin, file, theme);
        problems.push(...(await scan(page, `${file} (${theme}, ${window.name})`)));
        scans += 1;
      }

      await open(page, origin, "index.html", theme);
      await page.locator("#mdbook-theme-toggle").click();
      await page.locator("#mdbook-theme-list").waitFor({ state: "visible" });
      problems.push(...(await scan(page, `index.html, theme list open (${theme}, ${window.name})`)));
      scans += 1;

      // mdBook searches on each key press, not on an input event.
      await open(page, origin, "index.html", theme);
      await page.locator("#mdbook-search-toggle").click();
      await page.locator("#mdbook-searchbar").pressSequentially(SEARCH_TERM);
      await page.locator("#mdbook-searchresults > li").first().waitFor({ state: "visible" });
      problems.push(...(await scan(page, `index.html, search results for "${SEARCH_TERM}" (${theme}, ${window.name})`)));
      scans += 1;

      // A result opens its page with each match marked in mdBook's search
      // highlight, a color pair of the theme's own.
      await page.locator("#mdbook-searchresults > li a").first().click();
      await page.locator("main mark").first().waitFor({ state: "visible" });
      await checkTheme(page, theme);
      const result = new URL(page.url()).pathname.slice(BASE.length);
      problems.push(...(await scan(page, `${result}, matches for "${SEARCH_TERM}" marked (${theme}, ${window.name})`)));
      scans += 1;

      await context.close();
    }
  }
} finally {
  await browser.close();
  server.close();
}

const runs = `${scans} scans: ${pages.length} pages, then the landing page's theme list and search results and a result's page with its matches marked, in ${THEMES.length} themes at ${WINDOWS.map((w) => w.name).join(" and ")}`;
if (problems.length > 0) {
  for (const problem of problems) console.error(problem);
  console.error(`docs-site-a11y: ${problems.length} violations in ${runs}`);
  process.exitCode = 1;
} else {
  console.log(`ok: axe-core found no violations in ${runs}`);
}
