// Fails when docs/guide/theme/toc.html.hbs, the user docs site's copy of
// mdBook's template for the sidebar page it frames only for a reader with
// JavaScript off, no longer matches the installed mdBook's own (§16, decision
// 128; #1216). The copy exists only to add a <title>; mdBook has none
// (rust-lang/mdBook#3274). A copied template goes stale silently when the
// pinned mdBook changes, so this compares what each renders rather than
// trusting a comment.
//
// It builds the site twice in a scratch folder, with the copy and without
// it, so the installed mdBook's own template renders the second, and
// requires the two toc.html files to differ by exactly one added line that is
// a non-empty <title> and nothing else. Once mdBook writes a title of its
// own, it says to delete the copy. A part of the template this site never
// renders isn't compared.
//
// Usage: node scripts/check-theme-override.ts
// Exits 0 with an `ok:` line when the copy matches, 1 with what differs on
// standard error otherwise. Runs offline, from any directory, and doesn't
// read or write docs/guide/book/.

import { execFileSync } from "node:child_process";
import { cpSync, mkdtempSync, readFileSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

/** The copied template, relative to the site's folder. */
export const OVERRIDE = "theme/toc.html.hbs";

const ANY_TITLE = /<title[\s>]/i;
const TITLE_LINE = /^\s*<title>[^<]*\S[^<]*<\/title>\s*$/;
const RECOPY = "Copy the installed mdBook's toc.html.hbs again and add the title";

/**
 * What is wrong with `ours`, the page the copy renders, against `pristine`,
 * the page mdBook's own template renders for the same site; empty when the
 * copy adds exactly one line, a non-empty <title>, and changes nothing else.
 */
export function compareToc(pristine: string, ours: string): string[] {
  if (ANY_TITLE.test(pristine)) {
    return [
      `mdBook now writes a <title> in toc.html itself: delete docs/guide/${OVERRIDE}, and its mentions in decision 128 and the Justfile`,
    ];
  }
  const before = pristine.split("\n");
  const after = ours.split("\n");
  if (after.length !== before.length + 1) {
    return [
      `docs/guide/${OVERRIDE} must render mdBook's toc.html with one line added, its <title>; it renders ${after.length - before.length} lines more. ${RECOPY}`,
    ];
  }
  const added = after.findIndex((line, i) => line !== before[i]);
  if (!TITLE_LINE.test(after[added] ?? "")) {
    return [
      `docs/guide/${OVERRIDE} changes line ${added + 1} of mdBook's toc.html rather than only adding a non-empty <title> line: ${JSON.stringify(after[added])}. ${RECOPY}`,
    ];
  }
  if (after.slice(added + 1).join("\n") !== before.slice(added).join("\n")) {
    return [`docs/guide/${OVERRIDE} differs from mdBook's toc.html after its <title>. ${RECOPY}`];
  }
  return [];
}

function main(): void {
  const site = path.resolve(import.meta.dirname, "../docs/guide");
  const scratch = mkdtempSync(path.join(tmpdir(), "theme-override-"));
  try {
    // The site's toc.html, built in the scratch folder with the copy or without it.
    const renderToc = (withOverride: boolean): string => {
      const source = path.join(scratch, withOverride ? "ours" : "pristine");
      cpSync(site, source, {
        recursive: true,
        filter: (from) => {
          const relative = path.relative(site, from).split(path.sep).join("/");
          return relative !== "book" && !relative.startsWith("book/") && (withOverride || relative !== OVERRIDE);
        },
      });
      const out = `${source}-book`;
      // MDBOOK_LOG=warn keeps its progress lines out of `just docs-site`, and its warnings and errors in.
      execFileSync("mdbook", ["build", source, "--dest-dir", out], {
        stdio: ["ignore", "ignore", "inherit"],
        env: { ...process.env, MDBOOK_LOG: "warn" },
      });
      return readFileSync(path.join(out, "toc.html"), "utf8");
    };
    const problems = compareToc(renderToc(false), renderToc(true));
    for (const problem of problems) console.error(problem);
    if (problems.length > 0) {
      process.exitCode = 1;
    } else {
      console.log(`ok: docs/guide/${OVERRIDE} renders mdBook's toc.html with only its <title> added`);
    }
  } catch (error) {
    console.error(`check-theme-override: ${error instanceof Error ? error.message.split("\n")[0] : String(error)}`);
    process.exitCode = 1;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

// Run as a script, not imported: argv[1] can be a symlink, which
// import.meta.url has already resolved.
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) main();
