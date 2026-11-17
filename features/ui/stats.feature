@ui
Feature: A repository's stats on the Snapshots page
  The repository picked on the Snapshots page ends with its stats: how many
  snapshots it holds, their total size, the size it stores and what
  compression saved, as last read, and a button that reads them now (§16,
  decision 104). The journeys use the keyboard alone, against a daemon this
  run started with a demo plan and a scratch repository, so every read is
  real.

  Scenario: A repository's stats are read when asked, and the reading is shown
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the section heading "Stats of demo"
    And the main content shows the line "No stats have been read yet."
    When I press Tab until focus is on the button "Read the stats now"
    And I press Enter
    Then the polite region comes to contain "Stats read."
    And focus is on the button "Read the stats now"
    And the table "Stats of demo" has a row "Snapshots" that shows "2"
    And the table "Stats of demo" has a row "Compression" that shows "% saved"
    And the page has no accessibility violations

  Scenario: A read of the stats the engine refuses is an error alert, said assertively, and focus stays on the button
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And the repository is moved away
    And I press Tab until focus is on the button "Read the stats now"
    And I press Enter
    Then the page shows an error alert containing "No repository was found at this location"
    And the assertive region comes to contain "No repository was found at this location"
    And focus is on the button "Read the stats now"
    And the alert marks the sender's words as English
    And the page has no accessibility violations

  Scenario: A check reads the stats, and the page shows them once it ends
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Check the repository"
    And I press Enter
    Then the dialog "Check demo" opens with focus inside it
    When I press Tab until focus is on the button "Start check"
    And I press Enter
    Then the polite region comes to contain "Check finished. It found nothing wrong."
    And the table "Stats of demo" has a row "Snapshots" that shows "2"
