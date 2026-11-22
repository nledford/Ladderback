// Fails if a step file uses a pointer action. Journeys must work by
// keyboard alone (A1), so they may only use support/keyboard.ts. The one
// exception is support/pointer.ts, which the maintainer approved for the
// tooltip's hover contract (WCAG 1.4.13 is pointer behaviour), a link's
// underline under the pointer, and the one drag of a table's Columns panel
// (decision 140), whose Move buttons do the same by keyboard: it presses
// only for that drag, and every control is still reached and pressed by
// keyboard alone. Steps call its moves by name; a new pointer action there
// needs the maintainer's word too.
//
// Fails too if an `@ui` scenario's keyboard walk crosses the header to reach
// the page's content (§4.4, skip-to-content.ts), which the walk's run-time
// guard would fail only in a browser.

import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { headerTargets, walksWithoutSkip } from "./skip-to-content.ts";

const here = path.dirname(fileURLToPath(import.meta.url));
const stepsDir = path.resolve(here, "../steps");
const featuresDir = path.resolve(here, "../../features");
const catalogDir = path.resolve(here, "../../crates/l10n/locales/en-US");
// Pointer actions, and calls that set state or focus without the keyboard.
const banned =
  /\.(click|dblclick|fill|focus|check|uncheck|setChecked|hover|tap|dragTo|dispatchEvent|selectOption|setInputFiles)\(|\bdragAndDrop\b|\bmouse\.|\btouchscreen\./;

let failures = 0;
for (const entry of await readdir(stepsDir, { recursive: true })) {
  if (!entry.endsWith(".ts")) continue;
  const file = path.join(stepsDir, entry);
  const lines = (await readFile(file, "utf8")).split("\n");
  lines.forEach((line, index) => {
    if (banned.test(line)) {
      failures += 1;
      console.error(`${path.relative(process.cwd(), file)}:${index + 1}: pointer action in a journey: ${line.trim()}`);
    }
  });
}
if (failures > 0) console.error(`${failures} pointer action(s); use support/keyboard.ts instead.`);

const catalog = await Promise.all(
  (await readdir(catalogDir)).filter((entry) => entry.endsWith(".ftl")).map((entry) => readFile(path.join(catalogDir, entry), "utf8")),
);
const header = headerTargets(catalog.join("\n"));
let crossings = 0;
for (const entry of (await readdir(featuresDir, { recursive: true })).sort()) {
  if (!entry.endsWith(".feature")) continue;
  const file = path.join(featuresDir, entry);
  for (const { line, step, from } of walksWithoutSkip(await readFile(file, "utf8"), header)) {
    crossings += 1;
    const where = `${path.relative(process.cwd(), file)}:${line}`;
    const advice = {
      top: 'this walk starts at the top of the page and crosses the header: add "And I skip to the content" before it, after the page loads.',
      header:
        'this walk starts on a stop in the header and crosses the rest of it: press Enter on that stop first, or walk from a page just loaded, after "I skip to the content".',
      "content's start":
        "this walk goes back with Shift+Tab from the start of the content into the header: walk forward with Tab, or back from a stop past its target.",
    }[from];
    console.error(`${where}: ${advice} ${step}`);
  }
}
if (crossings > 0)
  console.error(`${crossings} walk(s) to the page's content cross the header (docs/design/components/web-ui.md §4.4).`);

if (failures + crossings > 0) process.exit(1);
