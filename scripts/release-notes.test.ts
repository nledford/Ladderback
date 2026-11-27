// Tests for scripts/release-notes.ts, the release notes `just release-pr`
// writes after a release candidate, when release-plz opens no release PR
// (technical design §12.1, #1669). `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { commitLine, nextRelease, releaseNotes } from "./release-notes.ts";

const repo = "https://github.com/nledford/Ladderback";

const header = `# Changelog

All notable changes to Ladderback are listed here.

## [Unreleased]
`;

const rc1 = `## [0.6.0-rc.1](${repo}/compare/v0.5.0...v0.6.0-rc.1) - 2026-11-20

### Upgrading from 0.5.0

Stop 0.5.0 first, then install 0.6.0.

\`\`\`sh
### not a heading: a comment in a shell block
rustic tag
\`\`\`

### Breaking

- The wire names say machine (#1631).

### Added

- A Docker image ([#1196](${repo}/pull/1196))

### Fixed

- A fix in rc.1 ([#1200](${repo}/pull/1200))
`;

const rc2 = `## [0.6.0-rc.2](${repo}/compare/v0.6.0-rc.1...v0.6.0-rc.2) - 2026-11-26

### Upgrading

From 0.6.0-rc.1: install this release and restart Ladderback.

### Added

- A hub's start page is its dashboard ([#1667](${repo}/pull/1667))
`;

const older = `## [0.5.0](${repo}/compare/v0.4.0...v0.5.0) - 2026-10-01

### Added

- Something from 0.5.0
`;

const changelog = `${header}\n${rc2}\n${rc1}\n${older}`;

const commits = [
  { message: "feat(web): show the hub's machines (#1670)" },
  { message: "docs: say so in the guide (#1671)" },
  { message: "fix: keep the dashboard's order (#1672)" },
  { message: "chore: release v0.6.0-rc.2 (#1668)" },
  { message: "perf: draw the table once (#1673)" },
  { message: "refactor!: rename the hub's table (#1674)" },
  { message: "feat: a key file per hub (#1675)\n\nBREAKING CHANGE: the old key file is read no more." },
  { message: "Merge something unconventional" },
];

test("a commit's line is release-plz's: the description capitalized, the PR linked; skipped types have none", () => {
  assert.deepEqual(commitLine({ message: "feat(web): show the hub's machines (#1670)" }, repo), {
    group: "Added",
    line: `- Show the hub's machines ([#1670](${repo}/pull/1670))`,
  });
  assert.deepEqual(commitLine({ message: "fix: keep the order" }, repo), { group: "Fixed", line: "- Keep the order" });
  assert.deepEqual(commitLine({ message: "perf: faster" }, repo), { group: "Changed", line: "- Faster" });
  assert.deepEqual(commitLine({ message: "revert: undo it" }, repo), { group: "Changed", line: "- Undo it" });
  for (const type of ["docs", "test", "chore", "ci", "build", "refactor", "style"]) {
    assert.equal(commitLine({ message: `${type}: nothing to say` }, repo), undefined, type);
  }
  assert.equal(commitLine({ message: "Merge branch 'x'" }, repo), undefined);
});

test("a breaking change of any type goes under Breaking, with its footer's description", () => {
  assert.deepEqual(commitLine({ message: "refactor!: rename the table" }, repo), { group: "Breaking", line: "- Rename the table" });
  assert.deepEqual(commitLine({ message: "feat: a key file\n\nBREAKING CHANGE: the old one is gone." }, repo), {
    group: "Breaking",
    line: "- A key file: the old one is gone.",
  });
});

test("the next candidate's entry goes above the last one's, from its tag, the older entries untouched", () => {
  const out = releaseNotes({
    changelog,
    version: "0.6.0-rc.3",
    tags: ["v0.5.0", "v0.6.0-rc.1", "v0.6.0-rc.2"],
    commits,
    date: "2026-11-27",
  });
  const entry = `## [0.6.0-rc.3](${repo}/compare/v0.6.0-rc.2...v0.6.0-rc.3) - 2026-11-27

### Breaking

- Rename the hub's table ([#1674](${repo}/pull/1674))
- A key file per hub ([#1675](${repo}/pull/1675)): the old key file is read no more.

### Added

- Show the hub's machines ([#1670](${repo}/pull/1670))

### Changed

- Draw the table once ([#1673](${repo}/pull/1673))

### Fixed

- Keep the dashboard's order ([#1672](${repo}/pull/1672))
`;
  assert.equal(out, `${header}\n${entry}\n${rc2}\n${rc1}\n${older}`);
});

test("the final release's entry folds in every candidate's since the last release, keeping theirs below", () => {
  const out = releaseNotes({
    changelog,
    version: "0.6.0",
    tags: ["v0.4.0", "v0.5.0", "v0.6.0-rc.1", "v0.6.0-rc.2"],
    commits: [{ message: "fix: the last fix (#1680)" }, { message: "feat: one more thing (#1681)" }],
    date: "2026-12-01",
  });
  const entry = `## [0.6.0](${repo}/compare/v0.5.0...v0.6.0) - 2026-12-01

### Upgrading from 0.5.0

Stop 0.5.0 first, then install 0.6.0.

\`\`\`sh
### not a heading: a comment in a shell block
rustic tag
\`\`\`

### Breaking

- The wire names say machine (#1631).

### Added

- A Docker image ([#1196](${repo}/pull/1196))
- A hub's start page is its dashboard ([#1667](${repo}/pull/1667))
- One more thing ([#1681](${repo}/pull/1681))

### Fixed

- A fix in rc.1 ([#1200](${repo}/pull/1200))
- The last fix ([#1680](${repo}/pull/1680))
`;
  assert.equal(out, `${header}\n${entry}\n${rc2}\n${rc1}\n${older}`);
});

test("the final release with no commits since the last candidate still gets its entry", () => {
  const out = releaseNotes({
    changelog,
    version: "0.6.0",
    tags: ["v0.5.0", "v0.6.0-rc.1", "v0.6.0-rc.2"],
    commits: [],
    date: "2026-12-01",
  });
  assert.match(
    out,
    /## \[0\.6\.0\]\(https:\/\/github\.com\/nledford\/Ladderback\/compare\/v0\.5\.0\.\.\.v0\.6\.0\) - 2026-12-01\n\n### Upgrading from 0\.5\.0\n/,
  );
  assert.ok(out.indexOf("## [0.6.0](") < out.indexOf("## [0.6.0-rc.2]("));
});

test("the version must follow the last candidate: its next candidate or its release", () => {
  const tags = ["v0.5.0", "v0.6.0-rc.1", "v0.6.0-rc.2"];
  assert.deepEqual(nextRelease("0.6.0-rc.3", tags), { from: "v0.6.0-rc.2", since: "v0.6.0-rc.2", candidates: [] });
  assert.deepEqual(nextRelease("0.6.0", tags), { from: "v0.5.0", since: "v0.6.0-rc.2", candidates: ["0.6.0-rc.1", "0.6.0-rc.2"] });
  assert.throws(() => nextRelease("0.6.0-rc.2", tags), /already tagged/);
  assert.throws(() => nextRelease("0.6.0-rc.1", tags), /already tagged/);
  assert.throws(() => nextRelease("0.7.0", tags), /0\.6\.0-rc\.2/);
  assert.throws(() => nextRelease("0.7.0-rc.1", tags), /0\.6\.0-rc\.2/);
  assert.throws(() => nextRelease("0.6.1", ["v0.5.0", "v0.6.0"]), /not a release candidate/);
});

test("run in a repository, it writes CHANGELOG.md from the commits since the last tag", () => {
  const dir = mkdtempSync(join(tmpdir(), "release-notes-"));
  try {
    const git = (...args: string[]) =>
      execFileSync(
        "git",
        ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false", ...args],
        {
          cwd: dir,
          encoding: "utf8",
        },
      );
    git("init", "--quiet", "--initial-branch=main");
    writeFileSync(join(dir, "CHANGELOG.md"), `${header}\n${rc1}\n${older}`);
    git("add", ".");
    git("commit", "--quiet", "-m", "chore: release v0.5.0");
    git("tag", "v0.5.0");
    git("commit", "--quiet", "--allow-empty", "-m", "chore: release v0.6.0-rc.1");
    git("tag", "v0.6.0-rc.1");
    git("commit", "--quiet", "--allow-empty", "-m", "feat: a new thing (#1700)");
    git("commit", "--quiet", "--allow-empty", "-m", "docs: about it");
    execFileSync(process.execPath, [join(import.meta.dirname, "release-notes.ts"), "0.6.0-rc.2", "--date", "2026-11-28"], {
      cwd: dir,
    });
    const out = readFileSync(join(dir, "CHANGELOG.md"), "utf8");
    const entry = `## [0.6.0-rc.2](${repo}/compare/v0.6.0-rc.1...v0.6.0-rc.2) - 2026-11-28

### Added

- A new thing ([#1700](${repo}/pull/1700))
`;
    assert.equal(out, `${header}\n${entry}\n${rc1}\n${older}`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
