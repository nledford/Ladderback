import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { Given, Then, When } from "@cucumber/cucumber";
import type { BrowserContextOptions } from "playwright";
import { expect } from "playwright/test";

import { checkoutBuild, shown } from "../../scripts/build-identity.ts";
import { journey, namePattern } from "../support/keyboard.ts";
import { DOUBLE_FONT_CSS, TEXT_SPACING_CSS } from "../support/reader-styles.ts";
import { type BrowserWorld, repoRoot } from "../support/world.ts";

// WCAG 2.2 A and AA, including the 2.0 and 2.1 criteria (§4.5).
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

// Browser settings a reader can ask for, as Playwright emulates them.
const BROWSER_SETTINGS: Record<string, BrowserContextOptions> = {
  "light colors": { colorScheme: "light" },
  "dark colors": { colorScheme: "dark" },
  "forced colors": { forcedColors: "active" },
  "more contrast": { contrast: "more" },
  "reduced motion": { reducedMotion: "reduce" },
};

Given("a viewport of {int} by {int} pixels", function (this: BrowserWorld, width: number, height: number) {
  this.contextOptions.viewport = { width, height };
});

Given("the browser asks for {string}", function (this: BrowserWorld, setting: string) {
  const options = BROWSER_SETTINGS[setting];
  assert.ok(options, `unknown browser setting "${setting}"`);
  Object.assign(this.contextOptions, options);
});

Given("the reader sets the text size to {int} percent", function (this: BrowserWorld, percent: number) {
  assert.ok(percent === 100 || percent === 200, "only 100 percent, the browser's own size, and 200 percent are defined");
  if (percent === 200) this.readerStyles.push(DOUBLE_FONT_CSS);
});

Given("the reader increases the text spacing", function (this: BrowserWorld) {
  this.readerStyles.push(TEXT_SPACING_CSS);
});

When("I open the web UI", async function (this: BrowserWorld) {
  await this.open("/");
});

When("I press {word}", async function (this: BrowserWorld, key: string) {
  await journey(this.page).press(key);
});

Then("the page has no accessibility violations", async function (this: BrowserWorld) {
  const { violations } = await new AxeBuilder({ page: this.page }).withTags(WCAG_TAGS).analyze();
  const summary = violations.map((v) => `${v.id}: ${v.help} (${v.nodes.length} nodes)`);
  assert.deepEqual(summary, []);
});

Then("the page is titled {string}", async function (this: BrowserWorld, title: string) {
  assert.equal(await this.page.title(), title);
});

Then("the main heading reads {string}", async function (this: BrowserWorld, name: string) {
  const keys = journey(this.page);
  assert.equal(await keys.byRole("heading", { level: 1 }).count(), 1, "exactly one h1");
  await keys.byRole("heading", { level: 1, name }).waitFor();
});

Then("focus is on the link {string}", async function (this: BrowserWorld, name: string) {
  const keys = journey(this.page);
  await keys.assertFocused(keys.byRole("link", { name }), `the link "${name}"`);
});

// The project's own documentation at the tag of the release this build is,
// where a reader of that release finds what the page says is there. The
// section must exist in this checkout, so a renamed heading fails here and not
// for a reader. GitHub's anchor for a heading is its text in lower case, with
// spaces as hyphens and other punctuation left out.
Then(
  "the link {string} leads to the section {string} of {string} in this release",
  async function (this: BrowserWorld, name: string, heading: string, file: string) {
    const version = await workspaceVersion();
    const document = await readFile(path.join(repoRoot, file), "utf8");
    const headings = document.split("\n").filter((line) => /^#+ /.test(line));
    assert.ok(headings.some((line) => line.replace(/^#+ /, "") === heading), `${file} has no heading "${heading}"`);
    const anchor = heading.toLowerCase().replace(/[^a-z0-9 -]/g, "").replaceAll(" ", "-");
    const link = this.page.getByRole("link", { name: namePattern(name) });
    await expect(link).toHaveAttribute(
      "href",
      `https://github.com/nledford/Ladderback/blob/v${version}/${file}#${anchor}`,
    );
  },
);

// The workspace's version, which the binary and the web UI are built with.
async function workspaceVersion(): Promise<string> {
  const manifest = await readFile(path.join(repoRoot, "Cargo.toml"), "utf8");
  const version = /^\[workspace\.package\]$[^[]*?^version = "([^"]+)"$/m.exec(manifest)?.[1];
  assert.ok(version, "Cargo.toml's [workspace.package] names no version");
  return version;
}

// The build the binary under test came from, as people read it: this
// checkout's, with its commit between releases (decision 90).
async function runningBuild(): Promise<string> {
  const build = checkoutBuild(repoRoot);
  return build === undefined ? await workspaceVersion() : shown(build);
}

// The version is the header's last part, after the navigation in reading
// order, and plain text rather than a focus stop (#502). It names the build
// the daemon runs (#516).
Then("the header shows the running version after the navigation", async function (this: BrowserWorld) {
  const banner = this.page.getByRole("banner");
  const version = banner.getByText(namePattern(`Version ${await runningBuild()}`));
  await expect(version).toBeVisible();
  const navigation = banner.getByRole("navigation");
  await expect(navigation).toBeVisible();
  const follows = await navigation.evaluate(
    (nav, shown) => (nav.compareDocumentPosition(shown) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0,
    await version.elementHandle(),
  );
  assert.ok(follows, "the version comes before the navigation in reading order");
  assert.equal(await version.evaluate((shown) => shown.matches(":is(a, button, input, [tabindex])")), false);
  const [shownBox, navigationBox] = [await version.boundingBox(), await navigation.boundingBox()];
  assert.ok(shownBox && navigationBox);
  const overlaps =
    shownBox.x < navigationBox.x + navigationBox.width &&
    navigationBox.x < shownBox.x + shownBox.width &&
    shownBox.y < navigationBox.y + navigationBox.height &&
    navigationBox.y < shownBox.y + shownBox.height;
  assert.equal(overlaps, false, "the version overlaps the navigation");
});

Then("the page doesn't show the running version", async function (this: BrowserWorld) {
  await expect(this.page.getByText(await workspaceVersion())).toHaveCount(0);
});

Then("focus is in the main content", async function (this: BrowserWorld) {
  const keys = journey(this.page);
  await keys.assertFocused(keys.byRole("main"), "the main landmark");
});

Then("the page doesn't scroll sideways", async function (this: BrowserWorld) {
  const overflow = await this.page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  assert.equal(overflow, 0, `the page is ${overflow}px wider than the viewport`);
});

// "Clipped" is decided from layout, not pixels, so it is deterministic:
//  1. an element that hides or clips overflow (overflow other than visible or
//     scroll/auto) holds text and its content is larger than its box, by
//     more than 1 px of rounding, in either direction;
//  2. any rendered text sits partly beyond the left or right edge of the
//     viewport, where a reader can't reach it without scrolling sideways.
// Text hidden from sight on purpose is left out: a box that `clip-path:
// inset(50%)` clips to nothing, as the live regions of index.html are (§4.5).
// It is a screen reader's alone and no one sees it at any size, so nothing in
// it is clipped from a reader; its words are checked by the region steps.
// Counting it made the check depend on when the region was last written: the
// announcer writes it a moment after a press or an event, and its one line
// never fits the region's 1 px box.
Then("no text is clipped", async function (this: BrowserWorld) {
  const clipped = await this.page.evaluate(async () => {
    await document.fonts.ready;
    // Let layout settle after injected styles.
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));

    const problems: string[] = [];
    const describe = (el: Element) => `<${el.localName}> "${(el.textContent ?? "").trim().slice(0, 40)}"`;
    const viewportWidth = document.documentElement.clientWidth;
    const hides = (value: string) => value === "hidden" || value === "clip";
    const unseenBoxes = [...document.body.querySelectorAll("*")].filter(
      (el) => getComputedStyle(el).clipPath === "inset(50%)",
    );
    const seen = (el: Element) => !unseenBoxes.some((box) => box.contains(el));

    for (const el of document.body.querySelectorAll("*")) {
      const style = getComputedStyle(el);
      if (style.display === "none" || !(el.textContent ?? "").trim() || !seen(el)) continue;
      if (hides(style.overflowX) && el.scrollWidth > el.clientWidth + 1) {
        problems.push(`${describe(el)} clips ${el.scrollWidth - el.clientWidth}px sideways`);
      }
      if (hides(style.overflowY) && el.scrollHeight > el.clientHeight + 1) {
        problems.push(`${describe(el)} clips ${el.scrollHeight - el.clientHeight}px vertically`);
      }
    }

    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const parent = node.parentElement;
      if (!(node.textContent ?? "").trim() || parent === null || !seen(parent)) continue;
      if (getComputedStyle(parent).display === "none") continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      for (const rect of range.getClientRects()) {
        if (rect.width === 0) continue;
        if (rect.left < -0.5 || rect.right > viewportWidth + 0.5) {
          problems.push(
            `${describe(parent)} reaches ${Math.round(rect.left)}..${Math.round(rect.right)}px of a ${viewportWidth}px viewport`,
          );
          break;
        }
      }
    }
    return problems;
  });
  assert.equal(clipped.length, 0, `text is clipped or off screen:\n${clipped.join("\n")}`);
});

Then("the page has a content security policy", async function (this: BrowserWorld) {
  const response = await this.page.request.get(this.page.url());
  const policy = response.headers()["content-security-policy"];
  assert.ok(policy, "the page came with no Content-Security-Policy header");
  assert.match(policy, /frame-ancestors 'none'/);
  assert.doesNotMatch(policy, /unsafe-inline/);
});

Then("the browser blocked nothing the page asked for", async function (this: BrowserWorld) {
  assert.deepEqual(this.policyBlocks, []);
});

When("a script the page doesn't carry is added to it", async function (this: BrowserWorld) {
  this.expectsPolicyBlocks = true;
  this.injectedScriptRan = await this.page.evaluate(() => {
    const script = document.createElement("script");
    script.textContent = "window.injectedScriptRan = true";
    document.head.append(script);
    // The inline script, had it run, set this flag on the page's window.
    return (window as Window & { injectedScriptRan?: boolean }).injectedScriptRan === true;
  });
});

Then("the script didn't run and the browser said it blocked it", async function (this: BrowserWorld) {
  assert.equal(this.injectedScriptRan, false, "an inline script the page doesn't carry ran");
  const deadline = Date.now() + 5_000;
  while (this.policyBlocks.length === 0 && Date.now() < deadline) {
    await new Promise((done) => setTimeout(done, 50));
  }
  assert.ok(this.policyBlocks.length > 0, "the browser reported nothing blocked");
});
