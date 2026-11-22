@ui
Feature: The running version in the footer
  The footer at the foot of every page shows the version of Ladderback that is
  running, which a bug report names and which confirms a new build (decision
  142). It is reference, not something to act on, so it stands below the
  page's content, away from the header's pages and actions. The footer is the
  page's contentinfo landmark, after the main content, and plain text, never a
  stop for Tab. The sign-in page shows only what signing in needs, so it has no
  footer at all (decision 107).

  Scenario: With sign-in off, the footer shows the running version after the main content, and the header doesn't
    When I open the web UI
    Then the footer shows the running version after the main content
    And the banner doesn't show the running version
    And the page has no accessibility violations

  # The version's width depends on the build's commit, whose digits differ in
  # width, so the scenario measures the widest version a build between
  # releases can show, never this build's own commit. A window 1280 px wide
  # zoomed to 400 % is 320 px wide at the page's own text size.
  Scenario Outline: On a narrow screen at <size> percent text, the widest version a build can show wraps inside the footer
    Given a viewport of 320 by <height> pixels
    And the reader sets the text size to <size> percent
    When I open the web UI
    And the running version is the widest one a build between releases can show
    And I reload the page
    Then the footer shows that version, every line of it inside the footer
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | height | size |
      | 256    | 200  |
      | 200    | 100  |

  Scenario: The sign-in page has no footer, and signing in shows the footer with the running version
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    Then the main heading reads "Sign in to Ladderback"
    And the page has no footer
    And the page doesn't show the running version
    When I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    And the footer shows the running version after the main content
    And the banner doesn't show the running version
    And the page has no accessibility violations
