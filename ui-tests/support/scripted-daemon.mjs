// A daemon the journey plays itself (#124): what the operations page reads of
// it, which is its history, its plans and its events stream, said as the steps
// say.
//
// A real backup reports progress when it happens to. A journey that must hold
// the reader's focus on a control while the row beside it updates cannot wait
// on one: the update would arrive at a moment nobody chose, and whether it fell
// inside what the journey watches would be luck. Here an update reaches the
// page when a step sends it, and only then.
//
// Only the daemon's answers are played. The page is the real one, served by the
// run's own daemon, and a request this does not answer, such as a start or a
// cancel, goes on to that daemon.

// The page opens its events stream once it has read the history.
const STREAM_PATIENCE_MS = 10_000;

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

export class ScriptedDaemon {
  /** The history the page reads: the operations as the daemon's wire has
   *  them, newest first. */
  #operations = [];
  /** The events stream the page last opened, once it has opened one. */
  #stream = null;

  /** Adds a backup of `plan` to the history, newer than every one before it.
   *  Its instants are RFC 3339, UTC; the one that has not happened yet is
   *  left out. */
  addBackup(plan, status, { queuedAt, startedAt = null, finishedAt = null }) {
    this.#operations.unshift({
      id: `018f0000-0000-7000-8000-${String(this.#operations.length + 1).padStart(12, "0")}`,
      plan,
      kind: "backup",
      status,
      "queued-at": queuedAt,
      "started-at": startedAt,
      "finished-at": finishedAt,
      "warning-codes": [],
      "failure-code": null,
      result: null,
    });
  }

  /** The newest backup of `plan` in the history. */
  #backupOf(plan) {
    const found = this.#operations.find((operation) => operation.plan === plan);
    if (found === undefined) throw new Error(`the scripted daemon has no backup of ${plan}`);
    return found;
  }

  /** Answers a context's reads of the daemon with this script. */
  async install(context) {
    await context.route("**/api/v1/operations", (route) =>
      route.request().method() === "GET"
        ? route.fulfill({ json: { operations: this.#operations, next: null } })
        : route.fallback(),
    );
    await context.route("**/api/v1/plans", (route) =>
      route.fulfill({
        json: {
          plans: [...new Set(this.#operations.map((operation) => operation.plan))].map((id) => ({
            id,
            repository: "demo",
          })),
        },
      }),
    );
    await context.routeWebSocket("**/api/v1/events", (stream) => {
      this.#stream = stream;
    });
  }

  /** Tells the page `event` over its events stream, once it has one open. */
  async #tell(event) {
    const end = Date.now() + STREAM_PATIENCE_MS;
    while (this.#stream === null) {
      if (Date.now() > end) throw new Error("the page never opened its events stream");
      await sleep(50);
    }
    this.#stream.send(JSON.stringify(event));
  }

  /** The backup of `plan` begins. */
  async starts(plan, startedAt) {
    await this.#tell({
      type: "operation-started",
      operation: this.#backupOf(plan).id,
      repository: "demo",
      started_at: startedAt,
    });
  }

  /** The backup of `plan` reports `steps` steps of progress, the last one its
   *  whole total, one event each. */
  async progresses(plan, steps) {
    const operation = this.#backupOf(plan).id;
    for (let current = 1; current <= steps; current += 1) {
      await this.#tell({ type: "operation-progress", operation, current, total: steps });
    }
  }
}
