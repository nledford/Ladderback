@ui
Feature: A row whose status changes is lit for a moment
  A live table changes by itself: on the Operations page a run starts, ends
  or fails while the reader looks elsewhere, and a badge that flips unwatched
  is easy to miss. So a row whose status a live update changed is lit once in
  Ledger Gray, which settles back to the row's own fill over about 600 ms.
  It never loops and never comes back for a row whose status stayed as it
  was: not as the page first shows its rows, not for a progress report, not
  while live updates are paused, and only once for each changed row as they
  resume. It is a change of color, so it stays for a reader who asks for
  less motion (decision 127); a screen reader hears the announcer instead.
  The journeys play the daemon, so each update reaches the page when a step
  sends it, after the page is listening.

  Scenario: A row whose status changes is lit once, and only that row, which a re-sort leaves alone
    Given I watch the rows of the page for their change cue
    And the daemon reports running backups of "demo-home" and "demo-docs"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-docs" that shows "Running"
    Then the row "demo-docs" of the table "Operations" is not lit
    And the row "demo-home" of the table "Operations" is not lit
    When the daemon reports 4 steps of progress of the backup of "demo-home"
    And the daemon reports that the backup of "demo-docs" has succeeded with 5 files
    Then the row "demo-docs" of the table "Operations" shows "Succeeded" in its "Status" column
    And the row "demo-docs" of the table "Operations" is lit once in Ledger Gray, settling back over 600 ms
    And the row "demo-home" of the table "Operations" is not lit
    When I press Shift+Tab until focus is on the button "Sort by started"
    And I press Enter
    Then the polite region comes to contain "Sorted by started"
    And the row "demo-docs" of the table "Operations" is lit once in Ledger Gray, settling back over 600 ms
    And the row "demo-home" of the table "Operations" is not lit

  Scenario: A paused page lights nothing, and its resume lights a changed row once
    Given I watch the rows of the page for their change cue
    And the daemon reports a queued backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Queued"
    And I skip to the content
    And I press Tab until focus is on the button "Pause live updates"
    And I press Enter
    And the daemon reports that the backup of "demo-home" has started
    And the daemon reports that the backup of "demo-home" has succeeded with 3 files
    And the button "Pause live updates" is described by "Live updates are paused. One change is waiting."
    Then the row "demo-home" of the table "Operations" is not lit
    When I press Enter
    Then the row "demo-home" of the table "Operations" shows "Succeeded" in its "Status" column
    And the row "demo-home" of the table "Operations" is lit once in Ledger Gray, settling back over 600 ms

  Scenario: A row is lit for a reader who asks for less motion, while nothing on the page moves
    Given I watch the rows of the page for their change cue
    And the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the reader asks for less motion
    And the daemon reports that the backup of "demo-home" is backing up, 2 GiB of 20 GiB done
    Then the progress bar "Backup progress of demo-home" is drawn at 10 percent
    And nothing on the page moves, turns or changes size
    When the daemon reports that the backup of "demo-home" has failed
    Then the row "demo-home" of the table "Operations" shows "Failed" in its "Status" column
    And the row "demo-home" of the table "Operations" is lit once in Ledger Gray, settling back over 600 ms
