#!/usr/bin/env node
// `just install-restart` (#519): installs Ladderback from this checkout
// (`just install`), waits until the installed daemon has nothing running or
// queued, restarts its service with the new binary (`ladderback
// install-service`, decision 64), then checks that the daemon now runs this
// checkout's build (scripts/build-identity.ts, decision NN).
//
//   1. `just install [args]`: a failed build stops here, before the running
//      daemon is touched.
//   2. The wait: `GET /api/v1/operations` on the service's address every 5 s,
//      every page of it, until no operation is `running` or `queued`, for up
//      to `--wait` minutes, after which it gives up without restarting.
//      A daemon it can't reach, or one that refuses it (sign-in on: there are
//      no API tokens yet), may be busy: it refuses to restart. `--now` skips
//      the wait.
//   3. `<bin>/ladderback install-service`, from Cargo's bin folder (`--root`
//      when given), which rewrites the service file and restarts the service.
//   4. The check: `GET /api/v1/session` until the daemon answers, which must
//      name the build of this checkout as it was when the install began.
//
// The wait is a race in principle: a backup can start between the last
// check and the restart. The window is seconds. A run the restart catches
// ends failed (`daemon-stopped`), as any restart ends it, and a backup still
// queued runs again once the daemon is back (§5,
// features/execution/recovery.feature).
//
// Usage: node scripts/install-restart.ts [--now] [--wait <minutes>]
//          [args for `just install`]...
// Exit status: 0 once the daemon runs the new build; 1 when it isn't
// restarted, or is but can't be confirmed to run the new build; 2 on bad
// usage.
//
// INSTALL_RESTART_ADDRESS names the address the service listens on
// (`host:port`, default 127.0.0.1:9898, where `install-service` has it
// listen): set it for a service file edited to `serve --listen` elsewhere,
// or the wait can't reach the daemon and refuses to restart it.
// INSTALL_RESTART_JUST and INSTALL_RESTART_LADDERBACK name the programs run
// for `just` and for the installed `ladderback`. With the three, the script
// can be tried against a `just try` daemon without installing anything or
// touching the real service.

import { spawnSync } from "node:child_process";
import { homedir, platform } from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import type { Build } from "./build-identity.ts";
import { checkoutBuild, shown, workspaceVersion } from "./build-identity.ts";

const ROOT = path.resolve(import.meta.dirname, "..");
const DEFAULT_ADDRESS = "127.0.0.1:9898";
const DEFAULT_WAIT_MINUTES = 120;
const POLL_MS = 5_000;
// The most operations one page holds (app's `MAX_PAGE_SIZE`).
const PAGE_LIMIT = 200;
// How long one request may take, and how long a restarted daemon has to
// answer.
const REQUEST_MS = 10_000;
const RESTART_MS = 60_000;

function usage(message: string): never {
  console.error(`error: ${message}`);
  console.error("usage: just install-restart [--now] [--wait <minutes>] [args for just install]...");
  process.exit(2);
}

/** Where the daemon stands: idle, busy with operations, or unknown and why. */
type Stand =
  | { kind: "idle" }
  | { kind: "busy"; running: number; queued: number }
  | { kind: "unknown"; why: string };

/** What the daemon at `base` has running and queued, read page by page. */
async function stand(base: string): Promise<Stand> {
  let running = 0;
  let queued = 0;
  let after: string | null = null;
  do {
    const query = new URLSearchParams({ limit: String(PAGE_LIMIT) });
    if (after !== null) query.set("after", after);
    let response: Response;
    try {
      response = await fetch(`${base}/api/v1/operations?${query}`, { signal: AbortSignal.timeout(REQUEST_MS) });
    } catch (error) {
      return { kind: "unknown", why: `no daemon answers at ${base} (${(error as Error).message})` };
    }
    if (response.status === 401) {
      return { kind: "unknown", why: "sign-in is on, and the daemon won't list its operations without a session" };
    }
    if (response.status !== 200) {
      return { kind: "unknown", why: `the daemon answered the operations list with ${response.status}` };
    }
    const page = (await response.json()) as { operations?: { status?: string }[]; next?: string | null };
    for (const operation of page.operations ?? []) {
      if (operation.status === "running") running++;
      if (operation.status === "queued") queued++;
    }
    after = page.next ?? null;
  } while (after !== null);
  return running + queued === 0 ? { kind: "idle" } : { kind: "busy", running, queued };
}

/** Waits until the daemon at `base` is idle, for up to `minutes`; whether it is. */
async function waitUntilIdle(base: string, minutes: number): Promise<boolean> {
  const deadline = Date.now() + minutes * 60_000;
  let said = "";
  for (;;) {
    const now = await stand(base);
    if (now.kind === "idle") return true;
    if (now.kind === "unknown") {
      console.error(`install-restart: can't tell whether the daemon is busy: ${now.why}. Not restarting; --now restarts without asking.`);
      return false;
    }
    if (Date.now() >= deadline) {
      console.error(`install-restart: still busy after ${minutes} min (${now.running} running, ${now.queued} queued). Not restarting; the new binary is installed, and --now restarts at once.`);
      return false;
    }
    const saying = `install-restart: waiting for ${now.running} running and ${now.queued} queued operation(s) to finish (up to ${minutes} min in all)`;
    if (saying !== said) console.log(saying);
    said = saying;
    await sleep(POLL_MS);
  }
}

/**
 * The build the daemon at `base` runs once it answers with the one expected,
 * within a minute of the restart; otherwise why it can't be confirmed.
 */
async function restartedBuild(base: string, expected: Build | undefined, version: string): Promise<Build | string> {
  const deadline = Date.now() + RESTART_MS;
  let last = "no answer";
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${base}/api/v1/session`, { signal: AbortSignal.timeout(REQUEST_MS) });
      if (response.status === 200) {
        const read = (await response.json()) as Partial<Build>;
        if (typeof read.version !== "string") {
          // Sign-in is on: a page without a session isn't told the build.
          return "the daemon tells the build only to a signed-in page (sign-in is on)";
        }
        const build: Build = {
          version: read.version,
          commit: read.commit ?? null,
          dirty: read.dirty === true,
          release: read.release === true,
        };
        if (same(build, expected, version)) return build;
        last = `it runs ${shown(build)}`;
      } else {
        last = `it answered ${response.status}`;
      }
    } catch (error) {
      last = `no answer (${(error as Error).message})`;
    }
    await sleep(1_000);
  }
  return last;
}

/** Whether `build` is the one expected: this checkout's, or, when git couldn't say, its version. */
function same(build: Build, expected: Build | undefined, version: string): boolean {
  if (expected === undefined) return build.version === version;
  return build.version === expected.version && build.commit === expected.commit && build.dirty === expected.dirty;
}

/** Cargo's bin folder for `cargo install` with `args`. */
function binFolder(args: string[]): string {
  for (let i = 0; i < args.length; i++) {
    const arg = args[i] ?? "";
    if (arg === "--root" && args[i + 1] !== undefined) return path.join(path.resolve(args[i + 1] ?? ""), "bin");
    if (arg.startsWith("--root=")) return path.join(path.resolve(arg.slice("--root=".length)), "bin");
  }
  const root = process.env["CARGO_INSTALL_ROOT"] || process.env["CARGO_HOME"] || path.join(homedir(), ".cargo");
  return path.join(root, "bin");
}

/** Runs `program` with `args` here, its output passed through; its exit status. */
function run(program: string, args: string[]): number {
  const result = spawnSync(program, args, { cwd: ROOT, stdio: "inherit" });
  if (result.error) {
    console.error(`install-restart: couldn't run ${program}: ${result.error.message}`);
    return 1;
  }
  return result.status ?? 1;
}

async function main(argv: string[]): Promise<number> {
  let now = false;
  let minutes = DEFAULT_WAIT_MINUTES;
  const install: string[] = [];
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i] ?? "";
    const value = () => argv[++i] ?? usage(`${arg} needs a value`);
    if (arg === "--help" || arg === "-h") usage("help");
    else if (arg === "--now") now = true;
    else if (arg === "--wait") {
      const given = value();
      minutes = Number(given);
      if (given.trim() === "" || !Number.isFinite(minutes) || minutes < 0) usage(`--wait takes minutes, not ${JSON.stringify(given)}`);
    } else install.push(arg);
  }
  const address = process.env["INSTALL_RESTART_ADDRESS"] || DEFAULT_ADDRESS;
  const base = `http://${address}`;

  // What the binary will be built from: the tree now, before the build.
  const expected = checkoutBuild(ROOT);
  const version = workspaceVersion(ROOT);

  const installed = run(process.env["INSTALL_RESTART_JUST"] || "just", ["install", ...install]);
  if (installed !== 0) {
    console.error("install-restart: the install failed; the running daemon is untouched.");
    return 1;
  }

  if (now) console.log("install-restart: --now: restarting without waiting.");
  else if (!(await waitUntilIdle(base, minutes))) return 1;

  const ladderback = process.env["INSTALL_RESTART_LADDERBACK"] || path.join(binFolder(install), "ladderback");
  if (run(ladderback, ["install-service"]) !== 0) {
    console.error(`install-restart: ${ladderback} install-service failed.`);
    return 1;
  }

  if (platform() === "darwin") {
    console.log("install-restart: macOS will ask again for each keychain item Ladderback reads: a local build is signed ad hoc, so the keychain sees a new program (#281).");
  }

  const wanted = expected === undefined ? version : shown(expected);
  const running = await restartedBuild(base, expected, version);
  if (typeof running === "string") {
    console.error(`install-restart: the service was restarted, but can't confirm it runs ${wanted}: ${running}.`);
    return 1;
  }
  console.log(`install-restart: the daemon at ${address} runs ladderback ${shown(running)}.`);
  return 0;
}

process.exitCode = await main(process.argv.slice(2));
