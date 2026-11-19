// Tests for how long an `@ui` step waits for something to appear
// (ui-tests/support/patience.ts, #1104): until its own limit, less the moment
// it needs to report. `just lint` runs them with `node --test`, on a mocked
// clock, so a wait of thirty seconds takes none.

import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { type TestContext, test } from "node:test";
import { setImmediate as settle } from "node:timers/promises";
import {
  patienceMs,
  REPORT_MS,
  Signal,
  STEP_LIMIT_MS,
  startStep,
  stepLimit,
  timed,
  untilSettled,
  untilStepLimit,
} from "../ui-tests/support/patience.ts";

/** The clock stopped at 0: only `advance` moves it. */
function stopClock(t: TestContext): void {
  t.mock.timers.enable({ apis: ["setTimeout", "Date"], now: 0 });
}

/** Moves the stopped clock on by `ms`, 10 ms at a time, letting each
 *  moment's timers and promises run. */
async function advance(t: TestContext, ms: number): Promise<void> {
  for (let left = ms; left > 0; left -= 10) {
    t.mock.timers.tick(Math.min(10, left));
    await settle();
  }
}

/** When `waited` settled, by the stopped clock, and whether it failed. */
function watch(waited: Promise<unknown>): { at: number; failed: unknown } {
  const outcome: { at: number; failed: unknown } = { at: -1, failed: undefined };
  waited.then(
    () => {
      outcome.at = Date.now();
    },
    (error: unknown) => {
      outcome.at = Date.now();
      outcome.failed = error;
    },
  );
  return outcome;
}

test("a wait outlasts the deadlines steps gave up at, and returns what it read", async (t) => {
  stopClock(t);
  startStep(STEP_LIMIT_MS);
  let text = "";
  const waited = untilStepLimit(
    'the polite region comes to contain "Saved"',
    async () => text,
    (read) => read.includes("Saved"),
  );
  const outcome = watch(waited);
  // Past the 5 and 10 s the steps had of their own.
  await advance(t, 12_000);
  assert.equal(outcome.at, -1, "the wait ended before the page said it");
  text = "Saved the plan";
  await advance(t, 200);
  assert.equal(await waited, "Saved the plan");
});

test("a wait that never sees what it waits for fails at the step's limit, with the last text read", async (t) => {
  stopClock(t);
  startStep(STEP_LIMIT_MS);
  const outcome = watch(
    untilStepLimit(
      'the polite region comes to contain "Saved"',
      async () => "Saving the plan",
      (read) => read.includes("Saved"),
    ),
  );
  await advance(t, STEP_LIMIT_MS);
  assert.ok(outcome.at >= STEP_LIMIT_MS - REPORT_MS, `the wait gave up at ${outcome.at} ms`);
  assert.ok(outcome.at < STEP_LIMIT_MS, `the wait outlasted its step, to ${outcome.at} ms`);
  assert.ok(outcome.failed instanceof Error);
  assert.equal(outcome.failed.message, 'the polite region comes to contain "Saved" (last read "Saving the plan")');
});

test("a wait that read nothing says so", async (t) => {
  stopClock(t);
  startStep(STEP_LIMIT_MS);
  const outcome = watch(
    untilStepLimit(
      "the row demo-docs",
      async () => null,
      (read) => read !== null,
    ),
  );
  await advance(t, STEP_LIMIT_MS);
  assert.ok(outcome.failed instanceof Error);
  assert.equal(outcome.failed.message, "the row demo-docs (last read nothing)");
});

// Playwright reads a timeout of 0 as no limit at all, which would leave the
// step to cucumber's bare timeout.
test("a wait in a step with no time left still has a moment, never no limit", (t) => {
  stopClock(t);
  startStep(REPORT_MS - 500);
  assert.equal(patienceMs(), 1);
});

test("a step's own limit reaches the waits in it", (t) => {
  stopClock(t);
  const options = stepLimit(130_000);
  assert.equal(options.timeout, 130_000, "cucumber's limit on the step");
  const step = timed(function (this: unknown, plan: string) {
    return [this, plan, patienceMs()];
  }, options.wrapperOptions);
  const world = { name: "the world" };
  assert.deepEqual(step.call(world, "demo-docs"), [world, "demo-docs", 130_000 - REPORT_MS]);
});

test("a step with no limit of its own waits up to cucumber's default", (t) => {
  stopClock(t);
  startStep(130_000);
  const step = timed(() => patienceMs());
  assert.equal(step.call(undefined), STEP_LIMIT_MS - REPORT_MS);
});

test("a wait on a signal ends the moment it is told, not at a poll", async (t) => {
  stopClock(t);
  startStep(STEP_LIMIT_MS);
  const signal = new Signal();
  let reads = 0;
  const outcome = watch(
    signal.until(
      () => reads === 2,
      () => `the page read ${reads} times`,
    ),
  );
  await advance(t, 7_000);
  reads = 1;
  signal.notify();
  await settle();
  assert.equal(outcome.at, -1, "the wait ended before its condition held");
  await advance(t, 5_005);
  reads = 2;
  signal.notify();
  await settle();
  assert.equal(outcome.failed, undefined);
  assert.equal(outcome.at, 12_005);
});

test("a wait on a signal that never comes fails at the step's limit, saying what it saw", async (t) => {
  stopClock(t);
  startStep(STEP_LIMIT_MS);
  const signal = new Signal();
  const outcome = watch(
    signal.until(
      () => false,
      () => "the page read the history 1 time",
    ),
  );
  await advance(t, STEP_LIMIT_MS);
  assert.ok(outcome.at >= STEP_LIMIT_MS - REPORT_MS, `the wait gave up at ${outcome.at} ms`);
  assert.ok(outcome.at < STEP_LIMIT_MS, `the wait outlasted its step, to ${outcome.at} ms`);
  assert.ok(outcome.failed instanceof Error);
  assert.equal(outcome.failed.message, "the page read the history 1 time");
});

test("a wait for the next change ends at the change, or else at its own time, with no failure", async (t) => {
  stopClock(t);
  startStep(STEP_LIMIT_MS);
  const signal = new Signal();
  const told = watch(signal.next(1_000));
  await advance(t, 300);
  signal.notify();
  await settle();
  assert.equal(told.at, 300);
  const quiet = watch(signal.next(1_000));
  await advance(t, 1_000);
  assert.equal(quiet.failed, undefined);
  assert.equal(quiet.at, 1_300);
});

test("a wait for what the page does ends when it does, past the deadlines steps gave up at", async (t) => {
  stopClock(t);
  startStep(STEP_LIMIT_MS);
  const { promise: read, resolve: wasRead } = Promise.withResolvers<string>();
  const outcome = watch(untilSettled(read, "the page never asked for its notes"));
  await advance(t, 12_000);
  assert.equal(outcome.at, -1, "the wait ended before the page read its notes");
  wasRead("notes");
  await settle();
  assert.equal(outcome.failed, undefined);
  assert.equal(outcome.at, 12_000);
});

test("a wait for what the page never does fails at the step's limit, saying so", async (t) => {
  stopClock(t);
  startStep(STEP_LIMIT_MS);
  const outcome = watch(untilSettled(Promise.withResolvers<void>().promise, "the page never asked for its notes"));
  await advance(t, STEP_LIMIT_MS);
  assert.ok(outcome.at >= STEP_LIMIT_MS - REPORT_MS, `the wait gave up at ${outcome.at} ms`);
  assert.ok(outcome.at < STEP_LIMIT_MS, `the wait outlasted its step, to ${outcome.at} ms`);
  assert.ok(outcome.failed instanceof Error);
  assert.equal(outcome.failed.message, "the page never asked for its notes");
});

// cucumber's own `timeout` option tells the waits in a step nothing: only a
// limit given with `stepLimit` reaches them.
test("every step with a limit of its own gives it with stepLimit", async () => {
  const steps = new URL("../ui-tests/steps/", import.meta.url);
  for (const file of await readdir(steps)) {
    const text = await readFile(new URL(file, steps), "utf8");
    assert.doesNotMatch(text, /^(?:Given|When|Then)\(.*\{ timeout:|^ {2}\{ timeout:/m, file);
  }
});
