// Fails when a link in the user docs site's pages (docs/guide/, which mdBook
// builds) doesn't resolve. mdBook copies a broken link into the page as it
// is, so without this the site would publish it (#1177).
//
// Each Markdown page's links outside code are checked: inline links and
// images (one inside another too), reference definitions, autolinks, and the
// href, src and srcset of HTML tags.
// - A relative link must stay inside the site and name a file that exists.
//   A page must be one SUMMARY.md lists (mdBook builds no other), a folder
//   must hold a README.md (its index.html on the site), and a README.md
//   takes its folder's link: mdBook links it as a README.html it never builds.
// - A link to this repository on GitHub (`…/Ladderback/blob/main/<path>`, or
//   `tree`, `raw`, `HEAD`, in any case) must name a file or folder git has in
//   the checkout, in its case.
// - An anchor on a Markdown page must be one of its headings' anchors, made
//   as mdBook makes them (GitHub makes the same for the headings these pages
//   use), or an `<a id>`.
// Links elsewhere aren't fetched: the check runs offline. A link whose text
// runs over a line break isn't seen; the pages keep a paragraph on one line.
//
// Usage: node scripts/check-site-links.ts
// Exits 0 when every link resolves, 1 with `file:line: target: problem` lines
// on standard error otherwise. Runs from any directory.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

/** The site's source folder, which docs/guide/book.toml builds. */
export const SITE = "docs/guide";

const REPOSITORY_URL = "https://github.com/nledford/Ladderback";
const ON_MAIN =
  /^https?:\/\/(?:www\.)?github\.com\/nledford\/ladderback\/(?:blob|tree|raw)\/(?:main|HEAD)\/([^?#]*)(?:\?[^#]*)?(?:#(.*))?$/i;
const SCHEME = /^[a-z][a-z0-9+.-]*:/i;
const FENCE = /^ {0,3}(`{3,}|~{3,})/;
const HEADING = /^ {0,3}#{1,6}\s+(.*?)(?:\s+#+)?\s*$/;
const CUSTOM_ID = /\s*\{#([^}\s]+)\}$/;
const HTML_ANCHOR = /<a\s[^>]*?\b(?:id|name)="([^"]+)"/g;
const ENTITIES: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'" };
// A code span closes only on a backtick run as long as its opener (CommonMark).
const CODE_SPAN = /(?<!`)(`+)(?!`)[\s\S]*?(?<!`)\1(?!`)/g;
const INLINE_LINK = /!?\[((?:\\.|[^[\]\\]|\[(?:\\.|[^\]\\])*\])*)\]\(\s*(<[^>]*>|[^\s)]+)(?:\s+(?:"[^"]*"|'[^']*'))?\s*\)/g;
const REFERENCE = /^ {0,3}\[(?!\^)[^\]]+\]:\s*(<[^>]*>|\S+)/;
const AUTOLINK = /<([a-z][a-z0-9+.-]*:[^\s<>]*)>/gi;
const HTML_TAG = /<[a-z][a-z0-9-]*\s[^>]*>/gi;
const HTML_URL = /\b(href|src|srcset)="([^"]*)"/g;

/** What the check reads of the repository: paths are relative to its root, with `/`. */
export interface Repository {
  /** A file's text, or `undefined` when there is none. */
  read(file: string): string | undefined;
  /** Whether a file or folder exists. */
  exists(file: string): boolean;
  /** The Markdown pages under `folder`, sorted, the build's output left out. */
  pages(folder: string): string[];
}

export interface Link {
  line: number;
  target: string;
}

/** The lines of `markdown` outside fenced code, with their 1-based numbers. */
function proseLines(markdown: string): Array<[number, string]> {
  const lines: Array<[number, string]> = [];
  let fence: string | undefined;
  markdown.split("\n").forEach((text, index) => {
    const marker = FENCE.exec(text)?.[1];
    if (fence !== undefined) {
      if (marker !== undefined && marker[0] === fence[0] && marker.length >= fence.length) fence = undefined;
    } else if (marker !== undefined) {
      fence = marker;
    } else {
      lines.push([index + 1, text]);
    }
  });
  return lines;
}

/** mdBook's anchor for a heading's text: lower case, emphasis and punctuation dropped, spaces as hyphens. */
function slug(heading: string): string {
  return heading
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]*>/g, "")
    .replace(/&(amp|lt|gt|quot|#39);/g, (_, name: string) => ENTITIES[name] ?? "")
    .replace(/(^|[^\p{L}\p{N}_])_+|_+(?=[^\p{L}\p{N}_]|$)/gu, "$1")
    .toLowerCase()
    .replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, "")
    .replaceAll(" ", "-");
}

/**
 * The anchors a page has: its headings' (an `{#id}` as written, a repeat of
 * one already made with `-1`, `-2`… until it is new), and each `<a id>` or `<a name>`.
 */
export function anchors(markdown: string): Set<string> {
  const found = new Set<string>();
  const made = new Set<string>();
  for (const [, text] of proseLines(markdown)) {
    const heading = HEADING.exec(text)?.[1];
    if (heading !== undefined) {
      const custom = CUSTOM_ID.exec(heading)?.[1];
      const base = custom ?? slug(heading);
      let id = base;
      for (let count = 1; custom === undefined && made.has(id); count++) id = `${base}-${count}`;
      made.add(id);
      found.add(id);
    }
    for (const match of text.matchAll(HTML_ANCHOR)) found.add(match[1] ?? "");
  }
  return found;
}

/** Every link target in `markdown` outside code, in order of kind on each line, angle brackets dropped. */
export function links(markdown: string): Link[] {
  const found: Link[] = [];
  const unwrap = (target: string) => (target.startsWith("<") ? target.slice(1, -1) : target);
  for (const [line, text] of proseLines(markdown)) {
    const prose = text.replace(CODE_SPAN, "");
    const reference = REFERENCE.exec(prose)?.[1];
    if (reference !== undefined) {
      found.push({ line, target: unwrap(reference) });
      continue;
    }
    const inline = (part: string) => {
      for (const match of part.matchAll(INLINE_LINK)) {
        inline(match[1] ?? "");
        found.push({ line, target: unwrap(match[2] ?? "") });
      }
    };
    inline(prose);
    const rest = prose.replace(INLINE_LINK, "");
    for (const match of rest.matchAll(AUTOLINK)) found.push({ line, target: match[1] ?? "" });
    for (const tag of rest.matchAll(HTML_TAG)) {
      for (const [, attribute, value = ""] of tag[0].matchAll(HTML_URL)) {
        const targets = attribute === "srcset" ? value.split(",").map((source) => source.trim().split(/\s+/)[0] ?? "") : [value];
        for (const target of targets) if (target !== "") found.push({ line, target });
      }
    }
  }
  return found;
}

/** `text` percent-decoded, or `undefined` when it holds a `%` that doesn't decode. */
function decoded(text: string): string | undefined {
  try {
    return decodeURIComponent(text);
  } catch {
    return undefined;
  }
}

const MALFORMED = "malformed percent-encoding";

/** The problem with an anchor on `file`, or `undefined` when it resolves or isn't checked. */
function anchorProblem(repository: Repository, file: string, anchor: string | undefined, shown: string): string | undefined {
  if (anchor === undefined || anchor === "" || !file.endsWith(".md")) return undefined;
  const id = decoded(anchor);
  if (id === undefined) return MALFORMED;
  return anchors(repository.read(file) ?? "").has(id) ? undefined : `${shown} has no heading with that anchor`;
}

/** The problem with one link from `page`, or `undefined` when it resolves. */
function problem(site: string, repository: Repository, built: ReadonlySet<string>, page: string, target: string): string | undefined {
  const onMain = ON_MAIN.exec(target);
  if (onMain !== null) {
    const named = decoded(onMain[1] ?? "");
    if (named === undefined) return MALFORMED;
    const file = path.posix.normalize(named).replace(/\/$/, "");
    if (file === ".." || file.startsWith("../")) return "points outside the repository";
    if (!repository.exists(file)) return "no such file in the repository";
    return anchorProblem(repository, file, onMain[2], file);
  }
  if (SCHEME.test(target) || target.startsWith("//")) return undefined;
  if (target.startsWith("/")) return "a path from the site's root breaks under its /Ladderback/ prefix; use a relative link";
  const [pathPart = "", anchor] = target.split(/#(.*)/s);
  const named = decoded(pathPart.replace(/\?.*$/, ""));
  if (named === undefined) return MALFORMED;
  let file = pathPart === "" ? page : path.posix.normalize(path.posix.join(path.posix.dirname(page), named)).replace(/\/$/, "");
  if (file === ".." || file.startsWith("../")) return "points outside the repository";
  if (file !== site && !file.startsWith(`${site}/`)) return `leaves the site; link to ${REPOSITORY_URL}/blob/main/${file}`;
  if (!repository.exists(file)) return "no such file in the site";
  // SUMMARY.md lists chapters by file, the landing page's README.md too.
  if (pathPart !== "" && path.posix.basename(file) === "README.md" && page !== `${site}/SUMMARY.md`) {
    return "mdBook links it as README.html, which it doesn't build; link its folder (./) instead";
  }
  if (repository.read(file) === undefined) {
    file = `${file}/README.md`;
    if (!repository.exists(file)) return "a folder with no README.md, so no page";
  }
  if (pathPart !== "" && file.endsWith(".md") && !built.has(file)) return "SUMMARY.md doesn't list it, so the site doesn't build it";
  return anchorProblem(repository, file, anchor, path.posix.relative(site, file));
}

/** Every link in the site's pages that doesn't resolve, as `file:line: target: problem`. */
export function brokenLinks(site: string, repository: Repository): string[] {
  const summary = `${site}/SUMMARY.md`;
  const built = new Set(
    links(repository.read(summary) ?? "")
      .filter((link) => !SCHEME.test(link.target))
      .map((link) => path.posix.normalize(path.posix.join(site, link.target.split("#")[0] ?? ""))),
  );
  const found: string[] = [];
  for (const page of repository.pages(site)) {
    for (const { line, target } of links(repository.read(page) ?? "")) {
      const wrong = problem(site, repository, built, page, target);
      if (wrong !== undefined) found.push(`${page}:${line}: ${target}: ${wrong}`);
    }
  }
  return found;
}

/**
 * The checkout at `root` as git sees it: tracked files and untracked ones it
 * doesn't ignore, named in their case, so a link in the wrong case or into
 * the site's build output (`book/`) fails on any disk.
 */
function checkout(root: string): Repository {
  const listed = execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"], { cwd: root, encoding: "utf8" })
    .split("\0")
    .filter((file) => file !== "" && existsSync(path.join(root, file)));
  const files = new Set(listed);
  const folders = new Set(
    listed.flatMap((file) =>
      file
        .split("/")
        .slice(0, -1)
        .map((_, end, parts) => parts.slice(0, end + 1).join("/")),
    ),
  );
  return {
    read: (file) => (files.has(file) ? readFileSync(path.join(root, file), "utf8") : undefined),
    exists: (file) => files.has(file) || folders.has(file),
    pages: (folder) => listed.filter((file) => file.startsWith(`${folder}/`) && file.endsWith(".md")).sort(),
  };
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  const found = brokenLinks(SITE, checkout(path.resolve(import.meta.dirname, "..")));
  if (found.length === 0) {
    process.stdout.write(`ok: every link in ${SITE}/ resolves\n`);
  } else {
    process.stderr.write(`${found.join("\n")}\n`);
    process.stderr.write(
      `error: the site link check failed (docs/design/development.md §13.4): fix each link; a file outside ${SITE}/ takes a link to it on GitHub\n`,
    );
    process.exit(1);
  }
}
