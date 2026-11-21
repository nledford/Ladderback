// Ending what an `@ui` worker started: its daemons, its browser and their
// scratch folders (#866). A worker's end-of-run hook once closed the browser
// first, with no limit of its own below the hook's: Playwright waits its own
// 30 s for a browser to exit before it kills it, the hook's limit was the
// same 30 s, and on a busy machine the hook timed out before it stopped the
// daemon, which then outlived the run.
//
// So the daemon stops alongside the browser's close, the scratch folder goes
// whatever happened before, the hook's limit leaves room past Playwright's
// own, and a daemon still running when the run's process exits is killed
// then (`backstop`). This module takes neither cucumber nor Playwright, so
// that `node --test` can drive it with fakes (scripts/ui-teardown.test.ts).

import type { ChildProcess } from "node:child_process";
import { once } from "node:events";
import { rm } from "node:fs/promises";
import { BroadcastChannel, isMainThread } from "node:worker_threads";

/** How long a daemon has to stop in order after SIGTERM before it is killed:
 *  an idle one takes well under a second. */
export const STOP_MS = 10_000;

/** How long the teardown waits for the browser to close: past the 30 s
 *  Playwright gives a browser to exit before it kills it itself. */
export const CLOSE_MS = 35_000;

/** The limit of the end-of-run hook: the browser's close and the daemon's
 *  stop side by side, `CLOSE_MS` at most, then the scratch folder's removal. */
export const TEARDOWN_MS = 60_000;

/** A browser, as far as the teardown needs one. */
export interface Closable {
  close(): Promise<void>;
}

/** What a worker started and the teardown ends. */
export interface Started {
  browser?: Closable | undefined;
  daemon?: ChildProcess | undefined;
  scratch?: string | undefined;
}

/** Whether `promise` settled within `ms`; its rejection is passed on. */
async function within(promise: Promise<unknown>, ms: number): Promise<boolean> {
  const { promise: late, resolve } = Promise.withResolvers<false>();
  const timer = setTimeout(() => resolve(false), ms);
  try {
    return await Promise.race([promise.then(() => true), late]);
  } finally {
    clearTimeout(timer);
  }
}

/** Stops `daemon`: SIGTERM, which it answers by stopping in order, then
 *  SIGKILL if it still runs after `graceMs`. Resolves once it has exited. */
export async function stopDaemon(daemon: ChildProcess, graceMs = STOP_MS): Promise<void> {
  if (daemon.exitCode !== null || daemon.signalCode !== null) return;
  const exit = once(daemon, "exit");
  daemon.kill("SIGTERM");
  if (await within(exit, graceMs)) return;
  daemon.kill("SIGKILL");
  await exit;
}

/** Stops the daemon and closes the browser side by side, then removes the
 *  scratch folder, even when one of them failed, and only then passes on
 *  the first failure. A browser that hasn't closed within `closeMs` is left
 *  to Playwright, which kills it when the process exits. */
export async function tearDown(started: Started, { closeMs = CLOSE_MS, stopMs = STOP_MS } = {}): Promise<void> {
  const { browser, daemon, scratch } = started;
  try {
    const ended = await Promise.allSettled([
      daemon === undefined ? undefined : stopDaemon(daemon, stopMs),
      browser === undefined
        ? undefined
        : within(browser.close(), closeMs).then((closed) => {
            if (!closed) process.stderr.write(`the browser had not closed after ${closeMs} ms; leaving it to Playwright\n`);
          }),
    ]);
    for (const end of ended) if (end.status === "rejected") throw end.reason;
  } finally {
    if (scratch !== undefined) await rm(scratch, { recursive: true, force: true });
  }
}

// The backstop. A worker is a thread of cucumber-js's process, and a thread
// that ends, or is terminated, emits no `exit` of its own, while the daemons
// it started are processes that outlive it. So each daemon's ID goes to the
// main thread, which kills those still running when the process exits.

/** The channel on which a worker tells the main thread of its daemons. */
export const DAEMONS_CHANNEL = "ladderback-ui-daemons";

/** What a worker says of one of its daemons. */
interface DaemonNews {
  pid: number;
  running: boolean;
}

/** The main thread's list of the daemons still running. */
const running = new Set<number>();

const channel = new BroadcastChannel(DAEMONS_CHANNEL);
// Neither the main thread nor a worker waits for this channel to end.
channel.unref();

if (isMainThread) {
  channel.onmessage = (event: unknown) => {
    const { pid, running: alive } = (event as MessageEvent<DaemonNews>).data;
    if (alive) running.add(pid);
    else running.delete(pid);
  };
  process.on("exit", () => {
    for (const pid of running) {
      try {
        process.kill(pid, "SIGKILL");
      } catch {
        // It exited since; nothing to kill.
      }
    }
  });
}

/** Tells the main thread whether the daemon `pid` runs. */
function report(news: DaemonNews): void {
  if (!isMainThread) channel.postMessage(news);
  else if (news.running) running.add(news.pid);
  else running.delete(news.pid);
}

/** Puts `daemon` under the backstop: if it still runs when the run's process
 *  exits, it is killed then. Returns it. */
export function backstop<Daemon extends ChildProcess>(daemon: Daemon): Daemon {
  const { pid } = daemon;
  if (pid === undefined) return daemon;
  report({ pid, running: true });
  daemon.once("exit", () => report({ pid, running: false }));
  return daemon;
}
