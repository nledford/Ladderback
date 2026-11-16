// Keeps each translation in step with its American English (technical design
// §8.4, decision 101).
//
// A language's catalog is crates/l10n/locales/<tag>/*.ftl, and its
// sources.txt holds, for each message accepted after review, the message's
// ID and the SHA-256 of the en-US text it was translated from. That text is
// the message's lines in the en-US catalog, from its ID to its last line,
// value and attributes together, without the comment above it and without
// the spaces at the end of each line. A message is then:
//
// - current: its line's hash is the hash of its en-US text now;
// - missing: the language's catalog lacks it;
// - not reviewed: translated, with no line in sources.txt;
// - stale: its line's hash is another's. The listing gives the English it
//   was translated from, found in the git history of locales/en-US, and the
//   English now.
//
// Translations and lines of messages en-US no longer has are listed apart,
// except a term only the language has (`-snapshot` in ja): that one is the
// language's own, with no English to be translated from.
//
// Usage:
//   node scripts/translations.ts list [<tag>...] [--locales <dir>]
//   node scripts/translations.ts accept <tag> (<id>... | --all) [--locales <dir>]
//
// `list` exits 0 whatever it finds; `accept` writes the lines of the
// messages named (`--all`: every translated message that isn't current),
// skips a term that is the language's own, and refuses, exiting 1, a message
// en-US lacks or the language doesn't translate. Both exit 1 on a catalog
// folder or sources.txt they can't read. `accept` also drops the lines of
// messages en-US or the language no longer has, naming each one.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export interface Entry {
  id: string;
  text: string;
}

export interface Stale {
  id: string;
  /** The hash sources.txt holds: of the English it was translated from. */
  hash: string;
  /** The English now. */
  now: string;
}

export interface Listing {
  tag: string;
  total: number;
  current: number;
  missing: string[];
  unreviewed: string[];
  stale: Stale[];
  unknown: string[];
}

const SOURCE_TAG = "en-US";
const SOURCES_FILE = "sources.txt";

/** A line that begins a message or a term. Only such a line, or a comment,
 * ends the entry above it: any other line belongs to it, whether indented,
 * blank, or at the first column, where Fluent allows a select's `[`, `*[` and
 * `}`, an attribute's `.` and the inside of a placeable. */
const MESSAGE = /^(-?[A-Za-z][A-Za-z0-9_-]*) *=/;
const SOURCE_LINE = /^(-?[A-Za-z][A-Za-z0-9_-]*) ([0-9a-f]{64})$/;

const SOURCES_HEADER = [
  "# The en-US text each message of this catalog was translated from: its ID",
  "# and the SHA-256 of that text (technical design §8.4, decision 101).",
  "# Written by `just accept-translations` when a translation is accepted",
  "# after review; don't edit it by hand.",
];

/** The messages and terms of a catalog file, each with its text as hashed. */
export function entries(ftl: string): Entry[] {
  const found: Entry[] = [];
  let id: string | null = null;
  let lines: string[] = [];
  const close = (): void => {
    if (id === null) return;
    while (lines.length > 0 && lines.at(-1) === "") lines.pop();
    found.push({ id, text: lines.join("\n") });
    id = null;
  };
  for (const raw of ftl.replace(/^\uFEFF/, "").split(/\r?\n/)) {
    const line = raw.trimEnd();
    const name = MESSAGE.exec(line)?.[1];
    if (name !== undefined || line.startsWith("#")) close();
    if (name !== undefined) {
      id = name;
      lines = [line];
    } else if (id !== null) {
      lines.push(line);
    }
  }
  close();
  return found;
}

export function sourceHash(text: string): string {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

/** The lines of a sources.txt (`file` names it in an error): ID to hash. */
export function parseSources(text: string, file: string): Map<string, string> {
  const sources = new Map<string, string>();
  text.split(/\r?\n/).forEach((raw, index) => {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) return;
    const match = SOURCE_LINE.exec(line);
    if (!match?.[1] || !match[2]) {
      throw new Error(`${file}:${index + 1}: not a message ID and a SHA-256 in lowercase hexadecimal: ${line}`);
    }
    if (sources.has(match[1])) throw new Error(`${file}:${index + 1}: ${match[1]} is listed twice`);
    sources.set(match[1], match[2]);
  });
  return sources;
}

export function renderSources(sources: ReadonlyMap<string, string>): string {
  const ids = [...sources.keys()].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  return [...SOURCES_HEADER, ...ids.map((id) => `${id} ${sources.get(id)}`), ""].join("\n");
}

/** Where each en-US message stands in the language `tag`. */
export function listing(
  tag: string,
  english: readonly Entry[],
  translated: ReadonlySet<string>,
  sources: ReadonlyMap<string, string>,
): Listing {
  const found: Listing = { tag, total: english.length, current: 0, missing: [], unreviewed: [], stale: [], unknown: [] };
  for (const { id, text } of english) {
    const hash = sources.get(id);
    if (!translated.has(id)) found.missing.push(id);
    else if (hash === undefined) found.unreviewed.push(id);
    else if (hash !== sourceHash(text)) found.stale.push({ id, hash, now: text });
    else found.current += 1;
  }
  const known = new Set(english.map((entry) => entry.id));
  found.unknown = [...new Set([...translated, ...sources.keys()])]
    .filter((id) => !known.has(id) && !ownTerm(id, translated))
    .sort();
  return found;
}

/** Whether `id`, which en-US lacks, is a term of the language's own. */
function ownTerm(id: string, translated: ReadonlySet<string>): boolean {
  return id.startsWith("-") && translated.has(id);
}

const indented = (text: string, by: string): string[] => text.split("\n").map((line) => (line === "" ? "" : by + line));

/** The listing as `just translations` prints it. `translatedFrom` finds the
 * English a stale message was translated from, by its ID and hash. */
export function renderListing(
  listings: readonly Listing[],
  translatedFrom: (id: string, hash: string) => string | null,
): string {
  if (listings.length === 0) return `No language besides ${SOURCE_TAG} has a catalog.\n`;
  const lines: string[] = [];
  for (const found of listings) {
    lines.push(`${found.tag}: ${found.current} of ${found.total} messages current`);
    for (const id of found.missing) lines.push(`  missing: ${id}`);
    for (const id of found.unreviewed) lines.push(`  not reviewed: ${id}`);
    for (const { id, hash, now } of found.stale) {
      lines.push(`  stale: ${id}`);
      const then = translatedFrom(id, hash);
      if (then === null) lines.push("    translated from: not in this repository's history");
      else lines.push("    translated from:", ...indented(then, "      "));
      lines.push("    now:", ...indented(now, "      "));
    }
    for (const id of found.unknown) lines.push(`  not in ${SOURCE_TAG}: ${id}`);
  }
  return `${lines.join("\n")}\n`;
}

/** What `accept --all` says of the stale messages `ids` it left out:
 * nothing when it left none. */
export function leftOut(tag: string, ids: readonly string[]): string | null {
  throw new Error(`todo: ${tag} ${ids.join(", ")}`);
}

/** The sources after accepting `ids` (`"all"`: every translated message
 * that isn't current) in the language `tag`, or why they can't be. A term of
 * the language's own is skipped: it has no English to record. The lines of
 * messages en-US or the language no longer has are removed. */
export function accepting(
  tag: string,
  english: readonly Entry[],
  translated: ReadonlySet<string>,
  sources: ReadonlyMap<string, string>,
  ids: readonly string[] | "all",
): { sources: Map<string, string>; accepted: string[]; removed: string[]; staleLeftOut: string[] } | string {
  const hashes = new Map(english.map((entry) => [entry.id, sourceHash(entry.text)]));
  const chosen =
    ids === "all"
      ? english.map((entry) => entry.id).filter((id) => translated.has(id) && sources.get(id) !== hashes.get(id))
      : ids;
  const next = new Map(sources);
  const accepted: string[] = [];
  for (const id of chosen) {
    const hash = hashes.get(id);
    if (hash === undefined && ownTerm(id, translated)) continue;
    if (hash === undefined) return `${tag}: ${SOURCE_TAG} has no message ${id}`;
    if (!translated.has(id)) return `${tag}: ${id} isn't translated`;
    next.set(id, hash);
    accepted.push(id);
  }
  const removed = [...next.keys()].filter((id) => !hashes.has(id) || !translated.has(id)).sort();
  for (const id of removed) next.delete(id);
  return { sources: next, accepted, removed, staleLeftOut: [] };
}

function git(dir: string, args: string[]): string | null {
  const result = spawnSync("git", args, { cwd: dir, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
  return result.error || result.status !== 0 ? null : result.stdout;
}

/** Finds, in the git history of the catalog folder `dir`, the text of a
 * message whose hash is the one given: newest version first, each file
 * version read once. Nothing when `dir` isn't in a repository, git can't be
 * run, or no version matches. */
export function history(dir: string): (id: string, hash: string) => string | null {
  let blobs: string[] | null = null;
  const read = new Map<string, Map<string, Map<string, string>>>();
  return (id, hash) => {
    blobs ??= [
      ...new Set(
        (git(dir, ["log", "--format=", "--raw", "--no-abbrev", "--no-renames", "--", "."]) ?? "")
          .split("\n")
          .map((line) => /^:\S+ \S+ \S+ ([0-9a-f]+) [A-Z]\d*\t.*\.ftl$/.exec(line)?.[1])
          .filter((blob): blob is string => blob !== undefined && !/^0+$/.test(blob)),
      ),
    ];
    for (const blob of blobs) {
      let byId = read.get(blob);
      if (byId === undefined) {
        byId = new Map();
        for (const entry of entries(git(dir, ["cat-file", "blob", blob]) ?? "")) {
          const texts = byId.get(entry.id) ?? new Map<string, string>();
          texts.set(sourceHash(entry.text), entry.text);
          byId.set(entry.id, texts);
        }
        read.set(blob, byId);
      }
      const text = byId.get(id)?.get(hash);
      if (text !== undefined) return text;
    }
    return null;
  };
}

interface Catalog {
  dir: string;
  entries: Entry[];
  sources: Map<string, string>;
}

function readCatalog(dir: string): Catalog {
  const files = readdirSync(dir)
    .filter((name) => name.endsWith(".ftl"))
    .sort();
  const all = files.flatMap((name) => entries(readFileSync(path.join(dir, name), "utf8")));
  const sourcesPath = path.join(dir, SOURCES_FILE);
  const sources = existsSync(sourcesPath)
    ? parseSources(readFileSync(sourcesPath, "utf8"), path.join(path.basename(dir), SOURCES_FILE))
    : new Map<string, string>();
  return { dir, entries: all, sources };
}

function main(argv: string[]): number {
  const args = [...argv];
  let locales = path.resolve(import.meta.dirname, "../crates/l10n/locales");
  const at = args.indexOf("--locales");
  if (at >= 0) {
    const dir = args[at + 1];
    if (dir === undefined) throw new Error("--locales needs a folder");
    locales = path.resolve(dir);
    args.splice(at, 2);
  }
  const [command, ...rest] = args;
  const tags = readdirSync(locales, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
  if (!tags.includes(SOURCE_TAG)) throw new Error(`${locales} has no ${SOURCE_TAG} catalog`);
  const english = readCatalog(path.join(locales, SOURCE_TAG)).entries;
  const translation = (tag: string): Catalog => {
    if (tag === SOURCE_TAG) throw new Error(`${SOURCE_TAG} is the source: nothing in it is translated`);
    if (!tags.includes(tag)) throw new Error(`${tag} has no catalog in ${locales}`);
    return readCatalog(path.join(locales, tag));
  };
  if (command === "list") {
    const listed = (rest.length > 0 ? rest : tags.filter((tag) => tag !== SOURCE_TAG)).map((tag) => {
      const catalog = translation(tag);
      return listing(tag, english, new Set(catalog.entries.map((entry) => entry.id)), catalog.sources);
    });
    process.stdout.write(renderListing(listed, history(path.join(locales, SOURCE_TAG))));
    return 0;
  }
  if (command === "accept") {
    const [tag, ...ids] = rest;
    if (tag === undefined || ids.length === 0) throw new Error("usage: accept <tag> (<id>... | --all)");
    if (ids.includes("--all") && ids.length > 1) throw new Error("accept: --all takes no IDs");
    const catalog = translation(tag);
    const result = accepting(
      tag,
      english,
      new Set(catalog.entries.map((entry) => entry.id)),
      catalog.sources,
      ids.includes("--all") ? "all" : ids,
    );
    if (typeof result === "string") {
      console.error(result);
      return 1;
    }
    writeFileSync(path.join(catalog.dir, SOURCES_FILE), renderSources(result.sources));
    for (const id of result.accepted) console.log(`${tag}: accepted ${id}`);
    for (const id of result.removed) console.log(`${tag}: removed ${id}`);
    return 0;
  }
  throw new Error("usage: translations.ts (list [<tag>...] | accept <tag> (<id>... | --all)) [--locales <dir>]");
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
