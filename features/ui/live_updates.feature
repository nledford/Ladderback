@ui
Feature: Live updates can be paused
  The Operations page follows the daemon live: a run's progress, its start
  and its end change the page by themselves, and the announcer tells a screen
  reader of each end. A reader who needs the page still (WCAG 2.2.2) presses
  "Pause live updates". The page then stops changing, the announcer says
  nothing nobody asked for, and the paused button is described by a banner
  that counts the changes waiting. Pressing it again shows every change at
  once and says once how many runs ended meanwhile, never once for each.
  Focus stays on the button throughout, and the pause lasts as long as the
  page is open: a reload ends it.
  The journeys play the daemon, so each update reaches the page when a step
  sends it, after the page is listening.

  Scenario: A paused page holds still while its banner counts the changes waiting
    Given the daemon reports running backups of "demo-home" and "demo-docs"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-docs" that shows "Running"
    And I press Tab until focus is on the button "Pause live updates"
    And I press Enter
    Then the button "Pause live updates" is pressed
    And focus stays on the button "Pause live updates"
    And the button "Pause live updates" is described by "Live updates are paused. Nothing has changed since."
    And the polite region reads "Live updates are paused."
    When the daemon reports 4 steps of progress of the backup of "demo-home"
    And the daemon reports that the backup of "demo-docs" has succeeded with 5 files
    Then the button "Pause live updates" is described by "Live updates are paused. 2 changes are waiting."
    And the row "demo-docs" of the table "Operations" keeps showing "Running" in its "Status" column
    And the table "Operations" shows no progress bar
    And the polite region keeps reading "Live updates are paused."
    And focus stays on the button "Pause live updates"

  Scenario: Resuming shows every change at once and says once how many runs ended
    Given the daemon reports running backups of "demo-home" and "demo-docs"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-docs" that shows "Running"
    And I press Tab until focus is on the button "Pause live updates"
    And I press Enter
    And the daemon reports that the backup of "demo-home" has succeeded with 3 files
    And the daemon reports that the backup of "demo-docs" has failed
    And the button "Pause live updates" is described by "Live updates are paused. 2 changes are waiting."
    And I press Enter
    Then the button "Pause live updates" is not pressed
    And focus stays on the button "Pause live updates"
    And the button "Pause live updates" has no description
    And the polite region reads "Live updates are on again. 2 runs ended while they were paused, and one of them failed."
    And the row "demo-home" of the table "Operations" shows "Succeeded" in its "Status" column
    And the row "demo-home" of the table "Operations" lists "3 files" in its "Result" column
    And the row "demo-docs" of the table "Operations" shows "Failed" in its "Status" column
    And the table "Operations" has 2 rows
    And the polite region keeps reading "Live updates are on again. 2 runs ended while they were paused, and one of them failed."
    And the assertive region stays empty

  Scenario: Resuming with nothing ended says so
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And I press Tab until focus is on the button "Pause live updates"
    And I press Enter
    And the polite region reads "Live updates are paused."
    And I press Enter
    Then the polite region reads "Live updates are on again. No run ended while they were paused."
    And focus stays on the button "Pause live updates"

  Scenario: A reload ends the pause
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And I press Tab until focus is on the button "Pause live updates"
    And I press Enter
    And the button "Pause live updates" is pressed
    And I reload the page
    Then the button "Pause live updates" is not pressed
    When the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the row "demo-home" of the table "Operations" shows "Succeeded" in its "Status" column
    And the polite region comes to contain "Backup finished."

  Scenario: A paused page still answers the reader's own presses
    Given a demo plan "demo-home" with a big scratch repository
    When I open the address "/operations"
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the table "Operations" has a row "demo-home" that shows "Running"
    And I press Shift+Tab until focus is on the button "Pause live updates"
    And I press Enter
    And I press Tab until focus is on the button "Cancel demo-home"
    And I press Enter
    Then the polite region comes to contain "Backup cancelled."
    And the row "demo-home" of the table "Operations" keeps showing "Running" in its "Status" column
    When I press Shift+Tab until focus is on the button "Sort by started"
    And I press Enter
    Then the polite region comes to contain "Sorted by started, newest first."
    When I press Shift+Tab until focus is on the button "Pause live updates"
    And I press Enter
    Then the polite region reads "Live updates are on again. No run ended while they were paused."
    And the row "demo-home" of the table "Operations" shows "Cancelled" in its "Status" column

  Scenario: Leaving a paused page ends the pause
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/operations"
    And I press Tab until focus is on the button "Pause live updates"
    And I press Enter
    And the button "Pause live updates" is pressed
    And I press Shift+Tab until focus is on the link "Snapshots"
    And I press Enter
    And I press Shift+Tab until focus is on the link "Operations"
    And I press Enter
    Then the button "Pause live updates" is not pressed
    And the page shows the button "Start backup of demo-home"
    When I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    Then the table "Operations" has a row "demo-home" that shows "Succeeded"
    And the polite region comes to contain "Backup finished."

  Scenario Outline: A paused page reflows at <width> pixels with <size> percent text and has no accessibility violations
    Given the daemon reports a running backup of "demo-home"
    And a viewport of <width> by <height> pixels
    And the reader sets the text size to <size> percent
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And I press Tab until focus is on the button "Pause live updates"
    And I press Enter
    And the daemon reports 4 steps of progress of the backup of "demo-home"
    Then the button "Pause live updates" is described by "Live updates are paused. One change is waiting."
    And the banner describing the button "Pause live updates" is edged unlike its focus ring
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | width | height | size |
      | 1280  | 800    | 100  |
      | 320   | 256    | 200  |
