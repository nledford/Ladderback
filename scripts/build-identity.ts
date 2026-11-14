// The build a checkout of Ladderback makes (technical design §16, decision
// 90), as `git` says it now: what a binary built from it names. The release
// smoke test and the UI tests check the binary against it, and
// `just install-restart` the daemon it restarted. `build.rs` decides the same
// from the same answers; this is the independent reading the checks compare
// it with.

import { execFileSync } from "node:child_process";
import { readFileSync, realpathSync } from "node:fs";
import path from "node:path";

/** The fields `GET /api/v1/session` names the build by. */
export interface Build {
  version: string;
  commit: string | null;
  dirty: boolean;
  release: boolean;
}

/** `[workspace.package] version` in the checkout at `root`. */
export function workspaceVersion(root: string): string {
  const manifest = readFileSync(path.join(root, "Cargo.toml"), "utf8");
  const section = manifest.split(/^\[workspace\.package\]$/m)[1]?.split(/^\[/m)[0] ?? "";
  const version = /^version = "([^"]+)"$/m.exec(section)?.[1];
  if (!version) throw new Error("no [workspace.package] version in Cargo.toml");
  return version;
}

/**
 * The build the checkout at `root` makes now: the commit
 * `LADDERBACK_BUILD_COMMIT` names, when it is one (7 to 64 hex digits), as a
 * build between releases without changes; otherwise what `git` says;
 * `undefined` when it can't say (no `git`, or `root` isn't the top of a work
 * tree of its own).
 */
export function checkoutBuild(root: string): Build | undefined {
  const named = process.env["LADDERBACK_BUILD_COMMIT"] ?? "";
  if (/^[0-9a-fA-F]{7,64}$/.test(named)) {
    return { version: workspaceVersion(root), commit: named.toLowerCase(), dirty: false, release: false };
  }
  const git = (...args: string[]): string | undefined => {
    try {
      return execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    } catch {
      return undefined;
    }
  };
  // Only the line break git ends the answer with: a folder's name may end in
  // a space.
  const top = git("rev-parse", "--show-toplevel")?.replace(/\n$/, "");
  if (top === undefined || realpathSync(top) !== realpathSync(root)) return undefined;
  const commit = git("rev-parse", "HEAD")?.trim();
  const tags = git("tag", "--points-at", "HEAD");
  // `--no-optional-locks`, as build.rs asks: a `status` that refreshed the
  // index would have the next build run the build script again.
  const changes = git("--no-optional-locks", "status", "--porcelain", "--untracked-files=no");
  if (commit === undefined || tags === undefined || changes === undefined) return undefined;
  const version = workspaceVersion(root);
  return {
    version,
    commit,
    dirty: changes.trim() !== "",
    release: tags.split("\n").some((tag) => tag.trim() === `v${version}`),
  };
}

/** The build as people read it: `0.3.0`, `0.3.0+3f937cf` or `0.3.0+3f937cf.dirty`. */
export function shown(build: Build): string {
  if (build.commit === null || (build.release && !build.dirty)) return build.version;
  return `${build.version}+${build.commit.slice(0, 7)}${build.dirty ? ".dirty" : ""}`;
}
