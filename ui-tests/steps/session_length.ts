// Steps of the session length journeys (features/security/session_length.feature):
// Stay signed in for set before the page opens, through the browser
// context's own requests, which share its cookies, as Settings sets it; and
// what the sign-in form offers. Elements are found by role and accessible
// name, as in every journey.

import assert from "node:assert/strict";

import { Given, Then } from "@cucumber/cucumber";

import { namePattern } from "../support/keyboard.ts";
import { patienceMs } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";
import { changeHeaders, ownBase } from "./api_tokens.ts";

/** The name the wire gives each choice of Stay signed in for, by its words. */
const LENGTHS: Record<string, string> = {
  "Until I close the browser": "until-browser-closes",
  "7 days": "7-days",
  "30 days": "30-days",
  "90 days": "90-days",
  "1 year": "1-year",
  "Until I sign out": "until-sign-out",
};

Given("the admin has set Stay signed in for to {string}", function (this: BrowserWorld, choice: string) {
  const length = LENGTHS[choice];
  assert.ok(length, `Stay signed in for has no choice "${choice}"`);
  const base = ownBase(this);
  this.contextRoutes.push(async (context) => {
    const answer = await context.request.put(`${base}/api/v1/settings/sign-in`, {
      data: { "session-length": length },
      headers: await changeHeaders(context, base),
    });
    assert.equal(answer.status(), 200, `Stay signed in for was not set: ${await answer.text()}`);
  });
});

// Asked once the sign-in form is there: the form is drawn with what the
// daemon said of the checkbox, so nothing comes later.
Then("the page offers no checkbox {string}", async function (this: BrowserWorld, name: string) {
  await this.page.getByRole("button", { name: "Sign in", exact: true }).waitFor({ timeout: patienceMs() });
  const boxes = this.page.getByRole("checkbox", { name: namePattern(name) });
  assert.equal(await boxes.count(), 0, `the page offers the checkbox "${name}"`);
});
