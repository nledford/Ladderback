// Fails when an exclusion in .cargo/mutants.toml no longer matches anything.
// An `exclude_re` entry named by `file:line:col` stops matching once code
// above it moves the line, and an `exclude_globs` entry once its file is
// renamed; the mutant it held back is then judged again, but a pull request's
// run tests only the lines its diff touches, so it never sees one whose line
// only moved (nor a file only renamed, which git's diff shows without a line).
// The nightly full run found it a day later, for someone else (2026-11-26:
// three such mutants). This check finds the dead entry in the change that
// moved it.
//
// Each `exclude_re` entry must match a mutant, and each `exclude_globs` entry
// a source file, from cargo-mutants' own lists of every workspace crate with
// the configuration left out (`cargo mutants --list` and `--list-files`,
// `--workspace --no-config`: a second or three each, nothing built). A glob
// is checked against files rather than mutants because a file of DOM glue
// can have no mutant today and still be excluded on purpose. A pattern
// matches as cargo-mutants matches it: a regex anywhere in the mutant's name,
// a glob against the whole path of a file. Only the syntax both sides read
// the same is supported (JavaScript's `u` regexes; globs of `*`, `**` and
// `?`): anything else fails as unreadable rather than pass.
//
// Usage: node scripts/check-mutants-exclusions.ts
// Exits 0 when every exclusion matches, 1 with one line per dead entry and
// the reason on standard error otherwise. Runs from any directory.

import { execFileSync } from "node:child_process";
import { readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

/** The two lists of .cargo/mutants.toml that name code by pattern. */
export interface Exclusions {
  exclude_re: string[];
  exclude_globs: string[];
}

/** A TOML string on a line of its own in a list: basic (`"…"`) or literal (`'…'`). */
const ENTRY = /^("(?:[^"\\]|\\.)*"|'[^']*'),?$/;

/**
 * The `exclude_re` and `exclude_globs` lists of `toml`, each written as
 * `key = [`, one string a line (comments and blank lines between them), `]`.
 * Throws on a line in a list that isn't one of those, so a list written
 * another way fails the check rather than pass it unread.
 */
export function parseExclusions(toml: string): Exclusions {
  const found: Exclusions = { exclude_re: [], exclude_globs: [] };
  let list: string[] | undefined;
  for (const [index, raw] of toml.split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (list === undefined) {
      const opens = /^(exclude_re|exclude_globs)\s*=\s*\[$/.exec(line);
      if (opens?.[1] === "exclude_re" || opens?.[1] === "exclude_globs") {
        list = found[opens[1]];
      } else if (/^(exclude_re|exclude_globs)\b/.test(line)) {
        throw new Error(`.cargo/mutants.toml:${index + 1}: write the list as \`key = [\` on a line of its own: ${line}`);
      }
    } else if (line === "]") {
      list = undefined;
    } else if (line !== "" && !line.startsWith("#")) {
      const entry = ENTRY.exec(line)?.[1];
      if (entry === undefined) {
        throw new Error(`.cargo/mutants.toml:${index + 1}: not one string a line: ${line}`);
      }
      list.push(entry.startsWith('"') ? (JSON.parse(entry) as string) : entry.slice(1, -1));
    }
  }
  if (list !== undefined) {
    throw new Error(".cargo/mutants.toml: a list never closes");
  }
  return found;
}

/**
 * `glob` as a regex over a file's path from the workspace root: `**` crosses
 * folders (between two slashes, none too), `*` and `?` don't. One without a `/` matches a
 * file or folder of that name anywhere, and what is under such a folder.
 * Throws on `{…}` and `[…]`, which this check doesn't read.
 */
export function globRegExp(glob: string): RegExp {
  if (/[{}[\]]/.test(glob)) {
    throw new Error("braces and brackets aren't read");
  }
  const body = glob
    .split(/(\*\*\/|\*\*|\*|\?)/)
    .map((part) => {
      if (part === "**/") return "(.*/)?";
      if (part === "**") return ".*";
      if (part === "*") return "[^/]*";
      if (part === "?") return "[^/]";
      return part.replace(/[.+^$()|\\]/g, "\\$&");
    })
    .join("");
  return new RegExp(glob.includes("/") ? `^${body}$` : `(^|/)${body}(/.*)?$`);
}

/** An `exclude_re` entry naming a mutant by `file:line:col`, with the line and column left open. */
function anyPosition(pattern: string): RegExp | undefined {
  const opened = pattern.replace(/^([^:]*):\d+:\d+:/, "$1:\\d+:\\d+:");
  return opened === pattern ? undefined : new RegExp(opened, "u");
}

/**
 * The problems with `exclusions` against the mutants `names` and the source
 * `files` cargo-mutants lists, one message per dead entry.
 */
export function deadExclusions(exclusions: Exclusions, names: readonly string[], files: readonly string[]): string[] {
  const found: string[] = [];
  for (const pattern of exclusions.exclude_re) {
    let regex: RegExp;
    try {
      regex = new RegExp(pattern, "u");
    } catch (error) {
      found.push(`exclude_re '${pattern}': not a regex this check can read (${String(error)})`);
      continue;
    }
    if (names.some((name) => regex.test(name))) continue;
    const elsewhere = anyPosition(pattern);
    const moved = elsewhere === undefined ? [] : names.filter((name) => elsewhere.test(name));
    const now = moved.length === 0 ? "" : `; the same mutant is now at ${moved.map((name) => name.split(": ")[0]).join(", ")}`;
    found.push(`exclude_re '${pattern}' matches no mutant${now}`);
  }
  for (const glob of exclusions.exclude_globs) {
    let regex: RegExp;
    try {
      regex = globRegExp(glob);
    } catch (error) {
      found.push(`exclude_globs '${glob}': not a glob this check can read (${String(error)})`);
      continue;
    }
    if (!files.some((file) => regex.test(file))) {
      found.push(`exclude_globs '${glob}' matches no source file`);
    }
  }
  return found;
}

/** A color code (ESC, `[`, its numbers, `m`): built from the code, as a regex may not hold ESC itself. */
const COLOR = new RegExp(`${String.fromCharCode(27)}\\[[0-9;]*m`, "g");

/**
 * The lines cargo-mutants listed in `output`, one name or path each, without
 * color codes or trailing whitespace a terminal setting or a wrapper may add.
 */
export function listedLines(output: string): string[] {
  return output
    .split("\n")
    .map((line) => line.replace(COLOR, "").trimEnd())
    .filter((line) => line !== "");
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  const root = path.resolve(import.meta.dirname, "..");
  const exclusions = parseExclusions(readFileSync(path.join(root, ".cargo/mutants.toml"), "utf8"));
  // One line per mutant (`--list`) or source file (`--list-files`). The
  // binary is run itself, not through `cargo`, so no wrapper of cargo's
  // (CI's build cache has one) stands between it and the list.
  const list = (flag: string): string[] => {
    try {
      const listed = execFileSync("cargo-mutants", ["mutants", flag, "--workspace", "--no-config"], {
        cwd: root,
        encoding: "utf8",
        maxBuffer: 64 * 1024 * 1024,
        stdio: ["ignore", "pipe", "inherit"],
      });
      return listedLines(listed);
    } catch {
      process.stderr.write(`error: \`cargo mutants ${flag}\` failed; is cargo-mutants installed? (\`just setup\` installs it)\n`);
      process.exit(1);
    }
  };
  const names = list("--list");
  const found = deadExclusions(exclusions, names, list("--list-files"));
  if (found.length === 0) {
    const count = exclusions.exclude_re.length + exclusions.exclude_globs.length;
    process.stdout.write(`ok: each of the ${count} exclusions in .cargo/mutants.toml matches a mutant or a source file\n`);
  } else {
    process.stderr.write(`${found.map((problem) => `.cargo/mutants.toml: ${problem}`).join("\n")}\n`);
    // What a listed name looks like here, escaped, in case the lists differ
    // from the ones the exclusions were written against.
    process.stderr.write(`cargo-mutants listed ${names.length} mutants; the first is ${JSON.stringify(names[0] ?? "")}\n`);
    process.stderr.write(
      "error: an exclusion matches nothing: the line it names moved or the code is gone. Re-judge the mutant and update or remove the exclusion in this change (docs/design/development.md §13.1)\n",
    );
    process.exit(1);
  }
}
