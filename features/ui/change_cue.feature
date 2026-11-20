@ui
Feature: A row whose status changes stays marked until it is seen
  A live table changes by itself: on the Operations page a run starts, ends
  or fails while the reader looks elsewhere, and a badge that flips unwatched
  is easy to miss. So a row whose status a live update changed is marked
  "changed, not yet seen": its status reads in bold, an edge stands at the
  row's start, and a screen reader hears "Changed, not yet seen" with the
  status. The mark stays until the reader sees it, as focus enters the row
  or they mark every row as seen with the button in the page's row of
  actions, or until they leave the page. It arrives once with a fade, from
  the cue's fill into the still mark over about 600 ms. Nothing marks a row
  whose status stayed as it was: not the page's first rows, not a progress
  report, not a re-sort, not a paused page; a resume marks each changed row
  once. A mark, its fade and its button move nothing, so all stay for a
  reader who asks for less motion (decisions 129 and 130). The journeys
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

  Scenario: Marking every row as seen clears the marks, says how many, and hands focus to the page's heading
    Given the daemon reports running backups of "demo-home" and "demo-docs"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-docs" that shows "Running"
    And the page doesn't offer the button "Mark all as seen"
    And the daemon reports that the backup of "demo-docs" has succeeded with 5 files
    And the daemon reports that the backup of "demo-home" has failed
    Then the row "demo-docs" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    And the row "demo-home" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    And the row "demo-docs" of the table "Operations" says "Changed, not yet seen" in its "Status" column to a screen reader
    And the row "demo-home" of the table "Operations" says "Changed, not yet seen" in its "Status" column to a screen reader
    When I skip to the content
    And I press Tab until focus is on the button "Mark all as seen"
    And I press Tab
    Then focus is on the region "Operations"
    When I press Shift+Tab until focus is on the button "Mark all as seen"
    And I press Enter
    Then the polite region comes to contain "2 changed rows marked as seen."
    And focus is on the heading "Operations"
    And the row "demo-docs" of the table "Operations" is not marked
    And the row "demo-home" of the table "Operations" is not marked
    And the row "demo-docs" of the table "Operations" doesn't say "Changed, not yet seen" in its "Status" column to a screen reader
    And the row "demo-home" of the table "Operations" doesn't say "Changed, not yet seen" in its "Status" column to a screen reader
    And the page doesn't offer the button "Mark all as seen"

  # Mark all as seen hides itself when its last mark goes without its press:
  # a refusal or the page's unreachable state takes the table's place, and
  # the rows take their marks with them. Focus on it goes where a press
  # sends it, the page's heading, and nothing is said for it (2.4.3).
  Scenario: Mark all as seen hands its focus to the page's heading when a refusal takes the table's place
    Given the daemon reports running backups of "demo-home" and "demo-docs"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-docs" that shows "Running"
    And the daemon reports that the backup of "demo-docs" has succeeded with 5 files
    And the row "demo-docs" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    And I skip to the content
    And I press Tab until focus is on the button "Mark all as seen"
    And the daemon can't read its store, and its events stream drops
    Then the page doesn't offer the button "Mark all as seen"
    And focus is on the heading "Operations"
    And the polite region never says "marked as seen"

  Scenario: Mark all as seen hands its focus to the page's heading when the daemon goes away
    Given the daemon reports running backups of "demo-home" and "demo-docs"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-docs" that shows "Running"
    And the daemon reports that the backup of "demo-docs" has succeeded with 5 files
    And the row "demo-docs" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    And I skip to the content
    And I press Tab until focus is on the button "Mark all as seen"
    And the daemon goes away
    Then the page shows the heading "Cannot reach Ladderback"
    And the page doesn't offer the button "Mark all as seen"
    And focus is on the heading "Cannot reach Ladderback"
    And the polite region never says "marked as seen"

  # Mark all as seen stands in the page's row of actions, after the plans'
  # start buttons, in a place kept for it while it is hidden, so it comes
  # and goes without moving the table, however the row wraps (1.4.10).
  Scenario: Mark all as seen comes and goes in the page's row of actions and moves nothing, on a small screen with larger text
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    And the daemon reports running backups of "demo-home" and "demo-docs"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-docs" that shows "Running"
    And I note where the table "Operations" starts
    And the daemon reports that the backup of "demo-docs" has failed
    Then the row "demo-docs" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    And the table "Operations" starts where it did
    When I skip to the content
    And I press Tab until focus is on the button "Mark all as seen"
    And I press Enter
    Then focus is on the heading "Operations"
    And the page doesn't offer the button "Mark all as seen"
    And the table "Operations" starts where it did
    And the page doesn't scroll sideways

  # The row of actions and the toast's slot under it each keep their room,
  # so Mark all as seen and a toast come and go together without moving a
  # row's button under the reader's pointer. On a small screen with larger
  # text the first toast moves the table once, by the lines its words take
  # past the room kept for one (as the Operations page's own scenarios say),
  # so the row's place is noted after it.
  Scenario Outline: Mark all as seen and a toast come and go together and move none of the table's rows, at <width> pixels wide with text at <size> percent
    Given the page's clock can be moved on
    And the daemon reports running backups of "demo-home" and "demo-docs"
    And the daemon queues each backup the page starts
    And a viewport of <width> by <height> pixels
    And the reader sets the text size to <size> percent
    When I open the address "/operations"
    And I skip to the content
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the polite region comes to contain "Backup started."
    And the table "Operations" has 3 rows
    And the page's clock stands still until a step moves it on
    And the page's clock moves on until the page stops waiting for the run's end
    Then the toast reads "Still waiting for Ladderback to say how the run ended." after its icon "Information"
    When I note where the first row's button of the table "Operations" lies
    And the daemon reports that the backup of "demo-docs" has failed
    Then the row "demo-docs" of the table "Operations" is marked as changed, its "Status" in bold and an edge at its start
    And the first row's button of the table "Operations" lies where it was
    When the page's clock moves on 7 seconds
    Then the page shows no toast
    And the first row's button of the table "Operations" lies where it was
    When I press Tab until focus is on the button "Mark all as seen"
    And I press Enter
    Then the page doesn't offer the button "Mark all as seen"
    And the first row's button of the table "Operations" lies where it was
    And the page doesn't scroll sideways

    Examples:
      | width | height | size |
      | 1280  | 800    | 100  |
      | 320   | 256    | 200  |
