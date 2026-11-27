// The release notes `just release-pr` writes after a release candidate, when
// release-plz opens no release PR (technical design §12.1, #1669).

export type Commit = { message: string };

export function commitLine(_commit: Commit, _repo: string): { group: string; line: string } | undefined {
  throw new Error("todo");
}

export function nextRelease(_version: string, _tags: string[]): { from: string; since: string; candidates: string[] } {
  throw new Error("todo");
}

export function releaseNotes(_input: { changelog: string; version: string; tags: string[]; commits: Commit[]; date: string }): string {
  throw new Error("todo");
}
