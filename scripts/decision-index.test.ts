// Tests for the §16 decision index (scripts/decision-index.ts). `just lint`
// runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { parseDecision, renderIndex, replaceIndex, type Decision } from "./decision-index.ts";

const decision = (number: number, file: string, question: string): Decision => ({ number, file, question });

test("a decision file gives its number and the question its heading asks", () => {
  assert.deepEqual(
    parseDecision("07-what-a-plan-owns.md", "# Decision 7: What a plan owns\n\nText.\n"),
    decision(7, "07-what-a-plan-owns.md", "What a plan owns"),
  );
});

test("a file not named NN-<slug>.md is refused", () => {
  assert.match(String(parseDecision("plans.md", "# Decision 7: Plans\n")), /named NN-<slug>\.md/);
  assert.match(String(parseDecision("07-Plans.md", "# Decision 7: Plans\n")), /named NN-<slug>\.md/);
});

test("a heading whose number differs from the file's is refused", () => {
  assert.match(String(parseDecision("07-plans.md", "# Decision 8: Plans\n")), /heading says decision 8, its name 7/);
});

test("a heading that isn't a decision heading is refused", () => {
  assert.match(String(parseDecision("07-plans.md", "# Plans\n")), /must read "# Decision 7: <question>"/);
  assert.match(String(parseDecision("07-plans.md", "Text only.\n")), /must read/);
});

test("a heading inside fenced code doesn't count", () => {
  const markdown = "```\n# Decision 9: Not this\n```\n\n# Decision 7: Plans\n";
  assert.deepEqual(parseDecision("07-plans.md", markdown), decision(7, "07-plans.md", "Plans"));
});

test("a question with a pipe, which would split its row, is refused", () => {
  assert.match(String(parseDecision("07-plans.md", "# Decision 7: Plans | jobs\n")), /has a "\|"/);
});

test("a superseded decision's row says what superseded it", () => {
  const markdown = "# Decision 7: Plans\n\n**Superseded by decision 12** (2026-11-01).\n";
  assert.equal((parseDecision("07-plans.md", markdown) as Decision).question, "Plans (superseded by decision 12)");
});

test("a decision file with a conflict marker is refused, with its line", () => {
  const markdown = "# Decision 7: Plans\n<<<<<<< HEAD\na\n=======\nb\n>>>>>>> branch\n";
  assert.match(String(parseDecision("07-plans.md", markdown)), /07-plans\.md:2: a merge-conflict marker/);
});

test("the index lists decisions in number order, whatever order they come in", () => {
  const { lines, problems } = renderIndex([decision(10, "10-b.md", "B"), decision(9, "09-a.md", "A")]);
  assert.deepEqual(problems, []);
  assert.deepEqual(lines, ["| # | Question |", "|---|---|", "| [9](decisions/09-a.md) | A |", "| [10](decisions/10-b.md) | B |"]);
});

test("two files with one number are a problem naming both", () => {
  const { problems } = renderIndex([decision(9, "09-a.md", "A"), decision(9, "09-b.md", "B")]);
  assert.equal(problems.length, 1);
  assert.match(problems[0] ?? "", /decision 9 is both 09-a\.md and 09-b\.md/);
});

const roadmap = (table: string): string =>
  ["# Roadmap", "", "## 16. Decisions", "", "Intro.", "", table, "", "Closing words.", "", "## 17. Next", "| keep | me |", ""].join(
    "\n",
  );
const TABLE = ["| # | Question |", "|---|---|", "| [9](decisions/09-a.md) | A |"];

test("only §16's table is replaced; the text around it and other tables stay", () => {
  const result = replaceIndex(roadmap("| # | Question |\n|---|---|\n| [1](decisions/01-old.md) | Old |"), TABLE);
  assert.deepEqual(result, { text: roadmap(TABLE.join("\n")) });
});

test("a roadmap with a conflict marker is refused, naming the fix", () => {
  const text = roadmap("| # | Question |\n|---|---|\n<<<<<<< HEAD\n| [1](decisions/01-a.md) | A |\n=======\n>>>>>>> b");
  assert.match(String(replaceIndex(text, TABLE)), /roadmap\.md:\d+: a merge-conflict marker: .*just decision-index/);
});

test("a row after a blank line in the index is refused rather than left behind", () => {
  const text = roadmap("| # | Question |\n|---|---|\n| [1](decisions/01-a.md) | A |\n\n| [2](decisions/02-b.md) | B |");
  assert.match(String(replaceIndex(text, TABLE)), /a table row after the §16 index/);
});

test("a roadmap without the index table is refused", () => {
  assert.match(String(replaceIndex(roadmap("No table."), TABLE)), /no "\| # \| Question \|" table under "## 16\. Decisions"/);
  assert.match(String(replaceIndex("# Roadmap\n", TABLE)), /no "\| # \| Question \|" table/);
});
