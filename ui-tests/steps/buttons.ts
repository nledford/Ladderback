// Journey steps for the shared button on the pages (technical design §4.5,
// decision 113): how a page's buttons are drawn, and a page's button busy
// while the daemon answers its press. Role-and-name locators, as everywhere
// in steps/.

import assert from "node:assert/strict";

import { Given, Then } from "@cucumber/cucumber";
import { expect } from "playwright/test";

import { namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";

// What each look draws, read from the page's own tokens: a filled button's
// background is its tone, an outlined one's is the page's, edged in its tone.
const LOOKS: Record<string, { fill: string | null; edge: string }> = {
  "filled in Ladder Blue": { fill: "var(--accent)", edge: "var(--accent)" },
  "outlined in Ladder Blue": { fill: null, edge: "var(--accent)" },
  "outlined in the color of errors": { fill: null, edge: "var(--error)" },
  outlined: { fill: null, edge: "var(--muted)" },
};

Then("the button {string} has the look {string}", async function (this: BrowserWorld, name: string, look: string) {
  const wanted = LOOKS[look];
  assert.ok(wanted, `no look "${look}"`);
  const button = this.page.getByRole("button", { name: namePattern(name) });
  await expect(button).toHaveCount(1);
  const found = await button.evaluate((element, wanted) => {
    const colorOf = (value: string) => {
      const probe = document.createElement("span");
      probe.style.color = value;
      element.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    };
    const style = getComputedStyle(element);
    return {
      fill: style.backgroundColor,
      edge: style.borderTopColor,
      edgeStyle: style.borderTopStyle,
      wantedFill: wanted.fill === null ? "rgba(0, 0, 0, 0)" : colorOf(wanted.fill),
      wantedEdge: colorOf(wanted.edge),
    };
  }, wanted);
  assert.equal(found.edgeStyle, "solid", `the edge of "${name}"`);
  assert.equal(found.fill, found.wantedFill, `the fill of "${name}"`);
  assert.equal(found.edge, found.wantedEdge, `the edge of "${name}"`);
});

// Every button, those in closed dialogs too, has the shared button's one
// small corner (`--radius-small`), which no browser draws its own button
// with: a button in the browser's look is named in the failure.
Then(
  "every button on the page is drawn from the tokens, none in the browser's own look",
  async function (this: BrowserWorld) {
    const native = await this.page.evaluate(() => {
      const probe = document.createElement("div");
      probe.style.borderRadius = "var(--radius-small)";
      document.body.append(probe);
      const corner = getComputedStyle(probe).borderTopLeftRadius;
      probe.remove();
      return Array.from(document.querySelectorAll("button"))
        .filter((button) => getComputedStyle(button).borderTopLeftRadius !== corner)
        .map((button) => (button.getAttribute("aria-label") ?? button.textContent ?? "").trim());
    });
    assert.deepEqual(native, [], "buttons drawn in the browser's own look");
  },
);

// The daemon holds the page's next save of a plan for a while before it
// answers, and the page's saves are counted.
const savesAsked = new WeakMap<BrowserWorld, { count: number }>();

Given("the daemon answers the next save of a plan {int} seconds late", function (this: BrowserWorld, seconds: number) {
  const asked = { count: 0 };
  savesAsked.set(this, asked);
  this.contextRoutes.push(async (context) => {
    await context.route(
      (url) => url.pathname === "/api/v1/plans" || url.pathname.startsWith("/api/v1/plans/"),
      async (route) => {
        if (route.request().method() !== "GET") {
          asked.count += 1;
          if (asked.count === 1) await new Promise((done) => setTimeout(done, seconds * 1_000));
        }
        await route.continue();
      },
    );
  });
});

Then("the page asked to save a plan once", function (this: BrowserWorld) {
  const asked = savesAsked.get(this);
  assert.ok(asked, "the page's saves were never counted");
  assert.equal(asked.count, 1, "the page's saves of a plan");
});

// The first test of a repository the page asks for is answered late, so its
// buttons can be seen while it waits.
Given("the daemon answers the next test of a repository {int} seconds late", function (this: BrowserWorld, seconds: number) {
  const asked = { count: 0 };
  this.contextRoutes.push(async (context) => {
    await context.route(
      (url) => url.pathname === "/api/v1/repositories/_test",
      async (route) => {
        asked.count += 1;
        if (asked.count === 1) await new Promise((done) => setTimeout(done, seconds * 1_000));
        await route.continue();
      },
    );
  });
});

// The first approval, or rejection, of a proposal the page asks for is
// answered late, so the open proposal's buttons can be seen while it waits,
// and the page's asks are counted.
const decisionsAsked = new WeakMap<BrowserWorld, { count: number }>();

function answerProposalLate(world: BrowserWorld, decision: "approve" | "reject", seconds: number) {
  const asked = { count: 0 };
  decisionsAsked.set(world, asked);
  world.contextRoutes.push(async (context) => {
    await context.route(
      (url) => url.pathname.startsWith("/api/v1/proposals/") && url.pathname.endsWith(`/${decision}`),
      async (route) => {
        asked.count += 1;
        if (asked.count === 1) await new Promise((done) => setTimeout(done, seconds * 1_000));
        await route.continue();
      },
    );
  });
}

Given("the daemon answers the next approval of a proposal {int} seconds late", function (this: BrowserWorld, seconds: number) {
  answerProposalLate(this, "approve", seconds);
});

Given("the daemon answers the next rejection of a proposal {int} seconds late", function (this: BrowserWorld, seconds: number) {
  answerProposalLate(this, "reject", seconds);
});

Then("the page asked to approve the proposal once", function (this: BrowserWorld) {
  const asked = decisionsAsked.get(this);
  assert.ok(asked, "the page's approvals were never counted");
  assert.equal(asked.count, 1, "the page's approvals of the proposal");
});

// Unavailable for now: it says so and stays in the Tab order.
Then("the button {string} is unavailable", async function (this: BrowserWorld, name: string) {
  const button = this.page.getByRole("button", { name: namePattern(name) });
  await expect(button).toHaveCount(1);
  await expect(button).toHaveAttribute("aria-disabled", "true");
  await expect(button).not.toHaveAttribute("disabled");
});
