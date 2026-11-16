// Journey steps for restoring an entry of a snapshot from its page (technical
// design §10.4) and for the step-up field the restore dialog confirms with
// (§4.5): keyboard only, role-and-name locators, as everywhere in steps/.

import assert from "node:assert/strict";
import { rename } from "node:fs/promises";
import path from "node:path";

import { Then, When } from "@cucumber/cucumber";
import type { Locator } from "playwright";
import { expect } from "playwright/test";

import { journey, namePattern } from "../support/keyboard.ts";
import { type BrowserWorld, engine } from "../support/world.ts";
import { clipboardPermissions } from "./api_tokens.ts";
import { latestSnapshotOf } from "./maintenance.ts";

const PAGE_PATIENCE_MS = 10_000;

// A dry run reads the repository and the original files: as long as the
// other journeys give a read of the repository.
const READ_PATIENCE_MS = 10_000;

// Bidi isolation marks, which a reader never hears (§4.5).
const ISOLATION_MARKS = /[\u2066-\u2069]/g;

/** What each scenario pasted last. */
const pasted = new WeakMap<BrowserWorld, string>();

function dialogOf(world: BrowserWorld, name: string): Locator {
  return journey(world.page).byRole("dialog", { name: namePattern(name) });
}

/** What a reader gets from `locator`: its text without isolation marks. */
async function heard(locator: Locator): Promise<string> {
  return (await locator.innerText()).replace(ISOLATION_MARKS, "");
}

/** The ID of the snapshot the page shows, which its address ends with. */
function shownSnapshot(world: BrowserWorld): string {
  const id = new URL(world.page.url()).pathname.split("/").at(-1);
  assert.ok(id !== undefined && /^[0-9a-f]{64}$/.test(id), `the page ${world.page.url()} shows no snapshot`);
  return id;
}

// A short ID is text a person can select and copy into the field: not
// drawn, and not made unselectable.
async function showsCopyableText(world: BrowserWorld, name: string, short: string): Promise<void> {
  const dialog = dialogOf(world, name);
  await expect(dialog).toContainText(short, { timeout: PAGE_PATIENCE_MS });
  const selectable = await dialog.evaluate((el, id) => {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      if (node.textContent?.includes(id)) {
        const holder = node.parentElement;
        return holder !== null && getComputedStyle(holder).userSelect !== "none";
      }
    }
    return false;
  }, short);
  assert.ok(selectable, `the dialog "${name}" shows ${short} as text that can't be selected`);
}

Then(
  "the dialog {string} shows the snapshot's short ID as text that can be copied",
  async function (this: BrowserWorld, name: string) {
    await showsCopyableText(this, name, shownSnapshot(this).slice(0, 8));
  },
);

Then(
  "the dialog {string} shows {string} as text that can be copied",
  async function (this: BrowserWorld, name: string, short: string) {
    await showsCopyableText(this, name, short);
  },
);

Then("the clipboard holds {string}", async function (this: BrowserWorld, text: string) {
  await expect
    .poll(() => this.page.evaluate(() => navigator.clipboard.readText()), { timeout: PAGE_PATIENCE_MS })
    .toBe(text);
});

When("I type the newest snapshot's short ID of the plan {string}", async function (this: BrowserWorld, plan: string) {
  const short = (await latestSnapshotOf(this, plan)).slice(0, 8);
  await journey(this.page).type(short);
});

// What the dialog says once something it waits for, such as a dry run, has
// answered.
Then("the dialog {string} comes to say {string}", async function (this: BrowserWorld, name: string, text: string) {
  const dialog = dialogOf(this, name);
  await dialog.waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  await expect
    .poll(() => heard(dialog), {
      message: `the dialog "${name}" never said "${text}"`,
      timeout: READ_PATIENCE_MS,
    })
    .toContain(text);
});

// An alert inside the dialog, which stays open: a reader hears it where
// focus is.
Then("the dialog {string} alerts {string}", async function (this: BrowserWorld, name: string, text: string) {
  const dialog = dialogOf(this, name);
  const alert = dialog.getByRole("alert");
  await alert.first().waitFor({ state: "visible", timeout: PAGE_PATIENCE_MS });
  await expect
    .poll(async () => (await Promise.all((await alert.all()).map(heard))).join("\n"), {
      message: `the dialog "${name}" never alerted "${text}"`,
      timeout: PAGE_PATIENCE_MS,
    })
    .toContain(text);
  assert.ok(await dialog.isVisible(), `the dialog "${name}" closed`);
});

// A paste from the clipboard into the focused field, by the keyboard, as a
// password manager's or a person's paste lands.
When("I paste {string}", async function (this: BrowserWorld, text: string) {
  await this.page.context().grantPermissions(clipboardPermissions[engine]);
  await this.page.evaluate((copied) => navigator.clipboard.writeText(copied), text);
  await journey(this.page).press("ControlOrMeta+V");
  pasted.set(this, text);
});

Then("the field {string} holds what was pasted", async function (this: BrowserWorld, name: string) {
  const text = pasted.get(this);
  assert.ok(text !== undefined, "no step pasted anything");
  await expect(journey(this.page).byRole("textbox", { name: namePattern(name) })).toHaveValue(text);
});

// The scratch repository moved aside, so a dry run can't read it, and back,
// so the next one can.
function repositoryFolder(world: BrowserWorld, moved: boolean): string {
  assert.ok(world.ownServer, "this scenario's daemon was never started");
  return path.join(world.ownServer.scratch, moved ? "repo-moved-away" : "repo");
}

When("the repository is moved away", async function (this: BrowserWorld) {
  await rename(repositoryFolder(this, false), repositoryFolder(this, true));
});

When("the repository is put back", async function (this: BrowserWorld) {
  await rename(repositoryFolder(this, true), repositoryFolder(this, false));
});

Then("the dialog {string} alerts nothing", async function (this: BrowserWorld, name: string) {
  await expect(dialogOf(this, name).getByRole("alert")).toHaveCount(0);
});
