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
// Usage: node scripts/check-paragraph-length.ts
// Exits 0 when the docs pass, 1 with `file:line: N words` lines and the reason
// on standard error otherwise. Runs from any directory.

import { readdirSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const LIMIT = 150;

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

/** The problems with `longByFile` (the long blocks of each file), as messages. */
export function problems(longByFile: ReadonlyMap<string, Block[]>): string[] {
  const found: string[] = [];
  for (const [file, blocks] of longByFile) {
    for (const { line, words } of blocks) {
      found.push(`${file}:${line}: ${words} words`);
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
  const longByFile = new Map<string, Block[]>();
  for (const file of markdownFiles(root)) {
    longByFile.set(file, longBlocks(readFileSync(path.join(root, file), "utf8")));
  }
  const found = problems(longByFile);
  if (found.length === 0) {
    process.stdout.write(`ok: no block in docs/ is over ${LIMIT} words\n`);
  } else {
    process.stderr.write(`${found.join("\n")}\n`);
    process.stderr.write(
      `error: the paragraph-length check failed (docs/design/development.md §13.4): split a block over ${LIMIT} words, or make it a list or a table\n`,
    );
    process.exit(1);
  }
}
