// The release notes `just release-pr` writes after a release candidate, when
// release-plz opens no release PR (technical design §12.1, #1669). In git-only
// mode release-plz skips pre-release tags when it looks for the last release,
// so after `vX.Y.Z-rc.N` it finds nothing to release, and its `set-version`
// renames the newest CHANGELOG.md entry instead of adding one. This adds the
// entry release-plz would write, from the commits since the last tag of any
// kind (`release-plz.toml`'s `commit_parsers` and `body`), above the others:
//
// - for the next candidate, X.Y.Z-rc.N+1, an entry of its own, compared with
//   vX.Y.Z-rc.N;
// - for the release, X.Y.Z, one entry compared with the last release before
//   the candidates, folding in every candidate's entry and the commits since
//   the last one. A section a candidate's notes share is merged, in release
//   order; a candidate's "Upgrading" notes are left out, except "Upgrading
//   from <the last release>", and of those only the newest candidate's, which
//   restates the others. The candidates' own entries stay below it.
//
// Run in a checkout after `release-plz set-version <version>` with
// CHANGELOG.md as it was: `node scripts/release-notes.ts <version>
// [--date YYYY-MM-DD]`. The pure parts are exported for
// scripts/release-notes.test.ts.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

export type Commit = { message: string };

type Version = { major: number; minor: number; patch: number; rc?: number };

const part = "(0|[1-9][0-9]*)";
const versionPattern = new RegExp(`^${part}\\.${part}\\.${part}(?:-rc\\.${part})?$`);

function parseVersion(text: string): Version | undefined {
  const m = versionPattern.exec(text);
  if (!m) return undefined;
  return { major: Number(m[1]), minor: Number(m[2]), patch: Number(m[3]), rc: m[4] === undefined ? undefined : Number(m[4]) };
}

/** Semver order for X.Y.Z and X.Y.Z-rc.N: a candidate sorts before its release. */
function compare(a: Version, b: Version): number {
  return (
    a.major - b.major ||
    a.minor - b.minor ||
    a.patch - b.patch ||
    (a.rc ?? Number.POSITIVE_INFINITY) - (b.rc ?? Number.POSITIVE_INFINITY) ||
    0
  );
}

const base = (v: Version) => `${v.major}.${v.minor}.${v.patch}`;

/** release-plz's groups, in its order (`<!-- N -->` in `commit_parsers`). */
const groups: [RegExp, string][] = [
  [/^feat$/, "Added"],
  [/^(perf|revert)$/, "Changed"],
  [/^fix$/, "Fixed"],
];

/** A footer's first line (`Token: value` or `Token #value`), which ends the footer before it. */
const footerToken = /^(BREAKING[ -]CHANGE|[A-Za-z][\w-]*)(: | #)/;

/**
 * The CHANGELOG.md line release-plz writes for a commit, and its section:
 * `Breaking` for a breaking change of any type (`!` or a `BREAKING CHANGE:`
 * footer, kept by `protect_breaking_commits`), else Added, Changed or Fixed;
 * `undefined` for a skipped type or a message that isn't a Conventional Commit.
 * A footer's description runs to the next footer, as in a squashed commit
 * whose `Closes #N` follows it.
 */
export function commitLine(commit: Commit, repo: string): { group: string; line: string } | undefined {
  // release-plz's default preprocessor links each `(#N)` to its pull request.
  const message = commit.message.replace(/\(#(\d+)\)/g, `([#$1](${repo}/pull/$1))`);
  const [subject = "", ...rest] = message.split("\n");
  const m = /^([a-z]+)(?:\([^)]*\))?(!)?: (.+)$/.exec(subject.trim());
  if (!m) return undefined;
  const [, type = "", bang, description = ""] = m;
  const start = rest.findIndex((line) => /^BREAKING[ -]CHANGE: /.test(line));
  const text = description.charAt(0).toUpperCase() + description.slice(1);
  if (bang || start >= 0) {
    let why = "";
    if (start >= 0) {
      const end = rest.findIndex((line, i) => i > start && footerToken.test(line));
      why = rest
        .slice(start, end < 0 ? undefined : end)
        .join("\n")
        .replace(/^BREAKING[ -]CHANGE: /, "")
        .trim();
    }
    return { group: "Breaking", line: `- ${text}${why && why !== description ? `: ${why}` : ""}` };
  }
  const group = groups.find(([pattern]) => pattern.test(type))?.[1];
  return group ? { group, line: `- ${text}` } : undefined;
}

/**
 * What the release `version` follows, given the repository's tags: `from`,
 * the tag its compare link starts at; `since`, the tag its commits start
 * after; and for a release after candidates, those `candidates`, oldest
 * first. Throws unless the newest tag is a candidate and `version` is its next
 * candidate or its release.
 */
export function nextRelease(version: string, tags: string[]): { from: string; since: string; candidates: string[] } {
  const wanted = parseVersion(version);
  if (!wanted) throw new Error(`version '${version}' is not X.Y.Z or X.Y.Z-rc.N (no leading zeros)`);
  const tagged = tags
    .map((tag) => ({ tag, v: tag.startsWith("v") ? parseVersion(tag.slice(1)) : undefined }))
    .filter((t): t is { tag: string; v: Version } => t.v !== undefined)
    .sort((a, b) => compare(a.v, b.v));
  if (tagged.some((t) => compare(t.v, wanted) === 0)) throw new Error(`${version} is already tagged`);
  const last = tagged.at(-1);
  if (!last || last.v.rc === undefined) {
    throw new Error(`the last tag, ${last?.tag ?? "none"}, is not a release candidate: the Release workflow opens this release PR`);
  }
  const follows = base(wanted) === base(last.v) && (wanted.rc === undefined || wanted.rc > last.v.rc);
  if (!follows) {
    throw new Error(`${version} doesn't follow ${last.tag.slice(1)}: give ${base(last.v)}-rc.${last.v.rc + 1} or ${base(last.v)}`);
  }
  if (wanted.rc !== undefined) return { from: last.tag, since: last.tag, candidates: [] };
  const candidates = tagged.filter((t) => base(t.v) === base(wanted) && t.v.rc !== undefined);
  const first = candidates[0] ?? last;
  const before = tagged.filter((t) => compare(t.v, first.v) < 0 && t.v.rc === undefined).at(-1);
  if (!before) throw new Error(`no release before ${first.tag} to compare ${version} with`);
  return { from: before.tag, since: last.tag, candidates: candidates.map((t) => t.tag.slice(1)) };
}

type Section = { title: string; body: string[] };

/**
 * The `###` sections of an entry's text, a fenced code block's lines kept
 * whole; any text above the first, under the title `""`. Empty ones are left out.
 */
function sections(entry: string): Section[] {
  const out: Section[] = [{ title: "", body: [] }];
  let fenced = false;
  for (const line of entry.split("\n").slice(1)) {
    if (line.trimStart().startsWith("```")) fenced = !fenced;
    const heading = fenced ? undefined : /^### (.+)$/.exec(line);
    if (heading) out.push({ title: (heading[1] ?? "").trim(), body: [] });
    else out.at(-1)?.body.push(line);
  }
  for (const s of out) {
    while (s.body[0] === "") s.body.shift();
    while (s.body.at(-1) === "") s.body.pop();
  }
  return out.filter((s) => s.title !== "" || s.body.length > 0);
}

/** Each release entry's text by version, as `## [version]` starts it, up to the next entry. */
function entries(changelog: string): Map<string, string> {
  const out = new Map<string, string>();
  const starts = [...changelog.matchAll(/^## \[([^\]]+)\]/gm)];
  starts.forEach((m, i) => {
    const end = starts[i + 1]?.index ?? changelog.length;
    out.set(m[1] ?? "", changelog.slice(m.index, end));
  });
  return out;
}

/** The sections of the commits' lines, oldest first, as release-plz groups them. */
function commitSections(commits: Commit[], repo: string): Section[] {
  const out: Section[] = [];
  for (const commit of commits) {
    const line = commitLine(commit, repo);
    if (!line) continue;
    let section = out.find((s) => s.title === line.group);
    if (!section) {
      section = { title: line.group, body: [] };
      out.push(section);
    }
    section.body.push(line.line);
  }
  return out;
}

/** Text above the sections, then Upgrading notes, Breaking, release-plz's groups and any other section. */
function rank(title: string): number {
  if (title === "") return -1;
  if (title.startsWith("Upgrading")) return 0;
  const order = ["Breaking", "Added", "Changed", "Fixed"].indexOf(title);
  return order < 0 ? 5 : order + 1;
}

/** Sections with the same title merged, the earlier's lines first, in `rank` order. */
function merge(lists: Section[][]): Section[] {
  const out: Section[] = [];
  for (const s of lists.flat()) {
    const same = out.find((o) => o.title === s.title);
    if (!same) out.push({ title: s.title, body: [...s.body] });
    else if (s.body.length > 0)
      same.body.push(...(same.body.length > 0 && !(s.body[0] ?? "").startsWith("- ") ? [""] : []), ...s.body);
  }
  return out.sort((a, b) => rank(a.title) - rank(b.title));
}

/**
 * CHANGELOG.md with the entry for `version` added above the newest one, from
 * the `commits` since the last tag, oldest first, and for a release after
 * candidates their entries folded in (see the top of this file).
 */
export function releaseNotes(input: { changelog: string; version: string; tags: string[]; commits: Commit[]; date: string }): string {
  const { changelog, version, tags, commits, date } = input;
  const repo = /\((https:\/\/github\.com\/[^/\s]+\/[^/\s]+)\/compare\//.exec(changelog)?.[1];
  if (!repo) throw new Error("CHANGELOG.md has no compare link to take the repository's address from");
  const known = entries(changelog);
  if (known.has(version)) throw new Error(`CHANGELOG.md already has an entry for ${version}`);
  const { from, candidates } = nextRelease(version, tags);
  const last = from.slice(1);
  const upgrading = `Upgrading from ${last}`;
  const folded = candidates.map((candidate) => {
    const entry = known.get(candidate);
    if (entry === undefined) throw new Error(`CHANGELOG.md has no entry for ${candidate} to fold into ${version}`);
    return sections(entry).filter((s) => !s.title.startsWith("Upgrading") || s.title === upgrading);
  });
  // A later candidate's "Upgrading from <the last release>" replaces an earlier one's: it restates them.
  const newest = folded.findLastIndex((list) => list.some((s) => s.title === upgrading));
  const kept = folded.map((list, i) => list.filter((s) => s.title !== upgrading || i === newest));
  const body = merge([...kept, commitSections(commits, repo)])
    .map((s) => (s.title === "" ? `\n${s.body.join("\n")}\n` : `\n### ${s.title}\n\n${s.body.join("\n")}\n`))
    .join("");
  const entry = `## [${version}](${repo}/compare/${from}...v${version}) - ${date}\n${body}`;
  const unreleased = /^## \[Unreleased\]\n\n?/m.exec(changelog);
  if (!unreleased) throw new Error("CHANGELOG.md has no `## [Unreleased]` heading to add the entry under");
  const at = unreleased.index + unreleased[0].length;
  return `${changelog.slice(0, at).replace(/\n*$/, "\n\n")}${entry}\n${changelog.slice(at)}`;
}

function main(args: string[]): void {
  const [version, flag, date] = args;
  if (!version || (flag !== undefined && (flag !== "--date" || !/^\d{4}-\d{2}-\d{2}$/.test(date ?? "")))) {
    console.error("usage: node scripts/release-notes.ts <version> [--date YYYY-MM-DD]");
    process.exit(2);
  }
  const git = (...a: string[]) => execFileSync("git", a, { encoding: "utf8" });
  const tags = git("tag", "--list", "v*", "--merged", "HEAD").split("\n").filter(Boolean);
  const { since } = nextRelease(version, tags);
  const log = git("log", "--reverse", "--no-merges", "--format=%B%x00", `${since}..HEAD`);
  const commits = log
    .split("\0")
    .map((message) => message.trim())
    .filter(Boolean)
    .map((message) => ({ message }));
  const today = new Date();
  const local = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
  const changelog = readFileSync("CHANGELOG.md", "utf8");
  writeFileSync("CHANGELOG.md", releaseNotes({ changelog, version, tags, commits, date: date ?? local }));
}

if (import.meta.main) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(`error: ${(error as Error).message}`);
    process.exit(1);
  }
}
