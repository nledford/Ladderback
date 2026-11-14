// The build a checkout of Ladderback makes (technical design §16, decision
// NN), as `git` says it now: what a binary built from it names. The release
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
 * The build the checkout at `root` makes now; `undefined` when `git` can't
 * say (no `git`, or `root` isn't the top of a work tree of its own) or
 * `LADDERBACK_BUILD_COMMIT` names the commit instead.
 */
export function checkoutBuild(root: string): Build | undefined {
  if (process.env["LADDERBACK_BUILD_COMMIT"]) return undefined;
  const git = (...args: string[]): string | undefined => {
    try {
      return execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    } catch {
      return undefined;
    }
  };
  const top = git("rev-parse", "--show-toplevel")?.trim();
  if (top === undefined || realpathSync(top) !== realpathSync(root)) return undefined;
  const commit = git("rev-parse", "HEAD")?.trim();
  const tags = git("tag", "--points-at", "HEAD");
  const changes = git("status", "--porcelain", "--untracked-files=no");
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
