@e2e
Feature: An AI agent dry-runs a draft plan through MCP
  Before it proposes a plan, an AI agent connected to a running Ladderback
  through MCP asks for a plan dry run: the daemon walks the draft plan's
  sources through its source filter and answers how many files and bytes it
  would back up, and what each exclude removes on its own (decision 89,
  point 13, §4.6). It answers counts and sizes only, never a path, and reads
  only within the dry-run allowlist, which a person sets on the Settings
  page and no API token can change. A source outside the allowlist and one
  that doesn't exist are refused alike, so a refusal never tells whether a
  path exists. The daemon logs at debug here, so the check that no path is
  shown covers every record the walk makes.

  Background:
    Given Ladderback is serving on loopback with sign-in on, the admin password "demo-admin-pass" and its log at debug
    And the daemon's config file holds only the repository "demo"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And a demo folder holding the files "canary-alpha-notes.txt" of 100 bytes and "canary-beta-trace.log" of 40 bytes
    And the admin has set the dry-run allowlist to the demo folder over HTTP
    And the admin has created an API token over HTTP with the scopes "propose" and the label "assistant"
    And an MCP server connected to it with that API token in a file

  Scenario: A dry run counts what a plan would back up and never shows a path
    When the MCP client dry-runs the plan "nightly" over the demo folder, excluding "!*.log"
    Then the result counts 1 file of 100 bytes
    And the result says the exclude "globs" 1 removes 1 file of 40 bytes
    And nothing the MCP server or Ladderback wrote or logged shows "canary-alpha" or "canary-beta"

  Scenario: A source outside the allowlist and one that doesn't exist are refused alike
    When the MCP client dry-runs the plan "nightly" over a folder outside the allowlist
    Then the result is the error "dry-run-not-allowed"
    And the error names the setting "sources" and the entry 1
    When the MCP client dry-runs the plan "nightly" over a folder that doesn't exist
    Then the result is the error "dry-run-not-allowed"
    And the error names the setting "sources" and the entry 1

  Scenario Outline: A draft whose filter would read the folders above its sources is refused
    When the MCP client dry-runs the plan "nightly" over the demo folder with "<setting>" set
    Then the result is the error "dry-run-reads-above"
    And the error names the setting "<setting>" alone

    Examples:
      | setting            |
      | custom-ignorefiles |
      | git-ignore         |
      | no-require-git     |

  Scenario: An API token can't change the dry-run allowlist, even with the propose scope
    When the API token sets the dry-run allowlist to "/" over HTTP
    Then the daemon refuses it with the status 403
    And the admin's session reads the dry-run allowlist as only the demo folder
