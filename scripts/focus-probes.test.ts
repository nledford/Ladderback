// Tests for where the keyboard helper looks for what covers a focused element
// (ui-tests/support/focus-probes.ts, §4.5). `just lint` runs them with
// `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { focusProbes, type Box } from "../ui-tests/support/focus-probes.ts";

const view = { width: 320, height: 256 };
const box = (left: number, top: number, right: number, bottom: number): Box => ({ left, top, right, bottom });

test("an element on one line is probed at its center", () => {
  const line = box(32, 80, 232, 120);
  assert.deepEqual(focusProbes([line], line, view), [{ x: 132, y: 100 }]);
});

// #949: "Back to the start page" at 320 pixels with 200 % text, on two lines
// of a paragraph. The center of the box around both, (143.75, 127.75), lies
// between the lines, where only the paragraph is.
test("a link wrapped onto two lines is probed at the center of each line, never between them", () => {
  const first = box(32, 84.75, 255.5, 125.75);
  const second = box(32, 129.75, 155.5, 170.75);
  const whole = box(32, 84.75, 255.5, 170.75);
  assert.deepEqual(focusProbes([first, second], whole, view), [
    { x: 143.75, y: 105.25 },
    { x: 93.75, y: 150.25 },
  ]);
});

test("an empty fragment, as at a line break, is not probed", () => {
  const line = box(32, 80, 232, 120);
  assert.deepEqual(focusProbes([box(232, 80, 232, 120), line], line, view), [{ x: 132, y: 100 }]);
});

test("an element with no fragment of its own is probed at the center of its box", () => {
  const whole = box(32, 80, 232, 120);
  assert.deepEqual(focusProbes([], whole, view), [{ x: 132, y: 100 }]);
});

test("an element with no area has nothing to probe", () => {
  assert.deepEqual(focusProbes([], box(32, 80, 32, 120), view), []);
});

// #1292: a table's scroll frame, 658 px tall at 320 by 256 with 200 % text,
// focused while its top was already in view, so Chromium scrolled only until
// its top edge showed. The center of its box, y = 329, lies below the
// viewport; the 256 px of it in view are what its reader sees (2.4.11).
test("an element taller than the viewport is probed at the center of its part in view", () => {
  const frame = box(0, -0.25, 320, 657.75);
  assert.deepEqual(focusProbes([frame], frame, view), [{ x: 160, y: 128 }]);
});

test("the part of each line in view is probed, and a line out of view is not", () => {
  const partly = box(32, 236, 232, 276);
  const out = box(32, 280, 132, 320);
  assert.deepEqual(focusProbes([partly, out], box(32, 236, 232, 320), view), [{ x: 132, y: 246 }]);
});

test("an element wholly out of view has nothing to probe, so the check fails", () => {
  const below = box(32, 280, 132, 320);
  assert.deepEqual(focusProbes([below], below, view), []);
  const above = box(32, -60, 132, -20);
  assert.deepEqual(focusProbes([above], above, view), []);
});

test("a line is clipped to the viewport on every side", () => {
  const wide = box(-40, -10, 360, 30);
  assert.deepEqual(focusProbes([wide], wide, view), [{ x: 160, y: 15 }]);
});
