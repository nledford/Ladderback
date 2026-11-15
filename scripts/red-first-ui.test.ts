// Tests for the red-first check's verdict on a new @ui scenario
// (scripts/red-first-ui.ts). `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { countedRed, enginesFor, headChecks, outcome, type Engine, type Outcome } from "./red-first-ui.ts";

const runs = (entries: [Engine, Outcome][]): Map<Engine, Outcome> => new Map(entries);

test("a run whose every scenario failed is failed", () => {
  assert.equal(outcome(1, "1 scenario (1 failed)\n5 steps (1 failed, 4 skipped)"), "failed");
  assert.equal(outcome(1, "2 scenarios (2 failed)"), "failed");
});

test("a run whose every scenario passed, with exit status 0, is passed", () => {
  assert.equal(outcome(0, "1 scenario (1 passed)\n5 steps (5 passed)"), "passed");
  assert.equal(outcome(0, "2 scenarios (2 passed)"), "passed");
});

test("a run that matched no scenario is neither", () => {
  assert.equal(outcome(0, "0 scenarios\n0 steps"), "other");
  assert.equal(outcome(1, "0 scenarios\n0 steps"), "other");
  assert.equal(outcome(1, "Error: cannot find module"), "other");
});

test("an outline whose rows split is neither", () => {
  assert.equal(outcome(1, "2 scenarios (1 failed, 1 passed)"), "other");
});

test("a step with no definition is neither failed nor passed", () => {
  assert.equal(outcome(1, "1 scenario (1 undefined)"), "other");
});

test("the exit status must agree with the summary", () => {
  assert.equal(outcome(0, "1 scenario (1 failed)"), "other");
  assert.equal(outcome(1, "1 scenario (1 passed)"), "other");
  assert.equal(outcome(null, "1 scenario (1 failed)"), "other");
});

test("the last summary counts", () => {
  assert.equal(outcome(1, "1 scenario (1 passed)\n…\n1 scenario (1 failed)"), "failed");
});

test("a tree whose world.ts reads LADDERBACK_UI_BROWSER runs all three engines", () => {
  assert.deepEqual(enginesFor('const name = process.env.LADDERBACK_UI_BROWSER || "chromium";'), [
    "chromium",
    "firefox",
    "webkit",
  ]);
});

test("a tree that predates the switch, or has no world.ts, runs Chromium alone", () => {
  assert.deepEqual(enginesFor("browser = await chromium.launch();"), ["chromium"]);
  assert.deepEqual(enginesFor(undefined), ["chromium"]);
});

test("red in Chromium needs no check at the head", () => {
  assert.deepEqual(headChecks(runs([["chromium", "failed"], ["firefox", "failed"], ["webkit", "passed"]])), []);
});

test("red only outside Chromium is checked at the head in each engine it failed in", () => {
  assert.deepEqual(headChecks(runs([["chromium", "passed"], ["firefox", "failed"], ["webkit", "passed"]])), [
    "firefox",
  ]);
  assert.deepEqual(headChecks(runs([["chromium", "other"], ["firefox", "failed"], ["webkit", "failed"]])), [
    "firefox",
    "webkit",
  ]);
});

test("green everywhere needs no check at the head", () => {
  assert.deepEqual(headChecks(runs([["chromium", "passed"], ["firefox", "passed"], ["webkit", "other"]])), []);
});

test("red in Chromium counts", () => {
  assert.deepEqual(countedRed(runs([["chromium", "failed"], ["firefox", "passed"], ["webkit", "passed"]]), new Map()), [
    "chromium",
  ]);
  assert.deepEqual(countedRed(runs([["chromium", "failed"]]), new Map()), ["chromium"]);
});

test("red only in Firefox counts when it passes in Firefox at the head", () => {
  const atFirst = runs([["chromium", "passed"], ["firefox", "failed"], ["webkit", "passed"]]);
  assert.deepEqual(countedRed(atFirst, runs([["firefox", "passed"]])), ["firefox"]);
});

test("red only in Firefox doesn't count when it still fails there at the head", () => {
  const atFirst = runs([["chromium", "passed"], ["firefox", "failed"], ["webkit", "passed"]]);
  assert.deepEqual(countedRed(atFirst, runs([["firefox", "failed"]])), []);
  assert.deepEqual(countedRed(atFirst, runs([["firefox", "other"]])), []);
  assert.deepEqual(countedRed(atFirst, new Map()), []);
});

test("each engine's red counts on its own head result", () => {
  const atFirst = runs([["chromium", "passed"], ["firefox", "failed"], ["webkit", "failed"]]);
  assert.deepEqual(
    countedRed(
      atFirst,
      runs([
        ["firefox", "failed"],
        ["webkit", "passed"],
      ]),
    ),
    ["webkit"],
  );
});

test("a pass at the head in an engine that didn't fail at the test: commit counts nothing", () => {
  const atFirst = runs([["chromium", "passed"], ["firefox", "passed"], ["webkit", "passed"]]);
  assert.deepEqual(countedRed(atFirst, runs([["firefox", "passed"]])), []);
});
