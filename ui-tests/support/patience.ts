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

/** The limit of a step, or hook, that sets none of its own; world.ts gives it
 *  to cucumber. */
export const STEP_LIMIT_MS = 30_000;

/** What a wait leaves of its step's limit for the step to fail in. */
export const REPORT_MS = 1_000;

const TODO = "not yet: #1104";

/** The options of a step with a limit of its own: cucumber's `timeout`, and
 *  the same limit for its waits, which cucumber's option doesn't reach. */
export function stepLimit(_ms: number): { timeout: number; wrapperOptions: { limitMs: number } } {
  throw new Error(TODO);
}

/** Starts the clock of a step, or hook, with this limit. */
export function startStep(_limitMs: number): void {
  throw new Error(TODO);
}

/** How long a wait in the step running now may take: to the step's limit,
 *  less `REPORT_MS`. At least 1 ms, since Playwright reads a timeout of 0 as
 *  no limit at all. */
export function patienceMs(): number {
  throw new Error(TODO);
}

/** Wraps a step's or hook's code so that it starts its clock as it begins,
 *  with the limit `stepLimit` gave it or else `STEP_LIMIT_MS`
 *  (`setDefinitionFunctionWrapper`, world.ts). */
export function timed<A extends unknown[], R>(
  _code: (this: unknown, ...args: A) => R,
  _options?: { limitMs?: number },
): (this: unknown, ...args: A) => R {
  throw new Error(TODO);
}

/** Waits for `event`, which the page or a step settles, and returns what it
 *  settled with, or fails at the step's limit with `failure`. */
export async function untilSettled<T>(_event: Promise<T>, _failure: string): Promise<T> {
  throw new Error(TODO);
}

/** Reads `read` until `done` holds of what it read, and returns that, or
 *  fails at the step's limit with `what` and the last value read. */
export async function untilStepLimit<T, U extends T>(what: string, read: () => Promise<T>, done: (value: T) => value is U): Promise<U>;
export async function untilStepLimit<T>(what: string, read: () => Promise<T>, done: (value: T) => boolean): Promise<T>;
export async function untilStepLimit<T>(_what: string, _read: () => Promise<T>, _done: (value: T) => boolean): Promise<T> {
  throw new Error(TODO);
}

/** Something that tells its waiters each time it changes, so that a wait on
 *  it ends the moment what it waits for holds, without polling. */
export class Signal {
  /** Tells each wait that something changed. */
  notify(): void {
    throw new Error(TODO);
  }

  /** Waits for the next change, or `ms`, whichever comes first. */
  async next(_ms: number): Promise<void> {
    throw new Error(TODO);
  }

  /** Waits until `holds`, checked each time this is notified, or fails at
   *  the step's limit with `failure()`, which says what the wait saw. */
  async until(_holds: () => boolean, _failure: () => string): Promise<void> {
    throw new Error(TODO);
  }
}
