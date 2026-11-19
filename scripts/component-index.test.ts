// Tests for the §4.5 index of component contracts (scripts/component-index.ts).
// `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { type Contract, parseContract, renderIndex, replaceIndex } from "./component-index.ts";

const contract = (component: string, title: string): Contract => ({ component, file: `${component}.md`, title });

test("a contract file gives its component, from its name, and its heading", () => {
  assert.deepEqual(
    parseContract("text_field.md", "# The text field\n\n- **The text field** is `TextField`.\n"),
    contract("text_field", "The text field"),
  );
});

test("a file not named as a component's .rs file is refused", () => {
  assert.match(String(parseContract("TextField.md", "# The text field\n")), /named as its component's \.rs file/);
  assert.match(String(parseContract("text-field.md", "# The text field\n")), /named as its component's \.rs file/);
  assert.match(String(parseContract("text_field.txt", "# The text field\n")), /named as its component's \.rs file/);
});

test("a file without a heading is refused", () => {
  assert.match(String(parseContract("tooltip.md", "- **The tooltip** is `Tooltip`.\n")), /needs a heading/);
  assert.match(String(parseContract("tooltip.md", "## The tooltip\n")), /needs a heading/);
});

test("a heading inside fenced code doesn't count", () => {
  const markdown = "```\n# Not this\n```\n\n# The tooltip\n";
  assert.deepEqual(parseContract("tooltip.md", markdown), contract("tooltip", "The tooltip"));
});

test("a heading with a pipe, which would split its row, is refused", () => {
  assert.match(String(parseContract("tooltip.md", "# The tooltip | note\n")), /has a "\|"/);
});

test("a contract file with a conflict marker is refused, with its line", () => {
  const markdown = "# The tooltip\n<<<<<<< HEAD\na\n=======\nb\n>>>>>>> branch\n";
  assert.match(String(parseContract("tooltip.md", markdown)), /tooltip\.md:2: a merge-conflict marker/);
});

test("the index lists contracts in component-name order, whatever order they come in", () => {
  assert.deepEqual(renderIndex([contract("tooltip", "The tooltip"), contract("text_field", "The text field")]), [
    "| Contract | Component |",
    "|---|---|",
    "| [The text field](accessibility/text_field.md) | `components/text_field.rs` |",
    "| [The tooltip](accessibility/tooltip.md) | `components/tooltip.rs` |",
  ]);
});

const page = (table: string): string =>
  [
    "### 4.5 Accessibility",
    "",
    "| Component | Contract |",
    "|---|---|",
    "| Dialog | Modal |",
    "",
    "Intro:",
    "",
    table,
    "",
    "After.",
    "",
  ].join("\n");
const TABLE = ["| Contract | Component |", "|---|---|", "| [The tooltip](accessibility/tooltip.md) | `components/tooltip.rs` |"];

test("only the contracts' table is replaced; the text around it and other tables stay", () => {
  const result = replaceIndex(
    page("| Contract | Component |\n|---|---|\n| [Old](accessibility/old.md) | `components/old.rs` |"),
    TABLE,
  );
  assert.deepEqual(result, { text: page(TABLE.join("\n")) });
});

test("a page with a conflict marker is refused, naming the fix", () => {
  const text = page(
    "| Contract | Component |\n|---|---|\n<<<<<<< HEAD\n| [A](accessibility/a.md) | `components/a.rs` |\n=======\n>>>>>>> b",
  );
  assert.match(String(replaceIndex(text, TABLE)), /accessibility\.md:\d+: a merge-conflict marker: .*just component-index/);
});

test("a row after a blank line in the index is refused rather than left behind", () => {
  const text = page(
    "| Contract | Component |\n|---|---|\n| [A](accessibility/a.md) | `components/a.rs` |\n\n| [B](accessibility/b.md) | `components/b.rs` |",
  );
  assert.match(String(replaceIndex(text, TABLE)), /a contract's row after the index/);
});

test("a page without the index table, or with two, is refused", () => {
  assert.match(String(replaceIndex(page("No table."), TABLE)), /no "\| Contract \| Component \|" table/);
  const twice = page(`${TABLE.join("\n")}\n\n${TABLE.join("\n")}`);
  assert.match(String(replaceIndex(twice, TABLE)), /more than one "\| Contract \| Component \|" table/);
});
