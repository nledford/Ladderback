// Steps of the Repositories page (features/ui/repositories.feature): a daemon
// with no repository in its config, typing into the focused field, and what
// the page says once an answer arrives. Elements are found by role and
// accessible name, as in every journey. The page's texts are read as a reader
// meets them: a placeable the page filled in carries bidi isolation marks
// (§4.5), which are not part of what is read.

import assert from "node:assert/strict";
import { cp, mkdir, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import { Given, Then, When } from "@cucumber/cucumber";

import { journey } from "../support/keyboard.mjs";
import { fixtureRepo, startServer } from "../support/world.mjs";

// A test, a connection or a creation takes the engine a moment, in a debug
// build more than one.
const ANSWER_PATIENCE_MS = 25_000;

const ISOLATION_MARKS = /[\u2066-\u2069]/g;

const escape = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

// A daemon of the scenario's own, its config naming no repository, and a
// scratch copy of the `demo-repo` fixture beside it. The After hook of
// live.mjs stops the daemon and removes the folder.
Given("a daemon with a scratch demo repository and no repositories", async function () {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-repos-"));
  await cp(fixtureRepo, path.join(scratch, "repo"), { recursive: true });
  const configDir = path.join(scratch, "config");
  await mkdir(configDir);
  const { child, url } = await startServer({
    LADDERBACK_CONFIG_DIR: configDir,
    LADDERBACK_DATA_DIR: path.join(scratch, "data"),
  });
  this.ownServer = { child, scratch };
  this.baseUrl = url;
});

// {repo} is the scenario's scratch copy of the demo repository; {empty} and
// {hot} are empty scratch folders, made when a step first names them.
async function expand(world, text) {
  const places = /\{(repo|empty|hot)\}/g;
  const named = [...text.matchAll(places)].map((match) => match[1]);
  if (named.length === 0) {
    return text;
  }
  const own = world.ownServer;
  assert.ok(own, "this scenario has no scratch folders: its daemon was never started");
  for (const name of named.filter((name) => name !== "repo")) {
    await mkdir(path.join(own.scratch, name), { recursive: true });
  }
  return text.replace(places, (_, name) => path.join(own.scratch, name));
}

When("I type {string}", async function (text) {
  await journey(this.page).type(await expand(this, text));
});

// Waits until the main content's text, as a reader gets it, matches `pattern`.
function mainTextMatches(world, pattern) {
  return world.page.waitForFunction(
    (source) => {
      const text = document.querySelector("main").innerText.replace(/[\u2066-\u2069]/g, "");
      return new RegExp(source).test(text);
    },
    pattern,
    { timeout: ANSWER_PATIENCE_MS },
  );
}

Then("the page shows a message containing {string}", async function (needle) {
  await mainTextMatches(this, escape(needle));
});

Then("the page shows a repository ID", async function () {
  await mainTextMatches(this, "Its ID is [0-9a-f]{64}\\b");
});

// I4: nothing a person typed into the password field comes back on the page,
// announcements included.
Then("the page doesn't show {string}", async function (text) {
  const shown = await this.page.evaluate(() => document.body.innerText);
  assert.ok(
    !shown.replace(ISOLATION_MARKS, "").includes(text),
    `the page shows ${JSON.stringify(text)}`,
  );
});

// The form's fields, whatever language the page is in.
Then("the page has {int} text boxes", async function (count) {
  const boxes = journey(this.page).byRole("textbox");
  await boxes
    .first()
    .waitFor({ timeout: 5_000 })
    .catch(() => {});
  assert.equal(await boxes.count(), count);
});

// Focus moves once the answer is in, a moment after the element exists.
async function focusComesTo(world, role, name, description) {
  const keys = journey(world.page);
  const target = keys.byRole(role, { name, exact: true });
  const deadline = Date.now() + ANSWER_PATIENCE_MS;
  for (;;) {
    try {
      return await keys.assertFocused(target, description);
    } catch (error) {
      if (Date.now() > deadline) {
        throw error;
      }
      await new Promise((done) => setTimeout(done, 100));
    }
  }
}

Then("focus is on the heading {string}", async function (name) {
  await focusComesTo(this, "heading", name, `the heading "${name}"`);
});

Then("focus is on the textbox {string}", async function (name) {
  await focusComesTo(this, "textbox", name, `the textbox "${name}"`);
});
