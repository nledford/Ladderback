#!/usr/bin/env node
// Fails when leptosfmt would drop a comment. leptosfmt formats the Rust code
// inside a `view!` block (an attribute's closure, a `{…}` child) with a fork
// of prettyplease, which keeps no non-doc comments, so a comment there is
// lost or moved without a word. Comments between a view's nodes are kept,
// except one just after a one-line text or `{…}` child.
// `just fmt` runs this before it formats, and `just fmt-check` with its
// checks, so a comment can't vanish on a reformat (§13.13).
//
// Usage: node scripts/leptosfmt-comments.ts <config> <file or folder>...
// Runs `leptosfmt --stdin` on each `.rs` file and compares the number of `//`
// in its input and its output, and runs leptosfmt again on that output. String literals come out of leptosfmt as they
// went in, so only a comment can change that number. Prints each file that
// would lose one, or whose layout a second run changes, and exits 1; prints nothing and exits 0 otherwise.

import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const [config, ...roots] = process.argv.slice(2);
if (config === undefined || roots.length === 0) {
  console.error("usage: node scripts/leptosfmt-comments.ts <config> <file or folder>...");
  process.exit(2);
}

/** The `.rs` files at or under `path`, sorted. */
function rustFiles(path: string): string[] {
  if (!statSync(path).isDirectory()) return path.endsWith(".rs") ? [path] : [];
  return readdirSync(path)
    .sort()
    .flatMap((entry) => rustFiles(join(path, entry)));
}

let failed = false;
for (const file of roots.flatMap(rustFiles)) {
  const source = readFileSync(file, "utf8");
  const run = spawnSync("leptosfmt", ["--stdin", "--quiet", "--config-file", config], {
    input: source,
    encoding: "utf8",
  });
  if (run.error !== undefined || run.status !== 0) {
    console.error(`${file}: leptosfmt failed: ${run.error?.message ?? run.stderr.trim()}`);
    failed = true;
    continue;
  }
  // A comment just after a one-line text or `{…}` child is joined to that
  // line on one run and lands above it on the next, keeping the count: a
  // second run that changes the output again catches it.
  const again = spawnSync("leptosfmt", ["--stdin", "--quiet", "--config-file", config], {
    input: run.stdout,
    encoding: "utf8",
  });
  if (again.status !== 0 || again.stdout !== run.stdout) {
    console.error(
      `${file}: leptosfmt doesn't settle on one layout. A comment just after a one-line text or {…} ` +
        "child moves onto another node: put it above the parent element.",
    );
    failed = true;
    continue;
  }
  const before = source.split("//").length;
  const after = run.stdout.split("//").length;
  if (after < before) {
    console.error(
      `${file}: leptosfmt would drop ${before - after} of its //. A comment inside the Rust code of a view! ` +
        "(an attribute's closure, a {…} child) is lost: move it to node level, above the element it explains.",
    );
    failed = true;
  }
}
process.exitCode = failed ? 1 : 0;
