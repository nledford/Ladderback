// Steps of the Repositories page (features/ui/repositories.feature): a daemon
// with no repository in its config, typing into the focused field, and what
// the page says once an answer arrives. Elements are found by role and
// accessible name, as in every journey. The page's texts are read as a reader
// meets them: a placeable the page filled in carries bidi isolation marks
// (§4.5), which are not part of what is read.

import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { access, cp, mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";

import { Given, Then, When } from "@cucumber/cucumber";

import { journey, namePattern } from "../support/keyboard.ts";
import { type BrowserWorld, binary, fixtureRepo, startServer } from "../support/world.ts";
import { api, ended } from "./maintenance.ts";

// A test, a connection or a creation takes the engine a moment, in a debug
// build more than one.
const ANSWER_PATIENCE_MS = 25_000;

// How long a test of the scratch repository needs to be answered, with room.
const ANSWER_SETTLES_MS = 6_000;

const ISOLATION_MARKS = /[\u2066-\u2069]/g;

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

// A daemon of the scenario's own, its config naming no repository, and a
// scratch copy of the `demo-repo` fixture beside it. The After hook of
// live.ts stops the daemon and removes the folder.
Given("a daemon with a scratch demo repository and no repositories", async function (this: BrowserWorld) {
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

// A daemon whose config holds only what `ladderback import-profile` added
// from a rustic profile named after `alias`: its repository, at an empty
// folder of the scratch, with its password in a file, and its plan
// `demo-home`, which backs up a small source folder. The import opens no
// repository, so nothing is there until the page creates it; the folder is
// there, so a backup to it finds no repository rather than no folder.
Given(
  "a daemon with a profile imported as {string} whose repository folder is empty",
  async function (this: BrowserWorld, alias: string) {
    const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-import-"));
    await mkdir(path.join(scratch, "repo"));
    const source = path.join(scratch, "source");
    await mkdir(source);
    for (let n = 1; n <= 3; n += 1) {
      await writeFile(path.join(source, `demo-${n}.txt`), `demo file ${n}\n`);
    }
    const passwordFile = path.join(scratch, "password");
    await writeFile(passwordFile, "demo-password\n", { mode: 0o600 });
    const profile = path.join(scratch, `${alias}.toml`);
    await writeFile(
      profile,
      [
        "[repository]",
        `repository = ${JSON.stringify(path.join(scratch, "repo"))}`,
        `password-file = ${JSON.stringify(passwordFile)}`,
        "",
        "[[backup.snapshots]]",
        'name = "demo-home"',
        `sources = [${JSON.stringify(source)}]`,
        "",
      ].join("\n"),
    );
    const configDir = path.join(scratch, "config");
    const dataDir = path.join(scratch, "data");
    await mkdir(configDir);
    const env = {
      ...process.env,
      LC_ALL: "C",
      LANGUAGE: "",
      LADDERBACK_CONFIG_DIR: configDir,
      LADDERBACK_DATA_DIR: dataDir,
    };
    await promisify(execFile)(binary, ["import-profile", profile], { env });
    const { child, url } = await startServer({
      LADDERBACK_CONFIG_DIR: configDir,
      LADDERBACK_DATA_DIR: dataDir,
    });
    this.ownServer = { child, scratch };
    this.baseUrl = url;
  },
);

// A backup of `plan`, started through the daemon's API, ends failed because
// it found no repository at the location: the evidence a row's Create needs
// (decision 53).
When("a backup of {string} fails because no repository is there", async function (this: BrowserWorld, plan: string) {
  const what = `the backup of ${plan}`;
  const { status, failureCode } = await ended(this, await api(this, "POST", "/api/v1/operations", { plan }), what);
  assert.equal(status, "failed", `${what} ended as ${status}`);
  assert.equal(failureCode, "repository-not-found", `${what} failed as ${failureCode}`);
});

// Someone puts a repository at the location after the backup found none.
When("the demo repository is copied into the profile's repository folder", async function (this: BrowserWorld) {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  await cp(fixtureRepo, path.join(own.scratch, "repo"), { recursive: true });
});

// The configuration's entry is pointed at a folder that isn't there, as a
// hand edit or a second import may, after the backup found none at the old
// one: the row still offers Create, on what the log says (decision 53).
When("the profile's repository is moved to a folder that isn't there", async function (this: BrowserWorld) {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  const file = path.join(own.scratch, "config", "config.toml");
  const before = await readFile(file, "utf8");
  const after = before.replace(JSON.stringify(path.join(own.scratch, "repo")), JSON.stringify(path.join(own.scratch, "moved")));
  assert.notEqual(after, before, "the config names the profile's repository folder");
  await writeFile(file, after);
});

Then("no folder is where the profile's repository was moved to", async function (this: BrowserWorld) {
  const own = this.ownServer;
  assert.ok(own, "this scenario's daemon was never started");
  await assert.rejects(access(path.join(own.scratch, "moved")), "Create made the folder");
});

// {repo} is the scenario's scratch copy of the demo repository; {empty} and
// {hot} are empty scratch folders, made when a step first names them.
async function expand(world: BrowserWorld, text: string): Promise<string> {
  const places = /\{(repo|empty|hot)\}/g;
  const named = [...text.matchAll(places)].map((match) => match[1] ?? "");
  if (named.length === 0) {
    return text;
  }
  const own = world.ownServer;
  assert.ok(own, "this scenario has no scratch folders: its daemon was never started");
  for (const name of named.filter((name) => name !== "repo")) {
    await mkdir(path.join(own.scratch, name), { recursive: true });
  }
  return text.replace(places, (_, name: string) => path.join(own.scratch, name));
}

When("I type {string}", async function (this: BrowserWorld, text: string) {
  await journey(this.page).type(await expand(this, text));
});

// Waits until the main content's text, as a reader gets it, matches `pattern`.
function mainTextMatches(world: BrowserWorld, pattern: string) {
  return world.page.waitForFunction(
    (source) => {
      const main = document.querySelector("main");
      if (main === null) throw new Error("the page has no main content");
      const text = main.innerText.replace(/[\u2066-\u2069]/g, "");
      return new RegExp(source).test(text);
    },
    pattern,
    { timeout: ANSWER_PATIENCE_MS },
  );
}

Then("the page shows a message containing {string}", async function (this: BrowserWorld, needle: string) {
  await mainTextMatches(this, escape(needle));
});

Then("the page shows a repository ID", async function (this: BrowserWorld) {
  await mainTextMatches(this, "Its ID is [0-9a-f]{64}\\b");
});

// I4: nothing a person typed into the password field comes back on the page,
// announcements included.
Then("the page doesn't show {string}", async function (this: BrowserWorld, text: string) {
  const shown = await this.page.evaluate(() => document.body.innerText);
  assert.ok(
    !shown.replace(ISOLATION_MARKS, "").includes(text),
    `the page shows ${JSON.stringify(text)}`,
  );
});

// The form's fields, whatever language the page is in.
Then("the page has {int} text boxes", async function (this: BrowserWorld, count: number) {
  const boxes = journey(this.page).byRole("textbox");
  await boxes
    .first()
    .waitFor({ timeout: 5_000 })
    .catch(() => {});
  assert.equal(await boxes.count(), count);
});

// Focus moves once the answer is in, a moment after the element exists.
async function focusComesTo(world: BrowserWorld, role: string, name: string, description: string): Promise<void> {
  const keys = journey(world.page);
  const target = keys.byRole(role, { name: namePattern(name) });
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

// A page that is left while a request is on its way gets the answer after it
// is gone. The engine takes a moment, so the step waits long enough for it.
Then("the page raises no script error while the daemon answers", async function (this: BrowserWorld) {
  await new Promise((done) => setTimeout(done, ANSWER_SETTLES_MS));
  assert.deepEqual(this.scriptErrors, [], "the page's scripts threw");
});

Then("focus is on the heading {string}", async function (this: BrowserWorld, name: string) {
  await focusComesTo(this, "heading", name, `the heading "${name}"`);
});

Then("focus is on the textbox {string}", async function (this: BrowserWorld, name: string) {
  await focusComesTo(this, "textbox", name, `the textbox "${name}"`);
});

// A sentence the page moved focus to, as it does once a row's button it
// answered has gone: the focused element is not the page itself and its
// text, as a reader gets it, is `message` and nothing more, so what follows
// it, such as an ID, is not read with it.
Then("focus is on the message {string}", async function (this: BrowserWorld, message: string) {
  await this.page
    .waitForFunction(
      ({ message, marks }) => {
        const focused = document.activeElement;
        if (!(focused instanceof HTMLElement) || focused === document.body) return false;
        return focused.innerText.replace(new RegExp(marks, "g"), "").trim() === message;
      },
      { message, marks: ISOLATION_MARKS.source },
      { timeout: ANSWER_PATIENCE_MS },
    )
    .catch(async () => {
      const focused = await this.page.evaluate(() => document.activeElement?.outerHTML ?? "nothing");
      assert.fail(`focus is not on the message ${JSON.stringify(message)}: it is on ${focused}`);
    });
});

// A control the page takes away once the list is read again.
Then("the button {string} leaves the page", async function (this: BrowserWorld, name: string) {
  await journey(this.page)
    .byRole("button", { name: namePattern(name) })
    .waitFor({ state: "detached", timeout: ANSWER_PATIENCE_MS });
});

// How long the list of repositories takes to answer once it is slow: long
// enough for a reader to press a button again while it is on its way.
const SLOW_LIST_MS = 3_000;

// The row Creates the page asked the daemon for, by alias, from the moment the
// list of repositories was made slow.
const createsAsked = new WeakMap<BrowserWorld, Map<string, number>>();

// Every read of the list of repositories from now on answers late, so the
// page shows what a Create came to a while before it reads the list again.
// The page's requests to create a row's repository are counted from now on.
When("the list of repositories is slow to answer from now on", async function (this: BrowserWorld) {
  const asked = new Map<string, number>();
  createsAsked.set(this, asked);
  this.page.on("request", (request) => {
    const created = /\/api\/v1\/repositories\/([^/]+)\/init$/.exec(new URL(request.url()).pathname);
    if (request.method() === "POST" && created?.[1] !== undefined) {
      asked.set(created[1], (asked.get(created[1]) ?? 0) + 1);
    }
  });
  await this.page.route(
    (url) => url.pathname === "/api/v1/repositories",
    async (route) => {
      if (route.request().method() === "GET") {
        await new Promise((done) => setTimeout(done, SLOW_LIST_MS));
      }
      await route.continue();
    },
  );
});

Then("the page asked to create {string} once", async function (this: BrowserWorld, alias: string) {
  const asked = createsAsked.get(this);
  assert.ok(asked, "the page's requests were never counted");
  assert.equal(asked.get(alias) ?? 0, 1, `the requests to create "${alias}"`);
});

// The roles of the things a person sees as a button, a column's header and a
// row's header.
const ROLE_OF: Record<string, string> = {
  button: "button",
  "column header": "columnheader",
  "row header": "rowheader",
};

// A short name, a column's or a row's, or a button's, is not split across
// lines: each line its text takes has a box of its own, so one line is one
// distinct top.
Then(
  /^the (button|column header|row header) "([^"]*)" sits on one line$/,
  async function (this: BrowserWorld, kind: string, name: string) {
    const role = ROLE_OF[kind];
    assert.ok(role !== undefined, `no role for ${kind}`);
    const target = journey(this.page).byRole(role, { name: namePattern(name) });
    assert.equal(await target.count(), 1, `the ${kind} "${name}" is not on the page once`);
    const lines = await target.evaluate((element) => {
      const range = document.createRange();
      range.selectNodeContents(element);
      const tops = [...range.getClientRects()].filter((box) => box.width > 0).map((box) => Math.round(box.top));
      return new Set(tops).size;
    });
    assert.equal(lines, 1, `the ${kind} "${name}" takes ${lines} lines`);
  },
);

// A name or a label may wrap on a small screen, but only where a reader
// expects it: after a space or a hyphen, never inside a word ("Crea/te",
// "de/mo"), unless the word is longer than a whole line, and then nowhere
// else. Each character's box says which line it is on; a character that
// starts a new line must follow a space or a hyphen, or be in a word wider
// than the element's lines.
Then(
  /^the (button|column header|row header) "([^"]*)" breaks (only at a space or a hyphen|a word only where the word is longer than a line)$/,
  async function (this: BrowserWorld, kind: string, name: string, where: string) {
    const role = ROLE_OF[kind];
    assert.ok(role !== undefined, `no role for ${kind}`);
    const target = journey(this.page).byRole(role, { name: namePattern(name) });
    assert.equal(await target.count(), 1, `the ${kind} "${name}" is not on the page once`);
    const longWordsMayBreak = where !== "only at a space or a hyphen";
    const inside = await target.evaluate((element, longWordsMayBreak) => {
      const style = getComputedStyle(element);
      const line = element.clientWidth - parseFloat(style.paddingInlineStart) - parseFloat(style.paddingInlineEnd);
      // Each word, cut at a space and after a hyphen, as the characters
      // with a box that it holds, in order.
      let word: { char: string; box: DOMRect }[] = [];
      const words = [word];
      const next = () => {
        word = [];
        words.push(word);
      };
      const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const text = node.textContent ?? "";
        for (let at = 0; at < text.length; at += 1) {
          const char = text.charAt(at);
          if (/\s/.test(char)) {
            next();
            continue;
          }
          const range = document.createRange();
          range.setStart(node, at);
          range.setEnd(node, at + 1);
          const box = [...range.getClientRects()].find((rect) => rect.width > 0);
          // A mark of no width, such as a bidi isolation mark, is on no line.
          if (box === undefined) continue;
          word.push({ char, box });
          if (char === "-") next();
        }
      }
      const found: string[] = [];
      for (const letters of words) {
        const wide = letters.reduce((sum, { box }) => sum + box.width, 0);
        if (longWordsMayBreak && wide > line) continue;
        letters.forEach(({ box }, at) => {
          const before = letters[at - 1];
          if (before !== undefined && Math.round(box.top) > Math.round(before.box.top)) {
            const text = letters.map(({ char }) => char).join("");
            found.push(`"${text}" (${Math.round(wide)}px on a ${Math.round(line)}px line) at ${at}`);
          }
        });
      }
      return found;
    }, longWordsMayBreak);
    assert.deepEqual(inside, [], `the ${kind} "${name}" breaks inside a word`);
  },
);
