#!/usr/bin/env node
// Smoke-tests Ladderback's Docker image (technical design §12, §16 decision
// 126): what `just image` builds from release archives. Nothing here
// publishes anything. It runs the image as the install guide does, in
// containers and volumes of its own, named `ladderback-smoke-<random>-…`, and
// removes them at the end, whether the checks passed or not.
//
// Checks:
//   - the image's labels name its source, its licenses and the version its
//     binary says; it runs as the user 10001:10001, not root, and exposes 9898;
//   - `--version` names the build of this checkout (scripts/build-identity.ts),
//     or the version `--version <x.y.z>` names;
//   - it has no hub until M8 (§16, decision 122): `serve --hub` and
//     `pairing-token` are refused as unknown, and `serve` answers 404 on
//     `/sync/v1`;
//   - run with no arguments, the daemon refuses to listen beyond the
//     container's loopback until an `--allowed-host` names a host (§10.1):
//     the image names none for the person;
//   - `set-password`, in a container of its own on the data volume, turns
//     sign-in on: the agent started after it, with a fixed `--hostname`,
//     `--init`, `--allowed-host localhost` and its port published on
//     127.0.0.1, serves the web UI, says it is signed out and answers 401 to
//     the operations without a session;
//   - signed in, a scratch repository on a volume is initialized, its
//     password read from a file under /run/secrets, and one backup of demo
//     files, mounted read-only at /srv/demo, succeeds and leaves one snapshot
//     whose host is the container's `--hostname`, whose path is /srv/demo,
//     the path inside the container, and whose time is in the zone `TZ`
//     names;
//   - `docker stop` ends the daemon cleanly, with exit status 0;
//   - a new container on the same volumes still has sign-in on, the snapshot
//     and the backup in its operation history;
//   - the install guide's example Compose file still parses
//     (`docker compose config`).
// The results go to standard output as Markdown, and are appended to the file
// `--summary` names, or to $GITHUB_STEP_SUMMARY in a GitHub Actions job.
//
// Usage: node scripts/smoke-image.ts <image> [--version <x.y.z>] [--summary <file>]
// Exit status: 0 when every check passed, 1 when one failed, 2 on bad usage.
// Needs Docker.

import { randomBytes } from "node:crypto";
import { appendFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { checkoutBuild, shown, workspaceVersion } from "./build-identity.ts";
import { details, type Output, Report, run } from "./smoke-report.ts";

const ROOT = path.resolve(import.meta.dirname, "..");
// The install guide's Compose file, the agent of its `docker run` (§12).
const COMPOSE = "docs/guide/examples/compose.yaml";

// What the image promises (Containerfile).
const USER = "10001:10001";
const PORT = "9898/tcp";
const CONFIG = "/var/lib/ladderback/config";
const DATA = "/var/lib/ladderback/data";
const CACHE = "/var/cache/ladderback";

// The agent as the install guide runs one: a fixed host name, the folder it
// backs up mounted read-only, and its repository's password in a file.
const HOSTNAME = "smoke-agent";
const SOURCE = "/srv/demo";
const REPOSITORY = "/srv/repository";
const SECRETS = "/run/secrets";
const PASSWORD_FILE = `${SECRETS}/repository-password`;

// The agent's time zone, set by `TZ`. One with no daylight saving time, so a
// snapshot it takes is always at +09:00.
const ZONE = "Asia/Tokyo";
const OFFSET = "+09:00";

// Canaries, not secrets: the scratch repository's password and the admin
// password sign-in checks.
const PASSWORD = "smoke-scratch-password";
const ADMIN_PASSWORD = "smoke-admin-canary";
const PLAN = "smoke";

const DOCKER_LIMIT_MS = 300_000;
const LISTEN_LIMIT_MS = 60_000;
const BACKUP_LIMIT_MS = 180_000;
const REQUEST_LIMIT_MS = 30_000;
// How long `docker stop` waits before it kills, as the install guide says:
// past the daemon's own 65 s limit on a stop (decision 92).
const STOP_SECONDS = "75";

// The Docker commands still running. Cleanup waits for them, so a volume or a
// container one of them is making exists by the time it is removed; once
// cleanup starts, no new command starts.
const running = new Set<Promise<Output>>();
let stopping = false;

const command = (args: string[], input?: string): Promise<Output> => run("docker", args, { input, limitMs: DOCKER_LIMIT_MS });

const docker = (args: string[], input?: string): Promise<Output> => {
  if (stopping) return Promise.reject(new Error("the run is stopping"));
  const started = command(args, input);
  running.add(started);
  const done = () => running.delete(started);
  started.then(done, done);
  return started;
};

const said = (output: Output) => `exit ${output.status}: ${`${output.stdout.toString()}${output.stderr}`.trim().slice(0, 300)}`;

// ---------------------------------------------------------------------------
// The containers and volumes this run makes, all removed at its end

class Scratch {
  readonly prefix = `ladderback-smoke-${randomBytes(4).toString("hex")}`;
  readonly containers: string[] = [];
  readonly volumes: string[] = [];

  private closing?: Promise<void>;

  async volume(name: string): Promise<string> {
    const volume = `${this.prefix}-${name}`;
    // Named before it is made, so a cancellation during `create` still removes it.
    this.volumes.push(volume);
    const made = await docker(["volume", "create", volume]);
    if (made.status !== 0) throw new Error(`docker volume create ${volume}: ${made.stderr.trim()}`);
    return volume;
  }

  container(name: string): string {
    const container = `${this.prefix}-${name}`;
    this.containers.push(container);
    return container;
  }

  /**
   * Notes each container's log in `report`, then removes the containers and the
   * volumes, once however often it is called. A removal that fails is a failed
   * check: `rm --force` exits 0 for one that is already gone.
   */
  close(report: Report): Promise<void> {
    this.closing ??= this.removeAll(report);
    return this.closing;
  }

  private async removeAll(report: Report): Promise<void> {
    stopping = true;
    await Promise.allSettled([...running]);
    const failures: string[] = [];
    for (const container of this.containers) {
      const logs = await command(["logs", container]);
      if (logs.status === 0)
        report.note(details(`${container}: the daemon's log`, `${logs.stdout.toString()}${logs.stderr}` || "(nothing)"));
      const removed = await command(["rm", "--force", "--volumes", container]);
      if (removed.status !== 0) failures.push(`${container}: ${said(removed)}`);
    }
    for (const volume of this.volumes) {
      const removed = await command(["volume", "rm", "--force", volume]);
      if (removed.status !== 0) failures.push(`${volume}: ${said(removed)}`);
    }
    report.add(
      "the run removes what it made",
      failures.length === 0,
      failures.join("; ") || `${this.containers.length} container(s), ${this.volumes.length} volume(s)`,
    );
  }
}

type Volumes = { config: string; data: string; cache: string; source: string; repository: string; secrets: string };

/** The agent's mounts: its own folders, the source read-only, the repository and the secrets. */
const mounts = (volumes: Volumes): string[] =>
  [
    `${volumes.config}:${CONFIG}`,
    `${volumes.data}:${DATA}`,
    `${volumes.cache}:${CACHE}`,
    `${volumes.source}:${SOURCE}:ro`,
    `${volumes.repository}:${REPOSITORY}`,
    `${volumes.secrets}:${SECRETS}:ro`,
  ].flatMap((mount) => ["--volume", mount]);

/**
 * Runs `script` with `sh` as root in a container of the image with no
 * network, `input` as its standard input: how the test puts files on the
 * volumes before the agent starts, as a person would on the host.
 */
async function prepare(image: string, scratch: Scratch, volumes: string[], script: string, input = ""): Promise<void> {
  const ran = await docker(
    [
      "run",
      "--rm",
      // Named, so that the run's end removes it should the client be killed.
      "--name",
      scratch.container(`prepare-${scratch.containers.length}`),
      "--interactive",
      "--network",
      "none",
      "--user",
      "0",
      "--entrypoint",
      "sh",
      ...volumes.flatMap((mount) => ["--volume", mount]),
      image,
      "-c",
      `set -eu; ${script}`,
    ],
    input,
  );
  if (ran.status !== 0) throw new Error(`preparing the volumes: ${said(ran)}`);
}

// ---------------------------------------------------------------------------
// HTTP, from this machine to the published port

type Session = { cookie: string; csrf: string };
type Answer = { status: number; text: string; cookies: string[] };

async function call(base: string, method: string, target: string, body?: unknown, session?: Session): Promise<Answer> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (session) {
    headers.cookie = session.cookie;
    if (method !== "GET") headers["x-csrf-token"] = session.csrf;
  }
  try {
    const response = await fetch(`${base}${target}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(REQUEST_LIMIT_MS),
    });
    return { status: response.status, text: await response.text(), cookies: response.headers.getSetCookie() };
  } catch (error) {
    return { status: 0, text: (error as Error).message, cookies: [] };
  }
}

function field(text: string, ...keys: string[]): unknown {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return undefined;
  }
  for (const key of keys) value = value !== null && typeof value === "object" ? (value as Record<string, unknown>)[key] : undefined;
  return value;
}

async function signIn(base: string, report: Report, when: string): Promise<Session | undefined> {
  const answer = await call(base, "POST", "/api/v1/session", { password: ADMIN_PASSWORD });
  const cookie = answer.cookies.find((text) => text.startsWith("ladderback-session="))?.split(";")[0];
  const csrf = field(answer.text, "csrf-token");
  const ok = answer.status === 200 && cookie !== undefined && typeof csrf === "string";
  report.add(`${when}: signing in with the admin password`, ok, `${answer.status}${ok ? "" : ` ${answer.text.slice(0, 300)}`}`);
  return ok ? { cookie: cookie as string, csrf: csrf as string } : undefined;
}

/** Sign-in is on: a request without a session is told it is signed out, and refused the operations. */
async function checkSignInOn(base: string, report: Report, when: string): Promise<void> {
  const session = await call(base, "GET", "/api/v1/session");
  report.add(
    `${when}: sign-in is on`,
    session.status === 200 && field(session.text, "sign-in") === "signed-out",
    `${session.status} ${session.text.slice(0, 300)}`,
  );
  const operations = await call(base, "GET", "/api/v1/operations");
  report.add(`${when}: the operations need a session`, operations.status === 401, `${operations.status}`);
}

// ---------------------------------------------------------------------------
// The agent

/**
 * Starts the agent as the install guide does, waits until it listens, and
 * returns the address of its published port, or `undefined` after saying why.
 */
async function startAgent(image: string, name: string, volumes: Volumes, report: Report, when: string): Promise<string | undefined> {
  const started = await docker([
    "run",
    "--detach",
    "--name",
    name,
    "--hostname",
    HOSTNAME,
    "--init",
    "--publish",
    "127.0.0.1::9898",
    // A release feed nothing answers at, so the test never asks GitHub.
    "--env",
    "LADDERBACK_RELEASES_URL=http://127.0.0.1:9/",
    "--env",
    `TZ=${ZONE}`,
    ...mounts(volumes),
    image,
    "serve",
    "--listen",
    "0.0.0.0:9898",
    "--allowed-host",
    "localhost",
  ]);
  if (!report.add(`${when}: the container starts`, started.status === 0, said(started))) return undefined;
  const begun = Date.now();
  let log = "";
  while (Date.now() - begun < LISTEN_LIMIT_MS) {
    const logs = await docker(["logs", name]);
    log = `${logs.stdout.toString()}${logs.stderr}`;
    if (/listening on http:\/\//.test(log)) break;
    const running = await docker(["inspect", "--format", "{{.State.Running}}", name]);
    if (running.stdout.toString().trim() !== "true") break;
    await sleep(250);
  }
  const listening = /listening on http:\/\//.test(log);
  if (
    !report.add(
      `${when}: serve starts`,
      listening,
      listening ? `after ${((Date.now() - begun) / 1000).toFixed(1)} s` : log.slice(-1000),
    )
  )
    return undefined;
  const port = await docker(["port", name, PORT]);
  const address = port.stdout.toString().split("\n")[0]?.trim() ?? "";
  if (!report.add(`${when}: the port is published`, /^127\.0\.0\.1:\d+$/.test(address), address || said(port))) return undefined;
  // Docker may forward the published port a moment after the daemon listens
  // (Docker Desktop and colima forward it from a VM): wait for an answer.
  const base = `http://${address}`;
  const asked = Date.now();
  let answer = await call(base, "GET", "/api/v1/session");
  while (answer.status === 0 && Date.now() - asked < LISTEN_LIMIT_MS) {
    await sleep(250);
    answer = await call(base, "GET", "/api/v1/session");
  }
  return report.add(
    `${when}: the port answers`,
    answer.status !== 0,
    `${answer.status} after ${((Date.now() - asked) / 1000).toFixed(1)} s`,
  )
    ? base
    : undefined;
}

async function checkImage(image: string, version: string, scratch: Scratch, report: Report): Promise<void> {
  const inspected = await docker(["image", "inspect", "--format", "{{json .Config}}", image]);
  if (!report.add("the image is there", inspected.status === 0, inspected.status === 0 ? image : said(inspected)))
    throw new Error(`no image ${image}`);
  const config = JSON.parse(inspected.stdout.toString()) as {
    User?: string;
    ExposedPorts?: Record<string, unknown>;
    Labels?: Record<string, string>;
  };
  report.add("it runs as 10001:10001, not root", config.User === USER, `User ${JSON.stringify(config.User ?? "")}`);
  report.add("it exposes 9898", Object.hasOwn(config.ExposedPorts ?? {}, PORT), Object.keys(config.ExposedPorts ?? {}).join(", "));
  const labels = config.Labels ?? {};
  const label = (key: string) => labels[`org.opencontainers.image.${key}`];
  report.add(
    "its labels name the source, the licenses and the version",
    label("source") === "https://github.com/nledford/Ladderback" &&
      label("licenses") === "MIT OR Apache-2.0" &&
      label("version") === version,
    `source ${label("source")}, licenses ${label("licenses")}, version ${label("version")}`,
  );

  const shownVersion = await docker(["run", "--rm", "--name", scratch.container("version"), "--network", "none", image, "--version"]);
  const text = shownVersion.stdout.toString().trim();
  report.add(
    "--version",
    shownVersion.status === 0 && text === `ladderback ${version}`,
    `said ${JSON.stringify(text)}${shownVersion.status === 0 ? "" : `, ${said(shownVersion)}`}`,
  );

  // A release has no hub (§16, decision 122): the argument parser refuses
  // its commands, exit 2, before anything starts.
  for (const args of [["serve", "--hub", "--listen", "127.0.0.1:0"], ["pairing-token"]]) {
    const refused = await docker(["run", "--rm", "--name", scratch.container(args[0] ?? "hub"), "--network", "none", image, ...args]);
    report.add(`no hub: ${args.slice(0, 2).join(" ")} is refused`, refused.status === 2, said(refused));
  }

  // The image's own command listens beyond loopback and names no host, so
  // the daemon refuses it rather than the image opening it for the person.
  const bare = await docker(["run", "--rm", "--name", scratch.container("bare"), "--network", "none", image]);
  const output = `${bare.stdout.toString()}${bare.stderr}`;
  report.add(
    "with no arguments it refuses to listen without --allowed-host",
    bare.status !== 0 && output.includes("refusing to listen on 0.0.0.0:9898") && output.includes("--allowed-host"),
    said(bare),
  );
}

// The install guide's Compose file still parses, as `docker compose` reads
// it. Parsing only: it runs nothing and needs none of the files it names.
async function checkCompose(report: Report): Promise<void> {
  const file = path.join(ROOT, COMPOSE);
  const parsed = await docker(["compose", "--file", file, "config", "--quiet"]);
  report.add("the example Compose file parses", parsed.status === 0, parsed.status === 0 ? COMPOSE : said(parsed));
}

async function checkAgent(image: string, scratch: Scratch, report: Report): Promise<void> {
  const volumes: Volumes = {
    config: await scratch.volume("config"),
    data: await scratch.volume("data"),
    cache: await scratch.volume("cache"),
    source: await scratch.volume("source"),
    repository: await scratch.volume("repository"),
    secrets: await scratch.volume("secrets"),
  };
  // Demo files any user may read, a repository folder the image's user owns,
  // and a password file and config.toml that only it can read; the config
  // and data volumes are new, so they take the image's folders' owner.
  await prepare(
    image,
    scratch,
    [`${volumes.source}:${SOURCE}`, `${volumes.repository}:${REPOSITORY}`],
    `for n in 1 2 3; do echo "demo file $n" > ${SOURCE}/file-$n.txt; done; chmod 0644 ${SOURCE}/*; chown ${USER} ${REPOSITORY}`,
  );
  await prepare(
    image,
    scratch,
    [`${volumes.secrets}:${SECRETS}`],
    `cat > ${PASSWORD_FILE}; chown ${USER} ${PASSWORD_FILE}; chmod 0400 ${PASSWORD_FILE}`,
    `${PASSWORD}\n`,
  );
  await prepare(
    image,
    scratch,
    [`${volumes.config}:${CONFIG}`],
    `cat > ${CONFIG}/config.toml; chown ${USER} ${CONFIG}/config.toml; chmod 0600 ${CONFIG}/config.toml`,
    [
      "schema = 1",
      "",
      `[repositories.${PLAN}]`,
      `repository = ${JSON.stringify(REPOSITORY)}`,
      `password-file = ${JSON.stringify(PASSWORD_FILE)}`,
      "",
      "[[plans]]",
      `id = ${JSON.stringify(PLAN)}`,
      `repository = ${JSON.stringify(PLAN)}`,
      `sources = [${JSON.stringify(SOURCE)}]`,
      "",
    ].join("\n"),
  );

  // The admin password, read as one line of standard input, never an argument.
  const password = await docker(
    [
      "run",
      "--rm",
      "--name",
      scratch.container("set-password"),
      "--interactive",
      "--network",
      "none",
      "--volume",
      `${volumes.data}:${DATA}`,
      image,
      "set-password",
    ],
    `${ADMIN_PASSWORD}\n`,
  );
  if (!report.add("set-password keeps an admin password on the data volume", password.status === 0, said(password))) return;

  const first = scratch.container("agent");
  const base = await startAgent(image, first, volumes, report, "first container");
  if (base === undefined) return;

  const page = await call(base, "GET", "/");
  report.add(
    "GET / serves the web UI",
    page.status === 200 && page.text.includes("<title>Ladderback</title>"),
    `${page.status}, ${page.text.length} bytes`,
  );
  const sync = await call(base, "GET", "/sync/v1");
  report.add("no hub: GET /sync/v1 answers 404", sync.status === 404, `${sync.status}`);
  await checkSignInOn(base, report, "first container");
  const session = await signIn(base, report, "first container");
  if (session === undefined) return;

  const init = await call(
    base,
    "POST",
    "/api/v1/repositories/_init",
    { settings: { repository: REPOSITORY, password: { source: "file", path: PASSWORD_FILE } } },
    session,
  );
  if (
    !report.add("a scratch repository is initialized on its volume", init.status === 201, `${init.status} ${init.text.slice(0, 300)}`)
  )
    return;

  const started = await call(base, "POST", "/api/v1/operations", { plan: PLAN }, session);
  const operation = started.status === 202 ? field(started.text, "operation") : undefined;
  if (!report.add("a backup is accepted", typeof operation === "string", `${started.status} ${started.text.slice(0, 300)}`)) return;

  let status: unknown;
  let detail = "";
  const begun = Date.now();
  while (Date.now() - begun < BACKUP_LIMIT_MS) {
    const answer = await call(base, "GET", `/api/v1/operations/${operation as string}`, undefined, session);
    detail = answer.text;
    status = answer.status === 200 ? field(answer.text, "operation", "status") : undefined;
    if (typeof status === "string" && ["succeeded", "succeeded-with-warnings", "failed", "cancelled"].includes(status)) break;
    await sleep(250);
  }
  report.add(
    "the backup of the read-only mount succeeds",
    status === "succeeded",
    `${String(status)} after ${((Date.now() - begun) / 1000).toFixed(1)} s${status === "succeeded" ? "" : `: ${detail.slice(0, 500)}`}`,
  );

  const listed = await call(base, "GET", `/api/v1/repositories/${PLAN}/snapshots`, undefined, session);
  const snapshots = listed.status === 200 ? field(listed.text, "snapshots") : undefined;
  const snapshot = Array.isArray(snapshots) && snapshots.length === 1 ? (snapshots[0] as Record<string, unknown>) : undefined;
  report.add(
    "one snapshot, of the container's hostname, path and time zone",
    snapshot !== undefined &&
      snapshot.hostname === HOSTNAME &&
      JSON.stringify(snapshot.paths) === JSON.stringify([SOURCE]) &&
      snapshot.offset === OFFSET,
    snapshot === undefined
      ? `${listed.status} ${listed.text.slice(0, 300)}`
      : `hostname ${JSON.stringify(snapshot.hostname)}, paths ${JSON.stringify(snapshot.paths)}, offset ${JSON.stringify(snapshot.offset)}`,
  );

  const stopped = await docker(["stop", "--time", STOP_SECONDS, first]);
  const exit = await docker(["inspect", "--format", "{{.State.ExitCode}}", first]);
  report.add(
    "docker stop ends the daemon cleanly",
    stopped.status === 0 && exit.stdout.toString().trim() === "0",
    `exit status ${exit.stdout.toString().trim()}`,
  );

  // A container made anew, as an update makes one: the volumes hold all
  // the agent knew.
  const second = scratch.container("agent-again");
  const again = await startAgent(image, second, volumes, report, "new container");
  if (again === undefined) return;

  await checkSignInOn(again, report, "new container");
  const renewed = await signIn(again, report, "new container");
  if (renewed === undefined) return;
  const history = await call(again, "GET", "/api/v1/operations", undefined, renewed);
  report.add(
    "new container: the backup is in the operation history",
    history.status === 200 && history.text.includes(operation as string),
    `${history.status}`,
  );
  const kept = await call(again, "GET", `/api/v1/repositories/${PLAN}/snapshots`, undefined, renewed);
  const count = kept.status === 200 ? field(kept.text, "snapshots") : undefined;
  report.add(
    "new container: the snapshot is listed",
    Array.isArray(count) && count.length === 1,
    Array.isArray(count) ? `${count.length} snapshot(s)` : `${kept.status} ${kept.text.slice(0, 300)}`,
  );
}

// ---------------------------------------------------------------------------

const USAGE = "usage: node scripts/smoke-image.ts <image> [--version <x.y.z>] [--summary <file>]";

function usage(message: string): never {
  console.error(`error: ${message}`);
  console.error(USAGE);
  process.exit(2);
}

async function main(argv: string[]): Promise<number> {
  let image: string | undefined;
  let version: string | undefined;
  let summary = process.env.GITHUB_STEP_SUMMARY;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i] ?? "";
    const value = () => argv[++i] ?? usage(`${arg} needs a value`);
    if (arg === "--help" || arg === "-h") {
      console.log(USAGE);
      return 0;
    } else if (arg === "--version") version = value();
    else if (arg === "--summary") summary = value();
    else if (arg.startsWith("-")) usage(`unknown option ${arg}`);
    else if (image === undefined) image = arg;
    else usage(`a second image: ${arg}`);
  }
  if (image === undefined) usage("no image given");
  const build = checkoutBuild(ROOT);
  const expected = version ?? (build === undefined ? workspaceVersion(ROOT) : shown(build));

  const report = new Report();
  const scratch = new Scratch();
  // Ctrl-C or a CI cancellation still removes what the run made, by the same
  // cleanup as the end of a run, and says what it couldn't remove.
  for (const signal of ["SIGINT", "SIGTERM"] as const)
    process.once(signal, () => {
      void scratch.close(report).finally(() => {
        if (report.failed) console.error(report.markdown(`Image smoke test, cancelled: ${image}`));
        process.exit(130);
      });
    });
  try {
    await checkImage(image, expected, scratch, report);
    await checkAgent(image, scratch, report);
    await checkCompose(report);
  } catch (error) {
    report.add("the run finishes", false, (error as Error).message);
  } finally {
    await scratch.close(report);
  }

  const text = report.markdown(`Image smoke test: ${image}`);
  console.log(text);
  if (summary) appendFileSync(summary, `${text}\n`);
  console.log(report.failed ? "smoke test: FAILED" : "smoke test: passed");
  return report.failed ? 1 : 0;
}

process.exitCode = await main(process.argv.slice(2));
