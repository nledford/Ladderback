// A daemon the journey plays itself (#124): what the operations page reads of
// it, which is its history, its plans and its events stream, said as the steps
// say, and, when a step asks, its answers to a start and to a cancel and the
// warnings it adds to a row of the repositories list.
//
// A real backup reports progress when it happens to. A journey that must hold
// the reader's focus on a control while the row beside it updates cannot wait
// on one: the update would arrive at a moment nobody chose, and whether it fell
// inside what the journey watches would be luck. Here an update reaches the
// page when a step sends it, and only then.
//
// Only the daemon's answers are played. The page is the real one, served by the
// run's own daemon, and a request this does not answer, such as a start or a
// cancel unless a step asked, goes on to that daemon.

import type { BrowserContext, Route, WebSocketRoute } from "playwright";

// How long a step waits for the page to open, or ask for, its events stream.
const STREAM_PATIENCE_MS = 10_000;

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** An operation as the daemon's wire has it, in the history the page reads. */
interface Operation {
  id: string;
  plan: string;
  kind: "backup" | "check";
  status: string;
  "queued-at": string;
  "started-at": string | null;
  "finished-at": string | null;
  "warning-codes": string[];
  "failure-code": string | null;
  result: Record<string, unknown> | null;
  repository?: string;
}

/** What a progress bar counts, as the wire names it (decision 83). */
export type ProgressUnit = "count" | "bytes";

/** A warning as the daemon's wire has it: a code and its arguments. */
export interface Warning {
  code: string;
  args?: Record<string, { type: string; value: unknown }>;
}

export class ScriptedDaemon {
  /** The history the page reads: the operations as the daemon's wire has
   *  them, newest first. */
  #operations: Operation[] = [];
  /** The events stream the page last opened, once it has one open; none
   *  between a drop, or the page's own close, and the page's next one. */
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
  /** The readings of the history the page asked for, and was answered,
   *  since its events stream last opened. */
  #readsSinceTheOpening = 0;
  /** The events streams the page has open, as this daemon sees them. */
  #open = new Set<WebSocketRoute>();
  /** How many events streams the page has opened, and how many times it has
   *  read the history: what a page that is gone must stop adding to. */
  #streamsOpened = 0;
  #historyReads = 0;
  /** Whether this daemon answers the page's starts itself (`answerStarts`). */
  #answersStarts = false;
  /** Whether this daemon accepts the page's cancels itself
   *  (`acceptCancels`), and the operations whose cancel it has accepted. */
  #acceptsCancels = false;
  #cancelsAccepted = new Set<string>();
  /** For each reading of the history the page asked for, in order, whether
   *  it marked the request as sent on its own. */
  #backgroundMarks: boolean[] = [];
  /** The warnings this daemon adds to rows of the repositories list, by the
   *  repository's alias (`warnOfRepository`). */
  #repositoryWarnings = new Map<string, Warning[]>();

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

  /** Adds a check of the repository `repository` (its alias on this
   *  agent) to the history, running since `startedAt`. */
  addRunningCheck(repository: string, startedAt: string): void {
    this.#operations.unshift({
      id: `018f0000-0000-7000-8000-${String(this.#operations.length + 1).padStart(12, "0")}`,
      plan: "",
      kind: "check",
      status: "running",
      "queued-at": startedAt,
      "started-at": startedAt,
      "finished-at": null,
      "warning-codes": [],
      "failure-code": null,
      result: null,
      repository,
    });
  }

  /** The newest backup of `plan` in the history. */
  #backupOf(plan: string): Operation {
    const found = this.#operations.find((operation) => operation.kind === "backup" && operation.plan === plan);
    if (found === undefined) throw new Error(`the scripted daemon has no backup of ${plan}`);
    return found;
  }

  /** The newest check of `repository` in the history. */
  #checkOf(repository: string): Operation {
    const found = this.#operations.find(
      (operation) => operation.kind === "check" && operation.repository === repository,
    );
    if (found === undefined) throw new Error(`the scripted daemon has no check of ${repository}`);
    return found;
  }

  /** Adds `warning` to the row of `alias` in the repositories list, as the
   *  daemon gives one when it notes something of that repository; the rest
   *  of the list is the run's own daemon's answer, and a warning it already
   *  gives is not given twice. */
  warnOfRepository(alias: string, warning: Warning): void {
    this.#repositoryWarnings.set(alias, [...(this.#repositoryWarnings.get(alias) ?? []), warning]);
  }

  /** Answers a context's reads of the daemon with this script. */
  async install(context: BrowserContext): Promise<void> {
    await context.route("**/api/v1/operations/*/cancel", async (route) => {
      if (route.request().method() !== "POST" || !this.#acceptsCancels) return route.fallback();
      const operation = new URL(route.request().url()).pathname.split("/").at(-2) ?? "";
      this.#cancelsAccepted.add(operation);
      await route.fulfill({ json: {} });
    });
    await context.route("**/api/v1/operations", async (route) => {
      if (route.request().method() === "POST" && this.#answersStarts) return this.#start(route);
      if (route.request().method() !== "GET") return route.fallback();
      this.#historyReads += 1;
      this.#backgroundMarks.push(route.request().headers()["x-ladderback-background"] !== undefined);
      const json = { operations: structuredClone(this.#operations), next: null };
      if (this.#holdNext) {
        this.#holdNext = false;
        this.#held = () => route.fulfill({ json });
        return;
      }
      const open = this.#stream;
      if (this.#held !== null && open !== null) this.#readsBesideTheHeld += 1;
      await route.fulfill({ json });
      if (open !== null && open === this.#stream) this.#readsSinceTheOpening += 1;
    });
    await context.route("**/api/v1/plans", (route) =>
      route.fulfill({
        json: {
          plans: [
            ...new Set(
              this.#operations.filter((operation) => operation.kind === "backup").map((operation) => operation.plan),
            ),
          ].map((id) => ({
            id,
            repository: "demo",
            scheduled: false,
            "next-run": null,
          })),
        },
      }),
    );
    await context.route("**/api/v1/repositories", async (route) => {
      if (route.request().method() !== "GET" || this.#repositoryWarnings.size === 0) return route.fallback();
      const response = await route.fetch();
      const json = (await response.json()) as { repositories: Array<{ alias: string; warnings?: Warning[] }> };
      const same = (one: Warning, other: Warning) =>
        one.code === other.code && JSON.stringify(one.args ?? {}) === JSON.stringify(other.args ?? {});
      for (const row of json.repositories) {
        const warnings = row.warnings ?? [];
        for (const added of this.#repositoryWarnings.get(row.alias) ?? []) {
          if (!warnings.some((given) => same(given, added))) warnings.push(added);
        }
        if (warnings.length > 0) row.warnings = warnings;
      }
      await route.fulfill({ response, json });
    });
    await context.routeWebSocket("**/api/v1/events", async (stream) => {
      // The page sees its stream open once this handler returns.
      if (this.#holdTheOpening) {
        this.#holdTheOpening = false;
        const { promise, resolve } = Promise.withResolvers<void>();
        this.#openTheHeld = resolve;
        await promise;
      }
      this.#stream = stream;
      this.#readsSinceTheOpening = 0;
      this.#open.add(stream);
      this.#streamsOpened += 1;
      // The page closing its stream; the daemon's own drop is `dropTheStream`.
      // A step after it waits for the page's next stream, never this one.
      stream.onClose(() => {
        this.#open.delete(stream);
        if (this.#stream === stream) this.#stream = null;
      });
    });
  }

  /** Has this daemon answer each start the page asks for: the backup is
   *  queued, and it never starts unless a step says so. */
  answerStarts(): void {
    this.#answersStarts = true;
  }

  /** Queues a backup of the plan the start names and answers with it, as the
   *  daemon answers a start. */
  async #start(route: Route): Promise<void> {
    const { plan } = route.request().postDataJSON() as { plan: string };
    this.addBackup(plan, "queued", { queuedAt: "2026-11-09T10:00:10Z" });
    await route.fulfill({ status: 202, json: { operation: this.#backupOf(plan).id } });
  }

  /** Has this daemon accept each cancel the page asks for, as the daemon
   *  does, while the run goes on: it ends only when a step says so. */
  acceptCancels(): void {
    this.#acceptsCancels = true;
  }

  /** Waits until this daemon has accepted the cancel of the backup of
   *  `plan`. */
  async untilCancelAccepted(plan: string): Promise<void> {
    const operation = this.#backupOf(plan).id;
    const end = Date.now() + STREAM_PATIENCE_MS;
    while (!this.#cancelsAccepted.has(operation)) {
      if (Date.now() > end) throw new Error(`the page never asked to cancel the backup of ${plan}`);
      await sleep(50);
    }
  }

  /** How many events streams the page has open. */
  get openStreams(): number {
    return this.#open.size;
  }

  /** How many events streams the page has opened so far. */
  get streamsOpened(): number {
    return this.#streamsOpened;
  }

  /** How many times the page has read the history so far. */
  get historyReads(): number {
    return this.#historyReads;
  }

  /** Waits until the page has `count` events streams open, or says how many
   *  it kept instead. */
  async untilStreamsOpen(count: number): Promise<void> {
    const end = Date.now() + STREAM_PATIENCE_MS;
    while (this.#open.size !== count) {
      if (Date.now() > end) {
        throw new Error(`the page has ${this.#open.size} events streams open, not ${count}`);
      }
      await sleep(50);
    }
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

  /** Waits until the page has asked for the history, and been answered, since
   *  its events stream last opened: the history it read is the one it had
   *  then, whatever the steps after this change. */
  async untilReadSinceTheOpening(): Promise<void> {
    const end = Date.now() + STREAM_PATIENCE_MS;
    while (this.#readsSinceTheOpening === 0) {
      if (Date.now() > end) throw new Error("the page never read the history after its events stream opened");
      await sleep(50);
    }
  }

  /** The backup of `plan` starts while the page has asked for its events
   *  stream and it is not open yet: the history says so, and the news goes
   *  to no page. */
  async startsUnheard(plan: string, startedAt: string): Promise<void> {
    await this.#untilAStreamIsHeld();
    this.#begin(plan, startedAt);
  }

  /** A before-hook of the backup of `plan` warns while the page has asked for
   *  its events stream and it is not open yet: the run's start write puts the
   *  warning in the history, and no event tells of it, then or later. */
  async warnsUnheard(plan: string): Promise<void> {
    await this.#untilAStreamIsHeld();
    this.#backupOf(plan)["warning-codes"].push("hook-warning");
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

  /** For each reading of the history the page asked for, in order, whether
   *  it marked the request as sent on its own. */
  get backgroundMarks(): readonly boolean[] {
    return this.#backgroundMarks;
  }

  /** Answers the reading held back, with the history as it was when the page
   *  asked for it. */
  async answerTheHeld(): Promise<void> {
    const held = this.#held;
    if (held === null) throw new Error("no reading of the history is held back");
    this.#held = null;
    await held();
  }

  /** Closes the events stream the page has open, once it has one; the page
   *  opens its next one after it has read the history again. */
  async dropTheStream(): Promise<void> {
    const stream = await this.#openStream();
    this.#stream = null;
    this.#open.delete(stream);
    await stream.close();
  }

  /** The events stream the page has open, once it has one. */
  async #openStream(): Promise<WebSocketRoute> {
    const end = Date.now() + STREAM_PATIENCE_MS;
    while (this.#stream === null) {
      if (Date.now() > end) throw new Error("the page never opened its events stream");
      await sleep(50);
    }
    return this.#stream;
  }

  /** Tells the page `event` over its events stream, once it has one open. */
  async #tell(event: Record<string, unknown>): Promise<void> {
    (await this.#openStream()).send(JSON.stringify(event));
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

  /** The backup of `plan` reports `bytes` processed and no total, as the
   *  daemon does while rustic is still walking the source for its size. */
  async progressesWithoutTotal(plan: string, bytes: number): Promise<void> {
    await this.#tell({ type: "operation-progress", operation: this.#backupOf(plan).id, current: bytes, total: null });
  }

  /** The backup of `plan` reports `current` of `total` (`null`: not known
   *  yet) in the step whose code is `step`, as the daemon does for the bar
   *  the engine began last; `unit` is what the bar counts, left out as a
   *  daemon older than it leaves it out. */
  async progressesInStep(
    plan: string,
    step: string,
    current: number,
    total: number | null,
    unit?: ProgressUnit,
  ): Promise<void> {
    await this.#tell({ type: "operation-progress", operation: this.#backupOf(plan).id, step, unit, current, total });
  }

  /** The check of `repository` reports `current` of `total` things counted
   *  in the step whose code is `step`. */
  async checkProgressesInStep(repository: string, step: string, current: number, total: number): Promise<void> {
    await this.#tell({
      type: "operation-progress",
      operation: this.#checkOf(repository).id,
      step,
      unit: "count",
      current,
      total,
    });
  }

  /** The backup of `plan` succeeds: its log has the end and what the backup
   *  went through, `files` files, before the page is told, as the daemon's
   *  own run tells of an end only after its last write to the log. A backup
   *  that recorded a warning ends with warnings, in the log as in the daemon.
   *  The event carries no result: the page has to read the history again to
   *  show it. */
  async succeeds(plan: string, files: number, finishedAt: string): Promise<void> {
    const backup = this.#backupOf(plan);
    backup.status = backup["warning-codes"].length === 0 ? "succeeded" : "succeeded-with-warnings";
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

  /** The backup of `plan` ends cancelled: its log has the end, and the page
   *  is told nothing, as the daemon tells of no cancelled run on the events
   *  stream. A page that asked for the cancel finds the end by reading the
   *  history. */
  isCancelled(plan: string, finishedAt: string): void {
    const backup = this.#backupOf(plan);
    backup.status = "cancelled";
    backup["finished-at"] = finishedAt;
  }
}
