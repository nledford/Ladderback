// Tests for the American spelling check (scripts/check-american-spelling.ts).
// `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { american, britishSpellings, excluded, problems } from "./check-american-spelling.ts";

/** The words `britishSpellings` finds in `text`, with their American forms. */
function found(text: string): string[] {
  return britishSpellings(text).map(({ word, american }) => `${word} → ${american}`);
}

test("each of the contract's British spellings has its American form", () => {
  const cases: [string, string][] = [
    ["behaviour", "behavior"],
    ["behavioural", "behavioral"],
    ["colours", "colors"],
    ["cancelled", "canceled"],
    ["cancelling", "canceling"],
    ["labelled", "labeled"],
    ["relabelled", "relabeled"],
    ["modelling", "modeling"],
    ["travelled", "traveled"],
    ["favourite", "favorite"],
    ["honoured", "honored"],
    ["neighbouring", "neighboring"],
    ["labour", "labor"],
    ["centre", "center"],
    ["centred", "centered"],
    ["licences", "licenses"],
    ["initialisation", "initialization"],
    ["reinitialised", "reinitialized"],
    ["deserialiser", "deserializer"],
    ["unrecognised", "unrecognized"],
    ["summarises", "summarizes"],
    ["analysed", "analyzed"],
    ["catalogue", "catalog"],
    ["greyscale", "grayscale"],
    ["fulfil", "fulfill"],
    ["fulfilment", "fulfillment"],
    ["defence", "defense"],
    ["offences", "offenses"],
    ["practising", "practicing"],
    ["whilst", "while"],
    ["amongst", "among"],
    ["learnt", "learned"],
    ["misspelt", "misspelled"],
    ["artefacts", "artifacts"],
    ["programme", "program"],
    ["enquiry", "inquiry"],
  ];
  for (const [british, expected] of cases) assert.equal(american(british), expected, british);
});

test("American spellings and look-alike words pass", () => {
  for (const word of [
    "behavior",
    "canceled",
    "cancellation",
    "labeled",
    "initialize",
    "analysis",
    "memories",
    "categories",
    "emphasis",
    "otherwise",
    "promise",
    "exercise",
    "advise",
    "realism",
    "programmed",
    "fulfill",
    "dialogue",
    "acknowledgement",
  ]) {
    assert.equal(american(word), undefined, word);
  }
});

test("the American form keeps the word's case", () => {
  assert.equal(american("Behaviour"), "Behavior");
  assert.equal(american("CANCELLED"), "CANCELED");
});

test("a British spelling inside an identifier is found, with its line and column", () => {
  assert.deepEqual(found("enum TabsName { LabelledBy }"), ["Labelled → Labeled"]);
  assert.deepEqual(found("fn refusing_snapshots_once_cancelled()"), ["cancelled → canceled"]);
  assert.deepEqual(found("const CANCELLED_AT = 1;"), ["CANCELLED → CANCELED"]);
  assert.deepEqual(britishSpellings("ok\nthe colour of it"), [{ line: 2, column: 5, word: "colour", american: "color" }]);
});

test("the stored and wire value `cancelled` passes in its exact contexts only", () => {
  assert.deepEqual(found(`#[serde(rename = "cancelled")]`), []);
  assert.deepEqual(found(`#[serde(rename = "Cancelled")]`), []);
  assert.deepEqual(found(`let status = "Cancelled";`), ["Cancelled → Canceled"]);
  assert.deepEqual(found("WHERE status = 'cancelled'"), []);
  assert.deepEqual(found("the operation ends `cancelled`"), []);
  assert.deepEqual(found("-- queued|running|failed|cancelled"), []);
  assert.deepEqual(found("announces `operation-cancelled`"), []);
  assert.deepEqual(found("handles `notifications/cancelled`"), []);
  assert.deepEqual(found("if: !cancelled() && github.event_name == 'push'"), []);
  assert.deepEqual(found("token.cancelled().await; token.is_cancelled()"), []);
  assert.deepEqual(found(`"completed cancelled") echo`), []);
  assert.deepEqual(found(`"the backup was cancelled"`), ["cancelled → canceled"]);
  assert.deepEqual(found("OperationStatus::Cancelled"), ["Cancelled → Canceled"]);
});

test("the `behaviour` label passes, the word behaviour doesn't", () => {
  assert.deepEqual(found("contains(github.event.pull_request.labels.*.name, 'behaviour')"), []);
  assert.deepEqual(found("carries the `behaviour` label"), []);
  assert.deepEqual(found("a pull request that changes behaviour"), ["behaviour → behavior"]);
});

test("the ARIA attribute and the DOM property pass", () => {
  assert.deepEqual(found(`attr:aria-labelledby="heading"`), []);
  assert.deepEqual(found("element.ariaLabelledByElements"), []);
  assert.deepEqual(found("avoidLabelledByCrash"), ["Labelled → Labeled"]);
});

test("the excluded paths are those the contract leaves alone", () => {
  for (const file of [
    "migrations/0001_init.sql",
    "crates/adapters/store/migrations-before-baseline/0001.sql",
    "CHANGELOG.md",
    "Cargo.lock",
    "ui-tests/package-lock.json",
    "CODE_OF_CONDUCT.md",
    ".github/skills/impeccable/reference/foundations.md",
    ".github/agents/impeccable-documenter.agent.md",
    ".github/hooks/impeccable.json",
    "scripts/check-american-spelling.test.ts",
  ]) {
    assert.equal(excluded(file), true, file);
  }
  for (const file of ["README.md", "docs/design/development.md", "crates/adapters/store/src/lib.rs", ".github/workflows/ci.yml"]) {
    assert.equal(excluded(file), false, file);
  }
});

test("each spelling is reported as `file:line:column: word (American: …)`", () => {
  const spellings = new Map([["docs/a.md", britishSpellings("one\nits colour")]]);
  assert.deepEqual(problems(spellings), ["docs/a.md:2:5: colour (American: color)"]);
});
