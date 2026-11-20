@ui
Feature: A row whose status changes stays marked until it is seen
  A live table changes by itself: on the Operations page a run starts, ends
  or fails while the reader looks elsewhere, and a badge that flips unwatched
  is easy to miss. So a row whose status a live update changed is marked
  "changed, not yet seen": its status reads in bold, and an edge stands at
  the row's start. The mark stays until the reader sees it, as focus enters
  the row or they mark every row as seen, or until they leave the page. It
  arrives once with a fade, from the cue's fill into the still mark over
  about 600 ms. Nothing marks a row whose status stayed as it was: not the
  page's first rows, not a progress report, not a re-sort, not a paused
  page; a resume marks each changed row once. A mark and its fade move
  nothing, so both stay for a reader who asks for less motion (decisions
  127 and 129); a screen reader hears the announcer instead. The journeys
  play the daemon, so each update reaches the page when a step sends it,
  after the page is listening.

  Scenario: A row whose status changes is marked, and only that row, which a re-sort leaves as it was
    Given I watch the rows of the page for their change cue
    And the daemon reports running backups of "demo-home" and "demo-docs"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-docs" that shows "Running"
    Then the row "demo-docs" of the table "Operations" is not lit
    And the row "demo-docs" of the table "Operations" is not marked
    And the row "demo-home" of the table "Operations" is not marked
    When the daemon reports 4 steps of progress of the backup of "demo-home"
    And the daemon reports that the backup of "demo-docs" has succeeded with 5 files
    Then the row "demo-docs" of the table "Operations" shows "Succeeded" in its "Status" column
    And the row "demo-docs" of the table "Operations" is lit once in the cue's fill, settling into its mark over 600 ms
    And the row "demo-docs" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    And the row "demo-home" of the table "Operations" is not lit
    And the row "demo-home" of the table "Operations" is not marked
    When I press Shift+Tab until focus is on the button "Sort by started"
    And I press Enter
    Then the polite region comes to contain "Sorted by started"
    And the row "demo-docs" of the table "Operations" is lit once in the cue's fill, settling into its mark over 600 ms
    And the row "demo-docs" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    And the row "demo-home" of the table "Operations" is not marked

  Scenario: A paused page marks nothing, and its resume marks a changed row once
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
    And the row "demo-home" of the table "Operations" is not marked
    When I press Enter
    Then the row "demo-home" of the table "Operations" shows "Succeeded" in its "Status" column
    And the row "demo-home" of the table "Operations" is lit once in the cue's fill, settling into its mark over 600 ms
    And the row "demo-home" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start

  Scenario: A row is marked for a reader who asks for less motion, while nothing on the page moves
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
    And the row "demo-home" of the table "Operations" is lit once in the cue's fill, settling into its mark over 600 ms
    And the row "demo-home" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    And nothing on the page moves, turns or changes size

  Scenario: Marking every row as seen clears the marks, says how many, and hands focus to the table
    Given the daemon reports running backups of "demo-home" and "demo-docs"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-docs" that shows "Running"
    And the page doesn't offer the button "Mark all as seen"
    And the daemon reports that the backup of "demo-docs" has succeeded with 5 files
    And the daemon reports that the backup of "demo-home" has failed
    Then the row "demo-docs" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    And the row "demo-home" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    When I skip to the content
    And I press Tab until focus is on the button "Mark all as seen"
    And I press Enter
    Then the polite region comes to contain "2 changed rows marked as seen."
    And focus is on the region "Operations"
    And the row "demo-docs" of the table "Operations" is not marked
    And the row "demo-home" of the table "Operations" is not marked
    And the page doesn't offer the button "Mark all as seen"
