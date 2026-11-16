// Steps of the "Pending changes" page (features/ui/proposals.feature): a
// proposal made through the API before the page opens, as an AI agent makes
// one, then its Review button and heading, which carry its ID, the short ID
// that confirms an approval with sign-in off, and what the open proposal
// shows. Elements are found by role and accessible name, as in every
// journey; the proposal's ID is data no catalog changes.

import assert from "node:assert/strict";
import path from "node:path";

import { Given, Then, When } from "@cucumber/cucumber";

import { journey, namePattern } from "../support/keyboard.ts";
import type { BrowserWorld } from "../support/world.ts";
import { pressUntilFocusIsOn } from "./accessibility.ts";
import { PASSWORD, serveDemoPlans, until } from "./live.ts";
import { focusComesTo } from "./repositories.ts";

// The page reads the proposal and the daemon applies it in well under this.
const ANSWER_PATIENCE_MS = 10_000;

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

When("I press Tab until focus is on the proposal's Review button", async function (this: BrowserWorld) {
  await pressUntilFocusIsOn(this, "Tab", "button", `Review proposal ${proposalOf(this)}, made without an API token`);
});

Then("focus is on the proposal's heading", async function (this: BrowserWorld) {
  const name = `Proposal ${proposalOf(this)}`;
  await focusComesTo(this, "heading", name, `the heading "${name}"`);
});

// What a person reads in the field's hint and copies, read here from the
// review the page was given.
When("I type the proposal's short ID", async function (this: BrowserWorld) {
  const review = (await api(this, "GET", `/api/v1/proposals/${proposalOf(this)}/review`)) as { "short-id": string };
  await journey(this.page).type(review["short-id"]);
});

Then("the proposal says {string}", async function (this: BrowserWorld, text: string) {
  await openProposal(this)
    .getByText(namePattern(text))
    .first()
    .waitFor({ state: "visible", timeout: ANSWER_PATIENCE_MS });
});

// A sentence of the page, wherever it stands, the values in it read whether
// or not bidi isolation marks surround them.
Then("the page says {string}", async function (this: BrowserWorld, text: string) {
  await this.page
    .getByText(namePattern(text))
    .first()
    .waitFor({ state: "visible", timeout: ANSWER_PATIENCE_MS });
});

// The context of the diff holds the repository's inline password (I4).
Then("the changes to the configuration file show the repository's password masked", async function (this: BrowserWorld) {
  const diff = (await openProposal(this).getByRole("code").textContent()) ?? "";
  assert.ok(diff.includes("password = "), `the diff shows no password line: ${JSON.stringify(diff)}`);
  assert.ok(diff.includes("***"), `the diff shows the password unmasked: ${JSON.stringify(diff)}`);
  const page = await this.page.evaluate(() => document.body.innerText);
  assert.ok(!page.includes(PASSWORD), "the page shows the repository's password");
});

Then("the daemon lists the plan {string}", async function (this: BrowserWorld, plan: string) {
  await until(
    async () => {
      const { plans } = (await api(this, "GET", "/api/v1/plans")) as { plans: { id: string }[] };
      return plans.some((listed) => listed.id === plan) ? true : null;
    },
    ANSWER_PATIENCE_MS,
    `the daemon never listed the plan ${plan}`,
  );
});
