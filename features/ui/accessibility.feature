@ui
Feature: Accessibility of the app shell
  Moving between pages and the page's own changes work for a screen reader and
  for the keyboard alone (A1, A3). When a reader navigates to another page,
  focus moves to that page's main heading and the title changes, and the
  announcer, the part of the page that tells a screen reader about changes,
  stays silent: the focused heading is read anyway. The announcer's own
  behavior is tried in the component gallery, a development page at
  `/_gallery`.

  Scenario: Navigating to another page moves focus to its heading and changes the title
    When I open the component gallery
    Then the page is titled "Component gallery – Ladderback"
    And the page has no accessibility violations
    When I press Tab until focus is on the link "Back to the start page"
    And I press Enter
    Then focus is on the main heading "Welcome to Ladderback"
    And the page is titled "Ladderback"
    And the live regions stay silent

  Scenario: A page that does not exist has a heading of its own and takes focus
    When I open the component gallery
    And I press Tab until focus is on the link "Open a page that does not exist"
    And I press Enter
    Then focus is on the main heading "Page not found"
    And the page is titled "Page not found – Ladderback"
    And the page has no accessibility violations
    And the live regions stay silent

  Scenario Outline: A page that does not exist is in the language of the page
    When I open the address "/_gallery?locale=<locale>"
    And I press Tab
    And I press Tab
    And I press Tab
    And I press Tab
    And I press Tab
    And I press Tab
    And I press Enter
    Then focus is on the main heading
    And every visible text is transformed
    And the page title is transformed

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  Scenario: Opening a page directly sets its title and leaves focus to the skip link
    When I open the address "/no/such/page"
    Then the page is titled "Page not found – Ladderback"
    When I press Tab
    Then focus is on the link "Skip to content"

  Scenario: A polite message reaches the polite region
    When I open the component gallery
    And I press Tab until focus is on the button "Say politely: backup finished"
    And I press Enter
    Then the polite region reads "Backup finished."
    And the assertive region is empty
    And the page has no accessibility violations

  Scenario: Polite messages sent within two seconds arrive merged
    When I open the component gallery
    And I press Tab until focus is on the button "Say politely: backup finished"
    And I press Enter
    And I press Tab until focus is on the button "Say politely: agent back online"
    And I press Enter
    Then the polite region reads "Backup finished. Agent back online."

  Scenario: An assertive message reaches the assertive region
    When I open the component gallery
    And I press Tab until focus is on the button "Say urgently: restore failed"
    And I press Enter
    Then the assertive region reads "Restore failed."
    And the polite region is empty

  Scenario: A polite message sent while a modal is open is spoken after it closes
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Enter
    Then focus is inside the dialog "Modal stand-in"
    When I press Tab until focus is on the button "Say politely in the modal: backup finished"
    And I press Enter
    Then the polite region stays empty
    When I press Tab until focus is on the button "Close the modal stand-in"
    And I press Enter
    Then the polite region reads "Backup finished."

  Scenario: An urgent message sent while a modal is open is spoken after it closes
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Enter
    Then focus is inside the dialog "Modal stand-in"
    When I press Tab until focus is on the button "Say urgently in the modal: restore failed"
    And I press Enter
    Then the assertive region stays empty
    When I press Tab until focus is on the button "Close the modal stand-in"
    And I press Enter
    Then the assertive region reads "Restore failed."

  Scenario: The gallery has no accessibility violations while a modal is open
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Enter
    Then focus is inside the dialog "Modal stand-in"
    And the page has no accessibility violations

  # The journeys over the daemon's live data (§4.4): the operations and
  # snapshots pages, driven by the keyboard alone against a daemon this run
  # started with a demo plan and a scratch repository, so every backup they
  # start is real.

  Scenario: Starting a backup from the operations page by keyboard alone
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/operations"
    Then the page is titled "Operations – Ladderback"
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    Then the table "Operations" has a row "demo-home" that shows "Running"
    And the polite region comes to contain "Backup started."
    And the table "Operations" has a row "demo-home" that shows "Succeeded"
    And the polite region comes to contain "Backup finished."
    And the page has no accessibility violations

  # A start button is named with the plan it starts (2.4.6, 4.1.2), so a reader
  # who tabs through the plans hears which one each button runs, and each
  # button starts that plan and no other.
  Scenario: Each plan's start button names the plan it starts
    Given two demo plans "demo-home" and "demo-work" with a scratch repository
    When I open the address "/operations"
    Then the page has no accessibility violations
    When I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    Then the table "Operations" has a row "demo-home" that shows "Succeeded"
    And the table "Operations" has 1 rows
    When I press Tab until focus is on the button "Start backup of demo-work"
    And I press Enter
    Then the table "Operations" has a row "demo-work" that shows "Succeeded"
    And the table "Operations" has 2 rows
    And the page has no accessibility violations

  Scenario: Cancelling a running backup by keyboard
    Given a demo plan "demo-home" with a big scratch repository
    When I open the address "/operations"
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the table "Operations" has a row "demo-home" that shows "Running"
    And I press Tab until focus is on the button "Cancel demo-home"
    And I press Enter
    Then the table "Operations" has a row "demo-home" that shows "Cancelled"
    And the polite region comes to contain "Backup cancelled."
    And the page has no accessibility violations

  Scenario: The operations table sorts by keyboard with the sort announced
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/operations"
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the table "Operations" has a row "demo-home" that shows "Succeeded"
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the table "Operations" has 2 rows
    And I press Tab until focus is on the button "Sort by started"
    And I press Enter
    Then the table "Operations" shows its "Started" column newest first
    And the polite region comes to contain "Sorted by started, newest first."

  # The daemon is the journey's own in the next four scenarios: its history and
  # its events stream say what the steps tell them to, so each update of a row
  # reaches the page when a step sends it, not when a real backup happens to
  # report one. Where a reader has put focus must outlast every update (WCAG
  # 2.4.3): the update is the one thing in the page that was not her doing.
  # A control the update takes away, as a run's end takes its Cancel button,
  # hands focus to its row's header, which names the run, so the reader is
  # still where she was and hears how the run ended as before.

  Scenario: Focus stays on a backup's Cancel button while its row updates
    Given the daemon reports a queued backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Queued"
    And I press Tab until focus is on the button "Cancel demo-home"
    And the daemon reports that the backup of "demo-home" has started
    And the daemon reports 4 steps of progress of the backup of "demo-home"
    Then the table "Operations" has a row "demo-home" that shows "Running"
    And the progress bar "Backup progress of demo-home" shows 100 percent
    And focus stays on the button "Cancel demo-home"

  Scenario: Focus moves to the row's header when the run of the focused Cancel button ends
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And I press Tab until focus is on the button "Cancel demo-home"
    And the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the table "Operations" has a row "demo-home" that shows "Succeeded"
    And focus is on the row header "demo-home"
    And the polite region comes to contain "Backup finished."

  # A cancelled run tells of no end on the events stream: the page reads the
  # history after the press until the run has ended there.
  Scenario: Focus moves to the row's header when the reader cancels the run of the focused Cancel button
    Given the daemon reports a running backup of "demo-home"
    And the daemon accepts each cancel the page asks for, and the run goes on
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And I press Tab until focus is on the button "Cancel demo-home"
    And I press Enter
    And the daemon has accepted the cancel of the backup of "demo-home"
    And the daemon reports that the backup of "demo-home" has been cancelled
    Then the table "Operations" has a row "demo-home" that shows "Cancelled"
    And focus is on the row header "demo-home"
    And the polite region comes to contain "Backup cancelled."

  Scenario: The operations table sorts oldest first on the second press of its sort button
    Given the daemon reports two finished backups of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has 2 rows
    And I press Tab until focus is on the button "Sort by started"
    And I press Enter
    Then the table "Operations" shows its "Started" column newest first
    When I press Enter
    Then the table "Operations" shows its "Started" column oldest first
    And the table "Operations" announces its "Started" column as sorted ascending
    And the polite region comes to contain "Sorted by started, oldest first."

  Scenario: The snapshots page announces the empty state of a plan without snapshots
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/snapshots"
    Then the page is titled "Snapshots – Ladderback"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the paragraph "No snapshots yet."
    And the page has no accessibility violations

  Scenario: The snapshots page lists a plan's snapshots newest first
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/operations"
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the table "Operations" has a row "demo-home" that shows "Succeeded"
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the table "Operations" has 2 rows
    And the table "Operations" has a row "demo-home" that shows "Succeeded"
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the table "Snapshots of plan demo-home" has 2 rows
    And the table "Snapshots of plan demo-home" shows its "Time" column newest first
    And the page has no accessibility violations

  # A listed snapshot's time is the one its own clock read, not the viewer's
  # (features/ui/snapshot_times.feature).
  Scenario: The operations page writes each time in the zone of the browser
    Given a demo plan "demo-home" with a scratch repository
    And the browser is in the time zone "Pacific/Auckland"
    When I open the address "/operations"
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the table "Operations" has a row "demo-home" that shows "Succeeded"
    Then the table "Operations" shows its "Started" column in the time zone "Pacific/Auckland"

  Scenario: The operations page announces its empty state
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/operations"
    Then the page shows the paragraph "No operations yet."
    And the page has no accessibility violations

  Scenario: A page opened mid-run shows the running operation before any event arrives
    Given a demo plan "demo-home" with a big scratch repository
    When I open the address "/operations"
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the table "Operations" has a row "demo-home" that shows "Running"
    And I reload the page
    Then the table "Operations" has a row "demo-home" that shows "Running"
    And the page has no accessibility violations

  # The daemon serves the page itself, so the state where it cannot be
  # reached is one the page is already open in: it stops while the reader is
  # looking at it, and the page says so without moving anywhere.
  Scenario: A page whose daemon has stopped shows an error state of its own
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/operations"
    And the daemon has stopped
    Then the page shows the heading "Cannot reach Ladderback"
    And the page has no accessibility violations

  Scenario: A page still waiting for the daemon's answer is accessible while it loads
    Given a demo plan "demo-home" with a scratch repository
    And the daemon's answers to the UI are delayed
    When I open the address "/operations"
    Then the page shows the paragraph "Loading…"
    And the page has no accessibility violations
