import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { Given, Then, When } from "@cucumber/cucumber";
import type { BrowserContextOptions } from "playwright";

import { checkoutBuild, shown } from "../../scripts/build-identity.ts";
import { expect } from "../support/expect.ts";
import { journey, namePattern } from "../support/keyboard.ts";
import { patienceMs, untilStepLimit } from "../support/patience.ts";
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

// The window changes width with the page open, as a reader resizing it or
// turning a tablet would; its height stays.
When(/^the window (?:narrows|widens) to (\d+) pixels$/, async function (this: BrowserWorld, width: string) {
  const now = this.page.viewportSize();
  assert.ok(now, "the page has no viewport");
  await this.page.setViewportSize({ width: Number(width), height: now.height });
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
  await keys.byRole("heading", { level: 1, name }).waitFor({ timeout: patienceMs() });
});

Then("focus is on the link {string}", async function (this: BrowserWorld, name: string) {
  const keys = journey(this.page);
  await keys.assertFocused(keys.byRole("link", { name }), `the link "${name}"`);
});

// The product name in the header is the start page's link, marked as the
// current page there like the navigation's links on theirs (#594).
Then("the link {string} in the banner is marked as the current page", async function (this: BrowserWorld, name: string) {
  const link = this.page.getByRole("banner").getByRole("link", { name, exact: true });
  await expect(link).toHaveAttribute("aria-current", "page");
});

// The project's own documentation at the tag of the release this build is,
// where a reader of that release finds what the page says is there.
Then(
  "the link {string} leads to the section {string} of {string} in this release",
  async function (this: BrowserWorld, name: string, heading: string, file: string) {
    await leadsToSection(this, name, heading, file, `v${await workspaceVersion()}`);
  },
);

// The same on `main`, for a page only builds that aren't releases have, whose
// release tag predates it, as a hub's (§16, decision 122).
Then(
  "the link {string} leads to the section {string} of {string} on main",
  async function (this: BrowserWorld, name: string, heading: string, file: string) {
    await leadsToSection(this, name, heading, file, "main");
  },
);

// The link `name` goes to the section `heading` of `file` on GitHub at
// `revision`. The section must exist in this checkout, so a renamed heading
// fails here and not for a reader. GitHub's anchor for a heading is its text
// in lower case, with spaces as hyphens and other punctuation left out.
async function leadsToSection(world: BrowserWorld, name: string, heading: string, file: string, revision: string): Promise<void> {
  const document = await readFile(path.join(repoRoot, file), "utf8");
  const headings = document.split("\n").filter((line) => /^#+ /.test(line));
  assert.ok(
    headings.some((line) => line.replace(/^#+ /, "") === heading),
    `${file} has no heading "${heading}"`,
  );
  const anchor = heading
    .toLowerCase()
    .replace(/[^a-z0-9 -]/g, "")
    .replaceAll(" ", "-");
  const link = world.page.getByRole("link", { name: namePattern(name) });
  await expect(link).toHaveAttribute("href", `https://github.com/nledford/Ladderback/blob/${revision}/${file}#${anchor}`);
}

// The workspace's version, which the binary and the web UI are built with.
async function workspaceVersion(): Promise<string> {
  const manifest = await readFile(path.join(repoRoot, "Cargo.toml"), "utf8");
  const version = /^\[workspace\.package\]$[^[]*?^version = "([^"]+)"$/m.exec(manifest)?.[1];
  assert.ok(version, "Cargo.toml's [workspace.package] names no version");
  return version;
}

// The build the binary under test came from, as people read it: this
// checkout's, with its commit between releases (decision 90).
export async function runningBuild(): Promise<string> {
  const build = checkoutBuild(repoRoot);
  return build === undefined ? await workspaceVersion() : shown(build);
}

// The version is in the footer, not the header (decision 142): the banner
// holds no text naming the build the daemon runs.
Then("the banner doesn't show the running version", async function (this: BrowserWorld) {
  const banner = this.page.getByRole("banner");
  await expect(banner).toBeVisible();
  await expect(banner.getByText(namePattern(`Version ${await runningBuild()}`))).toHaveCount(0);
});

Then("the page doesn't show the running version", async function (this: BrowserWorld) {
  await expect(this.page.getByText(await workspaceVersion())).toHaveCount(0);
});

// The release check never asks the network under test, so the session's
// answer is given the update it would have found, only where it tells the
// build, as the daemon does.
Given("the daemon has found that version {string} is available", function (this: BrowserWorld, version: string) {
  this.contextRoutes.push(async (context) => {
    await context.route("**/api/v1/session", async (route) => {
      const answer = await route.fetch();
      const body = (await answer.json()) as Record<string, unknown>;
      if (typeof body.version === "string") {
        body["available-update"] = {
          version,
          "release-notes": `https://github.com/nledford/Ladderback/releases/tag/v${version}`,
        };
      }
      await route.fulfill({ response: answer, json: body });
    });
  });
});

// The notice is the header's last line, under the pages' links and outside
// their navigation, or, below 54rem where the menu button holds the links
// (decisions 143, 152), under the line that ends with the button. It is no live
// region of its own: only the announcer talks (A3). Its link opens the
// release's notes without telling GitHub the page it came from, and its name
// says it goes to another site, as every outside link's does (§4.5).
Then(
  "the header links to the notes of version {string} on a line of its own under the pages' links",
  async function (this: BrowserWorld, version: string) {
    const banner = this.page.getByRole("banner");
    // By its tag: below 54rem the closed navigation is undrawn, so no role finds it.
    const navigation = banner.locator("nav");
    const link = banner.getByRole("link", { name: namePattern(`Version ${version} is available (goes to another site)`) });
    await expect(link).toHaveCount(1);
    await expect(banner.getByRole("status")).toHaveCount(0);
    await expect(link).toBeVisible();
    await expect(link).toHaveAttribute("href", `https://github.com/nledford/Ladderback/releases/tag/v${version}`);
    const rel = ((await link.getAttribute("rel")) ?? "").split(/\s+/);
    assert.ok(rel.includes("noreferrer"), `the link's rel is "${rel.join(" ")}"`);
    const inside = await navigation.evaluate((nav, notice) => notice !== null && nav.contains(notice), await link.elementHandle());
    assert.equal(inside, false, "the notice is in the navigation");
    // What the notice stands under: the pages' links, or the menu button
    // that holds them where they are hidden.
    const above = (await navigation.isVisible()) ? navigation : banner.getByRole("button", { name: namePattern("Menu") });
    await expect(above).toBeVisible();
    const [linkBox, aboveBox] = [await link.boundingBox(), await above.boundingBox()];
    assert.ok(linkBox && aboveBox);
    assert.ok(
      linkBox.y >= aboveBox.y + aboveBox.height - 1,
      `the notice starts ${Math.round(aboveBox.y + aboveBox.height - linkBox.y)}px above the foot of the pages' links`,
    );
  },
);

Then("focus is in the main content", async function (this: BrowserWorld) {
  const keys = journey(this.page);
  await keys.assertFocused(keys.byRole("main"), "the main landmark");
});

Then("the page doesn't scroll sideways", async function (this: BrowserWorld) {
  const overflow = await this.page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
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
// Text inside a focusable, named region that scrolls sideways, a table's
// frame (§4.5), is held to that frame instead of the viewport: it must lie
// within the frame's scroll extent, where scrolling the frame (with the arrow
// keys once it has focus) brings it into view, and the frame must lie within
// the viewport. 1.4.10 lets a data table scroll in two dimensions; the first
// rule still catches anything an inner box cuts off inside the frame.
Then("no text is clipped", async function (this: BrowserWorld) {
  const clipped = await this.page.evaluate(async () => {
    await document.fonts.ready;
    // Let layout settle after injected styles.
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));

    const problems: string[] = [];
    const describe = (el: Element) => `<${el.localName}> "${(el.textContent ?? "").trim().slice(0, 40)}"`;
    const viewportWidth = document.documentElement.clientWidth;
    const hides = (value: string) => value === "hidden" || value === "clip";
    const unseenBoxes = [...document.body.querySelectorAll("*")].filter((el) => getComputedStyle(el).clipPath === "inset(50%)");
    const seen = (el: Element) => !unseenBoxes.some((box) => box.contains(el));
    const scrollFrames = [...document.body.querySelectorAll('[role="region"][tabindex="0"]')].filter(
      (el) =>
        (el.hasAttribute("aria-labelledby") || el.hasAttribute("aria-label")) &&
        ["auto", "scroll"].includes(getComputedStyle(el).overflowX),
    );
    // The span, in viewport coordinates, a frame's content can be scrolled
    // through: from its start at a scroll of 0 to its end. In right to left a
    // scroll of 0 is the right end, and scrollLeft runs negative.
    const extent = (frame: Element) => {
      const box = frame.getBoundingClientRect();
      const rtl = getComputedStyle(frame).direction === "rtl";
      const left = rtl
        ? box.left + frame.clientLeft + frame.clientWidth - frame.scrollLeft - frame.scrollWidth
        : box.left + frame.clientLeft - frame.scrollLeft;
      return { left, right: left + frame.scrollWidth, box };
    };

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
      const frame = scrollFrames.find((frame) => frame.contains(parent));
      const span = frame === undefined ? undefined : extent(frame);
      if (span !== undefined && (span.box.left < -0.5 || span.box.right > viewportWidth + 0.5)) {
        problems.push(`the frame around ${describe(parent)} reaches beyond the ${viewportWidth}px viewport`);
        continue;
      }
      const [from, to] = span === undefined ? [0, viewportWidth] : [span.left, span.right];
      if (getComputedStyle(parent).display === "none") continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      for (const rect of range.getClientRects()) {
        if (rect.width === 0) continue;
        if (rect.left < from - 0.5 || rect.right > to + 0.5) {
          problems.push(
            `${describe(parent)} reaches ${Math.round(rect.left)}..${Math.round(rect.right)}px of ${frame === undefined ? `a ${viewportWidth}px viewport` : `its frame's ${Math.round(from)}..${Math.round(to)}px`}`,
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

// The tab shows the icon outside the page, where Playwright can't look, so
// the step asks the daemon for each icon the page links, then has the page
// load it as an image, under its content security policy.
Then("the page names an icon for the browser tab, served as an image the browser keeps", async function (this: BrowserWorld) {
  const icons = await this.page
    .locator('link[rel~="icon"]')
    .evaluateAll((links) => links.map((link) => (link as HTMLLinkElement).href));
  assert.notEqual(icons.length, 0, "the page links no icon");
  for (const icon of icons) {
    const response = await this.page.request.get(icon);
    assert.equal(response.status(), 200, icon);
    assert.match(response.headers()["content-type"] ?? "", /^image\//, icon);
    // Trunk names the file by its content, so the daemon lets the browser
    // keep it until a release changes it.
    assert.match(response.headers()["cache-control"] ?? "", /immutable/, icon);
    await this.page.evaluate(async (href) => {
      const image = new Image();
      image.src = href;
      await image.decode();
    }, icon);
  }
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
  await untilStepLimit(
    "the browser reports the script blocked",
    async () => this.policyBlocks.length,
    (blocks) => blocks > 0,
  );
});
