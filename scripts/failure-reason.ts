// Why a red scenario failed, in one line (scripts/red-first.mjs, #68): its first
// failing step and the error's message, read from the runner's own report. A
// scenario that can't load its page and one whose assertion fails are both red,
// and the CI log should show which.
//
// Two report formats: cucumber-js's (`@ui`), and cucumber-rs's (`@e2e` and the
// untagged acceptance scenarios). Neither is a contract, so the reading is
// forgiving: a report it can't read gives `null`, never an exception.

const MAX_LENGTH = 200;

// Colour and cursor sequences: cucumber-rs colours its report on a terminal.
const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]/g;

const indentOf = (line) => line.length - line.trimStart().length;

/** `parts` joined into one line of at most `MAX_LENGTH` characters. */
function oneLine(parts) {
  const text = parts
    .map((part) => part.trim())
    .filter((part) => part !== "")
    .join(" ")
    .replace(/\s+/g, " ");
  const characters = [...text];
  return characters.length <= MAX_LENGTH ? text : `${characters.slice(0, MAX_LENGTH - 1).join("")}…`;
}

/**
 * cucumber-js's report ("progress" format): under `Failed scenarios:`, each
 * scenario's header, the step that failed with its location, the step's table
 * or docstring if it has one, then the error indented below it, ending in the
 * stack. A hook that failed has no step line. A scenario with an undefined step
 * is reported as undefined, not failed, so red-first counts it as not red and
 * never asks for its reason.
 */
function fromCucumberJs(lines) {
  const section = lines.findIndex((line) => /^(Failed scenarios|Failed hooks):$/.test(line));
  if (section < 0) return null;
  const header = lines.findIndex((line, i) => i > section && /^\s+\d+\) .+/.test(line));
  if (header < 0) return null;
  const body = [];
  for (const line of lines.slice(header + 1)) {
    // The next scenario's header (two spaces) or the totals (none) end it.
    if (line.trim() !== "" && indentOf(line) < 5) break;
    body.push(line);
  }
  const first = body.findIndex((line) => line.trim() !== "");
  if (first < 0) return null;
  const step = body[first].match(/^\s+(.+?) # \S+:\d+$/)?.[1];
  const stepIndent = indentOf(body[first]);
  const error = [];
  for (const line of body.slice(step ? first + 1 : first)) {
    // A stack frame ends the message, and so does the next reported step or
    // hook, which is as shallow as this one.
    if (/^\s+at\s/.test(line)) break;
    if (step && line.trim() !== "" && indentOf(line) <= stepIndent) break;
    error.push(line);
  }
  // A step's table or docstring comes before the error, one level shallower.
  const start = error.findIndex((line) => line.trim() !== "" && indentOf(line) >= stepIndent + 4);
  const message = step && start >= 0 ? error.slice(start) : error;
  return oneLine(step ? [`${step}:`, ...message] : message);
}

/**
 * cucumber-rs's report: each step on a line with its mark, and under a failed
 * one (`✘`, or `✘>` in a Background) its table, if it has one, then its
 * details, indented: where it is defined and matched, then the panic's
 * message, or that no function matches the step.
 */
function fromCucumberRs(lines) {
  const at = lines.findIndex((line) => /^\s*✘>?\s/.test(line));
  if (at < 0) return null;
  const step = lines[at].replace(/^\s*✘>?\s+/, "").trim();
  const indent = indentOf(lines[at]);
  const details = [];
  for (const line of lines.slice(at + 1)) {
    // The next step, scenario or the summary is indented no deeper than this one.
    if (line.trim() !== "" && indentOf(line) <= indent) break;
    details.push(line);
  }
  // The message follows `Matched:`, or `Defined:` when no function matches.
  const marker = details.findLastIndex((line) => /^\s*(Defined|Matched):/.test(line));
  const message = details
    .slice(marker + 1)
    .filter((line) => !/^\s*Step failed:/.test(line))
    .map((line) => line.replace(/^\s*Step panicked\. Captured output:\s*/, ""));
  return oneLine([`${step}:`, ...message]);
}

/**
 * The first failing step of a scenario run and its message, on one line of at
 * most 200 characters, from the runner's whole output; `null` when the output
 * has no failing step to read. `runner` is the scenario's runner: `ui`,
 * `e2e` or `acceptance`.
 */
export function failureReason(runner, output) {
  const lines = output.replace(ANSI, "").split("\n").map((line) => line.replace(/\r$/, ""));
  const reason = runner === "ui" ? fromCucumberJs(lines) : fromCucumberRs(lines);
  return reason === "" ? null : reason;
}
