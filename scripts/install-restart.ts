#!/usr/bin/env node
// `just install-restart` (#519): installs Ladderback from this checkout
// (`just install`), waits until the installed daemon has nothing running or
// queued, restarts its service with the new binary (`ladderback
// install-service`, decision 64), then checks that the service runs this
// checkout's build (scripts/build-identity.ts, decision 90).
//
//   1. `just install [args]`: a failed build, or a failed signing with
//      LADDERBACK_SIGN_IDENTITY (scripts/install-signing.ts), stops here,
//      before the running daemon is touched. A failed signing comes after
//      the new program is installed, which the daemon runs at its next start.
//   2. The installed program's `--version` must name the build of this
//      checkout as it was when the install began, or nothing is restarted.
//      It is asked again after the wait, just before step 4.
//   2a. `<bin>/ladderback check-keyring`: the new program reads each
//      `password-keyring` entry once, in this terminal, so that the
//      keychain, which asks once for each new build (#892), asks while
//      someone is there. An entry it can't read stops the script here,
//      unless `--ignore-keyring-failure`.
//   3. The wait: every 5 s, until the daemon's operation log holds no
//      operation `running` or `queued`, for up to `--wait` minutes, after
//      which it gives up without restarting. `--now` skips the wait.
//   4. `<bin>/ladderback install-service`, from Cargo's bin folder (`--root`
//      when given), which rewrites the service file and restarts the service.
//   5. The check: the service manager names a new process for the service,
//      and the daemon answers `GET /api/v1/session` within a minute. With
//      sign-in off it names its build there, which must be this checkout's;
//      with sign-in on it names it only to a session, and the new process
//      started from the program step 2 checked is what confirms the build.
//
// The wait reads the operation log (`ladderback.db` in the daemon's data
// folder) itself, read-only, with Node's `node:sqlite`, rather than asking
// `GET /api/v1/operations`, which with sign-in on answers only a session or
// an API token. This is a development tool, run as the OS user the daemon
// runs as, who can already read and write that folder, so reading it exposes
// nothing; the product's own SQL stays in crates/adapters/store. The script
// depends on one column of the store's schema, `operations.status`, and on
// its values (crates/adapters/store/src/codec.rs). Anything it doesn't expect
// means it can't tell whether the daemon is busy, and it refuses to restart:
// no operation log, one it can't read (not a database, no such table or
// column, locked), a status it doesn't know, a service manager that names no
// process for the service, or a data folder whose lock file
// (`ladderback.lock`) names another process than the service's, so that the
// log it found isn't the service's. A migration can break the script, but
// not make it restart during an operation. It finds the data folder as the
// daemon does (crates/adapters/platform/src/dirs.rs): `LADDERBACK_DATA_DIR`
// when set, otherwise `~/Library/Application Support/ladderback` on macOS and
// `$XDG_DATA_HOME/ladderback`, or `~/.local/share/ladderback`, on Linux.
//
// The wait is a race in principle: a backup can start between the last
// check and the restart. The window is seconds. A run the restart catches
// ends failed (`daemon-stopped`), as any restart ends it, and a backup still
// queued runs again once the daemon is back (§5,
// features/execution/recovery.feature).
//
// Usage: node scripts/install-restart.ts [--now] [--wait <minutes>]
//          [--ignore-keyring-failure] [args for `just install`]...
// Exit status: 0 once the daemon runs the new build; 1 when it isn't
// restarted, or is but can't be confirmed to run the new build; 2 on bad
// usage.
//
// INSTALL_RESTART_ADDRESS names the address the service listens on
// (`host:port`, default 127.0.0.1:9898, where `install-service` has it
// listen): set it for a service file edited to `serve --listen` elsewhere,
// or the check after the restart can't reach the daemon. INSTALL_RESTART_JUST
// and INSTALL_RESTART_LADDERBACK name the programs run for `just` and for the
// installed `ladderback`, and the service manager is the `launchctl` or
// `systemctl` on the PATH. With these, a stand-in for the service manager
// first on the PATH and LADDERBACK_DATA_DIR naming the daemon's data folder,
// the script can be tried against a `just try` daemon without installing
// anything or touching the real service, as
// features/cli/install_restart.feature runs it.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { platform } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { setTimeout as sleep } from "node:timers/promises";

import type { Build } from "./build-identity.ts";
import { checkoutBuild, shown, workspaceVersion } from "./build-identity.ts";
import { binFolder, reinstallWarning } from "./install-signing.ts";

const ROOT = path.resolve(import.meta.dirname, "..");
const DEFAULT_ADDRESS = "127.0.0.1:9898";
const DEFAULT_WAIT_MINUTES = 120;
const POLL_MS = 5_000;
// How long one request or one call of another program may take, and how
// long a restarted service has to run a new process and answer.
const REQUEST_MS = 10_000;
const RESTART_MS = 60_000;
// How long a read of the operation log waits for a lock. The daemon keeps
// the log in WAL mode, where a reader waits only on an exclusive lock or a
// recovery after a crash.
const LOCKED_MS = 1_000;
// The service's name under each manager (crates/adapters/platform's
// `LAUNCHD_LABEL` and `SYSTEMD_UNIT`).
const LAUNCHD_LABEL = "io.github.nledford.ladderback";
const SYSTEMD_UNIT = "ladderback.service";
// The statuses of an operation that has ended (crates/adapters/store's
// codec); `running` and `queued` are the others.
const ENDED = new Set(["succeeded", "succeeded_with_warnings", "failed", "cancelled"]);

function usage(message: string): never {
  console.error(`error: ${message}`);
  console.error("usage: just install-restart [--now] [--wait <minutes>] [--ignore-keyring-failure] [args for just install]...");
  process.exit(2);
}

/** Why something can't be told. */
interface Unknown {
  why: string;
}

/** Where the daemon stands: idle, busy with operations, or unknown and why. */
type Stand = { kind: "idle" } | { kind: "busy"; running: number; queued: number } | { kind: "unknown"; why: string };

/** The first line of `text`, trimmed. */
function firstLine(text: string): string {
  return text.trim().split("\n")[0]?.trim() ?? "";
}

/** The process ID the service manager names for the service, or why it names none. */
function serviceProcess(): number | Unknown {
  let program: string;
  let args: string[];
  let read: (said: string) => string | undefined;
  if (platform() === "darwin") {
    const uid = process.getuid?.();
    if (uid === undefined) return { why: "this system has no user IDs" };
    program = "launchctl";
    args = ["print", `gui/${uid}/${LAUNCHD_LABEL}`];
    read = (said) => /^\tpid = (\S+)$/m.exec(said)?.[1];
  } else if (platform() === "linux") {
    program = "systemctl";
    args = ["--user", "show", "-p", "MainPID", "--value", SYSTEMD_UNIT];
    read = (said) => said.trim();
  } else {
    return { why: `install-restart knows launchd and systemd, and there is neither on ${platform()}` };
  }
  const call = `${program} ${args.join(" ")}`;
  const result = spawnSync(program, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: REQUEST_MS });
  if (result.error) return { why: `\`${call}\` couldn't run (${result.error.message})` };
  if (result.status !== 0) {
    return {
      why: `the service isn't running (\`${call}\` ended with ${result.status ?? result.signal}: ${firstLine(result.stderr)})`,
    };
  }
  const pid = read(result.stdout) ?? "";
  if (!/^[1-9][0-9]{0,9}$/.test(pid)) return { why: `the service isn't running (\`${call}\` names no process)` };
  return Number(pid);
}

/** The daemon's data folder, found as the daemon finds it, or why it can't be. */
function dataFolder(): string | Unknown {
  // An empty variable is as good as unset.
  const env = (name: string): string | undefined => process.env[name] || undefined;
  const set = env("LADDERBACK_DATA_DIR");
  if (set !== undefined) return set;
  const home = env("HOME");
  if (platform() === "darwin" && home !== undefined) return path.join(home, "Library/Application Support/ladderback");
  if (platform() === "linux") {
    const data = env("XDG_DATA_HOME");
    if (data !== undefined) return path.join(data, "ladderback");
    if (home !== undefined) return path.join(home, ".local/share/ladderback");
  }
  return { why: "LADDERBACK_DATA_DIR isn't set, and there is no home folder to find the data folder in" };
}

/** The process the data folder's lock file names: the daemon that holds the folder, or held it last. */
function folderHolder(folder: string): number | Unknown {
  const file = path.join(folder, "ladderback.lock");
  let text: string;
  try {
    text = readFileSync(file, "utf8").trim();
  } catch (error) {
    return { why: `no daemon has held the data folder ${folder} (${(error as Error).message})` };
  }
  if (!/^[1-9][0-9]{0,9}$/.test(text)) return { why: `the lock file ${file} names no process` };
  return Number(text);
}

/** What the operation log `file` holds running and queued, read-only. */
function logStand(file: string): Stand {
  if (!existsSync(file)) return { kind: "unknown", why: `there is no operation log at ${file}` };
  let log: DatabaseSync | undefined;
  try {
    log = new DatabaseSync(file, { readOnly: true, timeout: LOCKED_MS });
    const rows = log.prepare("SELECT status, count(*) AS count FROM operations GROUP BY status").all();
    let running = 0;
    let queued = 0;
    for (const row of rows) {
      const status = row["status"];
      const count = Number(row["count"]);
      if (status === "running") running += count;
      else if (status === "queued") queued += count;
      else if (typeof status !== "string" || !ENDED.has(status)) {
        return {
          kind: "unknown",
          why: `the operation log at ${file} holds an operation whose status is ${String(status)}, which install-restart doesn't know`,
        };
      }
    }
    return running + queued === 0 ? { kind: "idle" } : { kind: "busy", running, queued };
  } catch (error) {
    return { kind: "unknown", why: `the operation log at ${file} can't be read (${(error as Error).message})` };
  } finally {
    log?.close();
  }
}

/** What the service's daemon has running and queued, read from its operation log. */
function stand(): Stand {
  const service = serviceProcess();
  if (typeof service !== "number") return { kind: "unknown", ...service };
  const folder = dataFolder();
  if (typeof folder !== "string") return { kind: "unknown", ...folder };
  // The service's environment, not this shell's, decides its data folder: a
  // shell that sets XDG_DATA_HOME or LADDERBACK_DATA_DIR alone looks elsewhere.
  const elsewhere = "; if the service keeps its data elsewhere, set LADDERBACK_DATA_DIR to that folder";
  const holder = folderHolder(folder);
  if (typeof holder !== "number") return { kind: "unknown", why: `${holder.why}${elsewhere}` };
  if (holder !== service) {
    return {
      kind: "unknown",
      why: `the data folder ${folder} is held by process ${holder}, not the service's process ${service}${elsewhere}`,
    };
  }
  return logStand(path.join(folder, "ladderback.db"));
}

/** Waits until the service's daemon is idle, for up to `minutes`; whether it is. */
async function waitUntilIdle(minutes: number): Promise<boolean> {
  const deadline = Date.now() + minutes * 60_000;
  let said = "";
  for (;;) {
    const now = stand();
    if (now.kind === "idle") return true;
    if (now.kind === "unknown") {
      console.error(
        `install-restart: can't tell whether the daemon is busy: ${now.why}. Not restarting; --now restarts without asking.`,
      );
      return false;
    }
    if (Date.now() >= deadline) {
      console.error(
        `install-restart: still busy after ${minutes} min (${now.running} running, ${now.queued} queued). Not restarting; the new binary is installed, and --now restarts at once.`,
      );
      return false;
    }
    const saying = `install-restart: waiting for ${now.running} running and ${now.queued} queued operation(s) to finish (up to ${minutes} min in all)`;
    if (saying !== said) console.log(saying);
    said = saying;
    await sleep(POLL_MS);
  }
}

/** Whether the `ladderback --version` of `program` names the build expected; otherwise what it says instead. */
function installedBuild(program: string, wanted: string, expected: Build | undefined, version: string): true | Unknown {
  const result = spawnSync(program, ["--version"], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: REQUEST_MS });
  if (result.error) return { why: `${program} --version couldn't run (${result.error.message})` };
  const said = firstLine(result.stdout);
  if (result.status !== 0)
    return { why: `${program} --version ended with ${result.status ?? result.signal}: ${firstLine(result.stderr)}` };
  const named = /^ladderback (\S+)$/.exec(said)?.[1];
  // When git couldn't say, the version alone, with or without a commit.
  const ok = expected === undefined ? named === version || named?.startsWith(`${version}+`) === true : named === wanted;
  return ok ? true : { why: `the installed program says ${said}, not ladderback ${wanted}` };
}

/** What confirmed the restart: the build the daemon names, or none with sign-in on. */
type Confirmed = { process: number; build: Build | null };

/**
 * Within a minute of the restart, the service's new process, and the build
 * the daemon at `base` names once it answers: the one expected, or none with
 * sign-in on; otherwise why the restart can't be confirmed.
 */
async function restarted(
  before: number | Unknown,
  base: string,
  expected: Build | undefined,
  version: string,
): Promise<Confirmed | Unknown> {
  const deadline = Date.now() + RESTART_MS;
  let last = "no answer";
  let started: number | undefined;
  while (Date.now() < deadline) {
    if (started === undefined) {
      const now = serviceProcess();
      if (typeof now === "number" && now === before) {
        // `install-service` returns once the service manager has started the
        // service again, so the old process still running means it didn't.
        return { why: `the service manager still names process ${now}, the one from before the restart` };
      }
      if (typeof now === "number") started = now;
      else last = now.why;
    }
    if (started !== undefined) {
      try {
        const response = await fetch(`${base}/api/v1/session`, { signal: AbortSignal.timeout(REQUEST_MS) });
        if (response.status === 200) {
          const read = (await response.json()) as Partial<Build> & { "sign-in"?: string };
          // With sign-in on, a caller without a session is told no build.
          if (read["sign-in"] === "signed-out") return { process: started, build: null };
          if (typeof read.version !== "string") {
            // Sign-in off and no build: a build older than #516 answers, as
            // when the old one still does. Keep asking until the deadline.
            last = "it names no build: it runs a build older than 0.3.0";
          } else {
            const build: Build = {
              version: read.version,
              commit: read.commit ?? null,
              dirty: read.dirty === true,
              release: read.release === true,
            };
            if (same(build, expected, version)) return { process: started, build };
            last = `the daemon at ${base} runs ${shown(build)}`;
          }
        } else {
          last = `the daemon at ${base} answered ${response.status}`;
        }
      } catch (error) {
        last = `no daemon answers at ${base} (${(error as Error).message})`;
      }
    }
    await sleep(1_000);
  }
  return { why: last };
}

/** Whether `build` is the one expected: this checkout's, or, when git couldn't say, its version. */
function same(build: Build, expected: Build | undefined, version: string): boolean {
  if (expected === undefined) return build.version === version;
  return build.version === expected.version && build.commit === expected.commit && build.dirty === expected.dirty;
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
  let ignoreKeyring = false;
  let minutes = DEFAULT_WAIT_MINUTES;
  const install: string[] = [];
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i] ?? "";
    const value = () => argv[++i] ?? usage(`${arg} needs a value`);
    if (arg === "--help" || arg === "-h") usage("help");
    else if (arg === "--now") now = true;
    else if (arg === "--ignore-keyring-failure") ignoreKeyring = true;
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
  const wanted = expected === undefined ? version : shown(expected);

  const installed = run(process.env["INSTALL_RESTART_JUST"] || "just", ["install", ...install]);
  if (installed !== 0) {
    console.error("install-restart: the install failed; the running daemon is untouched.");
    return 1;
  }

  const ladderback = process.env["INSTALL_RESTART_LADDERBACK"] || path.join(binFolder(install, process.env), "ladderback");
  const checked = installedBuild(ladderback, wanted, expected, version);
  if (checked !== true) {
    console.error(`install-restart: ${checked.why}. Not restarting; the running daemon is untouched.`);
    return 1;
  }

  // The keychain asks once for each new build before it lets the build read
  // a `password-keyring` entry (#892): asked here, in the terminal, the
  // question comes while someone is there, not at the next scheduled run.
  if (run(ladderback, ["check-keyring"]) !== 0) {
    console.error(
      "install-restart: the new program couldn't read every keyring entry, or the configuration that names them (above), so a backup or a notification that needs one may fail too.",
    );
    if (!ignoreKeyring) {
      console.error(
        "install-restart: Not restarting; the running daemon is untouched. --ignore-keyring-failure restarts all the same.",
      );
      return 1;
    }
    console.log("install-restart: --ignore-keyring-failure: restarting all the same.");
  }

  if (now) console.log("install-restart: --now: restarting without waiting.");
  else if (!(await waitUntilIdle(minutes))) return 1;

  // Again, as the wait can last hours: another checkout's `just install`
  // meanwhile writes over the program, and `install-service` would start that.
  const still = installedBuild(ladderback, wanted, expected, version);
  if (still !== true) {
    console.error(`install-restart: ${still.why}. Not restarting; the running daemon is untouched.`);
    return 1;
  }

  const before = serviceProcess();
  if (run(ladderback, ["install-service"]) !== 0) {
    console.error(`install-restart: ${ladderback} install-service failed.`);
    return 1;
  }

  const warning = reinstallWarning(process.env, platform());
  if (warning !== undefined) console.log(`install-restart: ${warning}`);

  const confirmed = await restarted(before, base, expected, version);
  if ("why" in confirmed) {
    console.error(`install-restart: the service was restarted, but can't confirm it runs ${wanted}: ${confirmed.why}.`);
    return 1;
  }
  const processes =
    typeof before === "number"
      ? `process ${confirmed.process}, not ${before} as before the restart`
      : `process ${confirmed.process}, where it named none before the restart`;
  if (confirmed.build === null) {
    console.log(
      `install-restart: the service runs ${processes}, and the installed program says it is ladderback ${wanted}; the daemon at ${address} answers, but with sign-in on, it names its build only to a session.`,
    );
  } else {
    console.log(
      `install-restart: the service runs ${processes}, and the daemon at ${address} says it runs ladderback ${shown(confirmed.build)}.`,
    );
  }
  return 0;
}

process.exitCode = await main(process.argv.slice(2));
