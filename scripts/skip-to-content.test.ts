// Tests for the journey lint's rule that a keyboard walk to a page's content
// starts after the header (ui-tests/scripts/skip-to-content.ts, §4.4).
// `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { headerTargets, walksWithoutSkip } from "../ui-tests/scripts/skip-to-content.ts";

const catalog = [
  "-brand-name = Ladderback",
  "ui-skip-link = Skip to content",
  "ui-brand = { -brand-name }",
  "ui-update-available = Version { $version } is available",
  "ui-nav-label = Pages",
  "ui-nav-plans = Plans",
  "ui-nav-settings = Settings",
  "ui-sign-out = Sign out",
].join("\n");
// Built in each test, so a catalog it can't read fails that test alone.
const header = (): RegExp => headerTargets(catalog);

/** The lines `walksWithoutSkip` finds in a feature of `body`'s lines, each
 *  with where its walk starts. Line 1 is the tag, line 2 the feature. */
function found(body: string[], tag = "@ui"): string[] {
  const feature = [tag, "Feature: A page", "", ...body].join("\n");
  return walksWithoutSkip(feature, header()).map(({ line, from }) => `${line} ${from}`);
}

test("a Tab walk to the page's content right after the page loads crosses the header", () => {
  const findings = walksWithoutSkip(
    [
      "@ui",
      "Feature: A page",
      "  Scenario: Saving",
      "    When I open the web UI",
      '    And I press Tab until focus is on the button "Save"',
    ].join("\n"),
    header(),
  );
  assert.deepEqual(findings, [{ line: 5, step: 'I press Tab until focus is on the button "Save"', from: "top" }]);
});

test("a walk after the skip to the content crosses nothing", () => {
  assert.deepEqual(
    found([
      "  Scenario: Saving",
      "    When I open the web UI",
      "    And I skip to the content",
      '    And I press Tab until focus is on the button "Save"',
      '    And I press Shift+Tab until focus is on the textbox "Name"',
    ]),
    [],
  );
});

test("a walk to a stop in the header crosses the header on purpose", () => {
  assert.deepEqual(
    found([
      "  Scenario: The header",
      "    When I open the web UI",
      '    And I press Tab until focus is on the link "Settings"',
      "  Scenario: Signing out",
      "    When I open the web UI",
      '    And I press Tab until focus is on the button "Sign out"',
      "  Scenario: The update",
      "    When I open the web UI",
      '    And I press Tab until focus is on the link "Version 99.0.0 is available"',
      "  Scenario: The product's name",
      "    When I open the web UI",
      '    And I press Tab until focus is on the link "Ladderback"',
      "  Scenario: Back to the header",
      "    When I open the web UI",
      "    And I skip to the content",
      '    And I press Shift+Tab until focus is on the link "Plans"',
    ]),
    [],
  );
});

test("each page load needs its own skip", () => {
  assert.deepEqual(
    found([
      "  Scenario: Two pages",
      "    When I open the web UI",
      "    And I skip to the content",
      '    And I press Tab until focus is on the button "Save"',
      '    When I open the address "/plans"',
      '    And I press Tab until focus is on the button "Run now"',
      "    When I reload the page",
      '    And I press Tab until focus is on the button "Run now"',
    ]),
    ["9 top", "11 top"],
  );
});

test("a load at the Language section leaves focus on its heading, and no other fragment does", () => {
  assert.deepEqual(
    found([
      "  Scenario: The section",
      '    When I open the address "/settings#language"',
      '    And I press Tab until focus is on the button "Save the language"',
      "    When I reload the page",
      '    And I press Tab until focus is on the button "Save the language"',
      '    When I open the address "/plans#demo"',
      '    And I press Tab until focus is on the button "Run now"',
      "  Scenario: Saving the language",
      "    When I open the address \"/settings\"",
      "    And I skip to the content",
      '    And I press Tab until focus is on the button "Save the language"',
      "    When I press Enter, and the page loads again",
      '    And I press Tab until focus is on the button "Save the language"',
      "    When I reload the page",
      '    And I press Tab until focus is on the button "Save the language"',
      '    When I open the address "/settings#language" in a second tab',
      '    And I press Tab until focus is on the button "Save the language"',
    ]),
    ["10 top"],
  );
});

test("a load at a demo of the gallery leaves focus on the demo's heading", () => {
  assert.deepEqual(
    found([
      "  Scenario: A demo",
      '    When I open the "Tabs" demo',
      '    And I press Tab until focus is on the tab "Snapshots"',
      "    When I reload the page",
      '    And I press Tab until focus is on the tab "Snapshots"',
      "    When I open the component gallery",
      '    And I press Tab until focus is on the tab "Snapshots"',
      '    When I open the address "/_gallery#tabs-demo"',
      '    And I press Tab until focus is on the tab "Snapshots"',
      '    When I open the address "/_gallery#tabs"',
      '    And I press Tab until focus is on the tab "Snapshots"',
    ]),
    ["10 top", "14 top"],
  );
});

test("a background's steps come before each scenario's", () => {
  assert.deepEqual(
    found([
      "  Background:",
      "    Given I open the web UI",
      "  Scenario: Without the skip",
      '    When I press Tab until focus is on the button "Save"',
      "  Scenario: With it",
      "    When I skip to the content",
      '    And I press Tab until focus is on the button "Save"',
    ]),
    ["7 top"],
  );
  assert.deepEqual(
    found([
      "  Background:",
      "    Given I open the web UI",
      '    And I press Tab until focus is on the button "Save"',
      "  Scenario: One",
      "    Then the page says so",
      "  Scenario: Two",
      "    Then the page says so",
    ]),
    ["6 top"],
    "a background's walk is found once",
  );
});

test("a rule's background comes after the feature's, for the rule's scenarios only", () => {
  assert.deepEqual(
    found([
      "  Background:",
      "    Given I open the web UI",
      "  Rule: Skipped",
      "    Background:",
      "      Given I skip to the content",
      "    Scenario: In the rule",
      '      When I press Tab until focus is on the button "Save"',
      "  Rule: Not skipped",
      "    Scenario: In the other rule",
      '      When I press Tab until focus is on the button "Save"',
    ]),
    ["13 top"],
  );
});

test("Shift+Tab from the top starts at the page's last stop, in its content", () => {
  assert.deepEqual(
    found([
      "  Scenario: Backwards",
      "    When I open the component gallery",
      '    And I press Shift+Tab until focus is on the button "Sort by size"',
      '    And I press Tab until focus is on the button "Add a file"',
      "  Scenario: One press back",
      "    When I open the component gallery",
      "    And I press Shift+Tab",
      '    And I press Tab until focus is on the button "Add a file"',
    ]),
    [],
  );
});

test("Shift+Tab from the start of the content goes back into the header", () => {
  assert.deepEqual(
    found([
      "  Scenario: After the skip",
      "    When I open the web UI",
      "    And I skip to the content",
      '    And I press Shift+Tab until focus is on the button "Run now"',
      "  Scenario: After a link of the header",
      "    When I open the web UI",
      '    And I press Tab until focus is on the link "Plans"',
      "    And I press Enter",
      '    And I press Shift+Tab until focus is on the button "Run now"',
      "  Scenario: After one press back",
      "    When I open the web UI",
      "    And I skip to the content",
      "    And I press Shift+Tab",
      '    And I press Tab until focus is on the button "Run now"',
      "  Scenario: After one press on",
      "    When I open the web UI",
      "    And I skip to the content",
      "    And I press Tab",
      '    And I press Shift+Tab until focus is on the button "Run now"',
    ]),
    ["7 content's start", "12 content's start", "17 header"],
  );
});

test("a walk on from a stop in the header crosses the rest of it, unless Enter has left the header", () => {
  assert.deepEqual(
    found([
      "  Scenario: On from the header",
      "    When I open the web UI",
      '    And I press Tab until focus is on the link "Plans"',
      '    And I press Tab until focus is on the button "Run now"',
      "  Scenario: To the page the link leads to",
      "    When I open the web UI",
      '    And I press Tab until focus is on the link "Plans"',
      "    And I press Enter",
      '    And I press Tab until focus is on the button "Run now"',
      "  Scenario: Back from Sign out",
      "    When I open the web UI",
      '    And I press Tab until focus is on the button "Sign out"',
      "    And I press Escape",
      '    And I press Shift+Tab until focus is on the button "Run now"',
    ]),
    ["7 header", "17 header"],
  );
});

test("one Tab from the top is on the skip link, which Enter follows to the content", () => {
  assert.deepEqual(
    found([
      "  Scenario: By hand",
      "    When I open the web UI",
      "    And I press Tab",
      "    And I press Enter",
      '    And I press Tab until focus is on the button "Save"',
      "  Scenario: Tab only",
      "    When I open the web UI",
      "    And I press Tab",
      '    And I press Tab until focus is on the button "Save"',
    ]),
    ["12 header"],
  );
});

test("a step that says where focus is moves it there", () => {
  assert.deepEqual(
    found([
      "  Scenario: A dialog",
      '    When I open the address "/repositories"',
      '    Then the dialog "Sign in again" opens with focus inside it',
      '    When I press Tab until focus is on the button "Close"',
      "  Scenario: A field",
      "    When I open the web UI",
      '    Then focus is on the textbox "Admin password"',
      '    When I press Tab until focus is on the button "Sign in"',
      "  Scenario: Sign out refused",
      "    When I open the web UI",
      "    And I skip to the content",
      '    Then focus stays on the button "Sign out"',
      '    When I press Shift+Tab until focus is on the button "Change the password"',
      "  Scenario: The page's heading",
      "    When I open the web UI",
      '    Then focus is on the main heading "Plans"',
      '    When I press Shift+Tab until focus is on the button "Run now"',
    ]),
    ["16 header", "20 content's start"],
  );
});

test("a second tab starts at its top, and the first keeps its focus", () => {
  assert.deepEqual(
    found([
      "  Scenario: Two tabs",
      '    When I open the address "/repositories"',
      "    And I skip to the content",
      '    And I open the address "/repositories" in a second tab',
      '    And I press Tab until focus is on the textbox "Admin password"',
      "    And I go back to the first tab",
      '    And I press Tab until focus is on the textbox "Location"',
      "  Scenario: No second tab",
      '    When I open the address "/repositories"',
      "    And I go back to the first tab",
      '    And I press Tab until focus is on the textbox "Location"',
    ]),
    ["8 top", "14 top"],
  );
});

test("every walk step counts, whatever its target", () => {
  assert.deepEqual(
    found([
      "  Scenario: Walks",
      "    When I open the web UI",
      "    And I press Tab until focus is on a tree item",
      "    When I open the web UI",
      '    And I press "Test" for the repository "demo"',
      "    When I open the web UI",
      '    And I press Tab until focus is on the tree item "docs"',
      "    When I open the web UI",
      "    And I skip to the content",
      '    Then the plan "home" shows 2 snapshot rows',
    ]),
    ["6 top", "8 top", "10 top", "13 content's start"],
  );
});

test("an outline's walk is found once, with its examples filled in", () => {
  assert.deepEqual(
    found([
      "  Scenario Outline: <target>",
      "    When I open the web UI",
      '    And I press Tab until focus is on the link "<target>"',
      "    Examples:",
      "      | target   |",
      "      | Settings |",
      "      | Details  |",
      "      | Help     |",
    ]),
    ["6 top"],
  );
});

test("only a scenario the browser runs is checked, whichever level tags it", () => {
  const walk = ["    When I open the web UI", '    And I press Tab until focus is on the button "Save"'];
  assert.deepEqual(found(["  Scenario: Use case", ...walk], "@e2e"), []);
  assert.deepEqual(found(["  @ui # in the browser", "  Scenario: In the browser", ...walk], ""), ["7 top"]);
  assert.deepEqual(found(["  @e2e # not @ui", "  Scenario: Use case", ...walk], ""), [], "a tag's comment");
  assert.deepEqual(found(["  @ui", "  Rule: In the browser", "    Scenario: One", ...walk], ""), ["8 top"]);
  assert.deepEqual(
    found(["  Scenario Outline: <n>", ...walk, "    @ui", "    Examples:", "      | n |", "      | 1 |", "    Examples:", "      | n |", "      | 2 |"], ""),
    ["6 top"],
  );
  assert.deepEqual(
    found(["  Scenario Outline: <n>", ...walk, "    @e2e", "    Examples:", "      | n |", "      | 1 |"], ""),
    [],
    "an outline whose examples aren't tagged for the browser",
  );
});

test("a doc string's lines aren't steps", () => {
  for (const fence of ['"""', "```"]) {
    assert.deepEqual(
      found([
        "  Scenario: Text",
        "    When I open the web UI",
        "    And the config file says:",
        `      ${fence}`,
        '      And I press Tab until focus is on the button "Save"',
        `      ${fence}`,
      ]),
      [],
      fence,
    );
  }
});

test("the header's stops come from the English catalog, the navigation's own name aside", () => {
  const stops = header();
  assert.match('the link "Skip to content"', stops);
  assert.match('the link "Plans"', stops);
  assert.match('the link "Ladderback"', stops);
  assert.match('the link "Version 1.2.3 is available"', stops);
  assert.match('the button "Sign out"', stops);
  assert.doesNotMatch('the link "Pages"', stops);
  assert.doesNotMatch('the button "Plans"', stops);
  assert.doesNotMatch('the link "Sign out"', stops);
  assert.doesNotMatch('the button "Sign out everywhere"', stops);
  assert.doesNotMatch('the link "Plans and more"', stops);
  assert.match('the link "Logs"', headerTargets(`${catalog}\nui-nav-logs = Logs`), "a new page's link is a header stop");
});

test("a header message the catalog lacks is an error", () => {
  assert.throws(() => headerTargets(catalog.replace("ui-sign-out = Sign out", "")), /no message ui-sign-out/);
  assert.throws(() => headerTargets(catalog.replace("-brand-name = Ladderback", "")), /no message -brand-name/);
});
