// Fails when a paragraph or list item in docs/**/*.md runs over 150 words: the
// design documents grew one appended amendment at a time until single
// paragraphs ran to a thousand words, which nobody can read or search (#592).
//
// A block is a paragraph or a list item, up to the next blank line, heading,
// table row, code fence or list item. Fenced code, tables, headings and front
// matter aren't blocks. Words are whitespace-separated, after a link keeps its
// text and drops its target, and inline-code backticks are dropped; a list
// item's marker counts as a word, as in the measurements in #592.
//
// A file that already had a long block when the check landed carries MARKER
// on a line of its own, and is exempt. The exemption lives in the file it
// exempts, not in a shared list: when every restructure pull request deleted
// its line from one list, each merge made the others conflict. Markers only
// go: a marked file with no long block left fails until its marker line is
// deleted, and an unmarked file with one fails at once.
//
// Usage: node scripts/check-paragraph-length.ts
// Exits 0 when the docs pass, 1 with `file:line: N words` lines and the reason
// on standard error otherwise. Runs from any directory.

import { readdirSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const LIMIT = 150;

/** The line that exempts the file it is in, until its restructure (#592). */
export const MARKER = "<!-- paragraph-length: exempt until restructured (#592) -->";

const FENCE = /^(`{3,}|~{3,})(.*)$/;
const HORIZONTAL_RULE = /^([-*_])(\s*\1){2,}$/;
const LIST_ITEM = /^(?:[-*+]|\d+[.)])\s/;

export interface Block {
  line: number;
  words: number;
}

/** The words of a block's text: links keep their text, code loses its backticks. */
function countWords(text: string): number {
  const plain = text.replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1").replaceAll("`", "");
  return plain.split(/\s+/).filter((word) => word !== "").length;
}

/** Every block of `markdown` over `limit` words, with the line it starts on. */
export function longBlocks(markdown: string, limit: number = LIMIT): Block[] {
  const lines = markdown.split(/\r?\n/);
  const found: Block[] = [];
  let start = 0;
  let text = "";

  const end = () => {
    const words = countWords(text);
    if (start > 0 && words > limit) {
      found.push({ line: start, words });
    }
    start = 0;
    text = "";
  };

  let index = 0;
  if (lines[0]?.trimEnd() === "---") {
    index = lines.findIndex((line, i) => i > 0 && line.trimEnd() === "---") + 1;
    if (index === 0) {
      return found; // Front matter that never closes: nothing to read.
    }
  }

  let fence: { mark: string; length: number } | undefined;
  for (; index < lines.length; index++) {
    const line = (lines[index] ?? "").trim().replace(/^(>\s*)+/, "");
    const opens = FENCE.exec(line);
    if (fence !== undefined) {
      const closes = opens !== null && opens[1]?.[0] === fence.mark && (opens[1]?.length ?? 0) >= fence.length;
      if (closes && opens[2]?.trim() === "") {
        fence = undefined;
      }
    } else if (opens !== null) {
      end();
      fence = { mark: opens[1]?.[0] ?? "`", length: opens[1]?.length ?? 3 };
    } else if (line === "" || line.startsWith("#") || line.startsWith("|") || HORIZONTAL_RULE.test(line)) {
      end();
    } else {
      if (LIST_ITEM.test(line)) {
        end();
      }
      if (start === 0) {
        start = index + 1;
      }
      text += ` ${line}`;
    }
  }
  end();
  return found;
}

/** The problems with `longByFile` (the long blocks of each file) given the exempt files, as messages. */
export function problems(longByFile: ReadonlyMap<string, Block[]>, exempt: ReadonlySet<string>): string[] {
  const found: string[] = [];
  for (const [file, blocks] of longByFile) {
    if (!exempt.has(file)) {
      for (const { line, words } of blocks) {
        found.push(`${file}:${line}: ${words} words`);
      }
    }
  }
  for (const file of exempt) {
    if ((longByFile.get(file) ?? []).length === 0) {
      found.push(`${file} has the exemption marker but no block over ${LIMIT} words: delete its marker line`);
    }
  }
  return found;
}

function markdownFiles(root: string): string[] {
  return readdirSync(path.join(root, "docs"), { recursive: true, encoding: "utf8" })
    .filter((file) => file.endsWith(".md"))
    .map((file) => `docs/${file.split(path.sep).join("/")}`)
    .sort();
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  const root = path.resolve(import.meta.dirname, "..");
  const exempt = new Set<string>();
  const longByFile = new Map<string, Block[]>();
  for (const file of markdownFiles(root)) {
    const markdown = readFileSync(path.join(root, file), "utf8");
    if (markdown.split(/\r?\n/).some((line) => line.trim() === MARKER)) {
      exempt.add(file);
    }
    longByFile.set(file, longBlocks(markdown));
  }
  const found = problems(longByFile, exempt);
  if (found.length === 0) {
    process.stdout.write(`ok: no block in docs/ is over ${LIMIT} words (${exempt.size} files with the exemption marker are exempt)\n`);
  } else {
    process.stderr.write(`${found.join("\n")}\n`);
    process.stderr.write(
      `error: the paragraph-length check failed (docs/design/development.md §13.4): split a block over ${LIMIT} words, or make it a list or a table; delete a file's exemption marker once it has no long block left\n`,
    );
    process.exit(1);
  }
}
