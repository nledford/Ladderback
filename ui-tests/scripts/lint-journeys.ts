// Fails if a step file uses a pointer action. Journeys must work by
// keyboard alone (A1), so they may only use support/keyboard.ts.

import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const stepsDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../steps");
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
if (failures > 0) {
  console.error(`${failures} pointer action(s); use support/keyboard.ts instead.`);
  process.exit(1);
}
