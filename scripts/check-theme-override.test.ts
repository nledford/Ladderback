// Tests for the check of the docs site's copied sidebar template
// (scripts/check-theme-override.ts). `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { compareToc } from "./check-theme-override.ts";

const PRISTINE = ["<!DOCTYPE HTML>", "<html>", "    <head>", '        <meta charset="UTF-8">', "    </head>", "</html>"];
const TITLE = "        <title>Table of contents - Demo</title>";

/** The pristine page with `line` inserted before line `at` (0-based). */
function withLine(lines: string[], at: number, line: string): string {
  return [...lines.slice(0, at), line, ...lines.slice(at)].join("\n");
}

test("a copy that adds only its title passes", () => {
  assert.deepEqual(compareToc(PRISTINE.join("\n"), withLine(PRISTINE, 4, TITLE)), []);
});

test("mdBook writing a title of its own says to delete the copy", () => {
  const problems = compareToc(withLine(PRISTINE, 4, TITLE), withLine(PRISTINE, 4, TITLE));
  assert.equal(problems.length, 1);
  assert.match(problems[0] ?? "", /delete docs\/guide\/theme\/toc\.html\.hbs/);
});

test("a copy identical to mdBook's, so without its title, fails", () => {
  assert.equal(compareToc(PRISTINE.join("\n"), PRISTINE.join("\n")).length, 1);
});

test("a line mdBook changed in a new release fails, though the title is there", () => {
  const newer = PRISTINE.map((line) => (line === "<html>" ? '<html lang="en">' : line));
  assert.equal(compareToc(newer.join("\n"), withLine(PRISTINE, 4, TITLE)).length, 1);
});

test("a line mdBook added in a new release fails", () => {
  const newer = PRISTINE.toSpliced(4, 0, '        <meta name="robots" content="noindex">');
  assert.equal(compareToc(newer.join("\n"), withLine(PRISTINE, 4, TITLE)).length, 1);
});

test("an added line that isn't a title fails", () => {
  assert.equal(compareToc(PRISTINE.join("\n"), withLine(PRISTINE, 4, "        <style></style>")).length, 1);
});
