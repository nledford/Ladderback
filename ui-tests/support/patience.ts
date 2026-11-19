// How long a step waits for something to appear: until its own limit, never a
// shorter deadline of its own (#1104). On a loaded machine the page is only
// slow, not wrong, and a deadline inside the step's limit failed it anyway. A
// wait ends `REPORT_MS` before the limit, so that a step whose page never
// shows what it waits for still fails with what it last read, not with
// cucumber's bare timeout.
//
// Each step and hook starts its clock as it begins (`timed`, which world.ts
// wraps every step and hook in). A cucumber worker runs one step at a time,
// so the clock of the step running now is this module's.
//
// What waits here is what the step waits *for*. A check that something does
// not happen, or does not stay, watches for a window of its own instead.

import assert from "node:assert/strict";

/** The limit of a step, or hook, that sets none of its own; world.ts gives it
 *  to cucumber. */
export const STEP_LIMIT_MS = 30_000;

/** What a wait leaves of its step's limit for the step to fail in. */
export const REPORT_MS = 1_000;

/** How often a wait with nothing to tell it of a change reads again. */
const POLL_MS = 100;

/** When the step running now reaches its limit; until a step starts (a
 *  `node --test` of a helper), none has, and a wait has a whole step's limit. */
let stepEnds: number | undefined;

/** The options of a step with a limit of its own: cucumber's `timeout`, and
 *  the same limit for its waits, which cucumber's option doesn't reach. */
export function stepLimit(ms: number): { timeout: number; wrapperOptions: { limitMs: number } } {
  return { timeout: ms, wrapperOptions: { limitMs: ms } };
}

/** Starts the clock of a step, or hook, with this limit. */
export function startStep(limitMs: number): void {
  stepEnds = Date.now() + limitMs;
}

/** How long a wait in the step running now may take: to the step's limit,
 *  less `REPORT_MS`. At least 1 ms, since Playwright reads a timeout of 0 as
 *  no limit at all. */
export function patienceMs(): number {
  if (stepEnds === undefined) return STEP_LIMIT_MS - REPORT_MS;
  return Math.max(1, stepEnds - REPORT_MS - Date.now());
}

/** Wraps a step's or hook's code so that it starts its clock as it begins,
 *  with the limit `stepLimit` gave it or else `STEP_LIMIT_MS`
 *  (`setDefinitionFunctionWrapper`, world.ts). */
export function timed<A extends unknown[], R>(
  code: (this: unknown, ...args: A) => R,
  options?: { limitMs?: number },
): (this: unknown, ...args: A) => R {
  const limitMs = options?.limitMs ?? STEP_LIMIT_MS;
  return function (this: unknown, ...args: A): R {
    startStep(limitMs);
    return code.apply(this, args);
  };
}

/** Waits for `event`, which the page or a step settles, and returns what it
 *  settled with, or fails at the step's limit with `failure`. */
export async function untilSettled<T>(event: Promise<T>, failure: string): Promise<T> {
  const { promise: late, resolve: tooLate } = Promise.withResolvers<{ late: true }>();
  const timer = setTimeout(() => tooLate({ late: true }), patienceMs());
  try {
    const outcome = await Promise.race([event.then((value) => ({ late: false as const, value })), late]);
    if (outcome.late) assert.fail(failure);
    return outcome.value;
  } finally {
    clearTimeout(timer);
  }
}

/** Reads `read` until `done` holds of what it read, and returns that, or
 *  fails at the step's limit with `what` and the last value read. */
export async function untilStepLimit<T, U extends T>(what: string, read: () => Promise<T>, done: (value: T) => value is U): Promise<U>;
export async function untilStepLimit<T>(what: string, read: () => Promise<T>, done: (value: T) => boolean): Promise<T>;
export async function untilStepLimit<T>(what: string, read: () => Promise<T>, done: (value: T) => boolean): Promise<T> {
  const ends = Date.now() + patienceMs();
  let last: T | undefined;
  for (;;) {
    // A read that waits of its own (Playwright's actions wait up to 30 s)
    // ends at the step's limit too, so the step fails with what it last read.
    const { promise: late, resolve: tooLate } = Promise.withResolvers<{ late: true }>();
    const timer = setTimeout(() => tooLate({ late: true }), Math.max(0, ends - Date.now()));
    try {
      const outcome = await Promise.race([read().then((value) => ({ late: false as const, value })), late]);
      if (!outcome.late) last = outcome.value;
      else assert.fail(`${what} (last read ${last === null || last === undefined ? "nothing" : JSON.stringify(last)})`);
    } finally {
      clearTimeout(timer);
    }
    if (done(last as T)) return last as T;
    if (Date.now() >= ends) {
      assert.fail(`${what} (last read ${last === null || last === undefined ? "nothing" : JSON.stringify(last)})`);
    }
    const { promise, resolve } = Promise.withResolvers<void>();
    setTimeout(resolve, Math.min(POLL_MS, ends - Date.now()));
    await promise;
  }
}

/** Something that tells its waiters each time it changes, so that a wait on
 *  it ends the moment what it waits for holds, without polling. */
export class Signal {
  #waiters = new Set<() => void>();

  /** Tells each wait that something changed. */
  notify(): void {
    for (const wake of this.#waiters) wake();
  }

  /** Waits for the next change, or `ms`, whichever comes first. */
  async next(ms: number): Promise<void> {
    const { promise, resolve } = Promise.withResolvers<void>();
    const timer = setTimeout(resolve, ms);
    this.#waiters.add(resolve);
    await promise;
    clearTimeout(timer);
    this.#waiters.delete(resolve);
  }

  /** Waits until `holds`, checked each time this is notified, or fails at
   *  the step's limit with `failure()`, which says what the wait saw. */
  async until(holds: () => boolean, failure: () => string): Promise<void> {
    const ends = Date.now() + patienceMs();
    while (!holds()) {
      const left = ends - Date.now();
      if (left <= 0) assert.fail(failure());
      await this.next(left);
    }
  }
}
