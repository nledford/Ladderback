// Fails when a tracked text file spells a word the British way: the
// repository is written in American English, the maintainer's (2026-11-26).
// Prose, comments, identifiers and file names all follow it.
//
// The words are split out of identifiers first (`LabelledBy`,
// `is_cancelled`), so a British spelling inside a name is found too. Stored
// and wire values keep their spelling, since changing one needs a migration
// and changes the hub sync protocol; the label `behaviour` keeps its name.
// `ALLOWED` lists those exact contexts, each with its reason, and `EXCLUDED`
// the paths the check doesn't read.
//
// Usage: node scripts/check-american-spelling.ts [pathspec…]
// Reads the files git tracks (only those the pathspecs match, when given).
// Exits 0 when they pass, 1 with `file:line:column: word (American: …)` lines
// and the reason on standard error otherwise. Runs from any directory.

import { execFileSync } from "node:child_process";
import { readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

/** A British spelling: the word, as one whole part of an identifier, and its American form. */
interface Rule {
  british: RegExp;
  american: string;
}

const ISE = [
  "apologi",
  "authori",
  "capitali",
  "categori",
  "centrali",
  "characteri",
  "critici",
  "customi",
  "emphasi",
  "finali",
  "generali",
  "initiali",
  "internationali",
  "locali",
  "materiali",
  "maximi",
  "memori",
  "minimi",
  "normali",
  "optimi",
  "organi",
  "parameteri",
  "prioriti",
  "randomi",
  "reali",
  "recogni",
  "saniti",
  "seriali",
  "speciali",
  "stabili",
  "standardi",
  "styli",
  "summari",
  "synchroni",
  "tokeni",
  "utili",
  "visuali",
].join("|");

/** The words, matched against one lowercase part of a word. */
export const RULES: readonly Rule[] = [
  { british: /^(.*)behaviour(.*)$/, american: "$1behavior$2" },
  { british: /^(.*)colour(.*)$/, american: "$1color$2" },
  { british: /^(.*)favour(.*)$/, american: "$1favor$2" },
  { british: /^(.*)honour(.*)$/, american: "$1honor$2" },
  { british: /^(.*)neighbour(.*)$/, american: "$1neighbor$2" },
  { british: /^(.*)labour(.*)$/, american: "$1labor$2" },
  { british: /^(.*)(cancel|label|model|travel)l(ed|ing)$/, american: "$1$2$3" },
  { british: /^(.*)centred$/, american: "$1centered" },
  { british: /^(.*)centre(s?)$/, american: "$1center$2" },
  { british: /^(.*)licence(s?)$/, american: "$1license$2" },
  { british: /^(.*)catalogue(s|d)?$/, american: "$1catalog$2" },
  { british: /^(.*)artefact(s?)$/, american: "$1artifact$2" },
  { british: /^(.*)(defen|offen)ce(s?)$/, american: "$1$2se$3" },
  { british: /^enquir(.*)$/, american: "inquir$1" },
  { british: /^fulfil(s|ment|ments)?$/, american: "fulfill$1" },
  { british: /^practis(e|ed|es|ing)$/, american: "practic$1" },
  { british: /^grey(.*)$/, american: "gray$1" },
  { british: /^(mis)?learnt$/, american: "$1learned" },
  { british: /^(mis)?spelt$/, american: "$1spelled" },
  { british: /^whilst$/, american: "while" },
  { british: /^amongst$/, american: "among" },
  { british: /^programme(s?)$/, american: "program$1" },
  { british: /^(re)?analys(e|ed|es|ing)$/, american: "$1analyz$2" },
  {
    british: new RegExp(`^(auto|de|non|pre|re|un)?(${ISE})s(e|ed|es|ing|er|ers|ation|ations)$`),
    american: "$1$2z$3",
  },
];

/** An exact context a British spelling may stand in, and why. */
export interface Allowed {
  context: RegExp;
  reason: string;
}

export const ALLOWED: readonly Allowed[] = [
  {
    context: /"cancelled"|'cancelled'|`cancelled`|\|cancelled\b/g,
    reason:
      "the operation status value, stored in SQLite (the schema's CHECKs), sent in the API and in the hub sync messages: changing it needs a migration and changes the wire",
  },
  {
    context: /#\[serde\(rename = "Cancelled"\)\]/g,
    reason: "`OperationStatus::Canceled`'s serialized form, which stays the variant's old name",
  },
  {
    context: /\boperation-cancelled\b/g,
    reason: "the type of the server-sent event at a canceled operation's end, a wire value",
  },
  { context: /\bnotifications\/cancelled\b/g, reason: "MCP's method name" },
  {
    context: /\bcancelled\(\)|\bis_cancelled\b/g,
    reason: "GitHub Actions' `cancelled()` status function and tokio's `CancellationToken::cancelled` and `is_cancelled`",
  },
  {
    context: /\bcompleted cancelled\b/g,
    reason: "a GitHub run's status and conclusion, as `gh run view` prints them",
  },
  {
    context: /'behaviour'|`behaviour`/g,
    reason: "the pull request label red-first.yml and the merge train read",
  },
  {
    context: /\baria-labelledby\b|\bariaLabelledByElements\b/g,
    reason: "the HTML attribute and the DOM property, spelled as the standards spell them",
  },
];

/** Paths the check doesn't read: pinned by checksum, written by others, or quoting third parties. */
export const EXCLUDED: readonly RegExp[] = [
  /^migrations\//, // pinned by checksum
  /^crates\/adapters\/store\/migrations-before-baseline\//, // pinned by checksum
  /^CHANGELOG\.md$/, // the release pull request writes it
  /(^|\/)Cargo\.lock$/, // dependencies' names
  /(^|\/)package-lock\.json$/, // dependencies' names
  /^CODE_OF_CONDUCT\.md$/, // quoted verbatim from the Contributor Covenant
  /^\.github\/skills\/impeccable\//, // Impeccable's vendored skill, as upstream wrote it
  /^\.github\/agents\/impeccable-/, // Impeccable's vendored agents
  /^\.github\/hooks\/impeccable\.json$/, // Impeccable's vendored hook
  /^scripts\/check-american-spelling(\.test)?\.ts$/, // this check: it lists the spellings
];

export interface Spelling {
  line: number;
  column: number;
  word: string;
  american: string;
}

/** The parts of the words in `line`: identifiers split at case changes, digits and punctuation. */
const PARTS = /[A-Z]+(?![a-z])|[A-Z]?[a-z]+/g;

/** `word`'s American form, in `word`'s case, or undefined when it isn't a British spelling. */
export function american(word: string): string | undefined {
  const lower = word.toLowerCase();
  for (const rule of RULES) {
    if (!rule.british.test(lower)) continue;
    const fixed = lower.replace(rule.british, rule.american);
    if (word === word.toUpperCase() && word.length > 1) return fixed.toUpperCase();
    if (word[0] !== lower[0]) return `${fixed[0]?.toUpperCase()}${fixed.slice(1)}`;
    return fixed;
  }
  return undefined;
}

/** The spans of `line` an `ALLOWED` context covers. */
function allowedSpans(line: string): [number, number][] {
  const spans: [number, number][] = [];
  for (const { context } of ALLOWED) {
    for (const match of line.matchAll(context)) spans.push([match.index, match.index + match[0].length]);
  }
  return spans;
}

/** Every British spelling in `text` outside the allowed contexts, with its line and column (from 1). */
export function britishSpellings(text: string): Spelling[] {
  const found: Spelling[] = [];
  text.split(/\r?\n/).forEach((line, index) => {
    let spans: [number, number][] | undefined;
    for (const match of line.matchAll(PARTS)) {
      const word = match[0];
      const fixed = american(word);
      if (fixed === undefined) continue;
      const start = match.index;
      spans ??= allowedSpans(line);
      if (spans.some(([from, to]) => from <= start && start + word.length <= to)) continue;
      found.push({ line: index + 1, column: start + 1, word, american: fixed });
    }
  });
  return found;
}

/** Whether the check skips the repository path `file`. */
export function excluded(file: string): boolean {
  return EXCLUDED.some((pattern) => pattern.test(file));
}

/** The problems with `spellingsByFile` (the British spellings of each file), as messages. */
export function problems(spellingsByFile: ReadonlyMap<string, Spelling[]>): string[] {
  return [...spellingsByFile].flatMap(([file, spellings]) =>
    spellings.map(({ line, column, word, american }) => `${file}:${line}:${column}: ${word} (American: ${american})`),
  );
}

/** The text files git tracks under `root` that the check reads, limited to `pathspecs` when any. */
function trackedFiles(root: string, pathspecs: readonly string[]): string[] {
  return execFileSync("git", ["ls-files", "-z", "--", ...pathspecs], { cwd: root, encoding: "utf8" })
    .split("\0")
    .filter((file) => file !== "" && !excluded(file))
    .sort();
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  const root = path.resolve(import.meta.dirname, "..");
  const spellingsByFile = new Map<string, Spelling[]>();
  for (const file of trackedFiles(root, process.argv.slice(2))) {
    let bytes: Buffer;
    try {
      bytes = readFileSync(path.join(root, file));
    } catch {
      continue; // deleted in the worktree but still tracked
    }
    if (bytes.includes(0)) continue; // binary
    const spellings = britishSpellings(bytes.toString("utf8"));
    if (spellings.length > 0) spellingsByFile.set(file, spellings);
  }
  const found = problems(spellingsByFile);
  if (found.length === 0) {
    process.stdout.write("ok: no British spelling in the tracked files\n");
  } else {
    process.stderr.write(`${found.join("\n")}\n`);
    process.stderr.write(
      "error: the American spelling check failed (AGENTS.md, Conventions): write the American form, or, for a stored or wire value, add its exact context to ALLOWED in scripts/check-american-spelling.ts with its reason\n",
    );
    process.exit(1);
  }
}
