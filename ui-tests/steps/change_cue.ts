// Journey steps for the change cue (technical design §4.5, "Live updates";
// decisions 127 and 129): a row whose status a live update changed arrives
// with one fade, from the cue's fill (`--cue-fill`) into the still mark it
// keeps until it is seen (`table.ts` checks the mark); and the
// reduced-motion rule, which keeps a fade and stops anything that moves.

import assert from "node:assert/strict";

import { Given, Then } from "@cucumber/cucumber";

import { patienceMs } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";
import { rowNamed } from "./live.ts";

/** One background-color transition a cell of a row ran, as the page saw it
 *  start: its row by its header's words, which cell, and the transition's
 *  first and last colors, length in ms and number of runs. */
interface Cue {
  row: string;
  cell: number;
  from: string;
  to: string;
  duration: number;
  iterations: number;
}

/** How long the cue settles over, in ms (DESIGN.md, Tables). */
const CUE_MS = 600;

/** How long a step waits to be sure a cue doesn't come, or doesn't come
 *  again: the cue's length twice over, after the frames it starts in. */
const NO_CUE_MS = 2 * CUE_MS;

/** How long a step waits for a cue to start once the row shows its change. */
const CUE_WAIT_MS = 5_000;

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

// The page notes each background-color transition a table cell starts, from
// its first script on, so the rows a page first shows are watched too. A cue
// is one such transition on every cell of its row: the cells carry it, as
// the pinned row header's own fill would cover a fill on the row.
Given("I watch the rows of the page for their change cue", function (this: BrowserWorld) {
  this.contextRoutes.push(async (context) => {
    await context.addInitScript(() => {
      const cues: unknown[] = [];
      Object.assign(window, { changeCues: cues });
      document.addEventListener(
        "transitionrun",
        (event) => {
          const cell = event.target;
          if (!(cell instanceof HTMLTableCellElement) || (event as TransitionEvent).propertyName !== "background-color") return;
          const header = cell.parentElement?.querySelector(':scope > th[scope="row"]');
          const transition = cell
            .getAnimations()
            .find((animation) => animation instanceof CSSTransition && animation.transitionProperty === "background-color");
          const effect = transition?.effect;
          const keyframes = effect instanceof KeyframeEffect ? effect.getKeyframes() : [];
          const timing = effect?.getComputedTiming();
          cues.push({
            row: (header?.textContent ?? "").replace(/[\u2066-\u2069]/g, "").trim(),
            cell: cell.cellIndex,
            from: String(keyframes.at(0)?.backgroundColor ?? ""),
            to: String(keyframes.at(-1)?.backgroundColor ?? ""),
            duration: Number(timing?.duration ?? 0),
            iterations: Number(timing?.iterations ?? 0),
          });
        },
        true,
      );
    });
  });
});

/** The cues the page has noted for `row`. */
async function cuesOf(world: BrowserWorld, row: string): Promise<Cue[]> {
  // The list the Given's init script keeps, of the shape it pushes.
  const cues: Cue[] | null = await world.page.evaluate(() => Reflect.get(window, "changeCues") ?? null);
  assert.ok(cues !== null, "the page isn't watched for change cues: a Given must say so");
  return cues.filter((cue) => cue.row === row);
}

Then("the row {string} of the table {string} is not lit", async function (this: BrowserWorld, row: string, name: string) {
  await rowNamed(this, name, row).first().waitFor({ state: "visible", timeout: patienceMs() });
  await sleep(NO_CUE_MS);
  assert.deepEqual(await cuesOf(this, row), [], `the row ${row} was lit`);
});

// Once: every cell of the row runs one fade, from the cue's fill to the fill
// it has once settled, over 600 ms and a single run, and none runs again.
// The mark stays after it (the step after checks it). The step before it
// saw the row's new status, and the cue starts within the frames after it,
// so a few seconds is long enough to wait for it.
Then(
  "the row {string} of the table {string} is lit once in the cue's fill, settling into its mark over 600 ms",
  async function (this: BrowserWorld, row: string, name: string) {
    const found = rowNamed(this, name, row).first();
    await found.waitFor({ state: "visible", timeout: patienceMs() });
    const cells = await found.locator(":scope > :is(td, th)").count();
    const until = Date.now() + CUE_WAIT_MS;
    while ((await cuesOf(this, row)).length < cells && Date.now() < until) await sleep(50);
    await sleep(NO_CUE_MS);
    const cues = await cuesOf(this, row);
    const [lit, settled] = await found.evaluate((element) => {
      const probe = document.createElement("div");
      probe.style.backgroundColor = "var(--cue-fill)";
      document.body.append(probe);
      const fill = getComputedStyle(probe).backgroundColor;
      probe.remove();
      const fills = [...element.querySelectorAll(":scope > :is(td, th)")].map((cell) => getComputedStyle(cell).backgroundColor);
      return [fill, fills];
    });
    assert.deepEqual(
      cues.map((cue) => cue.cell).sort(),
      settled.map((_, at) => at),
      `each cell of the row ${row} is lit once: ${JSON.stringify(cues)}`,
    );
    for (const cue of cues) {
      assert.equal(cue.from, lit, `the cell ${cue.cell} is lit in ${cue.from}, not the cue's fill`);
      assert.equal(cue.to, settled[cue.cell], `the cell ${cue.cell} settles to ${cue.to}, not its own fill`);
      assert.equal(cue.duration, CUE_MS, `the cell ${cue.cell} settles over ${cue.duration} ms`);
      assert.equal(cue.iterations, 1, `the cell ${cue.cell}'s cue runs ${cue.iterations} times`);
    }
  },
);

// The reduced-motion rule (decision 127): no animation runs, and a transition
// runs, or is set to run, only on a color or the opacity (with `display`,
// `overlay` and `visibility`, which hold a fade's element until it ends),
// over no longer than the change cue's 600 ms, short enough to be feedback.
// Pseudo-elements count, as the rule covers them too.
Then("nothing on the page moves, turns or changes size", async function (this: BrowserWorld) {
  const moving = await this.page.evaluate((longest) => {
    const fades = new Set([
      "color",
      "background-color",
      "border-color",
      "border-top-color",
      "border-right-color",
      "border-bottom-color",
      "border-left-color",
      "outline-color",
      "text-decoration-color",
      "fill",
      "stroke",
      "opacity",
      "display",
      "overlay",
      "visibility",
    ]);
    const found: string[] = [];
    for (const animation of document.getAnimations()) {
      if (animation instanceof CSSTransition && fades.has(animation.transitionProperty)) continue;
      found.push(animation instanceof CSSAnimation ? `the animation ${animation.animationName}` : `a ${animation.constructor.name}`);
    }
    for (const element of document.querySelectorAll("*")) {
      for (const pseudo of [null, "::before", "::after"]) {
        const style = getComputedStyle(element, pseudo);
        const properties = style.transitionProperty.split(",").map((property) => property.trim());
        const durations = style.transitionDuration.split(",").map((duration) => Number.parseFloat(duration) * 1000);
        properties.forEach((property, at) => {
          const duration = durations[at % durations.length] ?? 0;
          if (duration === 0 || property === "none") return;
          if (!fades.has(property) || duration > longest) {
            found.push(`${element.tagName.toLowerCase()}${pseudo ?? ""}.${element.className}: ${property} over ${duration} ms`);
          }
        });
      }
    }
    return found;
  }, CUE_MS);
  assert.deepEqual(moving, [], "something moves under reduced motion");
});
