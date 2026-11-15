// The index of decisions in docs/design/roadmap.md §16, generated from the
// decision files: one row per docs/design/decisions/NN-<slug>.md, its number
// and its heading (`# Decision NN: <question>`), in number order.
//
// When every pull request that recorded a decision added its row by hand, two
// open ones always added a row at the same place, so each merge made the
// others conflict. Generated, the table's conflict resolves the same way every
// time: take either side, then write the table again.
//
// Usage: node scripts/decision-index.ts [--write]
// Without --write, exits 1 when the table differs from the decision files (or
// a file's name, heading or number is wrong), naming the problem on standard
// error. With --write, replaces the table and exits 0, or 1 on a bad file.
// Runs from any directory.

import { readdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export interface Decision {
  number: number;
  file: string;
  question: string;
}

const FILE = /^(\d+)-[a-z0-9-]+\.md$/;
const HEADING = /^# Decision (\d+): (.+)$/;
const SECTION = "## 16. Decisions";
const TABLE_HEAD = ["| # | Question |", "|---|---|"];

/** The decision `markdown` (the text of `file`) records, or why it can't be read as one. */
export function parseDecision(file: string, markdown: string): Decision | string {
  const name = FILE.exec(file);
  if (!name?.[1]) {
    return `${file}: a decision file is named NN-<slug>.md`;
  }
  const heading = markdown.split(/\r?\n/).find((line) => line.startsWith("# "));
  const parts = heading === undefined ? null : HEADING.exec(heading);
  if (!parts?.[1] || !parts[2]) {
    return `${file}: its heading must read "# Decision ${Number(name[1])}: <question>"`;
  }
  if (Number(parts[1]) !== Number(name[1])) {
    return `${file}: its heading says decision ${parts[1]}, its name ${Number(name[1])}`;
  }
  // A decision a later one replaced says so in a paragraph of its own, and its row says so too.
  const superseded = /^\*\*Superseded by decision (\d+)\*\*/m.exec(markdown)?.[1];
  const question = parts[2].trim() + (superseded ? ` (superseded by decision ${superseded})` : "");
  return { number: Number(name[1]), file, question };
}

/** The table's lines for `decisions`, in number order; a number two files share is a problem. */
export function renderIndex(decisions: readonly Decision[]): { lines: string[]; problems: string[] } {
  const sorted = [...decisions].sort((a, b) => a.number - b.number || a.file.localeCompare(b.file));
  const problems: string[] = [];
  for (let i = 1; i < sorted.length; i++) {
    const [before, here] = [sorted[i - 1], sorted[i]];
    if (before && here && before.number === here.number) {
      problems.push(`decision ${here.number} is both ${before.file} and ${here.file}: the branch that merged second renumbers (§13.5)`);
    }
  }
  const rows = sorted.map((d) => `| [${d.number}](decisions/${d.file}) | ${d.question} |`);
  return { lines: [...TABLE_HEAD, ...rows], problems };
}

/** `roadmap` with §16's table replaced by `table`, or null when §16 has no table. */
export function replaceIndex(roadmap: string, table: readonly string[]): string | null {
  const lines = roadmap.split("\n");
  const section = lines.indexOf(SECTION);
  if (section < 0) {
    return null;
  }
  const start = lines.findIndex((line, i) => i > section && line === TABLE_HEAD[0]);
  const next = lines.findIndex((line, i) => i > section && line.startsWith("## "));
  if (start < 0 || (next >= 0 && start > next)) {
    return null;
  }
  let end = start;
  while (end < lines.length && lines[end]?.startsWith("|")) {
    end++;
  }
  return [...lines.slice(0, start), ...table, ...lines.slice(end)].join("\n");
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  const root = path.resolve(import.meta.dirname, "..");
  const folder = path.join(root, "docs/design/decisions");
  const roadmapPath = path.join(root, "docs/design/roadmap.md");
  const problems: string[] = [];
  const decisions: Decision[] = [];
  for (const file of readdirSync(folder).filter((f) => f.endsWith(".md") && f !== "README.md").sort()) {
    const parsed = parseDecision(file, readFileSync(path.join(folder, file), "utf8"));
    if (typeof parsed === "string") {
      problems.push(parsed);
    } else {
      decisions.push(parsed);
    }
  }
  const index = renderIndex(decisions);
  problems.push(...index.problems);
  const roadmap = readFileSync(roadmapPath, "utf8");
  const written = replaceIndex(roadmap, index.lines);
  if (written === null) {
    problems.push(`docs/design/roadmap.md: no "${TABLE_HEAD[0]}" table under "${SECTION}"`);
  }
  if (problems.length > 0) {
    process.stderr.write(`${problems.join("\n")}\nerror: the decision index can't be written until the decision files are fixed\n`);
    process.exit(1);
  }
  if (written === roadmap) {
    process.stdout.write(`ok: the §16 index lists all ${decisions.length} decisions\n`);
  } else if (process.argv.includes("--write")) {
    writeFileSync(roadmapPath, written ?? roadmap);
    process.stdout.write(`wrote the §16 index: ${decisions.length} decisions\n`);
  } else {
    process.stderr.write(
      "error: the §16 index in docs/design/roadmap.md doesn't match the decision files: run `just decision-index` and commit the result (on a conflict in the table, take either side first)\n",
    );
    process.exit(1);
  }
}
