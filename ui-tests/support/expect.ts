// Playwright's `expect` for the steps, with each assertion waiting until the
// step running now reaches its limit (patience.ts, #1104) rather than
// Playwright's own 5 s: an assertion is a wait for the page to come to show
// what it expects, and a failed one reports the value it last received.
//
// An assertion's own `timeout` still wins. The ones left are windows for
// something that must not happen, or must not leave too soon.

import { expect as playwrightExpect } from "playwright/test";

import { patienceMs } from "./patience.ts";

export const expect: typeof playwrightExpect = new Proxy(playwrightExpect, {
  apply: (_expect, _this, args: Parameters<typeof playwrightExpect>) => playwrightExpect.configure({ timeout: patienceMs() })(...args),
  get: (_expect, key) => Reflect.get(playwrightExpect.configure({ timeout: patienceMs() }), key),
});
