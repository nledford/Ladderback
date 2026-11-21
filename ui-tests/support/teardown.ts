// Ending what an `@ui` worker started (#866). Stubs: the tests in
// scripts/ui-teardown.test.ts specify them.

import type { ChildProcess } from "node:child_process";

export const STOP_MS = 10_000;
export const CLOSE_MS = 35_000;
export const TEARDOWN_MS = 60_000;

export interface Closable {
  close(): Promise<void>;
}

export interface Started {
  browser?: Closable | undefined;
  daemon?: ChildProcess | undefined;
  scratch?: string | undefined;
}

export async function stopDaemon(_daemon: ChildProcess, _graceMs = STOP_MS): Promise<void> {}

export async function tearDown(_started: Started, _options: { closeMs?: number; stopMs?: number } = {}): Promise<void> {}

export const DAEMONS_CHANNEL = "ladderback-ui-daemons";

export function backstop(daemon: ChildProcess): ChildProcess {
  return daemon;
}
