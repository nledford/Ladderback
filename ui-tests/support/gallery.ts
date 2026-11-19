// What the steps over the gallery's demos share (technical design §4.5): a
// demo's section found by its heading, the items a locator finds, a status
// tone's pair as a part draws it, and what makes an element a live region.

import assert from "node:assert/strict";

import type { Locator, Page } from "playwright";

import { expect } from "./expect.ts";

/** What makes an element a live region, by role or by attribute. */
export const LIVE_REGION = "[aria-live], [role~='status'], [role~='alert'], [role~='log'], [role~='marquee'], [role~='timer'], output";

/** The gallery section of a component, found by its heading, not by a test id. */
export function demoSection(page: Page, component: string): Locator {
  return page.locator("section", {
    has: page.getByRole("heading", { level: 2, name: component, exact: true }),
  });
}

/** The `count` items a locator finds, each its own locator, found as `expect` finds them. */
export async function eachOf(items: Locator, count: number): Promise<Locator[]> {
  await expect(items).toHaveCount(count);
  return items.all();
}

// A status tone's pair (technical design §4.5, the shared vocabulary): a part
// tinted in it draws its words in the pair's words token on its fill token.
// Each token is read through a probe, as the page resolves it, with a
// fallback no token has, so a token style.css lacks fails here rather than
// matching whatever color the probe inherits. The part is read alone with
// `evaluate`, as `expect` finds it.
export async function drawnOnItsFill(part: Locator, name: string, words: string, fill: string) {
  await expect(part).toHaveCount(1);
  const missing = "rgb(1, 2, 3)";
  const drawn = await part.evaluate(
    (element, { tokens, missing }) => {
      const probe = document.createElement("span");
      document.body.append(probe);
      const resolve = (token: string) => {
        probe.style.color = `var(${token}, ${missing})`;
        return getComputedStyle(probe).color;
      };
      const expected = { words: resolve(tokens.words), fill: resolve(tokens.fill) };
      probe.remove();
      const style = getComputedStyle(element);
      return { expected, found: { words: style.color, fill: style.backgroundColor } };
    },
    { tokens: { words, fill }, missing },
  );
  assert.notEqual(drawn.expected.words, missing, `${words} is not defined`);
  assert.notEqual(drawn.expected.fill, missing, `${fill} is not defined`);
  assert.deepEqual(drawn.found, drawn.expected, name);
}

/** Text that is the whole of an element's, never a part of it. */
export function wholly(text: string): RegExp {
  return new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`);
}
