// The journey lint's rule for keyboard walks (§4.4): a walk to a target in a
// page's content starts after the header, with "I skip to the content"
// (`ui-tests/steps/accessibility.ts`). A page load leaves focus at the top of
// the page, before the skip link, so a Tab walk that follows it without the
// skip crosses the header, and `pressUntilFocused`
// (`ui-tests/support/keyboard.ts`) fails it at run time. This finds such a
// walk without running it, from the steps' words alone.

/** A walk that crosses the header to reach the page's content: from the top
 *  of the page, where "I skip to the content" belongs before it; from a stop
 *  in the header a step before it reached; or back with Shift+Tab from the
 *  start of `<main>`, where the skip or a link of the header left focus. */
export interface Finding {
  line: number;
  step: string;
  from: "top" | "header" | "content's start";
}

// A step's own words, after its keyword.
const STEP = /^\s*(?:Given|When|Then|And|But|\*)\s+(.*?)\s*$/;

// Steps that load a page: every step that opens one is worded "I open …".
// A second tab opens a page while the first keeps its own focus until the
// steps go back to it.
const OPEN = /^I open /;
const OPEN_ADDRESS = /^I open the address "([^"]*)"(?: in a second tab)?$/;
const SECOND_TAB = /^I open the address "[^"]*" in a second tab$/;
const FIRST_TAB = /^I go back to the first tab$/;
const RELOAD = /^I reload the page$/;
// Saving the language loads Settings again at its Language section.
const RELOAD_AT_LANGUAGE = /^I press Enter, and the page loads again$/;

// The one address that names a section whose heading takes focus as the
// page loads (§4.5, route changes: `page::focus_when_named`). Any other
// page load leaves focus at the top.
const AT_SECTION = /#language$/;

const SKIP = /^I skip to the content$/;

// The steps that walk, pressing a key until focus is on a target
// (`pressUntilFocused`): the key, then the target as the step names it. Two
// steps walk under other words: Tab to a repository's button, and, to read a
// plan's history again, Shift+Tab back to the repository's button.
const WALK = /^I press (.+?) until focus is on (.+)$/;
const WALK_TO_REPOSITORY_BUTTON = /^I press "[^"]*" for the repository "[^"]*"$/;
const WALK_BACK_TO_REPOSITORY_BUTTON = /^the plan "[^"]*" shows \d+ snapshot rows$/;

// One press of a key (`I press {word}`).
const PRESS = /^I press (\S+)$/;

// The steps that say where focus is: on a target as a walk names it, or in a
// dialog. The page's main heading starts `<main>`.
const FOCUS_IS = /^focus (?:is|stays|is kept) (?:on|in|inside) (.+)$|^the dialog "[^"]*" (?:opens|stays open) with focus inside it$/;
const MAIN_HEADING = /^the main heading\b/;

// The header's own stops, by their messages in the English catalog: the
// skip link, the product's name, the pages' links (every `ui-nav-` message
// but the navigation's own name), the available update's notes and Sign out.
// The notes are on another site, so their link's name ends with the words
// every outside link adds (`ui-link-outside`, §4.5). A walk to one of them
// crosses the header on purpose (§4.4), as the run-time guard allows.
const HEADER_LINKS = ["ui-skip-link", "ui-brand"];
const HEADER_OUTSIDE_LINKS = ["ui-update-available"];
const OUTSIDE_NOTE = "ui-link-outside";
const HEADER_NAV_LINK = /^ui-nav-(?!label$)/;
const HEADER_BUTTONS = ["ui-sign-out"];

/** How a walk step names a stop in the header, from the English catalog's
 *  text (`crates/l10n/locales/en-US/*.ftl`, the files joined): `the link
 *  "Settings"`, `the button "Sign out"`. A message's variable matches any
 *  words, and a string literal (`{ " " }`) its own text. A header message the
 *  catalog lacks is an error, so a rename can't leave the lint looking for a
 *  name the page no longer shows. */
export function headerTargets(catalog: string): RegExp {
  const messages = new Map<string, string>();
  for (const [, id = "", value = ""] of catalog.matchAll(/^(-?[a-z][a-z0-9-]*) = (.+)$/gm)) messages.set(id, value);
  const escaped = (text: string): string => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = (id: string): string => {
    const value = messages.get(id);
    if (value === undefined) throw new Error(`the English catalog has no message ${id}, a stop in the header`);
    return value
      .split(/(\{ *(?:[-$][a-z0-9-]+|"[^"]*") *\})/)
      .map((part) => {
        const literal = /^\{ *"([^"]*)" *\}$/.exec(part);
        if (literal !== null) return escaped(literal[1] ?? "");
        const placeable = /^\{ *([-$])([a-z0-9-]+) *\}$/.exec(part);
        if (placeable === null) return escaped(part);
        return placeable[1] === "$" ? '[^"]+' : pattern(`-${placeable[2]}`);
      })
      .join("");
  };
  const links = [
    ...[...HEADER_LINKS, ...[...messages.keys()].filter((id) => HEADER_NAV_LINK.test(id))].map(pattern),
    ...HEADER_OUTSIDE_LINKS.map((id) => pattern(id) + pattern(OUTSIDE_NOTE)),
  ];
  return new RegExp(`^the (?:link "(?:${links.join("|")})"|button "(?:${HEADER_BUTTONS.map(pattern).join("|")})")$`);
}

// Where focus is, as far as the steps' words tell: at the top of the page,
// before the skip link, as a page load leaves it; on a stop in the header; at
// the start of `<main>`, on it or on its heading, right after the header; or
// anywhere else, which a walk needs no skip from.
type Focus = "top" | "header" | "content's start" | "elsewhere";

/** A scenario as it runs: its tags, inherited ones included, and its steps,
 *  the backgrounds' first. An outline gives one scenario for each example. */
interface Scenario {
  tags: string[];
  steps: { line: number; text: string }[];
}

/** The scenarios of a feature file, in the subset of Gherkin the feature
 *  files use: tags, rules, backgrounds, outlines with examples, doc strings
 *  and data tables. */
function scenarios(feature: string): Scenario[] {
  const found: Scenario[] = [];
  let tags: string[] = [];
  let featureTags: string[] = [];
  let ruleTags: string[] = [];
  let featureBackground: Scenario["steps"] = [];
  let ruleBackground: Scenario["steps"] = [];
  let inRule = false;
  // Where a step line goes, and an outline's examples so far.
  let steps: Scenario["steps"] | undefined;
  let current: (Scenario & { outline: boolean }) | undefined;
  let examples: { tags: string[]; header?: string[] } | undefined;
  let docString: string | undefined;

  const finish = () => {
    if (current !== undefined && !current.outline) found.push(current);
    current = undefined;
    examples = undefined;
  };

  feature.split("\n").forEach((raw, index) => {
    const line = index + 1;
    const text = raw.trim();
    if (docString !== undefined) {
      if (text.startsWith(docString)) docString = undefined;
      return;
    }
    if (text.startsWith('"""') || text.startsWith("```")) {
      docString = text.slice(0, 3);
      return;
    }
    if (text === "" || text.startsWith("#")) return;
    if (text.startsWith("@")) {
      tags.push(...(text.split(/\s+#/)[0] ?? "").split(/\s+/).filter((tag) => tag.startsWith("@")));
      return;
    }
    const keyword = /^(Feature|Rule|Background|Scenario Outline|Scenario Template|Scenario|Example|Examples|Scenarios):/.exec(text);
    if (keyword !== null) {
      const kind = keyword[1];
      if (kind === "Examples" || kind === "Scenarios") {
        examples = { tags };
        steps = undefined;
      } else {
        finish();
        if (kind === "Feature") {
          featureTags = tags;
          steps = undefined;
        } else if (kind === "Rule") {
          inRule = true;
          ruleTags = tags;
          ruleBackground = [];
          steps = undefined;
        } else if (kind === "Background") {
          steps = inRule ? ruleBackground : featureBackground;
        } else {
          current = {
            tags: [...featureTags, ...(inRule ? ruleTags : []), ...tags],
            steps: [...featureBackground, ...(inRule ? ruleBackground : [])],
            outline: kind !== "Scenario" && kind !== "Example",
          };
          steps = current.steps;
        }
      }
      tags = [];
      return;
    }
    if (text.startsWith("|")) {
      if (examples === undefined || current === undefined) return;
      const cells = text.slice(1, -1).split("|").map((cell) => cell.trim());
      if (examples.header === undefined) {
        examples.header = cells;
        return;
      }
      const header = examples.header;
      found.push({
        tags: [...current.tags, ...examples.tags],
        steps: current.steps.map(({ line, text }) => ({
          line,
          text: text.replace(/<([^<>]+)>/g, (whole, column: string) => cells[header.indexOf(column)] ?? whole),
        })),
      });
      return;
    }
    const step = STEP.exec(raw);
    if (step !== null && steps !== undefined) steps.push({ line, text: step[1] ?? "" });
  });
  finish();
  return found;
}

/** Where one press of `key` leaves focus that was at `focus`. Enter on the
 *  skip link moves focus to `<main>`; on a header link or Sign out, to the
 *  heading of the page it leads to. */
function afterPress(focus: Focus, key: string): Focus {
  switch (focus) {
    case "top":
      return key === "Tab" ? "header" : key === "Shift+Tab" ? "elsewhere" : focus;
    case "header":
      return key === "Enter" ? "content's start" : focus;
    case "content's start":
      return key === "Tab" ? "elsewhere" : key === "Shift+Tab" ? "header" : focus;
    case "elsewhere":
      return focus;
  }
}

/** Whether a walk with `key` from `focus` to the page's content crosses the
 *  header, and from where. Shift+Tab from the top starts at the page's last
 *  stop, so it reaches `<main>` first; an arrow key moves focus only within
 *  the control that has it. */
function crossing(focus: Focus, key: string): Finding["from"] | undefined {
  if (focus === "top" && key === "Tab") return "top";
  if (focus === "header" && (key === "Tab" || key === "Shift+Tab")) return "header";
  if (focus === "content's start" && key === "Shift+Tab") return "content's start";
  return undefined;
}

/** The walks in `feature`'s `@ui` scenarios that cross the header to reach a
 *  target in the page's content; `header` matches a walk's target that is a
 *  stop in the header (`headerTargets`). A step that several scenarios share,
 *  in a background or an outline, is found once. */
export function walksWithoutSkip(feature: string, header: RegExp): Finding[] {
  const findings = new Map<number, Finding>();
  for (const scenario of scenarios(feature)) {
    if (!scenario.tags.includes("@ui")) continue;
    let focus: Focus = "elsewhere";
    // Whether the page's address names the Language section, so that a
    // reload leaves focus on its heading.
    let atSection = false;
    let firstTab: { focus: Focus; atSection: boolean } | undefined;
    for (const { line, text } of scenario.steps) {
      const opened = OPEN.test(text) || RELOAD.test(text) || RELOAD_AT_LANGUAGE.test(text);
      if (SECOND_TAB.test(text)) firstTab = { focus, atSection };
      if (FIRST_TAB.test(text)) {
        ({ focus, atSection } = firstTab ?? { focus, atSection });
      } else if (opened) {
        if (OPEN.test(text)) atSection = AT_SECTION.test(OPEN_ADDRESS.exec(text)?.[1] ?? "");
        if (RELOAD_AT_LANGUAGE.test(text)) atSection = true;
        focus = atSection ? "elsewhere" : "top";
      } else if (SKIP.test(text)) {
        focus = "content's start";
      } else if (PRESS.test(text)) {
        focus = afterPress(focus, PRESS.exec(text)?.[1] ?? "");
      } else if (FOCUS_IS.test(text)) {
        const target = FOCUS_IS.exec(text)?.[1] ?? "";
        focus = header.test(target) ? "header" : MAIN_HEADING.test(target) ? "content's start" : "elsewhere";
      } else {
        const walk = WALK.exec(text);
        const [key, target = ""] =
          walk !== null
            ? [walk[1], walk[2]]
            : WALK_TO_REPOSITORY_BUTTON.test(text)
              ? ["Tab"]
              : WALK_BACK_TO_REPOSITORY_BUTTON.test(text)
                ? ["Shift+Tab"]
                : [];
        if (key === undefined) continue;
        if (header.test(target)) {
          focus = "header";
        } else {
          const from = crossing(focus, key);
          if (from !== undefined && !findings.has(line)) findings.set(line, { line, step: text, from });
          focus = "elsewhere";
        }
      }
    }
  }
  return [...findings.values()].sort((a, b) => a.line - b.line);
}
