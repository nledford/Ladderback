// Keeps Firefox's page alive while the steps and axe-core read names given
// through `aria-labelledby` (issue #774). Playwright 1.65's Firefox (build
// 1572) crashes the page's content process once an element's
// `ariaLabelledByElements` is read both in the page's main world and in
// Playwright's utility world (microsoft/playwright#43967, Mozilla bug
// 2090055). A role-and-name locator reads it in the utility world, and
// axe-core and `locator.evaluateAll` read it in the main world, so a page
// with `aria-labelledby` crashed at its accessibility scan or at the next
// lookup by name.
//
// Ladderback's pages name their tables, dialogs and sections through
// `aria-labelledby` on purpose (technical design §4.5), so the tests change,
// not the pages: in Firefox, every page a scenario opens has the property
// taken off `Element.prototype` before its scripts run. Its readers in the
// main world, axe-core and Playwright's name computation alike, then find no
// property and read the `aria-labelledby` attribute, as they did before
// browsers had the property, which names the same elements; only the utility
// world still reads the property, and one world alone doesn't crash. The web
// UI reads the attribute, never the property, so it runs as it does for a
// reader.
//
// Remove this once Playwright ships a Firefox that has the fix (build 1575
// or later, Playwright 1.66).

import type { BrowserContext } from "playwright";

import type { Engine } from "./world.ts";

/** In Firefox, takes `ariaLabelledByElements` off `Element.prototype` in
 *  every page `context` opens from now on, before the page's own scripts run;
 *  in Chromium and WebKit, does nothing. */
export async function avoidLabeledByCrash(context: BrowserContext, engine: Engine): Promise<void> {
  if (engine !== "firefox") return;
  await context.addInitScript(() => {
    Reflect.deleteProperty(Element.prototype, "ariaLabelledByElements");
  });
}
