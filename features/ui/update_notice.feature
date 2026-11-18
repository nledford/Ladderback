@ui
Feature: The available update in the header
  When the agent's release check has found a newer release
  (`features/updates/release_check.feature`), the header says so on a line
  of its own after the running version, and links to that
  release's notes. The page yet to sign in shows no version, so it shows no
  update either.

  Scenario: The header says which version is available and links to its notes
    Given the daemon has found that version "99.0.0" is available
    When I open the web UI
    Then the header links to the notes of version "99.0.0" after the running version
    And the page has no accessibility violations

  Scenario: The link to the notes is reached by keyboard
    Given the daemon has found that version "99.0.0" is available
    When I open the web UI
    And I press Tab until focus is on the link "Version 99.0.0 is available (goes to another site)"
    Then the header links to the notes of version "99.0.0" after the running version

  # The notes are on GitHub, so the link says it leaves the app, as every
  # outside link does (§4.5), and its words are still the sentence a sighted
  # reader sees.
  Scenario: The link to the notes says it goes to another site and is underlined like every link
    Given the daemon has found that version "99.0.0" is available
    When I open the web UI
    Then the link "Version 99.0.0 is available (goes to another site)" shows the words "Version 99.0.0 is available", then an icon a screen reader skips
    And the link "Version 99.0.0 is available (goes to another site)" is marked external, sends no referrer and opens where the reader is
    And every link in the banner is underlined below its letters, 9 in all

  Scenario: On a narrow screen with large text, the notice wraps and stays whole
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the daemon has found that version "99.0.0" is available
    When I open the web UI
    Then the header links to the notes of version "99.0.0" after the running version
    And the page doesn't scroll sideways
    And the page has no accessibility violations

  Scenario: The notice is an information banner in the header
    Given the daemon has found that version "99.0.0" is available
    When I open the web UI
    Then the page shows an information banner that says "Version 99.0.0 is available"
    And the header links to the notes of version "99.0.0" after the running version
    And the page has no accessibility violations
