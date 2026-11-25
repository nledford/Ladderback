// Tests for the §16 decision index (scripts/decision-index.ts). `just lint`
// runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { parseDecision, renderIndex, roadmapProblem, type Decision } from "./decision-index.ts";

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

const roadmap = (body: string): string =>
  ["# Roadmap", "", "## 16. Decisions", "", "Intro.", "", body, "", "## 17. Next", "| [1](decisions/01-a.md) | A |", ""].join("\n");

test("a §16 whose prose links decisions, with no table of them, passes", () => {
  assert.equal(roadmapProblem(roadmap("The files in [decisions/](decisions/); see [decision 9](decisions/09-a.md).")), null);
});

test("a row linking a decision in §16 is refused, with its line", () => {
  assert.match(
    String(roadmapProblem(roadmap("| # | Question |\n|---|---|\n| [9](decisions/09-a.md) | A |"))),
    /roadmap\.md:9: §16 lists a decision in a table again/,
  );
});

test("an indented row, a ./ link and a row without a leading pipe are refused too", () => {
  for (const row of ["   | [9](decisions/09-a.md) | A |", "| [9](./decisions/09-a.md) | A |", "[9](decisions/09-a.md) | A"]) {
    assert.match(String(roadmapProblem(roadmap(row))), /lists a decision in a table again/, row);
  }
});

test("a row in a fenced example, or after §16 ends, doesn't count", () => {
  assert.equal(roadmapProblem(roadmap("```\n| [9](decisions/09-a.md) | A |\n```")), null);
});

test("a table in §16 that links no decision doesn't count", () => {
  assert.equal(roadmapProblem(roadmap("| Risk | Answer |\n|---|---|\n| Loss | Backups |")), null);
});

test("a roadmap without the §16 heading is refused, so the check can't pass by not running", () => {
  assert.match(
    String(roadmapProblem("# Roadmap\n\n## 16. Design decisions\n\n| [9](decisions/09-a.md) | A |\n")),
    /no "## 16\. Decisions" heading/,
  );
});

test("a conflict marker anywhere in the roadmap is refused, with its line", () => {
  assert.match(String(roadmapProblem(`${roadmap("Text.")}<<<<<<< HEAD\n`)), /roadmap\.md:\d+: a merge-conflict marker/);
});
