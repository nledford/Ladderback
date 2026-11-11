// Checks a pull request's title against Conventional Commits (AGENTS.md,
// "Conventions"; docs/design/development.md §13.8). The title becomes the
// squash commit's subject, and release-plz builds the changelog and the version
// bump from that subject (release-plz.toml), so a malformed title silently drops
// a change from the changelog or misses a release.
//
// Usage: node scripts/check-pr-title.ts "<title>"
// Exits 0 when the title is valid, 1 with the reasons on standard error
// otherwise, 2 when called without exactly one title.

// The types release-plz.toml's `commit_parsers` know: feat, fix, perf and
// revert reach the changelog; the others are skipped by name. A type outside
// this list would be neither.
const TYPES = [
  "feat",
  "fix",
  "perf",
  "revert",
  "docs",
  "test",
  "chore",
  "ci",
  "build",
  "refactor",
  "style",
] as const;

const HEADER = /^(?<type>[a-z]+)(?:\((?<scope>[^()\s]+)\))?(?<breaking>!)?: (?<subject>.*)$/;

export function problems(title: string): string[] {
  const found: string[] = [];
  const header = HEADER.exec(title);
  if (header?.groups === undefined) {
    found.push('it is not "<type>(<optional scope>)!?: <subject>", as in "fix(store): keep the log writable"');
    return found;
  }
  const { type, subject } = header.groups;
  if (type === undefined || !(TYPES as readonly string[]).includes(type)) {
    found.push(`"${type ?? ""}" is not one of the types release-plz knows: ${TYPES.join(", ")}`);
  }
  if (subject === undefined || subject.trim() === "") {
    found.push("the subject after the colon is empty");
    return found;
  }
  if (subject !== subject.trim()) {
    found.push("the subject starts or ends with white space");
  }
  if (subject.endsWith(".")) {
    found.push("the subject ends with a full stop");
  }
  if (/\(#\d+\)$/.test(subject)) {
    found.push('the subject ends with "(#N)": GitHub adds the pull request number when it squashes');
  }
  return found;
}

const args = process.argv.slice(2);
if (args.length !== 1 || args[0] === undefined) {
  process.stderr.write('usage: node scripts/check-pr-title.ts "<title>"\n');
  process.exit(2);
}
const title = args[0];
const found = problems(title);
if (found.length === 0) {
  process.stdout.write(`ok: ${JSON.stringify(title)}\n`);
} else {
  process.stderr.write(`error: the pull request title ${JSON.stringify(title)} is not a Conventional Commit:\n`);
  for (const problem of found) {
    process.stderr.write(`  - ${problem}\n`);
  }
  process.exit(1);
}
