// Journey steps for the web UI's precompressed files (technical design
// §10.1). The page's module is the file the browser fetched with a `.wasm`
// name, as its resource timing lists it. The steps ask the daemon for it
// again with Playwright's request client, which says what the scenario says
// the browser reads and decodes the answer as a browser does, so a step can
// compare it with the module as it is.

import assert from "node:assert/strict";

import { Then, When } from "@cucumber/cucumber";
import type { APIResponse } from "playwright";

import type { BrowserWorld } from "../support/world.ts";

interface Asked {
  module: string;
  answer: APIResponse;
}

const asked = new WeakMap<BrowserWorld, Asked>();

function askedIn(world: BrowserWorld): Asked {
  const noted = asked.get(world);
  assert.ok(noted, "no step asked for the page's module");
  return noted;
}

/**
 * The address of the module the page loaded. WebKit can list the module
 * twice under one address.
 */
async function moduleOf(world: BrowserWorld): Promise<string> {
  const modules = await world.page.evaluate(() => [
    ...new Set(
      performance
        .getEntriesByType("resource")
        .map((entry) => entry.name)
        .filter((name) => new URL(name).pathname.endsWith(".wasm")),
    ),
  ]);
  assert.equal(modules.length, 1, `the page loaded ${modules.length} modules: ${modules.join(", ")}`);
  return modules[0] as string;
}

/** The module as it is: what a client that reads no coding gets. */
async function moduleAsItIs(world: BrowserWorld, module: string): Promise<Buffer> {
  const answer = await world.page.request.get(module, { headers: { "Accept-Encoding": "identity" } });
  assert.equal(answer.status(), 200, module);
  assert.equal(answer.headers()["content-encoding"], undefined, `${module} came compressed to a client that reads no coding`);
  return answer.body();
}

When("the browser asks for the page's module again, saying it reads {string}", async function (this: BrowserWorld, accepts: string) {
  const module = await moduleOf(this);
  const answer = await this.page.request.get(module, { headers: { "Accept-Encoding": accepts } });
  assert.equal(answer.status(), 200, module);
  asked.set(this, { module, answer });
});

Then("the module arrives with the content coding {string}", async function (this: BrowserWorld, coding: string) {
  const { module, answer } = askedIn(this);
  const headers = answer.headers();
  assert.equal(headers["content-encoding"] ?? "identity", coding, module);
  assert.equal(headers["content-type"], "application/wasm", module);
  const sent = Number(headers["content-length"]);
  const asItIs = (await moduleAsItIs(this, module)).length;
  if (coding === "identity") {
    assert.equal(sent, asItIs, `${module} came as it is, but its length says ${sent} bytes, not ${asItIs}`);
  } else {
    assert.ok(sent < asItIs, `${module} compressed with ${coding} is ${sent} bytes, no smaller than its ${asItIs}`);
  }
});

Then("it is the module the page runs", async function (this: BrowserWorld) {
  const { module, answer } = askedIn(this);
  const decoded = await answer.body();
  assert.ok(decoded.equals(await moduleAsItIs(this, module)), `${module} decodes to other bytes than the module as it is`);
});

Then("the answer says a cache must keep one copy for each coding a browser reads", function (this: BrowserWorld) {
  const { module, answer } = askedIn(this);
  const vary = (answer.headers().vary ?? "").split(",").map((name) => name.trim().toLowerCase());
  assert.ok(vary.includes("accept-encoding"), `${module} came with Vary: ${answer.headers().vary ?? "(none)"}`);
});
