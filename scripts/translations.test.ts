// Tests for the translation listing and its record of reviewed sources
// (scripts/translations.ts). `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import {
  accepting,
  entries,
  history,
  leftOut,
  listing,
  parseSources,
  renderListing,
  renderSources,
  sourceHash,
} from "./translations.ts";

const HELLO = "9268556a04b6289bf6ca06646cd38241ab357daba442a5ce139c4df3a2ed6af1";

const ids = (...names: string[]): Set<string> => new Set(names);
const nowhere = (): string | null => null;

test("a message runs from its ID to its last line, attributes included, comments left out", () => {
  const ftl = [
    "# A comment about the file",
    "",
    "## Section",
    "-brand-name = Ladderback",
    "",
    "# What the button says",
    "greeting = Hello",
    "farewell = Goodbye",
    "    .title = Leave",
    "",
  ].join("\n");
  assert.deepEqual(entries(ftl), [
    { id: "-brand-name", text: "-brand-name = Ladderback" },
    { id: "greeting", text: "greeting = Hello" },
    { id: "farewell", text: "farewell = Goodbye\n    .title = Leave" },
  ]);
});

test("a select's lines at the first column, and blank lines inside a message, stay in the message", () => {
  const ftl = "count = { $n ->\n    [one] One\n\n   *[other] Many\n}\nnext = Next\n";
  assert.deepEqual(entries(ftl), [
    { id: "count", text: "count = { $n ->\n    [one] One\n\n   *[other] Many\n}" },
    { id: "next", text: "next = Next" },
  ]);
});

test("spaces at the ends of lines and Windows line ends don't change a message", () => {
  assert.deepEqual(entries("greeting = Hello  \r\n\r\n"), [{ id: "greeting", text: "greeting = Hello" }]);
});

test("a placeable's lines at the first column stay in the message", () => {
  const ftl = "total = { NUMBER($n,\nminimumFractionDigits: 2) } of {\n-brand-name }\nnext = Next\n";
  assert.deepEqual(entries(ftl), [
    { id: "total", text: "total = { NUMBER($n,\nminimumFractionDigits: 2) } of {\n-brand-name }" },
    { id: "next", text: "next = Next" },
  ]);
});

test("a byte order mark at the start of a file is no part of it", () => {
  assert.deepEqual(entries("\uFEFFgreeting = Hello\n"), [{ id: "greeting", text: "greeting = Hello" }]);
  assert.deepEqual(parseSources(`\uFEFFgreeting ${HELLO}\n`, "de/sources.txt"), new Map([["greeting", HELLO]]));
});

test("a source's hash is the SHA-256 of the message's text", () => {
  assert.equal(sourceHash("greeting = Hello"), HELLO);
});

test("a changed attribute changes the message's hash", () => {
  const [before] = entries("farewell = Goodbye\n    .title = Leave\n");
  const [after] = entries("farewell = Goodbye\n    .title = Quit\n");
  assert.ok(before && after);
  assert.notEqual(sourceHash(before.text), sourceHash(after.text));
});

test("a sources file is read without its comments and blank lines", () => {
  const text = `# Written by just accept-translations\n\ngreeting ${HELLO}\n`;
  assert.deepEqual(parseSources(text, "de/sources.txt"), new Map([["greeting", HELLO]]));
});

test("a line of a sources file that isn't an ID and a hash is refused, naming the line", () => {
  assert.throws(() => parseSources(`greeting ${HELLO}\ngreeting\n`, "de/sources.txt"), /de\/sources\.txt:2:/);
  assert.throws(() => parseSources(`greeting ${HELLO.toUpperCase()}\n`, "de/sources.txt"), /de\/sources\.txt:1:/);
});

test("a sources file that names a message twice is refused", () => {
  assert.throws(() => parseSources(`greeting ${HELLO}\ngreeting ${HELLO}\n`, "de/sources.txt"), /greeting.*twice/);
});

test("a sources file is written sorted by ID, after a header that says not to edit it", () => {
  const written = renderSources(
    new Map([
      ["zebra", HELLO],
      ["apple", HELLO],
    ]),
  );
  const lines = written.split("\n");
  assert.ok(lines[0]?.startsWith("# "));
  assert.ok(written.endsWith(`apple ${HELLO}\nzebra ${HELLO}\n`));
  assert.deepEqual(
    parseSources(written, "x"),
    new Map([
      ["apple", HELLO],
      ["zebra", HELLO],
    ]),
  );
});

test("each English message is current, missing, not reviewed or stale", () => {
  const source = entries("current = Hello\nmissing = Gone\nunreviewed = New\nstale = Changed\n");
  const sources = new Map([
    ["current", sourceHash("current = Hello")],
    ["stale", sourceHash("stale = Before")],
  ]);
  const found = listing("de", source, ids("current", "unreviewed", "stale"), sources);
  assert.equal(found.tag, "de");
  assert.equal(found.total, 4);
  assert.equal(found.current, 1);
  assert.deepEqual(found.missing, ["missing"]);
  assert.deepEqual(found.unreviewed, ["unreviewed"]);
  assert.deepEqual(found.stale, [{ id: "stale", hash: sourceHash("stale = Before"), now: "stale = Changed" }]);
  assert.deepEqual(found.unknown, []);
});

test("a translation or a source of a message English no longer has is listed apart", () => {
  const sources = new Map([["gone", HELLO]]);
  const found = listing("de", entries("greeting = Hello\n"), ids("greeting", "left"), sources);
  assert.deepEqual(found.unknown, ["gone", "left"]);
});

test("a term only the translation has is the language's own, and isn't listed", () => {
  const found = listing("ja", entries("greeting = Hello\n"), ids("greeting", "-snapshot", "left"), new Map());
  assert.deepEqual(found.unknown, ["left"]);
});

test("the listing gives each stale message's English then and now", () => {
  const found = listing("de", entries("greeting = Hello there\n"), ids("greeting"), new Map([["greeting", HELLO]]));
  const then = (id: string, hash: string): string | null => (id === "greeting" && hash === HELLO ? "greeting = Hello" : null);
  assert.equal(
    renderListing([found], then),
    [
      "de: 0 of 1 messages current",
      "  stale: greeting",
      "    translated from:",
      "      greeting = Hello",
      "    now:",
      "      greeting = Hello there",
      "",
    ].join("\n"),
  );
});

test("a stale message whose earlier English the history lacks says so", () => {
  const found = listing("de", entries("greeting = Hello there\n"), ids("greeting"), new Map([["greeting", HELLO]]));
  assert.match(renderListing([found], nowhere), /translated from: not in this repository's history/);
});

test("the listing names every kind of message that isn't current, in that order", () => {
  const source = entries("a = A\nb = B\nc = C\n");
  const found = listing("de", source, ids("b", "c", "z"), new Map([["c", HELLO]]));
  assert.equal(
    renderListing([found], nowhere),
    [
      "de: 0 of 3 messages current",
      "  missing: a",
      "  not reviewed: b",
      "  stale: c",
      "    translated from: not in this repository's history",
      "    now:",
      "      c = C",
      "  not in en-US: z",
      "",
    ].join("\n"),
  );
});

test("no translation at all is said plainly", () => {
  assert.equal(renderListing([], nowhere), "No language besides en-US has a catalog.\n");
});

test("accepting records the hash of the English a message is translated from now", () => {
  const source = entries("greeting = Hello\nfarewell = Goodbye\n");
  const result = accepting("de", source, ids("greeting", "farewell"), new Map(), ["greeting"]);
  assert.ok(typeof result !== "string", String(result));
  assert.deepEqual(result.accepted, ["greeting"]);
  assert.deepEqual(result.sources, new Map([["greeting", HELLO]]));
});

test("accepting a message English doesn't have, or the language doesn't translate, is refused", () => {
  const source = entries("greeting = Hello\n");
  assert.equal(accepting("de", source, ids("greeting"), new Map(), ["welcome"]), "de: en-US has no message welcome");
  assert.equal(accepting("de", source, ids(), new Map(), ["greeting"]), "de: greeting isn't translated");
});

test("accepting a term only the translation has records nothing, and refuses nothing", () => {
  const source = entries("greeting = Hello\n");
  const result = accepting("ja", source, ids("greeting", "-snapshot"), new Map(), ["-snapshot", "greeting"]);
  assert.ok(typeof result !== "string", String(result));
  assert.deepEqual(result.accepted, ["greeting"]);
  assert.deepEqual(result.sources, new Map([["greeting", HELLO]]));
});

test("accepting drops the lines of messages English or the language no longer has", () => {
  const source = entries("greeting = Hello\nuntranslated = Gone here\n");
  const sources = new Map([
    ["gone", HELLO],
    ["untranslated", HELLO],
    ["greeting", HELLO],
  ]);
  for (const chosen of [["greeting"], "all"] as const) {
    const result = accepting("de", source, ids("greeting"), sources, chosen);
    assert.ok(typeof result !== "string", String(result));
    assert.deepEqual(result.removed, ["gone", "untranslated"]);
    assert.deepEqual(result.sources, new Map([["greeting", HELLO]]));
  }
});

test("accepting every message takes the translated ones not reviewed yet, and leaves the stale ones", () => {
  const source = entries("greeting = Hello there\nfarewell = Goodbye\nwelcome = Welcome\nsame = Same\n");
  const sources = new Map([
    ["greeting", HELLO],
    ["same", sourceHash("same = Same")],
    ["gone", HELLO],
    ["welcome", HELLO],
  ]);
  const result = accepting("de", source, ids("greeting", "farewell", "same"), sources, "all");
  assert.ok(typeof result !== "string", String(result));
  assert.deepEqual(result.accepted, ["farewell"]);
  assert.deepEqual(result.staleLeftOut, ["greeting"]);
  assert.deepEqual(result.removed, ["gone", "welcome"]);
  assert.equal(result.sources.get("greeting"), HELLO);
  assert.equal(result.sources.get("farewell"), sourceHash("farewell = Goodbye"));
  assert.equal(result.sources.get("same"), sourceHash("same = Same"));
  assert.equal(result.sources.has("welcome"), false);
});

test("accepting a stale message by its ID takes it, and leaves nothing out", () => {
  const source = entries("greeting = Hello there\n");
  const result = accepting("de", source, ids("greeting"), new Map([["greeting", HELLO]]), ["greeting"]);
  assert.ok(typeof result !== "string", String(result));
  assert.deepEqual(result.accepted, ["greeting"]);
  assert.deepEqual(result.staleLeftOut, []);
  assert.equal(result.sources.get("greeting"), sourceHash("greeting = Hello there"));
});

test("accepting every message says which stale ones it left out, and how many", () => {
  assert.equal(leftOut("de", []), null);
  assert.equal(leftOut("de", ["greeting"]), "de: left out 1 stale message (greeting): name its ID to accept it after review");
  assert.equal(
    leftOut("de", ["farewell", "greeting"]),
    "de: left out 2 stale messages (farewell, greeting): name their IDs to accept them after review",
  );
});

test("accepting --all with IDs as well is refused, and writes nothing", (t) => {
  const locales = mkdtempSync(path.join(tmpdir(), "translations-"));
  t.after(() => rmSync(locales, { recursive: true, force: true }));
  for (const tag of ["en-US", "de"]) {
    mkdirSync(path.join(locales, tag));
    writeFileSync(path.join(locales, tag, "main.ftl"), "greeting = Hello\n");
  }
  const script = path.join(import.meta.dirname, "translations.ts");
  const result = spawnSync(process.execPath, [script, "accept", "de", "greeting", "--all", "--locales", locales], {
    encoding: "utf8",
  });
  assert.equal(result.status, 1, result.stdout);
  assert.match(result.stderr, /--all takes no IDs/);
  assert.equal(existsSync(path.join(locales, "de", "sources.txt")), false);
});

/** Runs git in `dir`, with no configuration but the repository's own. */
function git(dir: string, ...args: string[]): void {
  const env = { ...process.env, GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null" };
  const result = spawnSync("git", ["-c", "user.name=Owner", "-c", "user.email=owner@example.com", ...args], {
    cwd: dir,
    env,
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
}

test("the history gives the English a message was translated from, from an earlier commit", (t) => {
  const repository = mkdtempSync(path.join(tmpdir(), "translations-"));
  t.after(() => rmSync(repository, { recursive: true, force: true }));
  const dir = path.join(repository, "locales", "en-US");
  mkdirSync(dir, { recursive: true });
  git(repository, "init", "--quiet");
  writeFileSync(path.join(dir, "main.ftl"), "greeting = Hello\n");
  git(repository, "add", "--all");
  git(repository, "commit", "--quiet", "--message", "first");
  writeFileSync(path.join(dir, "main.ftl"), "greeting = Hello there\n");
  git(repository, "commit", "--quiet", "--all", "--message", "second");
  writeFileSync(path.join(dir, "main.ftl"), "greeting = Hi\n");
  const then = history(dir);
  assert.equal(then("greeting", HELLO), "greeting = Hello");
  assert.equal(then("greeting", sourceHash("greeting = Hello there")), "greeting = Hello there");
  assert.equal(then("greeting", sourceHash("greeting = Never")), null);
});

test("a folder no repository keeps has no history", (t) => {
  const dir = mkdtempSync(path.join(tmpdir(), "translations-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  assert.equal(history(dir)("greeting", HELLO), null);
});
