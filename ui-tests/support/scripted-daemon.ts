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
// real daemon, and a request this does not answer, such as a start or a
// cancel unless a step asked, goes on to that daemon.

import type { BrowserContext, Route, WebSocketRoute } from "playwright";

import { Signal } from "./patience.ts";

/** An operation as the daemon's wire has it, in the history the page reads. */
interface Operation {
  id: string;
  plan: string;
  kind: "backup" | "check" | "restore";
  status: string;
  "queued-at": string;
  "started-at": string | null;
  "finished-at": string | null;
  "warning-codes": string[];
  "failure-code": string | null;
  failure?: { code: string; args?: Record<string, unknown>; detail?: string };
  result: Record<string, unknown> | null;
  repository?: string;
  entry?: string;
  "restored-to"?: { type: "in-place" } | { type: "folder"; path: string };
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
  /** Whether the real daemon has ended every session (`endSessions`):
   *  the history is then that daemon's to answer, which refuses it, until
   *  the page signs in again; and how many readings it refused so. */
  #sessionsEnded = false;
  #readsRefused = 0;
  /** What this daemon does with each reading of the history from now on:
   *  answers it; refuses it, as a daemon that can't read its store does
   *  (`refuseReads`); or gives it no answer, as one that has gone (`goAway`). */
  #reads: "answered" | "refused" | "unanswered" = "answered";
  /** Tells the steps waiting on what the page did of each thing it does. */
  #changed = new Signal();

  /** Adds a backup of `plan` to the history, newer than every one before it.
   *  Its instants are RFC 3339, UTC; the one that has not happened yet is
   *  left out. A failed one carries its failure as the daemon's wire has it:
   *  a code and its arguments; a succeeded one may carry its result, and one
   *  that succeeded with warnings the code of each kind it kept. */
  addBackup(
    plan: string,
    status: string,
    {
      queuedAt,
      startedAt = null,
      finishedAt = null,
      failure,
      result = null,
      warningCodes = [],
    }: {
      queuedAt: string;
      startedAt?: string | null;
      finishedAt?: string | null;
      failure?: { code: string; args?: Record<string, { type: string; value: unknown }> };
      result?: Record<string, unknown> | null;
      warningCodes?: string[];
    },
  ): void {
    this.#operations.unshift({
      id: `018f0000-0000-7000-8000-${String(this.#operations.length + 1).padStart(12, "0")}`,
      plan,
      kind: "backup",
      status,
      "queued-at": queuedAt,
      "started-at": startedAt,
      "finished-at": finishedAt,
      "warning-codes": warningCodes,
      "failure-code": failure?.code ?? null,
      ...(failure === undefined ? {} : { failure }),
      result,
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
    const found = this.#operations.find((operation) => operation.kind === "check" && operation.repository === repository);
    if (found === undefined) throw new Error(`the scripted daemon has no check of ${repository}`);
    return found;
  }

  /** Adds a restore of `entry` from the repository `repository` (its alias
   *  on this agent) to the history, running since `startedAt`, writing into
   *  the new folder `folder`, or in place when `folder` is null (#813). */
  addRunningRestore(entry: string, repository: string, folder: string | null, startedAt: string): void {
    this.#operations.unshift({
      id: `018f0000-0000-7000-8000-${String(this.#operations.length + 1).padStart(12, "0")}`,
      plan: "",
      kind: "restore",
      status: "running",
      "queued-at": startedAt,
      "started-at": startedAt,
      "finished-at": null,
      "warning-codes": [],
      "failure-code": null,
      result: null,
      repository,
      entry,
      "restored-to": folder === null ? { type: "in-place" } : { type: "folder", path: folder },
    });
  }

  /** The newest restore of `entry` in the history. */
  #restoreOf(entry: string): Operation {
    const found = this.#operations.find((operation) => operation.kind === "restore" && operation.entry === entry);
    if (found === undefined) throw new Error(`the scripted daemon has no restore of ${entry}`);
    return found;
  }

  /** The restore of `entry` reports `current` of `total` files restored. */
  async restoreProgresses(entry: string, current: number, total: number): Promise<void> {
    await this.#tell({ type: "operation-progress", operation: this.#restoreOf(entry).id, unit: "count", current, total });
  }

  /** The restore of `entry` succeeds, its log ending before the page is
   *  told, as with `succeeds`. */
  async restoreSucceeds(entry: string, finishedAt: string): Promise<void> {
    const restore = this.#restoreOf(entry);
    restore.status = "succeeded";
    restore["finished-at"] = finishedAt;
    restore.result = { type: "restore", files: 10, bytes: 4096, "duration-ms": 1000 };
    await this.#tell({ type: "operation-succeeded", operation: restore.id, finished_at: finishedAt });
  }

  /** The restore of `entry` fails, the engine's own failure, its log ending
   *  before the page is told, as with `fails`. */
  async restoreFails(entry: string, finishedAt: string): Promise<void> {
    const restore = this.#restoreOf(entry);
    restore.status = "failed";
    restore["finished-at"] = finishedAt;
    restore["failure-code"] = "operation-failed";
    restore.failure = { code: "operation-failed" };
    await this.#tell({
      type: "operation-failed",
      operation: restore.id,
      code: "operation-failed",
      args: [],
      finished_at: finishedAt,
    });
  }

  /** Adds `warning` to the row of `alias` in the repositories list, as the
   *  daemon gives one when it notes something of that repository; the rest
   *  of the list is the real daemon's answer, and a warning it already
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
      this.#changed.notify();
      await route.fulfill({ json: {} });
    });
    await context.route("**/api/v1/operations", async (route) => {
      if (route.request().method() === "POST" && this.#answersStarts) return this.#start(route);
      if (route.request().method() !== "GET") return route.fallback();
      this.#historyReads += 1;
      this.#changed.notify();
      this.#backgroundMarks.push(route.request().headers()["x-ladderback-background"] !== undefined);
      if (this.#sessionsEnded) {
        this.#readsRefused += 1;
        this.#changed.notify();
        return route.fallback();
      }
      if (this.#reads === "refused") {
        return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ code: "store-unavailable" }) });
      }
      if (this.#reads === "unanswered") return route.abort("connectionrefused");
      const json = { operations: structuredClone(this.#operations), next: null };
      if (this.#holdNext) {
        this.#holdNext = false;
        this.#held = () => route.fulfill({ json });
        this.#changed.notify();
        return;
      }
      const open = this.#stream;
      if (this.#held !== null && open !== null) this.#readsBesideTheHeld += 1;
      await route.fulfill({ json });
      if (open !== null && open === this.#stream) this.#readsSinceTheOpening += 1;
      this.#changed.notify();
    });
    // A sign-in that the real daemon accepts gives the history back to
    // this script.
    await context.route("**/api/v1/session", async (route) => {
      if (route.request().method() !== "POST" || !this.#sessionsEnded) return route.fallback();
      const response = await route.fetch();
      if (response.ok()) this.#sessionsEnded = false;
      await route.fulfill({ response });
    });
    await context.route("**/api/v1/plans", (route) =>
      route.fulfill({
        json: {
          plans: [
            ...new Set(this.#operations.filter((operation) => operation.kind === "backup").map((operation) => operation.plan)),
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
        this.#changed.notify();
        await promise;
      }
      this.#stream = stream;
      this.#readsSinceTheOpening = 0;
      this.#open.add(stream);
      this.#streamsOpened += 1;
      this.#changed.notify();
      // The page closing its stream; the daemon's own drop is `dropTheStream`.
      // A step after it waits for the page's next stream, never this one.
      stream.onClose(() => {
        this.#open.delete(stream);
        if (this.#stream === stream) this.#stream = null;
        this.#changed.notify();
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
    await this.#changed.until(
      () => this.#cancelsAccepted.has(operation),
      () => `the page never asked to cancel the backup of ${plan}`,
    );
  }

  /** How many events streams the page has open. */
  get openStreams(): number {
    return this.#open.size;
  }

  /** How many events streams the page has opened so far. */
  get streamsOpened(): number {
    return this.#streamsOpened;
  }

  /** The real daemon has ended every session, as an admin password set
   *  anew does: the page's readings of the history go on to it, which refuses
   *  them, until the page signs in again. */
  endSessions(): void {
    this.#sessionsEnded = true;
  }

  /** The daemon can't read its store: it refuses each reading of the
   *  history from now on (503 `store-unavailable`). */
  refuseReads(): void {
    this.#reads = "refused";
  }

  /** The daemon has gone: each reading of the history from now on gets no
   *  answer. */
  goAway(): void {
    this.#reads = "unanswered";
  }

  /** Waits until the real daemon has refused a reading of the history
   *  since the sessions ended, or says it never was asked. */
  async untilAReadIsRefused(): Promise<void> {
    await this.#changed.until(
      () => this.#readsRefused > 0,
      () => "the page never read the history after its session ended",
    );
  }

  /** How many times the page has read the history so far. */
  get historyReads(): number {
    return this.#historyReads;
  }

  /** Waits until the page has read the history more than `reads` times, or
   *  `ms` has gone by: whichever comes first, and neither a failure. */
  async untilReadPast(reads: number, ms: number): Promise<void> {
    const ends = Date.now() + ms;
    while (this.#historyReads <= reads && Date.now() < ends) await this.#changed.next(ends - Date.now());
  }

  /** Waits until the page has `count` events streams open, or says how many
   *  it kept instead. */
  async untilStreamsOpen(count: number): Promise<void> {
    await this.#changed.until(
      () => this.#open.size === count,
      () => `the page has ${this.#open.size} events streams open, not ${count}`,
    );
  }

  /** Holds back the opening of the next events stream the page asks for,
   *  until `openTheHeldStream`. */
  holdTheNextStream(): void {
    this.#holdTheOpening = true;
  }

  /** Waits until the page has asked for the stream held back. */
  async #untilAStreamIsHeld(): Promise<void> {
    await this.#changed.until(
      () => this.#openTheHeld !== null,
      () => "the page never asked for an events stream",
    );
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
    await this.#changed.until(
      () => this.#readsSinceTheOpening > 0,
      () => "the page never read the history after its events stream opened",
    );
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
    await this.#changed.until(
      () => this.#held !== null,
      () => "the page never read the history again",
    );
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
    await this.#changed.until(
      () => this.#stream !== null,
      () => "the page never opened its events stream",
    );
    if (this.#stream === null) throw new Error("the page's events stream closed as it opened");
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
   *  daemon does while rustic is still walking the source for its size;
   *  `at` is when the daemon heard it, left out as a daemon older than the
   *  field leaves it out. */
  async progressesWithoutTotal(plan: string, bytes: number, at?: string): Promise<void> {
    await this.#tell({ type: "operation-progress", operation: this.#backupOf(plan).id, current: bytes, total: null, at });
  }

  /** The backup of `plan` reports `current` of `total` (`null`: not known
   *  yet) in the step whose code is `step`, as the daemon does for the bar
   *  the engine began last; `unit` is what the bar counts, left out as a
   *  daemon older than it leaves it out. */
  async progressesInStep(plan: string, step: string, current: number, total: number | null, unit?: ProgressUnit): Promise<void> {
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
    backup.failure = { code: "operation-failed" };
    await this.#tell({
      type: "operation-failed",
      operation: backup.id,
      code: "operation-failed",
      args: [],
      finished_at: finishedAt,
    });
  }

  /** The backup of `plan` ends cancelled: its log has the end, and the page
   *  is told nothing, as a page takes no news from a cancelled run's event.
   *  A page that asked for the cancel finds the end by reading the
   *  history. */
  isCancelled(plan: string, finishedAt: string): void {
    const backup = this.#backupOf(plan);
    backup.status = "cancelled";
    backup["finished-at"] = finishedAt;
  }
}
