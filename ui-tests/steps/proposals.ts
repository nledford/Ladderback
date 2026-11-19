// Steps of the "Pending changes" page (features/ui/proposals.feature): a
// proposal made through the API before the page opens, as an AI agent makes
// one, then its Review button and heading, which carry its ID, the short ID
// that confirms an approval with sign-in off, and what the open proposal
// shows. Elements are found by role and accessible name, as in every
// journey; the proposal's ID is data no catalog changes.

import assert from "node:assert/strict";
import path from "node:path";

import { Given, Then, When } from "@cucumber/cucumber";

import { expect } from "../support/expect.ts";
import { ISOLATION, journey, namePattern } from "../support/keyboard.ts";
import { patienceMs, untilStepLimit } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";
import { pressUntilFocusIsOn } from "./accessibility.ts";
import { PASSWORD, serveDemoPlans } from "./live.ts";
import { focusComesTo } from "./repositories.ts";
import { toastReads } from "./toast.ts";

// The proposal each scenario made, by its ID.
const proposals = new WeakMap<BrowserWorld, string>();

/** Sends `body` to the daemon's `route` as the page's own origin does, as
 *  any client may while sign-in is off, and gives its JSON answer. */
async function api(world: BrowserWorld, method: string, route: string, body?: unknown): Promise<unknown> {
  assert.ok(world.baseUrl, "this scenario's daemon was never started");
  const response = await fetch(new URL(route, world.baseUrl), {
    method,
    headers: {
      Origin: new URL(world.baseUrl).origin,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) {
    assert.fail(`${method} ${route} answered ${response.status}: ${await response.text()}`);
  }
  return response.json();
}

/** Proposes the one change `change`, made with no API token. */
async function propose(world: BrowserWorld, change: unknown): Promise<void> {
  const made = (await api(world, "POST", "/api/v1/proposals", {
    changes: [change],
    rationale: "Tidy the demo plans.",
  })) as { id: string };
  proposals.set(world, made.id);
}

function proposalOf(world: BrowserWorld): string {
  const id = proposals.get(world);
  assert.ok(id, "this scenario made no proposal: a Given must make one");
  return id;
}

/** The open proposal's section, named by its heading. */
function openProposal(world: BrowserWorld) {
  return world.page.getByRole("region", { name: namePattern(`Proposal ${proposalOf(world)}`) });
}

Given(
  "a demo plan {string} with a scratch repository, and a proposal to add the plan {string}",
  async function (this: BrowserWorld, plan: string, added: string) {
    await serveDemoPlans(this, [plan], "tiny");
    assert.ok(this.ownServer, "the daemon was never started");
    await propose(this, {
      kind: "add-plan",
      plan: { id: added, repository: "demo", sources: [path.join(this.ownServer.scratch, "source")] },
    });
  },
);

Given(
  "a demo plan {string} with a scratch repository, and a proposal to remove it",
  async function (this: BrowserWorld, plan: string) {
    await serveDemoPlans(this, [plan], "tiny");
    await propose(this, { kind: "remove-plan", plan });
  },
);

Given("a proposal to remove the plan {string}", async function (this: BrowserWorld, plan: string) {
  await propose(this, { kind: "remove-plan", plan });
});

When("I press Tab until focus is on the proposal's Review button", async function (this: BrowserWorld) {
  await pressUntilFocusIsOn(this, "Tab", "button", `Review proposal ${proposalOf(this)}, made without an API token`);
});

Then("focus is on the proposal's heading", async function (this: BrowserWorld) {
  const name = `Proposal ${proposalOf(this)}`;
  await focusComesTo(this, "heading", name, `the heading "${name}"`);
});

// The toast that answers a decision names the proposal by its ID, which only
// the scenario's proposal knows.
Then(
  "the toast names the proposal, then reads {string} after its icon {string}",
  async function (this: BrowserWorld, words: string, name: string) {
    await toastReads(this.page, `Proposal ${proposalOf(this)} ${words}`, name);
  },
);

// What a person reads in the field's hint and copies, read here from the
// review the page was given.
When("I type the proposal's short ID", async function (this: BrowserWorld) {
  const review = (await api(this, "GET", `/api/v1/proposals/${proposalOf(this)}/review`)) as { "short-id": string };
  await journey(this.page).type(review["short-id"]);
});

Then("the proposal says {string}", async function (this: BrowserWorld, text: string) {
  await openProposal(this).getByText(namePattern(text)).first().waitFor({ state: "visible", timeout: patienceMs() });
});

// A sentence of the page, wherever it stands, the values in it read whether
// or not bidi isolation marks surround them.
Then("the page says {string}", async function (this: BrowserWorld, text: string) {
  await this.page.getByText(namePattern(text)).first().waitFor({ state: "visible", timeout: patienceMs() });
});

// The context of the diff holds the repository's inline password (I4).
Then("the changes to the configuration file show the repository's password masked", async function (this: BrowserWorld) {
  const diff = (await openProposal(this).getByRole("code").textContent()) ?? "";
  assert.ok(diff.includes("password = "), `the diff shows no password line: ${JSON.stringify(diff)}`);
  assert.ok(diff.includes("***"), `the diff shows the password unmasked: ${JSON.stringify(diff)}`);
  const page = await this.page.evaluate(() => document.body.innerText);
  assert.ok(!page.includes(PASSWORD), "the page shows the repository's password");
});

/** The IDs of the plans the daemon lists. */
async function listedPlans(world: BrowserWorld): Promise<string[]> {
  const { plans } = (await api(world, "GET", "/api/v1/plans")) as { plans: { id: string }[] };
  return plans.map((listed) => listed.id);
}

// The daemon applies a proposal a moment after the page sends its approval.
Then("the daemon lists the plan {string}", async function (this: BrowserWorld, plan: string) {
  await untilStepLimit(
    `the daemon's plans include ${plan}`,
    () => listedPlans(this),
    (plans) => plans.includes(plan),
  );
});

Then("the daemon no longer lists the plan {string}", async function (this: BrowserWorld, plan: string) {
  await untilStepLimit(
    `the daemon's plans leave out ${plan}`,
    () => listedPlans(this),
    (plans) => !plans.includes(plan),
  );
});

// No refusal, of an approval or a rejection, is shown in the open proposal:
// no shared Alert (#929), and no `role="alert"` either.
Then("the open proposal shows no alert", async function (this: BrowserWorld) {
  await expect(openProposal(this).locator(".alert, [role~='alert']")).toHaveCount(0);
});

// Escapes what a regular expression would read as its own syntax.
const escapeRegExp = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

// A refusal shown inside the open proposal, not elsewhere on the page: an
// Error alert (#929), found by its class, as it has no role. What the
// catalog filled in may be set apart by bidi isolation marks.
Then("the open proposal shows an error alert containing {string}", async function (this: BrowserWorld, needle: string) {
  await openProposal(this)
    .locator(".alert.tone-error")
    .filter({ hasText: new RegExp([...needle].map(escapeRegExp).join(ISOLATION)) })
    .first()
    .waitFor({ state: "visible", timeout: patienceMs() });
});

// One failure, one alert (DESIGN.md): once the refusal of the last press
// shows, after its button, no alert of an earlier press is left beside it.
Then("the open proposal shows one alert, after the button {string}", async function (this: BrowserWorld, name: string) {
  const button = openProposal(this).getByRole("button", { name: namePattern(name) });
  const count = () =>
    button.evaluate((pressed) => {
      const section = pressed.closest("section");
      const alerts = [...(section?.querySelectorAll(".alert, [role~='alert']") ?? [])];
      return {
        all: alerts.length,
        after: alerts.filter((alert) => pressed.compareDocumentPosition(alert) & Node.DOCUMENT_POSITION_FOLLOWING).length,
      };
    });
  await expect.poll(async () => (await count()).after).toBeGreaterThan(0);
  assert.deepEqual(await count(), { all: 1, after: 1 }, `the alerts in the open proposal, and those after the button "${name}"`);
});

// The daemon can't read its store of proposals, as it answers when its
// database fails: the list is refused, each time the page reads it.
Given("the daemon refuses each read of the pending proposals", function (this: BrowserWorld) {
  refuseWithProposalsUnavailable(this, "GET", (pathname) => pathname === "/api/v1/proposals");
});

// The read of the open proposal's review is refused the same way.
Given("the daemon refuses the review of the proposal", function (this: BrowserWorld) {
  const review = `/api/v1/proposals/${encodeURIComponent(proposalOf(this))}/review`;
  refuseWithProposalsUnavailable(this, "GET", (pathname) => pathname === review);
});

/** Answers the page's `method` requests to a path `matches` takes as the
 *  daemon does when it can't read its store of proposals. The steps' own
 *  calls to the API don't pass through the browser, so they still reach it. */
function refuseWithProposalsUnavailable(world: BrowserWorld, method: string, matches: (pathname: string) => boolean): void {
  world.contextRoutes.push(async (context) => {
    await context.route(
      (url) => matches(url.pathname),
      async (route) => {
        if (route.request().method() !== method) return route.continue();
        await route.fulfill({
          status: 503,
          contentType: "application/json",
          body: JSON.stringify({ code: "proposals-unavailable" }),
        });
      },
    );
  });
}

// The AI agent takes its proposal back while the page shows it, so what the
// page then asks of it is refused.
When("the AI agent withdraws the proposal", async function (this: BrowserWorld) {
  await api(this, "POST", `/api/v1/proposals/${proposalOf(this)}/withdraw`, {});
});

// Focus a refused review did not take away (WCAG 2.4.3).
Then("focus stays on the proposal's Review button", async function (this: BrowserWorld) {
  const name = `Review proposal ${proposalOf(this)}, made without an API token`;
  const button = journey(this.page).byRole("button", { name: namePattern(name) });
  assert.equal(await button.count(), 1, `the button "${name}" is no longer on the page`);
  const stays = await button.evaluate((element) => element === document.activeElement);
  assert.ok(stays, `focus left the button "${name}"`);
});
