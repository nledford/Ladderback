// Fails when docs/guide/theme/toc.html.hbs, the user docs site's copy of
// mdBook's template for the sidebar page it frames only for a reader with
// JavaScript off, no longer matches the installed mdBook's own (§16, decision
// 128; #1216). The copy exists only to add a <title>; mdBook has none
// (rust-lang/mdBook#3274). A copied template goes stale silently when the
// pinned mdBook changes, so this compares what each renders rather than
// trusting a comment.
//
// It builds the site a second time without the copy, so the installed
// mdBook's own template renders the page, and requires the two toc.html files
// to differ by exactly one added line holding the <title>. Once mdBook writes
// a title of its own, it says to delete the copy.
//
// Usage: node scripts/check-theme-override.ts
// Runs after `mdbook build docs/guide` (`just docs-site`). Exits 0 when the
// copy matches, 1 with what differs on standard error otherwise. Runs offline,
// from any directory.

import { execFileSync } from "node:child_process";
import { cpSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

/** The copied template, relative to the site's folder. */
export const OVERRIDE = "theme/toc.html.hbs";

const TITLE = /<title>/i;

/**
 * What is wrong with `ours`, the page the copy renders, against `pristine`,
 * the page mdBook's own template renders for the same site; empty when the
 * copy adds exactly one line, its <title>, and changes nothing else.
 */
export function compareToc(pristine: string, ours: string): string[] {
  if (TITLE.test(pristine)) {
    return [
      `mdBook now writes a <title> in toc.html itself: delete docs/guide/${OVERRIDE}, and its mentions in decision 128 and the Justfile`,
    ];
  }
  const before = pristine.split("\n");
  const after = ours.split("\n");
  const added = after.findIndex((line, i) => line !== before[i]);
  if (added === -1 || after.length !== before.length + 1) {
    return [
      `docs/guide/${OVERRIDE} must render mdBook's toc.html with one line added, its <title>; it renders ${after.length - before.length} lines more. Copy the installed mdBook's toc.html.hbs again and add the title`,
    ];
  }
  const problems: string[] = [];
  if (!TITLE.test(after[added] ?? "")) {
    problems.push(
      `docs/guide/${OVERRIDE} changes line ${added + 1} of mdBook's toc.html, not only adding its <title>: ${JSON.stringify(after[added])}. Copy the installed mdBook's toc.html.hbs again and add the title`,
    );
  } else if (after.slice(added + 1).join("\n") !== before.slice(added).join("\n")) {
    problems.push(
      `docs/guide/${OVERRIDE} differs from mdBook's toc.html after its <title>. Copy the installed mdBook's toc.html.hbs again and add the title`,
    );
  }
  return problems;
}

function main(): void {
  const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
  const site = path.join(root, "docs/guide");
  const scratch = mkdtempSync(path.join(tmpdir(), "theme-override-"));
  try {
    const source = path.join(scratch, "guide");
    cpSync(site, source, {
      recursive: true,
      filter: (from) => {
        const relative = path.relative(site, from).split(path.sep).join("/");
        return relative !== "book" && !relative.startsWith("book/") && relative !== OVERRIDE;
      },
    });
    const out = path.join(scratch, "book");
    execFileSync("mdbook", ["build", source, "--dest-dir", out], { stdio: ["ignore", "ignore", "inherit"] });
    const problems = compareToc(
      readFileSync(path.join(out, "toc.html"), "utf8"),
      readFileSync(path.join(site, "book/toc.html"), "utf8"),
    );
    for (const problem of problems) console.error(problem);
    if (problems.length > 0) process.exitCode = 1;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) main();
