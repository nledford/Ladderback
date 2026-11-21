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

/** A stand-in daemon that idles until a signal ends it, or that ignores
 *  SIGTERM; resolves once it is ready for the signal. Killed when the test
 *  ends, should the test leave it running. */
async function standIn(t: TestContext, { ignoresSigterm = false } = {}): Promise<ChildProcess> {
  const code = `${ignoresSigterm ? "process.on('SIGTERM', () => {});" : ""} console.log('ready'); setInterval(() => {}, 1000);`;
  const daemon = spawn(process.execPath, ["-e", code], { stdio: ["ignore", "pipe", "inherit"] });
  t.after(() => {
    if (daemon.exitCode === null && daemon.signalCode === null) daemon.kill("SIGKILL");
  });
  await once(daemon.stdout, "data");
  return daemon;
}

/** A scratch folder with a file in it. */
async function scratchFolder(): Promise<string> {
  const scratch = await mkdtemp(path.join(tmpdir(), "ladderback-ui-teardown-test-"));
  await writeFile(path.join(scratch, "file"), "x");
  return scratch;
}

test("the daemon stops and the scratch folder goes while the browser never closes", async (t) => {
  const daemon = await standIn(t);
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

test("a browser that fails to close still leaves no daemon nor scratch folder, and its failure is passed on", async (t) => {
  const daemon = await standIn(t);
  const scratch = await scratchFolder();
  const browser = { close: () => Promise.reject(new Error("the browser crashed")) };
  await assert.rejects(tearDown({ browser, daemon, scratch }), /the browser crashed/);
  assert.equal(daemon.signalCode, "SIGTERM", "the daemon was not stopped");
  assert.equal(existsSync(scratch), false, "the scratch folder is still there");
});

test("a daemon that ignores SIGTERM is killed once its grace runs out", async (t) => {
  const daemon = await standIn(t, { ignoresSigterm: true });
  await stopDaemon(daemon, 200);
  assert.equal(daemon.signalCode, "SIGKILL");
});

test("a daemon that has already died, of a signal too, is not waited for", { timeout: 5_000 }, async (t) => {
  const daemon = await standIn(t);
  daemon.kill("SIGKILL");
  await once(daemon, "exit");
  await stopDaemon(daemon, 200);
  assert.equal(daemon.signalCode, "SIGKILL");
});

// As cucumber-js runs it: a worker thread starts the daemon, the run ends by
// terminating the worker, which emits no `exit` of its own, and the process
// then exits by itself.
test("a daemon still running when the run's process exits is killed then", async () => {
  const worker = `
    import { spawn } from "node:child_process";
    import { parentPort } from "node:worker_threads";
    import { backstop } from ${JSON.stringify(teardownModule)};
    const daemon = backstop(spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" }));
    parentPort.postMessage(daemon.pid);
  `;
  const run = `
    import { once } from "node:events";
    import { Worker } from "node:worker_threads";
    import ${JSON.stringify(teardownModule)};
    const worker = new Worker(${JSON.stringify(worker)}, { eval: true });
    const [pid] = await once(worker, "message");
    console.log(pid);
    // The worker's news of its daemon reaches this thread's backstop.
    await new Promise((resolve) => setTimeout(resolve, 200));
    await worker.terminate();
  `;
  const runner = spawn(process.execPath, ["--input-type=module", "-e", run], { stdio: ["ignore", "pipe", "inherit"] });
  let said = "";
  runner.stdout.on("data", (chunk: Buffer) => {
    said += chunk;
  });
  const [code] = await once(runner, "exit");
  assert.equal(code, 0, "the run failed");
  const pid = Number(said.trim());
  assert.ok(Number.isInteger(pid) && pid > 0, `the run named no daemon: ${JSON.stringify(said)}`);
  const alive = () => {
    try {
      process.kill(pid, 0);
      return true;
    } catch {
      return false;
    }
  };
  const by = Date.now() + 2_000;
  while (alive() && Date.now() < by) await sleep(20);
  const left = alive();
  if (left) process.kill(pid, "SIGKILL");
  assert.equal(left, false, `the daemon ${pid} outlived the run`);
});
