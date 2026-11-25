// The decisions of docs/design/roadmap.md §16, one file each in
// docs/design/decisions/NN-<slug>.md with `# Decision NN: <question>` as its
// heading. The files are the index: §16 keeps no table of them (decision 162).
// A committed table took a row from every pull request that recorded a
// decision, so any two open ones conflicted at its end, and each conflict
// stopped GitHub's update of the branch until someone merged by hand.
//
// Usage: node scripts/decision-index.ts [--print]
// Checks every decision file (its name, heading and number, no two files with
// one number, no conflict marker) and that §16 holds no index row again,
// exiting 1 and naming each problem on standard error. With --print, it then
// prints the index, one Markdown row per decision in number order.
// Runs from any directory.

import { readdirSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export interface Decision {
  number: number;
  file: string;
  question: string;
}

const FILE = /^(\d+)-[a-z0-9-]+\.md$/;
const HEADING = /^# Decision (\d+): (.+)$/;
const SUPERSEDED = /^\*\*Superseded by decision (\d+)\*\*/;
const CONFLICT = /^(<{7}|>{7}|\|{7})( |$)|^={7}$/;
const FENCE = /^(`{3,}|~{3,})/;
const SECTION = "## 16. Decisions";
const TABLE_HEAD = ["| # | Question |", "|---|---|"];
// A link to a decision file in a line with a cell separator: a table row,
// indented or not, with a leading pipe or not, linked as `decisions/` or
// `./decisions/`. §16's own prose links decisions too, but has no pipe.
const ROW_LINK = /\]\((\.\/)?decisions\/\d+-/;

/** The lines of `markdown` outside fenced code, with their 1-based numbers. */
export function proseLines(markdown: string): { line: string; number: number }[] {
  const out: { line: string; number: number }[] = [];
  let fence: string | null = null;
  markdown.split(/\r?\n/).forEach((line, i) => {
    const open = FENCE.exec(line)?.[1];
    if (fence === null && open) {
      fence = open;
    } else if (fence !== null && line.startsWith(fence)) {
      fence = null;
    } else if (fence === null) {
      out.push({ line, number: i + 1 });
    }
  });
  return out;
}

/** The 1-based number of the first line of `markdown` that is a merge-conflict marker, if any. */
export function conflictLine(markdown: string): number | null {
  const i = markdown.split(/\r?\n/).findIndex((line) => CONFLICT.test(line));
  return i < 0 ? null : i + 1;
}

/** The decision `markdown` (the text of `file`) records, or why it can't be read as one. */
export function parseDecision(file: string, markdown: string): Decision | string {
  const name = FILE.exec(file);
  if (!name?.[1]) {
    return `${file}: a decision file is named NN-<slug>.md`;
  }
  const conflict = conflictLine(markdown);
  if (conflict !== null) {
    return `${file}:${conflict}: a merge-conflict marker: resolve the conflict first`;
  }
  const prose = proseLines(markdown);
  const heading = prose.find(({ line }) => line.startsWith("# "))?.line;
  const parts = heading === undefined ? null : HEADING.exec(heading);
  const asked = parts?.[2]?.trim();
  if (!parts?.[1] || !asked) {
    return `${file}: its heading must read "# Decision ${Number(name[1])}: <question>"`;
  }
  if (Number(parts[1]) !== Number(name[1])) {
    return `${file}: its heading says decision ${parts[1]}, its name ${Number(name[1])}`;
  }
  if (asked.includes("|")) {
    return `${file}: its heading has a "|", which would split its row in the index`;
  }
  // A decision a later one replaced says so in a paragraph of its own, and its row says so too.
  const superseded = prose.map(({ line }) => SUPERSEDED.exec(line)?.[1]).find((n) => n !== undefined);
  const question = asked + (superseded ? ` (superseded by decision ${superseded})` : "");
  return { number: Number(name[1]), file, question };
}

/** The table's lines for `decisions`, in number order; a number two files share is a problem. */
export function renderIndex(decisions: readonly Decision[]): { lines: string[]; problems: string[] } {
  const sorted = [...decisions].sort((a, b) => a.number - b.number || a.file.localeCompare(b.file));
  const problems: string[] = [];
  for (let i = 1; i < sorted.length; i++) {
    const [before, here] = [sorted[i - 1], sorted[i]];
    if (before && here && before.number === here.number) {
      problems.push(
        `decision ${here.number} is both ${before.file} and ${here.file}: the branch that merged second renumbers (§13.5)`,
      );
    }
  }
  const rows = sorted.map((d) => `| [${d.number}](decisions/${d.file}) | ${d.question} |`);
  return { lines: [...TABLE_HEAD, ...rows], problems };
}

/** What is wrong with `roadmap` for the decisions, if anything: a conflict
 * marker anywhere, no §16 heading (so its check can't run), or a table row in
 * §16 that lists a decision again. */
export function roadmapProblem(roadmap: string): string | null {
  const conflict = conflictLine(roadmap);
  if (conflict !== null) {
    return `docs/design/roadmap.md:${conflict}: a merge-conflict marker`;
  }
  const lines = proseLines(roadmap);
  const section = lines.findIndex(({ line }) => line.trimEnd() === SECTION);
  if (section < 0) {
    return `docs/design/roadmap.md: no "${SECTION}" heading, so the check that it lists no decision in a table can't run: restore the heading, or change SECTION in scripts/decision-index.ts with it`;
  }
  for (const { line, number } of lines.slice(section + 1)) {
    if (line.startsWith("## ")) {
      return null;
    }
    if (line.includes("|") && ROW_LINK.test(line)) {
      return `docs/design/roadmap.md:${number}: §16 lists a decision in a table again: the decision files are the index (decision 162), so delete the table; \`just decision-index\` prints it`;
    }
  }
  return null;
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  const root = path.resolve(import.meta.dirname, "..");
  const folder = path.join(root, "docs/design/decisions");
  const roadmapPath = path.join(root, "docs/design/roadmap.md");
  const problems: string[] = [];
  const decisions: Decision[] = [];
  for (const file of readdirSync(folder)
    .filter((f) => f.endsWith(".md") && f !== "README.md")
    .sort()) {
    const parsed = parseDecision(file, readFileSync(path.join(folder, file), "utf8"));
    if (typeof parsed === "string") {
      problems.push(parsed);
    } else {
      decisions.push(parsed);
    }
  }
  const index = renderIndex(decisions);
  problems.push(...index.problems);
  const roadmapIssue = roadmapProblem(readFileSync(roadmapPath, "utf8"));
  if (roadmapIssue !== null) {
    problems.push(roadmapIssue);
  }
  if (problems.length > 0) {
    process.stderr.write(`${problems.join("\n")}\nerror: the decision files have the problems above\n`);
    process.exit(1);
  }
  if (process.argv.includes("--print")) {
    process.stdout.write(`${index.lines.join("\n")}\n`);
  } else {
    process.stdout.write(`ok: ${decisions.length} decision files, each with a number of its own\n`);
  }
}
