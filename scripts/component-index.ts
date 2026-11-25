// The index of the shared components' accessibility contracts in
// docs/design/components/accessibility.md (§4.5), generated from the contract
// files: one row per docs/design/components/accessibility/<component>.md, its
// heading (`# <title>`) and the component's file in crates/web/src/components/,
// in name order.
//
// Each component's branch adds or edits its own contract file. Were the rows
// kept by hand, two open branches would add a row at the same place, and each
// merge would make the others conflict. Generated, the table's conflict
// resolves the same way every time: take either side, then write the table
// again. (The §16 decisions went further and keep no table at all: decision
// 162; scripts/decision-index.ts still lends its conflict-marker check.)
//
// Usage: node scripts/component-index.ts [--write]
// Without --write, exits 1 when the table differs from the contract files (or
// a file's name or heading is wrong, or it names no component), naming the
// problem on standard error. With --write, replaces the table and exits 0, or
// 1 on a bad file. Runs from any directory.

import { existsSync, readdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { conflictLine } from "./decision-index.ts";

export interface Contract {
  component: string;
  file: string;
  title: string;
}

const FILE = /^([a-z0-9]+(?:_[a-z0-9]+)*)\.md$/;
const PAGE = "docs/design/components/accessibility.md";
const TABLE_HEAD = ["| Contract | Component |", "|---|---|"];

/** The contract `markdown` (the text of `file`) holds, or why it can't be read as one. */
export function parseContract(file: string, markdown: string): Contract | string {
  const component = FILE.exec(file)?.[1];
  if (!component) {
    return `${file}: a contract file is named as its component's .rs file: <component>.md, in snake_case`;
  }
  const conflict = conflictLine(markdown);
  if (conflict !== null) {
    return `${file}:${conflict}: a merge-conflict marker: resolve the conflict first`;
  }
  // The file opens with its heading (§13.7.5): its first line that isn't blank.
  const first = markdown.split("\n").find((line) => line.trim() !== "");
  if (first === undefined || !first.startsWith("# ")) {
    return `${file}: it opens with its heading, "# <title>", such as "# The tooltip"`;
  }
  const title = first.slice(2).trim();
  if (!title) {
    return `${file}: it opens with its heading, "# <title>", such as "# The tooltip"`;
  }
  if (title.includes("|")) {
    return `${file}: its heading has a "|", which would split its row in the index`;
  }
  if (title.includes("[") || title.includes("]")) {
    return `${file}: its heading has a "[" or "]", which would break its link in the index`;
  }
  if (title.endsWith("#")) {
    return `${file}: its heading ends with "#", which Markdown drops as a closing mark`;
  }
  return { component, file, title };
}

/** The table's lines for `contracts`, in component-name order. */
export function renderIndex(contracts: readonly Contract[]): string[] {
  const sorted = [...contracts].sort((a, b) => (a.component < b.component ? -1 : a.component > b.component ? 1 : 0));
  const rows = sorted.map((c) => `| [${c.title}](accessibility/${c.file}) | \`components/${c.component}.rs\` |`);
  return [...TABLE_HEAD, ...rows];
}

/** `page` with the contracts' table replaced by `table`, or why it can't be. */
export function replaceIndex(page: string, table: readonly string[]): { text: string } | string {
  const conflict = conflictLine(page);
  if (conflict !== null) {
    return `${PAGE}:${conflict}: a merge-conflict marker: take either side of the conflict, then run \`just component-index\``;
  }
  const lines = page.split(/\r?\n/);
  const starts = lines.flatMap((line, i) => (line === TABLE_HEAD[0] ? [i] : []));
  const start = starts[0];
  if (start === undefined || starts.length > 1) {
    return `${PAGE}: ${start === undefined ? "no" : "more than one"} "${TABLE_HEAD[0]}" table: the index is one table`;
  }
  let end = start;
  while (end < lines.length && lines[end]?.startsWith("|")) {
    end++;
  }
  const stray = lines.findIndex((line, i) => i >= end && line.startsWith("| [") && line.includes("](accessibility/"));
  if (stray >= 0) {
    return `${PAGE}:${stray + 1}: a contract's row after the index: the index is one table, with no blank line in it`;
  }
  return { text: [...lines.slice(0, start), ...table, ...lines.slice(end)].join("\n") };
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  const root = path.resolve(import.meta.dirname, "..");
  const folder = path.join(root, "docs/design/components/accessibility");
  const pagePath = path.join(root, PAGE);
  const problems: string[] = [];
  const contracts: Contract[] = [];
  for (const file of readdirSync(folder)
    .filter((f) => f.endsWith(".md"))
    .sort()) {
    const parsed = parseContract(file, readFileSync(path.join(folder, file), "utf8"));
    if (typeof parsed === "string") {
      problems.push(parsed);
    } else if (!existsSync(path.join(root, "crates/web/src/components", `${parsed.component}.rs`))) {
      problems.push(`${file}: names no component: crates/web/src/components/${parsed.component}.rs doesn't exist`);
    } else {
      contracts.push(parsed);
    }
  }
  const page = readFileSync(pagePath, "utf8");
  const replaced = replaceIndex(page, renderIndex(contracts));
  if (typeof replaced === "string") {
    problems.push(replaced);
  }
  if (problems.length > 0 || typeof replaced === "string") {
    process.stderr.write(
      `${problems.join("\n")}\nerror: the §4.5 contract index can't be checked or written until the problems above are fixed\n`,
    );
    process.exit(1);
  }
  if (replaced.text === page) {
    process.stdout.write(`ok: the §4.5 index lists all ${contracts.length} component contracts\n`);
  } else if (process.argv.includes("--write")) {
    writeFileSync(pagePath, replaced.text);
    process.stdout.write(`wrote the §4.5 index: ${contracts.length} component contracts\n`);
  } else {
    process.stderr.write(
      `error: the §4.5 index in ${PAGE} doesn't match the contract files: run \`just component-index\` and commit the result (on a conflict in the table, take either side first)\n`,
    );
    process.exit(1);
  }
}
