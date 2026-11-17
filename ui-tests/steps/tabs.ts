// Journey steps for the tabs (technical design §4.5, "Tabs"): the gallery's
// demo sets of tabs. Keyboard only, role-and-name locators, as everywhere in
// steps/.
//
// A tab is found by its role and its name, its own words. A tab list or a
// panel is found through its tab, never by its own name: a panel is named by
// its tab (`aria-labelledby`), and Firefox's page crashes on a role-and-name
// lookup while `aria-labelledby` is read in the page (#774). A panel is the
// element its tab's `aria-controls` names, the relation a screen reader
// follows from the tab to it.

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";
import { expect } from "playwright/test";
import type { Locator } from "playwright";

import { journey, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

function tab(world: BrowserWorld, name: string): Locator {
  return journey(world.page).byRole("tab", { name: namePattern(name) });
}

/** The tab list that holds the tab `name`. */
function listOf(world: BrowserWorld, name: string): Locator {
  return journey(world.page).byRole("tablist").filter({ has: tab(world, name) });
}

/** The element the tab's `aria-controls` names: its panel. */
async function panelOf(world: BrowserWorld, name: string): Promise<Locator> {
  const id = await tab(world, name).getAttribute("aria-controls");
  assert.ok(id, `the tab "${name}" names no panel it controls`);
  return world.page.locator(`[id="${id}"]`);
}

Then("focus is on the tab {string}", async function (this: BrowserWorld, name: string) {
  await expect(tab(this, name)).toBeFocused();
});

// The roving Tab stop (§4.5): the selected tab alone is in the Tab order,
// and every tab says whether it is the selected one.
Then(
  "the tab {string} is selected and the one Tab stop of its list",
  async function (this: BrowserWorld, name: string) {
    await expect(tab(this, name)).toHaveAttribute("aria-selected", "true");
    const tabs = await listOf(this, name).getByRole("tab").all();
    assert.ok(tabs.length > 1, `the list of the tab "${name}" holds ${tabs.length} tabs`);
    const drawn = await Promise.all(
      tabs.map(async (each) => ({
        name: ((await each.textContent()) ?? "").trim(),
        stop: await each.getAttribute("tabindex"),
        selected: await each.getAttribute("aria-selected"),
      })),
    );
    for (const each of drawn) {
      const own = namePattern(name).test(each.name);
      assert.equal(each.stop, own ? "0" : "-1", `the tabindex of the tab "${each.name}"`);
      assert.equal(each.selected, own ? "true" : "false", `the aria-selected of the tab "${each.name}"`);
    }
  },
);

// Every panel is drawn once and stays (§4.5): what a reader typed or
// scrolled to in one is there when it shows again. The others are hidden.
Then(
  "the tab {string} shows its panel, and every other panel of its list is drawn but hidden",
  async function (this: BrowserWorld, name: string) {
    await expect(await panelOf(this, name)).toBeVisible();
    const tabs = await listOf(this, name).getByRole("tab").all();
    for (const other of tabs) {
      const otherName = ((await other.textContent()) ?? "").trim();
      if (namePattern(name).test(otherName)) continue;
      const id = await other.getAttribute("aria-controls");
      assert.ok(id, `the tab "${otherName}" names no panel it controls`);
      const panel = this.page.locator(`[id="${id}"]`);
      await expect(panel, `the panel of the tab "${otherName}"`).toHaveCount(1);
      await expect(panel, `the panel of the tab "${otherName}"`).toBeHidden();
      await expect(panel, `the panel of the tab "${otherName}"`).toHaveAttribute("hidden", "");
    }
  },
);

// A panel nothing in which takes focus is the stop after its tab, a
// `tabpanel` named by that tab.
Then(
  "focus is on the panel of the tab {string}, which the tab names",
  async function (this: BrowserWorld, name: string) {
    const panel = await panelOf(this, name);
    await expect(panel).toBeFocused();
    await expect(panel).toHaveAttribute("role", "tabpanel");
    const id = await tab(this, name).getAttribute("id");
    assert.ok(id, `the tab "${name}" has no id for its panel to name`);
    await expect(panel).toHaveAttribute("aria-labelledby", id);
  },
);

Then("the panel of the tab {string} is no Tab stop of its own", async function (this: BrowserWorld, name: string) {
  const panel = await panelOf(this, name);
  await expect(panel).toBeVisible();
  await expect(panel).not.toHaveAttribute("tabindex");
});

Then(
  "the tab {string} stands to the left of the tab {string}",
  async function (this: BrowserWorld, left: string, right: string) {
    const [a, b] = [await tab(this, left).boundingBox(), await tab(this, right).boundingBox()];
    assert.ok(a && b, "a tab is not drawn");
    assert.ok(a.x + a.width <= b.x + 0.5, `the tab "${left}" ends at ${a.x + a.width}, past the start of "${right}" at ${b.x}`);
  },
);

/** A tab's weight, its words' color and its bar, beside the colors they are
 *  measured against: the page's text, a token or the system's text color
 *  (read through a probe), and the list's rule. */
async function drawnTab(world: BrowserWorld, name: string, token: string) {
  const list = await listOf(world, name).elementHandle();
  assert.ok(list, `no tab list holds the tab "${name}"`);
  return tab(world, name).evaluate(
    (element, { list, token }) => {
      const probe = document.createElement("span");
      document.body.append(probe);
      probe.style.color = token;
      const wanted = getComputedStyle(probe).color;
      probe.style.color = "";
      const text = getComputedStyle(probe).color;
      probe.remove();
      const style = getComputedStyle(element);
      const listStyle = getComputedStyle(list as Element);
      const box = element.getBoundingClientRect();
      const listBox = (list as Element).getBoundingClientRect();
      return {
        wanted,
        text,
        page: getComputedStyle(document.body).backgroundColor,
        weight: style.fontWeight,
        color: style.color,
        bar: { width: style.borderBottomWidth, color: style.borderBottomColor, style: style.borderBottomStyle },
        rule: { width: listStyle.borderBottomWidth, color: listStyle.borderBottomColor, style: listStyle.borderBottomStyle },
        bottom: box.bottom,
        listBottom: listBox.bottom,
      };
    },
    { list, token },
  );
}

// The selected tab is told by its weight and by a shape, never by color
// alone (1.4.1, The Weight Not Color Rule): bold, with a bar that lies on
// the list's rule, its bottom at the rule's.
for (const [what, token] of [
  ["--accent", "var(--accent)"],
  ["the system's text color", "CanvasText"],
] as const) {
  Then(
    `the tab {string} is bold, with a {int} pixel bar in ${what} on the rule under its list`,
    async function (this: BrowserWorld, name: string, width: number) {
      const drawn = await drawnTab(this, name, token);
      assert.equal(drawn.weight, "700", `the weight of the tab "${name}"`);
      assert.equal(drawn.bar.style, "solid", `the bar under the tab "${name}"`);
      assert.equal(drawn.bar.width, `${width}px`, `the bar under the tab "${name}"`);
      assert.equal(drawn.bar.color, drawn.wanted, `the bar under the tab "${name}" is not in ${what}`);
      assert.ok(
        Math.abs(drawn.bottom - drawn.listBottom) <= 0.5,
        `the bar under the tab "${name}" ends at ${drawn.bottom}, the list's rule at ${drawn.listBottom}`,
      );
    },
  );
}

// A tab that isn't selected stands on its own stretch of the rule, which no
// fill or bar of its own paints over, so the rule shows under every tab
// whatever line it is on, in the page's colors and the reader's alike.
// Under forced colors the text's color is the system's: WebKit's emulation
// only matches the media query and leaves the page's own text color, so the
// words are measured against the system color itself.
for (const [what, token, forced] of [
  ["--muted", "var(--muted)", false],
  ["the system's text color", "CanvasText", true],
] as const) {
  Then(
    `the tab {string} is in the text's weight and color, with no bar, on a {int} pixel rule in ${what}`,
    async function (this: BrowserWorld, name: string, width: number) {
      const drawn = await drawnTab(this, name, token);
      assert.equal(drawn.weight, "400", `the weight of the tab "${name}"`);
      const text = forced ? drawn.wanted : drawn.text;
      assert.equal(drawn.color, text, `the words of the tab "${name}" are not in the text's color`);
      assert.equal(drawn.bar.style, "solid", `the rule under the tab "${name}"`);
      assert.equal(drawn.bar.width, `${width}px`, `the rule under the tab "${name}"`);
      assert.equal(drawn.bar.color, drawn.wanted, `the rule under the tab "${name}" is not in ${what}`);
    },
  );

  Then(
    `the tab list of the tab {string} stands on a {int} pixel rule in ${what}`,
    async function (this: BrowserWorld, name: string, width: number) {
      const drawn = await drawnTab(this, name, token);
      assert.equal(drawn.rule.style, "solid", "the list's rule");
      assert.equal(drawn.rule.width, `${width}px`, "the list's rule");
      assert.equal(drawn.rule.color, drawn.wanted, `the list's rule is not in ${what}`);
    },
  );
}

// The words that say how a set of tabs is used are tied to its list
// (`aria-describedby`), so a screen reader says them on entering it. Read
// through the relation, as panelOf does (#774).
Then(
  "the tab list of the tab {string} is described by {string}",
  async function (this: BrowserWorld, name: string, words: string) {
    const id = await listOf(this, name).getAttribute("aria-describedby");
    assert.ok(id, `the tab list of the tab "${name}" names nothing that describes it`);
    await expect(this.page.locator(`[id="${id}"]`)).toHaveText(words);
  },
);

// What the gallery's demo shows of the state a component tells its caller.
// "says", not "shows the paragraph": the line isolates the tab's name in
// bidi marks, which an exact text match can't skip.
Then("the demo says {string}", async function (this: BrowserWorld, words: string) {
  await expect(this.page.getByText(namePattern(words))).toBeVisible();
});

Then(
  "the tab {string} stands on a line above the last of its list",
  async function (this: BrowserWorld, name: string) {
    const found = await listOf(this, name).evaluate(
      (element, name) =>
        Array.from(element.querySelectorAll('[role="tab"]'), (each) => ({
          name: (each.textContent ?? "").trim(),
          bottom: each.getBoundingClientRect().bottom,
        })).filter((each, _, all) => each.name === name || each === all.at(-1)),
      name,
    );
    const [own, last] = [found.find((each) => each.name === name), found.at(-1)];
    assert.ok(own && last, `the list holds no tab "${name}"`);
    assert.ok(own.bottom < last.bottom - 1, `the tab "${name}" ends at ${own.bottom}, on the last line (${last.bottom})`);
  },
);

// The rules under tabs side by side join into one unbroken line: no gap
// between neighbours on a line, in either reading direction.
Then(
  "the tabs of the list of the tab {string} touch their neighbours on each line",
  async function (this: BrowserWorld, name: string) {
    const boxes = await listOf(this, name).evaluate((element) =>
      Array.from(element.querySelectorAll('[role="tab"]'), (each) => {
        const box = each.getBoundingClientRect();
        return { name: (each.textContent ?? "").trim(), top: box.top, left: box.left, right: box.right };
      }),
    );
    const pairs = boxes.slice(1).flatMap((b, i) => {
      const a = boxes[i];
      return a && Math.abs(a.top - b.top) <= 0.5 ? [[a, b] as const] : [];
    });
    assert.ok(pairs.length > 0, `no two tabs of the list of the tab "${name}" share a line`);
    for (const [a, b] of pairs) {
      const gap = Math.min(Math.abs(a.right - b.left), Math.abs(b.right - a.left));
      assert.ok(gap <= 0.5, `the tabs "${a.name}" and "${b.name}" stand ${gap}px apart`);
    }
  },
);

// On a wrapped row the list's rule is under the last line only, so each
// tab carries its own stretch of it: a tab selected on an earlier line
// stands on its bar, and the tabs beside it on the rule.
Then(
  "every tab of the list of the tab {string} stands on its own rule: the selected one on a {int} pixel bar in --accent, the others on a {int} pixel rule in --muted",
  async function (this: BrowserWorld, name: string, barWidth: number, ruleWidth: number) {
    const tabs = await listOf(this, name).evaluate((element) => {
      const colorOf = (token: string): string => {
        const probe = document.createElement("span");
        document.body.append(probe);
        probe.style.color = token;
        const color = getComputedStyle(probe).color;
        probe.remove();
        return color;
      };
      const [accent, muted] = [colorOf("var(--accent)"), colorOf("var(--muted)")];
      return Array.from(element.querySelectorAll('[role="tab"]'), (each) => {
        const style = getComputedStyle(each);
        return {
          name: (each.textContent ?? "").trim(),
          selected: each.getAttribute("aria-selected") === "true",
          edge: { width: style.borderBottomWidth, color: style.borderBottomColor, style: style.borderBottomStyle },
          wanted: each.getAttribute("aria-selected") === "true" ? accent : muted,
        };
      });
    });
    assert.equal(tabs.filter((each) => each.selected).length, 1, "one tab of the list is selected");
    for (const each of tabs) {
      const [what, width] = each.selected ? ["bar", barWidth] : ["rule", ruleWidth];
      assert.deepEqual(
        each.edge,
        { width: `${width}px`, color: each.wanted, style: "solid" },
        `the ${what} under the tab "${each.name}"`,
      );
    }
  },
);

// The 320 Rule (1.4.10): the tabs wrap onto more lines rather than run past
// the window or scroll sideways, each tab whole, and the list's rule stays
// under its last line.
Then(
  "every tab of the list of the tab {string} shows whole inside the window, on more than one line, above the list's rule",
  async function (this: BrowserWorld, name: string) {
    const list = listOf(this, name);
    const found = await list.evaluate((element) => {
      const listBox = element.getBoundingClientRect();
      return {
        width: document.documentElement.clientWidth,
        listBottom: listBox.bottom,
        tabs: Array.from(element.querySelectorAll('[role="tab"]'), (each) => {
          const box = each.getBoundingClientRect();
          return {
            name: (each.textContent ?? "").trim(),
            left: box.left,
            right: box.right,
            top: box.top,
            bottom: box.bottom,
            clipped: each.scrollWidth > each.clientWidth + 1 || each.scrollHeight > each.clientHeight + 1,
          };
        }),
      };
    });
    assert.ok(found.tabs.length > 1, `the list of the tab "${name}" holds ${found.tabs.length} tabs`);
    for (const each of found.tabs) {
      assert.ok(each.left >= -0.5 && each.right <= found.width + 0.5, `the tab "${each.name}" runs past the window: ${each.left} to ${each.right} of ${found.width}`);
      assert.ok(!each.clipped, `the tab "${each.name}" is cut`);
      assert.ok(each.bottom <= found.listBottom + 0.5, `the tab "${each.name}" lies under the list's rule`);
    }
    const lines = new Set(found.tabs.map((each) => Math.round(each.top)));
    assert.ok(lines.size > 1, `the tabs stand on ${lines.size} line at 320 px with 200 % text`);
  },
);

/** Where a tab list stands beside the panel its selected tab shows, each
 *  tab's box and the direction the list's text runs. */
async function layoutOf(world: BrowserWorld, name: string) {
  const panel = await panelOf(
    world,
    ((await listOf(world, name).locator('[role="tab"][aria-selected="true"]').textContent()) ?? "").trim(),
  );
  const panelBox = await panel.boundingBox();
  assert.ok(panelBox, `the panel shown beside the list of the tab "${name}" is not drawn`);
  const found = await listOf(world, name).evaluate((element) => ({
    direction: getComputedStyle(element).direction,
    list: element.getBoundingClientRect().toJSON() as DOMRect,
    tabs: Array.from(element.querySelectorAll('[role="tab"]'), (each) => {
      const box = each.getBoundingClientRect();
      return { name: (each.textContent ?? "").trim(), top: box.top, left: box.left, right: box.right };
    }),
  }));
  return { ...found, panel: panelBox };
}

// A vertical list (§4.5) says so to a screen reader (`aria-orientation`),
// whose up and down arrows then move between its tabs, and is drawn as it
// says: its tabs one under another, the panel beside it at its end, the
// list at the panel's start in either direction.
Then(
  "the tab list of the tab {string} is vertical, a column at the start of its panel",
  async function (this: BrowserWorld, name: string) {
    await expect(listOf(this, name)).toHaveAttribute("aria-orientation", "vertical");
    const { direction, list, tabs, panel } = await layoutOf(this, name);
    for (const [above, below] of tabs.slice(0, -1).map((each, i) => [each, tabs[i + 1]] as const)) {
      assert.ok(below && below.top > above.top + 0.5, `the tab "${below?.name}" doesn't stand under "${above.name}"`);
      assert.ok(Math.abs(below.left - above.left) <= 0.5, `the tabs "${above.name}" and "${below.name}" don't share a column`);
    }
    if (direction === "rtl") {
      assert.ok(panel.x + panel.width <= list.left + 0.5, `the panel ends at ${panel.x + panel.width}, past the list's start at ${list.left}`);
    } else {
      assert.ok(panel.x >= list.right - 0.5, `the panel starts at ${panel.x}, before the list's end at ${list.right}`);
    }
    assert.ok(panel.y < list.bottom, `the panel starts at ${panel.y}, under the list's end at ${list.bottom}`);
  },
);

// A horizontal list says so too, which a list that can change layout must
// (a responsive one), and its panel stands under it.
Then(
  "the tab list of the tab {string} is horizontal, a row above its panel",
  async function (this: BrowserWorld, name: string) {
    await expect(listOf(this, name)).toHaveAttribute("aria-orientation", "horizontal");
    const { list, panel } = await layoutOf(this, name);
    assert.ok(panel.y >= list.bottom - 0.5, `the panel starts at ${panel.y}, above the list's end at ${list.bottom}`);
  },
);

/** A vertical tab's edges, beside the list's rule at its end, measured with
 *  a token or the system's text color read through a probe. */
async function drawnVerticalTab(world: BrowserWorld, name: string, token: string) {
  const list = await listOf(world, name).elementHandle();
  assert.ok(list, `no tab list holds the tab "${name}"`);
  return tab(world, name).evaluate(
    (element, { list, token }) => {
      const probe = document.createElement("span");
      document.body.append(probe);
      probe.style.color = token;
      const wanted = getComputedStyle(probe).color;
      probe.style.color = "";
      const text = getComputedStyle(probe).color;
      probe.remove();
      const edge = (style: CSSStyleDeclaration, side: string) => ({
        width: style.getPropertyValue(`border-${side}-width`),
        color: style.getPropertyValue(`border-${side}-color`),
        style: style.getPropertyValue(`border-${side}-style`),
      });
      const style = getComputedStyle(element);
      const listStyle = getComputedStyle(list as Element);
      const box = element.getBoundingClientRect();
      const listBox = (list as Element).getBoundingClientRect();
      const rtl = listStyle.direction === "rtl";
      return {
        wanted,
        text,
        weight: style.fontWeight,
        color: style.color,
        bar: edge(style, "inline-end"),
        under: edge(style, "block-end"),
        rule: edge(listStyle, "inline-end"),
        end: rtl ? box.left : box.right,
        listEnd: rtl ? listBox.left : listBox.right,
      };
    },
    { list, token },
  );
}

// In a column the selected tab's bar stands at its end, on the list's rule
// between the list and the panel, and nothing is drawn under a tab; the
// selected one is told by its weight and that shape, never color alone
// (1.4.1).
for (const [what, token, forced] of [
  ["--accent", "var(--accent)", false],
  ["--muted", "var(--muted)", false],
  ["the system's text color", "CanvasText", true],
] as const) {
  Then(
    `the tab {string} is bold, with a {int} pixel bar in ${what} on the rule beside its list`,
    async function (this: BrowserWorld, name: string, width: number) {
      const drawn = await drawnVerticalTab(this, name, token);
      assert.equal(drawn.weight, "700", `the weight of the tab "${name}"`);
      assert.deepEqual(drawn.bar, { width: `${width}px`, color: drawn.wanted, style: "solid" }, `the bar beside the tab "${name}"`);
      assert.equal(drawn.under.style, "none", `a line under the tab "${name}"`);
      assert.ok(Math.abs(drawn.end - drawn.listEnd) <= 0.5, `the bar beside the tab "${name}" stands at ${drawn.end}, the list's rule at ${drawn.listEnd}`);
    },
  );

  Then(
    `the tab {string} is in the text's weight and color, with no bar, on a {int} pixel rule in ${what} beside its list`,
    async function (this: BrowserWorld, name: string, width: number) {
      const drawn = await drawnVerticalTab(this, name, token);
      assert.equal(drawn.weight, "400", `the weight of the tab "${name}"`);
      assert.equal(drawn.color, forced ? drawn.wanted : drawn.text, `the words of the tab "${name}" are not in the text's color`);
      assert.deepEqual(drawn.bar, { width: `${width}px`, color: drawn.wanted, style: "solid" }, `the rule beside the tab "${name}"`);
      assert.equal(drawn.under.style, "none", `a line under the tab "${name}"`);
      assert.ok(Math.abs(drawn.end - drawn.listEnd) <= 0.5, `the rule beside the tab "${name}" stands at ${drawn.end}, the list's at ${drawn.listEnd}`);
    },
  );

  Then(
    `the tab list of the tab {string} stands beside a {int} pixel rule in ${what}`,
    async function (this: BrowserWorld, name: string, width: number) {
      const drawn = await drawnVerticalTab(this, name, token);
      assert.deepEqual(drawn.rule, { width: `${width}px`, color: drawn.wanted, style: "solid" }, "the list's rule");
    },
  );
}
