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

import type { BrowserContext, WebSocketRoute } from "playwright";

// How long a step waits for the page to open, or ask for, its events stream.
const STREAM_PATIENCE_MS = 10_000;

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** An operation as the daemon's wire has it, in the history the page reads. */
interface Operation {
  id: string;
  plan: string;
  kind: "backup";
  status: string;
  "queued-at": string;
  "started-at": string | null;
  "finished-at": string | null;
  "warning-codes": string[];
  "failure-code": string | null;
  result: Record<string, unknown> | null;
}

export class ScriptedDaemon {
  /** The history the page reads: the operations as the daemon's wire has
   *  them, newest first. */
  #operations: Operation[] = [];
  /** The events stream the page last opened, once it has one open; none
   *  between a drop and the page's next one. */
  #stream: WebSocketRoute | null = null;
  /** Whether the next reading of the history is held back. */
  #holdNext = false;
  /** The answer to the reading held back, as the history was when the page
   *  asked: what the daemon had then. */
  #held: (() => Promise<void>) | null = null;
  /** The readings of the history the page asked for, with its events stream
   *  open, while one was held back. */
  #readsBesideTheHeld = 0;
  /** Whether the next events stream the page asks for is held back. */
  #holdTheOpening = false;
  /** Opens the events stream held back, once the page has asked for one. */
  #openTheHeld: (() => void) | null = null;

  /** Adds a backup of `plan` to the history, newer than every one before it.
   *  Its instants are RFC 3339, UTC; the one that has not happened yet is
   *  left out. */
  addBackup(
    plan: string,
    status: string,
    {
      queuedAt,
      startedAt = null,
      finishedAt = null,
    }: { queuedAt: string; startedAt?: string | null; finishedAt?: string | null },
  ): void {
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
  #backupOf(plan: string): Operation {
    const found = this.#operations.find((operation) => operation.plan === plan);
    if (found === undefined) throw new Error(`the scripted daemon has no backup of ${plan}`);
    return found;
  }

  /** Answers a context's reads of the daemon with this script. */
  async install(context: BrowserContext): Promise<void> {
    await context.route("**/api/v1/operations", async (route) => {
      if (route.request().method() !== "GET") return route.fallback();
      const json = { operations: structuredClone(this.#operations), next: null };
      if (this.#holdNext) {
        this.#holdNext = false;
        this.#held = () => route.fulfill({ json });
        return;
      }
      if (this.#held !== null && this.#stream !== null) this.#readsBesideTheHeld += 1;
      await route.fulfill({ json });
    });
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
    await context.routeWebSocket("**/api/v1/events", async (stream) => {
      // The page sees its stream open once this handler returns.
      if (this.#holdTheOpening) {
        this.#holdTheOpening = false;
        const { promise, resolve } = Promise.withResolvers<void>();
        this.#openTheHeld = resolve;
        await promise;
      }
      this.#stream = stream;
    });
  }

  /** Holds back the opening of the next events stream the page asks for,
   *  until `openTheHeldStream`. */
  holdTheNextStream(): void {
    this.#holdTheOpening = true;
  }

  /** Waits until the page has asked for the stream held back. */
  async #untilAStreamIsHeld(): Promise<void> {
    const end = Date.now() + STREAM_PATIENCE_MS;
    while (this.#openTheHeld === null) {
      if (Date.now() > end) throw new Error("the page never asked for an events stream");
      await sleep(50);
    }
  }

  /** Opens the events stream held back. */
  async openTheHeldStream(): Promise<void> {
    await this.#untilAStreamIsHeld();
    const open = this.#openTheHeld;
    this.#openTheHeld = null;
    open?.();
  }

  /** The backup of `plan` starts while the page has asked for its events
   *  stream and it is not open yet: the history says so, and the news goes
   *  to no page. */
  async startsUnheard(plan: string, startedAt: string): Promise<void> {
    await this.#untilAStreamIsHeld();
    this.#begin(plan, startedAt);
  }

  /** The history has the backup of `plan` running since `startedAt`. */
  #begin(plan: string, startedAt: string): void {
    const backup = this.#backupOf(plan);
    backup.status = "running";
    backup["started-at"] = startedAt;
  }

  /** Holds back the answer to the page's next reading of the history, until
   *  `answerTheHeld`. */
  holdTheNextRead(): void {
    this.#holdNext = true;
  }

  /** Waits until the page has asked for the reading held back. */
  async untilAReadIsHeld(): Promise<void> {
    const end = Date.now() + STREAM_PATIENCE_MS;
    while (this.#held === null) {
      if (Date.now() > end) throw new Error("the page never read the history again");
      await sleep(50);
    }
  }

  /** How many readings of the history the page asked for, its events stream
   *  open, while one was held back. */
  get readsBesideTheHeld(): number {
    return this.#readsBesideTheHeld;
  }

  /** Answers the reading held back, with the history as it was when the page
   *  asked for it. */
  async answerTheHeld(): Promise<void> {
    const held = this.#held;
    if (held === null) throw new Error("no reading of the history is held back");
    this.#held = null;
    await held();
  }

  /** Closes the events stream the page has open; the page opens its next one
   *  after it has read the history again. */
  async dropTheStream(): Promise<void> {
    const stream = this.#stream;
    if (stream === null) throw new Error("the page has no events stream open");
    this.#stream = null;
    await stream.close();
  }

  /** Tells the page `event` over its events stream, once it has one open. */
  async #tell(event: Record<string, unknown>): Promise<void> {
    const end = Date.now() + STREAM_PATIENCE_MS;
    while (this.#stream === null) {
      if (Date.now() > end) throw new Error("the page never opened its events stream");
      await sleep(50);
    }
    this.#stream.send(JSON.stringify(event));
  }

  /** The backup of `plan` begins: its log says it is running before the page
   *  is told, as the daemon's own run tells of a start only after the write
   *  that says so (#305). */
  async starts(plan: string, startedAt: string): Promise<void> {
    this.#begin(plan, startedAt);
    await this.#tell({
      type: "operation-started",
      operation: this.#backupOf(plan).id,
      repository: "demo",
      started_at: startedAt,
    });
  }

  /** The backup of `plan` reports `steps` steps of progress, the last one its
   *  whole total, one event each. */
  async progresses(plan: string, steps: number): Promise<void> {
    const operation = this.#backupOf(plan).id;
    for (let current = 1; current <= steps; current += 1) {
      await this.#tell({ type: "operation-progress", operation, current, total: steps });
    }
  }

  /** The backup of `plan` succeeds: its log has the end and what the backup
   *  went through, `files` files, before the page is told, as the daemon's
   *  own run tells of an end only after its last write to the log. The event
   *  carries no result: the page has to read the history again to show it. */
  async succeeds(plan: string, files: number, finishedAt: string): Promise<void> {
    const backup = this.#backupOf(plan);
    backup.status = "succeeded";
    backup["finished-at"] = finishedAt;
    backup.result = { type: "backup", files, bytes: 4096, "duration-ms": 1000 };
    await this.#tell({ type: "operation-succeeded", operation: backup.id, finished_at: finishedAt });
  }

  /** The backup of `plan` fails, the engine's own failure: its log has the
   *  end before the page is told, as with `succeeds`. */
  async fails(plan: string, finishedAt: string): Promise<void> {
    const backup = this.#backupOf(plan);
    backup.status = "failed";
    backup["finished-at"] = finishedAt;
    backup["failure-code"] = "operation-failed";
    await this.#tell({
      type: "operation-failed",
      operation: backup.id,
      code: "operation-failed",
      args: [],
      finished_at: finishedAt,
    });
  }
}
