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
  assert.deepEqual(focusProbes([line], line, view, false), [{ x: 132, y: 100 }]);
});

// #949: "Back to the start page" at 320 pixels with 200 % text, on two lines
// of a paragraph. The center of the box around both, (143.75, 127.75), lies
// between the lines, where only the paragraph is.
test("a link wrapped onto two lines is probed at the center of each line, never between them", () => {
  const first = box(32, 84.75, 255.5, 125.75);
  const second = box(32, 129.75, 155.5, 170.75);
  const whole = box(32, 84.75, 255.5, 170.75);
  assert.deepEqual(focusProbes([first, second], whole, view, false), [
    { x: 143.75, y: 105.25 },
    { x: 93.75, y: 150.25 },
  ]);
});

test("an empty fragment, as at a line break, is not probed", () => {
  const line = box(32, 80, 232, 120);
  assert.deepEqual(focusProbes([box(232, 80, 232, 120), line], line, view, false), [{ x: 132, y: 100 }]);
});

test("an element with no fragment of its own is probed at the center of its box", () => {
  const whole = box(32, 80, 232, 120);
  assert.deepEqual(focusProbes([], whole, view, false), [{ x: 132, y: 100 }]);
});

test("an element with no area has nothing to probe", () => {
  assert.deepEqual(focusProbes([], box(32, 80, 32, 120), view, false), []);
});

// Chromium and WebKit scroll a focused element wholly into view: a line past
// the viewport's edge is probed there, where nothing is found, so it fails.
test("a whole-view engine probes a line past the viewport's edge at its own center", () => {
  const line = box(32, 236, 232, 276);
  assert.deepEqual(focusProbes([line], line, view, false), [{ x: 132, y: 256 }]);
});

// Firefox scrolls until part of the element shows: the part of each line in
// view is probed, and a line wholly out of view is not.
test("a part-view engine probes the part of each line in view and skips a line out of view", () => {
  const partly = box(32, 236, 232, 276);
  const out = box(32, 280, 132, 320);
  assert.deepEqual(focusProbes([partly, out], box(32, 236, 232, 320), view, true), [{ x: 132, y: 246 }]);
});

test("a part-view engine has nothing to probe when the element is wholly out of view", () => {
  const out = box(32, 280, 132, 320);
  assert.deepEqual(focusProbes([out], out, view, true), []);
});

test("a part-view engine clips a line on every side to the viewport", () => {
  const wide = box(-40, -10, 360, 30);
  assert.deepEqual(focusProbes([wide], wide, view, true), [{ x: 160, y: 15 }]);
});
