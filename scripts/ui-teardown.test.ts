// Tests for how an `@ui` worker ends what it started (ui-tests/support/
// teardown.ts, #866): the daemon stops however the browser's close goes, the
// scratch folder goes whatever failed, and a daemon still running when the
// run's process exits is killed then. `just lint` runs them with `node
// --test`. The daemons are stand-ins, Node processes that idle until stopped;
// signals and process exits take real time, so these tests run on the real
// clock, with windows of a few hundred milliseconds.

import assert from "node:assert/strict";
import { type ChildProcess, spawn } from "node:child_process";
import { once } from "node:events";
import { existsSync } from "node:fs";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { type TestContext, test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { stopDaemon, tearDown } from "../ui-tests/support/teardown.ts";

const teardownModule = new URL("../ui-tests/support/teardown.ts", import.meta.url).href;

/** The source of a stand-in daemon: it idles until a signal ends it, or
 *  ignores SIGTERM, and may start a child that leads a process group of its
 *  own, as a daemon's workers do. Its first line of output is that child's
 *  ID, or 0, once it is ready for the signal. */
function standInCode({ ignoresSigterm = false, withChild = false } = {}): string {
  return `
    const { spawn } = require("node:child_process");
    ${ignoresSigterm ? "process.on('SIGTERM', () => {});" : ""}
    const child = ${withChild} ? spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { detached: true, stdio: "ignore" }).pid : 0;
    console.log(child);
    setInterval(() => {}, 1000);
  `;
}

/** Whether a process `pid` exists. */
function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/** Whether the process `pid` is gone within 2 s; killed if not, so that a
 *  failed test leaves nothing behind. */
async function goneSoon(pid: number): Promise<boolean> {
  const by = Date.now() + 2_000;
  while (alive(pid) && Date.now() < by) await sleep(20);
  if (!alive(pid)) return true;
  process.kill(pid, "SIGKILL");
  return false;
}

/** Kills the processes `pids` when the test ends, should it leave them. */
function killAfter(t: TestContext, ...pids: number[]): void {
  t.after(() => {
    for (const pid of pids) if (pid > 0 && alive(pid)) process.kill(pid, "SIGKILL");
  });
}

/** Starts a stand-in daemon (`standInCode`) and resolves once it is ready,
 *  with its child's ID. */
async function standIn(
  t: TestContext,
  options: { ignoresSigterm?: boolean; withChild?: boolean } = {},
): Promise<{ daemon: ChildProcess; child: number }> {
  const daemon = spawn(process.execPath, ["-e", standInCode(options)], { stdio: ["ignore", "pipe", "inherit"] });
  t.after(() => {
    if (daemon.exitCode === null && daemon.signalCode === null) daemon.kill("SIGKILL");
  });
  const [line] = await once(daemon.stdout, "data");
  const child = Number(String(line).trim());
  killAfter(t, child);
  return { daemon, child };
}

/** A scratch folder with a file in it. */
async function scratchFolder(): Promise<string> {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-teardown-test-"));
  await writeFile(path.join(scratch, "file"), "x");
  return scratch;
}

test("the daemon stops and the scratch folder goes while the browser never closes", { timeout: 10_000 }, async (t) => {
  const { daemon } = await standIn(t);
  const scratch = await scratchFolder();
  const begun = Date.now();
  let stoppedAfter = -1;
  daemon.once("exit", () => {
    stoppedAfter = Date.now() - begun;
  });
  const browser = { close: () => new Promise<void>(() => {}) };
  await tearDown({ browser, daemon, scratch }, { closeMs: 1_000, stopMs: 1_000 });
  assert.equal(daemon.signalCode, "SIGTERM", "the daemon was not stopped by SIGTERM");
  assert.ok(stoppedAfter >= 0 && stoppedAfter < 1_000, `the daemon stopped ${stoppedAfter} ms in, not alongside the close`);
  assert.equal(existsSync(scratch), false, "the scratch folder is still there");
});

test("a browser that fails to close still leaves no daemon nor scratch folder, and its failure is passed on", {
  timeout: 10_000,
}, async (t) => {
  const { daemon } = await standIn(t);
  const scratch = await scratchFolder();
  const browser = { close: () => Promise.reject(new Error("the browser crashed")) };
  await assert.rejects(tearDown({ browser, daemon, scratch }), /the browser crashed/);
  assert.equal(daemon.signalCode, "SIGTERM", "the daemon was not stopped");
  assert.equal(existsSync(scratch), false, "the scratch folder is still there");
});

test("a daemon that ignores SIGTERM is killed with the group its child leads, and the stop says so", {
  timeout: 10_000,
}, async (t) => {
  const { daemon, child } = await standIn(t, { ignoresSigterm: true, withChild: true });
  await assert.rejects(stopDaemon(daemon, 200), /had not stopped 200 ms after SIGTERM/);
  assert.equal(daemon.signalCode, "SIGKILL");
  assert.ok(await goneSoon(child), `the daemon's child ${child} outlived it`);
});

test("a daemon that has already died, of a signal too, is not waited for", { timeout: 10_000 }, async (t) => {
  const { daemon } = await standIn(t);
  daemon.kill("SIGKILL");
  await once(daemon, "exit");
  await stopDaemon(daemon, 200);
  assert.equal(daemon.signalCode, "SIGKILL");
});

// As cucumber-js runs it: a worker thread starts the daemon, the run ends by
// terminating the worker, which emits no `exit` of its own, and the process
// then exits by itself.
test("a daemon still running when the run's process exits is killed then, with its child's group", { timeout: 10_000 }, async (t) => {
  const worker = `
    import { spawn } from "node:child_process";
    import { once } from "node:events";
    import { parentPort } from "node:worker_threads";
    import { backstop } from ${JSON.stringify(teardownModule)};
    const daemon = backstop(spawn(process.execPath, ["-e", ${JSON.stringify(standInCode({ withChild: true }))}], { stdio: ["ignore", "pipe", "inherit"] }));
    const [line] = await once(daemon.stdout, "data");
    parentPort.postMessage([daemon.pid, Number(String(line).trim())]);
  `;
  // The run's own channel is opened after the module's, whose listener
  // therefore has the worker's news first: once this one has it, the
  // backstop knows the daemon.
  const run = `
    import { once } from "node:events";
    import { BroadcastChannel, Worker } from "node:worker_threads";
    import { DAEMONS_CHANNEL } from ${JSON.stringify(teardownModule)};
    const news = new BroadcastChannel(DAEMONS_CHANNEL);
    const heard = new Promise((resolve) => { news.onmessage = resolve; });
    const worker = new Worker(${JSON.stringify(worker)}, { eval: true });
    const [pids] = await once(worker, "message");
    console.log(pids.join(" "));
    await heard;
    news.close();
    await worker.terminate();
  `;
  const runner = spawn(process.execPath, ["--input-type=module", "-e", run], { stdio: ["ignore", "pipe", "inherit"] });
  const [line] = await once(runner.stdout, "data");
  const [daemon = 0, child = 0] = String(line).trim().split(" ").map(Number);
  killAfter(t, daemon, child);
  const [code] = await once(runner, "exit");
  assert.equal(code, 0, "the run failed");
  assert.ok(daemon > 0 && child > 0, `the run named no daemon and child: ${JSON.stringify(String(line))}`);
  assert.ok(await goneSoon(daemon), `the daemon ${daemon} outlived the run`);
  assert.ok(await goneSoon(child), `the daemon's child ${child} outlived the run`);
});
