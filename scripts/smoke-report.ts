// What the smoke tests share (technical design §12.1): the report of their
// checks as Markdown, and running a program to its end with a time limit.
// scripts/smoke-release.ts tests a release binary and its packages, and
// scripts/smoke-image.ts the Docker image made from it.

import { spawn } from "node:child_process";

/** How long a command may run when its caller names no limit. */
export const COMMAND_LIMIT_MS = 60_000;

type Result = { check: string; ok: boolean; detail: string };

export class Report {
  results: Result[] = [];
  notes: string[] = [];

  add(check: string, ok: boolean, detail: string): boolean {
    this.results.push({ check, ok, detail });
    return ok;
  }

  note(markdown: string): void {
    this.notes.push(markdown);
  }

  get failed(): boolean {
    return this.results.some((result) => !result.ok);
  }

  markdown(title: string): string {
    const cell = (text: string) => text.replace(/\|/g, "\\|").replace(/\r?\n/g, " ");
    const rows = this.results.map(
      (result) => `| ${result.ok ? "pass" : "**FAIL**"} | ${cell(result.check)} | ${cell(result.detail)} |`,
    );
    return [`### ${title}`, "", "| Result | Check | Detail |", "|---|---|---|", ...rows, "", ...this.notes, ""].join("\n");
  }
}

/** A collapsed block of Markdown holding `body` as it is. */
export const details = (summary: string, body: string) =>
  ["<details>", `<summary>${summary}</summary>`, "", "```", body.trimEnd(), "```", "", "</details>", ""].join("\n");

export type Output = { status: number | null; stdout: Buffer; stderr: string };

/** Runs `command` to the end, or kills it after `limitMs`; never throws. */
export function run(
  command: string,
  args: string[],
  options: { input?: Buffer | string; env?: NodeJS.ProcessEnv; limitMs?: number } = {},
): Promise<Output> {
  const { promise, resolve } = Promise.withResolvers<Output>();
  const child = spawn(command, args, { env: options.env ?? process.env, stdio: ["pipe", "pipe", "pipe"] });
  const stdout: Buffer[] = [];
  let stderr = "";
  const timer = setTimeout(() => child.kill("SIGKILL"), options.limitMs ?? COMMAND_LIMIT_MS);
  child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
  child.stderr.on("data", (chunk: Buffer) => (stderr += chunk.toString()));
  child.on("error", (error) => {
    clearTimeout(timer);
    resolve({ status: null, stdout: Buffer.alloc(0), stderr: `${stderr}${error.message}` });
  });
  child.on("close", (status) => {
    clearTimeout(timer);
    resolve({ status, stdout: Buffer.concat(stdout), stderr });
  });
  child.stdin.on("error", () => {});
  child.stdin.end(options.input ?? "");
  return promise;
}
