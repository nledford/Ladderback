// The red-first check's verdict on a new @ui scenario (scripts/red-first.ts,
// technical design §13.1). The check runs the scenario at the test: commit in
// each of Playwright's engines, so a bug only Firefox or WebKit shows can be
// red. A red only one of those shows counts only when the scenario passes in
// that engine at the pull request's head: the required `UI journeys` job
// proves a Chromium red passes at the head, and no required job runs the other
// two, so without that check a scenario Chromium finds nothing wrong with,
// failing in Firefox for a reason the branch never fixes, would pass for red.
// Pure, so that `node --test` can test it (scripts/red-first-ui.test.ts).

export const ENGINES = ["chromium", "firefox", "webkit"] as const;

/** One of Playwright's engines, the values `LADDERBACK_UI_BROWSER` takes. */
export type Engine = (typeof ENGINES)[number];

/** What one cucumber-js run of a scenario showed: every scenario the filter
 *  matched failed, or every one passed, or anything else (no match, an
 *  outline whose rows split, an undefined step, a run that didn't start). */
export type Outcome = "failed" | "passed" | "other";

/** The outcome of a cucumber-js run, from its exit status and its last
 *  summary line (`2 scenarios (1 failed, 1 passed)`). A filter that matched
 *  nothing passes, and an outline can match several rows, so both a red and a
 *  green need the summary to report scenarios, all of them alike. */
export function outcome(status: number | null, output: string): Outcome {
  const summary = [...output.matchAll(/(\d+) scenarios? \(([^)]*)\)/g)].at(-1);
  const total = summary ? Number(summary[1]) : 0;
  if (total === 0) return "other";
  const count = (kind: string): number => Number(summary?.[2]?.match(new RegExp(`(\\d+) ${kind}`))?.[1] ?? 0);
  if (status !== 0 && status !== null && count("failed") === total) return "failed";
  if (status === 0 && count("passed") === total) return "passed";
  return "other";
}

/** The engines a tree's UI tests can drive, from its `ui-tests/support/world.ts`
 *  (`undefined` when it has none): all three once it reads
 *  `LADDERBACK_UI_BROWSER`, Chromium alone in a tree that predates it, where
 *  the variable would change nothing and every engine's run would be
 *  Chromium's. */
export function enginesFor(worldSource: string | undefined): Engine[] {
  return worldSource?.includes("LADDERBACK_UI_BROWSER") ? [...ENGINES] : ["chromium"];
}

/** The engines to run the scenario in at the head: none when it is red in
 *  Chromium, else each other engine it failed in at the test: commit. */
export function headChecks(atFirst: ReadonlyMap<Engine, Outcome>): Engine[] {
  if (atFirst.get("chromium") === "failed") return [];
  return ENGINES.filter((engine) => atFirst.get(engine) === "failed");
}

/** The engines whose red counts, and so whether the scenario is red: Chromium
 *  when it failed there at the test: commit, else each engine it failed in
 *  there and passes in at the head. Empty: the scenario is GREEN. */
export function countedRed(atFirst: ReadonlyMap<Engine, Outcome>, atHead: ReadonlyMap<Engine, Outcome>): Engine[] {
  if (atFirst.get("chromium") === "failed") return ["chromium"];
  return headChecks(atFirst).filter((engine) => atHead.get(engine) === "passed");
}
