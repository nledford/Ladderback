@ui
Feature: The running version in the header
  The header shows the version of Ladderback that is running, at its right,
  wherever it shows the navigation. The sign-in page shows only what signing
  in needs, so it leaves the version out.

  Scenario: With sign-in off, the header shows the running version
    When I open the web UI
    Then the header shows the running version after the navigation
    And the page has no accessibility violations

  Scenario: On a narrow screen with large text, the version wraps under the navigation
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the web UI
    Then the header shows the running version after the navigation
    And the page has no accessibility violations

  Scenario: The sign-in page hides the running version, and signing in shows it
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    Then the main heading reads "Sign in to Ladderback"
    And the page doesn't show the running version
    When I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    And the header shows the running version after the navigation
    And the page has no accessibility violations
