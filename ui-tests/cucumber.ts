// Runs only the @ui scenarios from the shared features/ folder. The Rust
// runners skip @ui, and this runner skips everything else (§13.2). cucumber-js
// finds only a .js, .cjs, .mjs, .json or YAML config by itself, not a .ts
// one, so every call names this file: `--config cucumber.ts` (the Justfile,
// package.json and scripts/red-first.ts).

import type { IConfiguration } from "@cucumber/cucumber/api";

// Each worker runs a browser and a debug daemon, so past a few they only take
// cores from one another (6 already lost a scenario to focus timing); the cap
// stops a typo such as 30 for 3 from starting dozens.
const MAX_WORKERS = 16;

/** How many scenarios run at once: `LADDERBACK_UI_WORKERS`, 3 when unset or
 *  empty, as `LADDERBACK_UI_BROWSER` is read. Each worker is a thread with its
 *  own browser and shared daemon (`support/world.ts`), and each scenario that
 *  starts a daemon of its own gives it its own scratch folder and port, so
 *  workers share nothing but the binary, the built UI and the fixture
 *  repository, which they only read. 1 runs the scenarios in cucumber-js's own
 *  thread, one at a time, as before workers. */
function workers(): number {
  const value = process.env.LADDERBACK_UI_WORKERS || "3";
  const count = /^[1-9][0-9]*$/.test(value) ? Number(value) : Number.NaN;
  if (!(count <= MAX_WORKERS)) {
    throw new Error(`LADDERBACK_UI_WORKERS is ${JSON.stringify(value)}; it must be a whole number from 1 to ${MAX_WORKERS}`);
  }
  // cucumber-js's `parallel` counts worker threads, and 0 runs none.
  return count === 1 ? 0 : count;
}

const config: Partial<IConfiguration> = {
  paths: ["../features/**/*.feature"],
  // Not `*.test.ts`: those are `node --test` files, which would run their
  // tests in cucumber-js's process (`just test-ui` runs them first).
  import: ["support/**/!(*.test).ts", "steps/**/*.ts"],
  tags: "@ui",
  strict: true,
  format: ["progress"],
  parallel: workers(),
};

export default config;
