@ui
Feature: Each row of the Operations page names its operation and says what it did
  The Operations page lists the operations of this agent: the backups of its
  plans, the maintenance of its repositories, a forget, an adoption, a prune
  and a check, and the restores and downloads read out of them. A backup's
  row is named by its plan, and any other row by what ran and the repository
  it ran on or read, under the alias this agent gave the repository. Once an
  operation has succeeded, its row lists what the operation recorded: a
  backup its snapshot, how many files and how much data it went through, what
  it added and how long it took, and a check what it found, and a row that
  was open as its run ended says it without a reload, even when the page lost
  its events stream on the way.
  The journeys run against a daemon this run started with a demo plan and a
  scratch repository, so every row they read is a real operation, except the
  ones that watch a run end: there the journey plays the daemon, so the end
  reaches the page when a step tells it, after the page is listening.

  Scenario: A check's row is named for the check and the repository it checked
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    And the repository "demo" has been checked
    When I open the address "/operations"
    Then the table "Operations" has the row "Check of demo"
    And the row "Check of demo" of the table "Operations" shows "Succeeded" in its "Status" column
    And the row "Check of demo" of the table "Operations" lists "found nothing wrong" in its "Result" column
    And the page has no accessibility violations

  Scenario: A restore's row is named for the entry it restored, the repository it read and the folder it went into
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the newest snapshot of the plan "home"
    And I skip to the content
    And I press Tab until focus is on a tree item
    And I press ArrowRight until focus is on the tree item "demo-1.txt"
    And I press Tab until focus is on the button "Restore demo-1.txt"
    And I press Enter
    And I press Tab until focus is on the textbox "Snapshot ID"
    And I type the newest snapshot's short ID of the plan "home"
    And I press Tab until focus is on the button "Restore"
    And I press Enter
    Then the polite region comes to contain "Restore of demo-1.txt finished."
    When I open the address "/operations"
    Then the table "Operations" has the row of a restore of "demo-1.txt" from "demo" into a new folder in the agent's restore folder
    And the table "Operations" has a row "demo-1.txt from demo to " that shows "Succeeded"
    And the page has no accessibility violations

  # A running restore's row, its Cancel button and its progress bar name what
  # it restores, the repository it reads and where it writes, so a reader can
  # tell two restores of the same repository apart; its end is announced with
  # the same names (#813).
  Scenario: A running restore into a new folder is named for its entry and its folder
    Given the daemon reports a running restore of "/home/docs" from "demo" into the folder "/srv/restore/laptop-3f2a1b4c"
    When I open the address "/operations"
    Then the table "Operations" has a row "Restore of /home/docs from demo to /srv/restore/laptop-3f2a1b4c" that shows "Running"
    And the page shows the button "Cancel the restore of /home/docs from demo to /srv/restore/laptop-3f2a1b4c"
    When the daemon reports that the restore of "/home/docs" is 2 of 10 done
    Then the progress bar "Restore progress of /home/docs from demo to /srv/restore/laptop-3f2a1b4c" shows 20 percent
    When the daemon reports that the restore of "/home/docs" has failed
    Then the assertive region comes to contain "Restore of /home/docs to /srv/restore/laptop-3f2a1b4c failed."
    And the page has no accessibility violations

  Scenario: A running restore over the original files is named as a restore in place
    Given the daemon reports a running restore of "/home/docs" from "demo" in place
    When I open the address "/operations"
    Then the table "Operations" has a row "In-place restore of /home/docs from demo" that shows "Running"
    And the page shows the button "Cancel the in-place restore of /home/docs from demo"
    When the daemon reports that the restore of "/home/docs" is 2 of 10 done
    Then the progress bar "In-place restore progress of /home/docs from demo" shows 20 percent
    When the daemon reports that the restore of "/home/docs" has succeeded
    Then the polite region comes to contain "In-place restore of /home/docs finished."
    And the page has no accessibility violations

  # A queued backup that waited, and found a newer run of its plan going
  # when its wait ended, gave way to it rather than run twice; its row says
  # so rather than blame a stopped daemon (#1245).
  Scenario: A backup that gave way to a newer run of its plan says so in its row
    Given the daemon reports a backup of "demo-home" that gave way to a newer run of its plan
    When I open the address "/operations"
    Then the row "demo-home" of the table "Operations" shows "Failed" in its "Status" column
    And the row "demo-home" of the table "Operations" shows "A newer run of this plan was already going" in its "Result" column
    And the page has no accessibility violations

  Scenario: A download's row is named for the download and the repository it read
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the newest snapshot of the plan "home"
    And I skip to the content
    And I press Tab until focus is on a tree item
    And I press ArrowRight until focus is on the tree item "demo-1.txt"
    And I press Tab until focus is on the button "Download demo-1.txt"
    And I press Enter
    And I press Tab until focus is on the textbox "Snapshot ID"
    And I type the newest snapshot's short ID of the plan "home"
    And I press Tab until focus is on the button "Download"
    And I press Enter
    Then the browser saves "demo-1.txt" holding what the plan backed up as "demo-1.txt"
    When I open the address "/operations"
    Then the table "Operations" has the row "Download from demo"
    And the page has no accessibility violations

  Scenario: A finished backup's row shows its snapshot and what it backed up
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the address "/operations"
    Then the table "Operations" has the row "home"
    And the row "home" of the table "Operations" lists the latest snapshot of the plan "home" in its "Result" column
    And the row "home" of the table "Operations" lists "3 files" in its "Result" column
    And the page has no accessibility violations

  # A failed run's Result says why it failed, in the page's language, with
  # the engine's own words marked as English (WCAG 3.1.2), and never the
  # password (I4, #474).
  Scenario: A failed backup's row says why it failed
    Given a demo plan "home" with a tiny scratch repository
    And the configuration has the password "canary-not-a-secret" for the repository "demo", which isn't its own
    And the configuration has the repository "demo" at a folder that isn't there
    And a backup of the plan "home" has failed
    When I open the address "/operations"
    Then the row "home" of the table "Operations" shows "Failed" in its "Status" column
    And the row "home" of the table "Operations" shows "No repository was found at this location" in its "Result" column
    And the row "home" of the table "Operations" gives the engine's own words in English in its "Result" column
    And the row "home" of the table "Operations" doesn't show "canary-not-a-secret" in its "Result" column
    And the page has no accessibility violations

  # The table takes its name from the page's heading, which says what the
  # rows are, so the name is shown and heard once, not again as a caption
  # under the heading (#431).
  Scenario: The Operations table is named by the page's heading, which it doesn't repeat
    Given the daemon reports two finished backups of "demo-home"
    When I open the address "/operations"
    Then the table "Operations" has 2 rows
    And the main content shows "Operations" once, as its heading
    And the page has no accessibility violations

  # The Result column has the room the other columns leave: each of those is
  # as wide as what it holds needs, Actions as wide as a Cancel button, so a
  # reason reads in a few lines rather than in a column of words. A name, a
  # path or an address in it stays whole on a line that has room for it and
  # moves to the next line whole rather than breaking at a hyphen. A reason
  # alone is a sentence; a list is for two or more items, one an item (#430).
  Scenario: A failed backup's one reason is a sentence with the room it needs, its path whole
    Given the daemon reports a backup of "demo-home" that failed because it could not read "/srv/backups/home-laptop.password"
    And a viewport of 1280 by 800 pixels
    When I open the address "/operations"
    Then the table "Operations" has 1 rows
    And the column "Result" of the table "Operations" is wider than each of the others
    And the row "demo-home" of the table "Operations" says "could not read" in its "Result" column, not as a list
    And the row "demo-home" of the table "Operations" keeps "/srv/backups/home-laptop.password" on one line in its "Result" column
    And the page has no accessibility violations

  # A running run's Cancel button says "Cancel" and is named for its run, so
  # it takes one line in the Actions column beside the Result column (#430).
  Scenario: A running backup's Cancel button takes one line on a wide screen
    Given the daemon reports a running backup of "demo-home"
    And a viewport of 1280 by 800 pixels
    When I open the address "/operations"
    Then the table "Operations" has a row "demo-home" that shows "Running"
    And the button "Cancel demo-home" shows "Cancel", which its name begins with
    And the button "Cancel demo-home" sits on one line
    And the page has no accessibility violations

  # Once a run has ended its Actions cell is empty, and the Result column
  # beside it has the room for each fact on a line of its own (#430).
  Scenario: A finished backup's facts each read on one line on a wide screen
    Given the daemon reports a finished backup of "demo-home" that went through 152.1 GiB
    And a viewport of 1280 by 800 pixels
    When I open the address "/operations"
    Then the table "Operations" has 1 rows
    And the row "demo-home" of the table "Operations" lists "152.1 GiB processed" in its "Result" column
    And the row "demo-home" of the table "Operations" lists each item on one line in its "Result" column
    And the page has no accessibility violations

  # A status badge's words wrap between words beside its icon (#1070): on a
  # wide screen the Status column holds the icon and "Succeeded" on one line,
  # and "Succeeded with warnings" wraps after the icon, never under it.
  Scenario: A finished backup's status shows its icon and its words on one line on a wide screen
    Given the daemon reports a finished backup of "demo-home" that went through 152.1 GiB
    And a viewport of 1280 by 800 pixels
    When I open the address "/operations"
    Then the row "demo-home" of the table "Operations" shows "Succeeded" in its "Status" column
    And the status badge of the row "demo-home" of the table "Operations" shows its icon and its words on one line

  Scenario: A status that succeeded with warnings wraps its words beside its icon on a wide screen
    Given the daemon reports a backup of "demo-home" that succeeded with warnings
    And a viewport of 1280 by 800 pixels
    When I open the address "/operations"
    Then the row "demo-home" of the table "Operations" shows "Succeeded with warnings" in its "Status" column
    And the status badge of the row "demo-home" of the table "Operations" wraps its words beside its icon, which stands at the start of its first line

  # The Status column's width above 48rem of room (#1070) is never less than
  # its share just below: a column that narrowed as the window widened would
  # wrap the words its badge held on one line a moment before.
  Scenario: The Status column never narrows as the window widens
    Given the daemon reports a finished backup of "demo-home" that went through 152.1 GiB
    And a viewport of 640 by 800 pixels
    When I open the address "/operations"
    Then the row "demo-home" of the table "Operations" shows "Succeeded" in its "Status" column
    And the "Status" column of the table "Operations" never narrows as the window widens to 1440 pixels

  Scenario: A failed backup's reason fits its cell on a small screen with larger text
    Given the daemon reports a backup of "demo-home" that failed because it could not read "/srv/backups/home-laptop.password"
    And a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the address "/operations"
    Then the table "Operations" has 1 rows
    And the row "demo-home" of the table "Operations" says "could not read" in its "Result" column, not as a list
    And the page doesn't scroll sideways
    And nothing in the table "Operations" runs outside its cell
    And no text is clipped
    And the page has no accessibility violations

  # A backup learns its size only once rustic has walked the whole source,
  # which on a big folder takes minutes; until then the row says the backup
  # is working and shows no percentage, never 0 percent (#422). Once the
  # size is known, the bar shows how far the backup has come.
  Scenario: A running backup whose size isn't known yet shows no percentage
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports 8 bytes processed of the backup of "demo-home" and not yet its total
    Then the row "demo-home" of the table "Operations" shows "Working…" in its "Status" column
    And the row "demo-home" of the table "Operations" shows no percentage in its "Status" column
    And the progress bar "Backup progress of demo-home" shows no percentage
    And the page has no accessibility violations
    When the daemon reports 4 steps of progress of the backup of "demo-home"
    Then the progress bar "Backup progress of demo-home" shows 100 percent
    And the row "demo-home" of the table "Operations" doesn't show "Working…" in its "Status" column

  # A run with no total shows no percentage, so its row says when the run
  # last reported: an instant that changes only when the run reports again.
  # A run that has stopped reporting keeps an old time, which a working run
  # doesn't, and nothing on the page ticks by itself (WCAG 2.2.2).
  Scenario: A running backup whose size isn't known yet says when it last reported, and only a new report changes it
    Given the page's clock can be moved on
    And the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports 8 bytes processed of the backup of "demo-home" and not yet its total, at "2026-11-09T10:01:00Z"
    Then the row "demo-home" of the table "Operations" shows "Working…" in its "Status" column
    And the progress bar "Backup progress of demo-home" shows its last report at "2026-11-09T10:01:00Z"
    And the progress bar "Backup progress of demo-home" reads "Working…" and its last report at "2026-11-09T10:01:00Z"
    When the page's clock moves on 60 seconds
    Then the progress bar "Backup progress of demo-home" shows its last report at "2026-11-09T10:01:00Z"
    When the daemon reports 16 bytes processed of the backup of "demo-home" and not yet its total, at "2026-11-09T10:02:30Z"
    Then the progress bar "Backup progress of demo-home" shows its last report at "2026-11-09T10:02:30Z"
    And the progress bar "Backup progress of demo-home" reads "Working…" and its last report at "2026-11-09T10:02:30Z"
    And the live regions stay silent
    And the page has no accessibility violations

  # A backup reads the repository's index before it backs up, and its row
  # says which step it is in (#423). The index's count is that step's, shown
  # with it, never as the backup's percentage; the backup shows its own
  # percentage once its total is known.
  Scenario: A running backup's row says it is reading the index, then backing up
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports that the backup of "demo-home" is reading the index, 3 of 10 done
    Then the row "demo-home" of the table "Operations" shows "Reading the index…" in its "Status" column
    And the progress bar "Backup progress of demo-home" shows 30 percent
    When the daemon reports that the backup of "demo-home" is backing up, 8 done of a total not known yet
    Then the row "demo-home" of the table "Operations" shows "Backing up…" in its "Status" column
    And the row "demo-home" of the table "Operations" doesn't show "Reading the index…" in its "Status" column
    And the row "demo-home" of the table "Operations" shows no percentage in its "Status" column
    And the progress bar "Backup progress of demo-home" shows no percentage
    And the page has no accessibility violations
    When the daemon reports that the backup of "demo-home" is backing up, 4 of 4 done
    Then the progress bar "Backup progress of demo-home" shows 100 percent
    And the row "demo-home" of the table "Operations" shows "Backing up…" in its "Status" column

  Scenario: A backup that ends while its row is open shows what it backed up
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the row "demo-home" of the table "Operations" shows "Succeeded" in its "Status" column
    And the row "demo-home" of the table "Operations" lists "3 files" in its "Result" column

  Scenario: A backup that ends while its row is open changes its status badge's icon
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the icon of the status badge of the row "demo-home" of the table "Operations" is noted
    And the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the row "demo-home" of the table "Operations" shows "Succeeded" in its "Status" column
    And the status badge of the row "demo-home" of the table "Operations" draws another icon than the one noted

  # A run that ended with warnings is a success with something to read: its
  # status is drawn in the Warning tone, with the warning icon, never in a
  # clean success's green, so it doesn't read as one that needs nothing
  # (#942). Its words say it too, so color is never the only signal.
  Scenario: An operation that succeeded with warnings shows its status as a warning
    Given the daemon reports a backup of "demo-home" that succeeded with warnings
    When I open the address "/operations"
    Then the row "demo-home" of the table "Operations" shows "Succeeded with warnings" in its "Status" column
    And the status badge of the row "demo-home" of the table "Operations" is drawn in the Warning tone, with the warning icon
    And the page has no accessibility violations

  Scenario: Runs that end while the page reads its history and reconnects each show what they did
    Given the daemon reports running backups of "demo-home" and "demo-docs"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-docs" that shows "Running"
    And the daemon holds back its answer to the next reading of the history
    And the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the page reads the history again
    When the daemon's events stream drops
    And the daemon reports that the backup of "demo-docs" has succeeded with 5 files
    Then the row "demo-docs" of the table "Operations" shows "Succeeded" in its "Status" column
    And the page reads the history no more while that answer is held back
    When the daemon gives the answer it held back
    Then the row "demo-home" of the table "Operations" lists "3 files" in its "Result" column
    And the row "demo-docs" of the table "Operations" lists "5 files" in its "Result" column

  # A run can start while the page is still opening its events stream, so
  # the news of the start goes out before the page listens (#305). The page
  # reads the history once its stream is open, and the run shows as running
  # all the same.
  Scenario: A run that starts while the page opens its events stream shows as running
    Given the daemon reports a queued backup of "demo-home"
    And the daemon holds back the opening of the page's events stream
    When I open the address "/operations"
    And the backup of "demo-home" starts before the page's events stream is open
    And the daemon opens the events stream it held back
    Then the row "demo-home" of the table "Operations" shows "Running" in its "Status" column

  # A run can warn before the page listens: a before-hook's warning, which no
  # event tells, is in the history while the run goes on (#328). The page
  # reads the history once its stream is open, and the run ends on the page
  # as it ended in the history, with its warnings, told once.
  Scenario: A backup that warned before the page's events stream opened ends with its warnings
    Given the daemon reports a running backup of "demo-home"
    And the daemon holds back the opening of the page's events stream
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And a before-hook of the backup of "demo-home" warns before the page's events stream is open
    And the daemon opens the events stream it held back
    And the page has read the history again since its events stream opened
    And the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the row "demo-home" of the table "Operations" shows "Succeeded with warnings" in its "Status" column
    And the polite region reads "Backup finished."
    And the polite region keeps reading "Backup finished."

  # A before-hook's warning is told to an open page as the hook ends, as an
  # engine's warning is (#369): the run ends on the page with its warnings
  # as soon as its end is heard, before the page has read the history
  # again, and its end is told once.
  Scenario: A before-hook's warning reaches an open Operations page as it happens
    Given a demo plan "home" whose before-hooks wait until they are let go, then warn
    And a backup of the plan "home" has been asked for
    When I open the address "/operations"
    And the table "Operations" has a row "home" that shows "Queued"
    And the daemon's next answer to a reading of the history is held back
    And the before-hooks of the plan "home" are let go
    Then the row "home" of the table "Operations" shows "Succeeded with warnings" in its "Status" column
    And the page's reading of the history is still held back
    And the polite region reads "Backup finished."
    And the polite region keeps reading "Backup finished."

  # A backup a before-hook stopped says which hook it was, by its place in
  # the plan's run-before list, and how it ended, never what the hook wrote:
  # that could hold a secret (#480, #853).
  Scenario: A backup a before-hook stopped says which hook failed and how
    Given a demo plan "home" whose before-hook says "canary-hook-output" and exits 3, with on-failure error
    And a backup of the plan "home" has failed
    When I open the address "/operations"
    Then the row "home" of the table "Operations" shows "Failed" in its "Status" column
    And the row "home" of the table "Operations" shows "Hook 1 of the plan's run-before list failed" in its "Result" column
    And the row "home" of the table "Operations" shows "It exited with status 3." in its "Result" column
    And the row "home" of the table "Operations" doesn't show "canary-hook-output" in its "Result" column
    And the page has no accessibility violations

  # The page listens to the daemon only while the reader is on it: leaving it
  # closes its events stream, ends the wait to reconnect a dropped one, and
  # stops the polling of a run the page started or canceled, so coming back
  # opens one stream, never a second beside the first (#300).
  Scenario: Leaving the Operations page closes its events stream, and coming back opens one
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And I press Shift+Tab until focus is on the link "Repositories"
    And I press Enter
    Then the page stops listening to the daemon
    When I press Shift+Tab until focus is on the link "Operations"
    And I press Enter
    Then the table "Operations" has a row "demo-home" that shows "Running"
    And the page has one events stream open
    When the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the row "demo-home" of the table "Operations" shows "Succeeded" in its "Status" column

  # A start's wait for the run's end lasts two minutes of the page's clock;
  # once they run out with the run still queued, the page says it is still
  # waiting, in a toast, since the row still shows the run as it was.
  Scenario: A start whose end the page stops waiting for is answered in a toast, said once, and focus stays on its button
    Given the page's clock can be moved on
    And the daemon reports a running backup of "demo-home"
    And the daemon queues each backup the page starts
    When I open the address "/operations"
    And I skip to the content
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the polite region comes to contain "Backup started."
    And the page's clock moves on until the page stops waiting for the run's end
    Then the toast reads "Still waiting for Ladderback to say how the run ended." after its icon "Information"
    And the toast is no live region, nor holds one
    And focus stays on the button "Start backup of demo-home"
    And the polite region reads "Still waiting for Ladderback to say how the run ended."
    And the polite region keeps reading "Still waiting for Ladderback to say how the run ended."
    And the page has no accessibility violations

  # The page keeps the toast's room above the table from its first paint,
  # one line of a toast tall, and never gives it back while the page is
  # open, so a toast coming or going never moves a row's button under the
  # reader's pointer.
  Scenario: A toast moves none of the operations table's rows when it comes, when it goes by itself, nor when it is dismissed
    Given the page's clock can be moved on
    And the daemon reports a running backup of "demo-home"
    And the daemon queues each backup the page starts
    And a viewport of 1280 by 800 pixels
    When I open the address "/operations"
    And I skip to the content
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the polite region comes to contain "Backup started."
    And the table "Operations" has 2 rows
    And I note where the first row's button of the table "Operations" lies
    And the page's clock stands still until a step moves it on
    And the page's clock moves on until the page stops waiting for the run's end
    Then the toast reads "Still waiting for Ladderback to say how the run ended." after its icon "Information"
    And the first row's button of the table "Operations" lies where it was
    When the page's clock moves on 7 seconds
    Then the page shows no toast
    And the first row's button of the table "Operations" lies where it was
    When I press Shift+Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the table "Operations" has 3 rows
    And I note where the first row's button of the table "Operations" lies
    And the page's clock moves on until the page stops waiting for the run's end
    Then the first row's button of the table "Operations" lies where it was
    When I press Tab until focus is on the toast's Dismiss button
    And I press Enter
    Then the page shows no toast
    And the first row's button of the table "Operations" lies where it was

  # On a small screen with larger text the toast's words take several lines,
  # more than the one the page keeps; the first toast moves the table down
  # once, by those lines, and from then on the page keeps that room.
  Scenario: On a small screen with larger text, a toast moves none of the operations table's rows when it goes by itself, when it comes again, nor when it is dismissed
    Given the page's clock can be moved on
    And the daemon reports a running backup of "demo-home"
    And the daemon queues each backup the page starts
    And a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the address "/operations"
    And I skip to the content
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the polite region comes to contain "Backup started."
    And the table "Operations" has 2 rows
    And the page's clock stands still until a step moves it on
    And the page's clock moves on until the page stops waiting for the run's end
    Then the toast reads "Still waiting for Ladderback to say how the run ended." after its icon "Information"
    When I note where the first row's button of the table "Operations" lies
    And the page's clock moves on 7 seconds
    Then the page shows no toast
    And the first row's button of the table "Operations" lies where it was
    When I press Shift+Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the table "Operations" has 3 rows
    And I note where the first row's button of the table "Operations" lies
    And the page's clock moves on until the page stops waiting for the run's end
    Then the first row's button of the table "Operations" lies where it was
    When I press Tab until focus is on the toast's Dismiss button
    And I press Enter
    Then the page shows no toast
    And the first row's button of the table "Operations" lies where it was
    And the page doesn't scroll sideways

  Scenario: Leaving the Operations page while it waits on a run and on its events stream ends both waits
    Given the daemon reports a running backup of "demo-home"
    And the daemon queues each backup the page starts
    When I open the address "/operations"
    And I skip to the content
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    And the polite region comes to contain "Backup started."
    And the daemon's events stream drops
    And I press Shift+Tab until focus is on the link "Repositories"
    And I press Enter
    Then the page stops listening to the daemon

  Scenario: Leaving the Operations page while it waits on a canceled run ends the wait
    Given the daemon reports a running backup of "demo-home"
    And the daemon accepts each cancel the page asks for, and the run goes on
    When I open the address "/operations"
    And I skip to the content
    And I press Tab until focus is on the button "Cancel demo-home"
    And I press Enter
    And the daemon has accepted the cancel of the backup of "demo-home"
    And I press Shift+Tab until focus is on the link "Repositories"
    And I press Enter
    Then the page stops listening to the daemon

  # The table's columns keep the widths they open with: a run that starts,
  # shows its progress and ends changes what its row says, never where the
  # columns are, so the table doesn't jump under the reader's eyes. The sort
  # button sits in the Started header beside its text, never over it, and on a
  # small screen or with larger text it goes below the text. Data tables are
  # read as a grid, so on a small screen a word breaks inside only when it is
  # longer than its column is wide (WCAG 1.4.10), and nothing in a cell runs
  # over the next column, even at 320 px with 200 % text.

  Scenario Outline: The columns of the Operations table keep their widths as a run starts, shows its progress and ends
    Given the daemon reports a queued backup of "demo-home"
    And a viewport of <width> by <height> pixels
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Queued"
    And the widths of the columns of the table "Operations" are noted
    And the daemon reports that the backup of "demo-home" has started
    And the daemon reports 8 bytes processed of the backup of "demo-home" and not yet its total
    Then the row "demo-home" of the table "Operations" shows "Working…" in its "Status" column
    And the columns of the table "Operations" keep the widths noted
    And the page doesn't scroll sideways
    And nothing in the table "Operations" runs outside its cell
    When the daemon reports 4 steps of progress of the backup of "demo-home"
    Then the progress bar "Backup progress of demo-home" shows 100 percent
    And the columns of the table "Operations" keep the widths noted
    And the page doesn't scroll sideways
    And nothing in the table "Operations" runs outside its cell
    When the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the row "demo-home" of the table "Operations" lists "3 files" in its "Result" column
    And the columns of the table "Operations" keep the widths noted
    And the page doesn't scroll sideways
    And nothing in the table "Operations" runs outside its cell

    Examples:
      | width | height |
      | 1280  | 800    |
      | 320   | 256    |

  Scenario Outline: The sort button sits in the Started header apart from its text
    Given the daemon reports two finished backups of "demo-home"
    And a viewport of <width> by <height> pixels
    When I open the address "/operations"
    And the table "Operations" has 2 rows
    Then the button "Sort by started" sits inside the column header "Started", apart from its text
    And the button "Sort by started" sits on one line
    And the page has no accessibility violations

    Examples:
      | width | height |
      | 1280  | 800    |
      | 800   | 600    |

  Scenario: The Operations page reflows with larger text and wider spacing while a run shows its progress and when it ends
    Given the daemon reports a running backup of "demo-home"
    And a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the reader increases the text spacing
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports 8 bytes processed of the backup of "demo-home" and not yet its total
    Then the row "demo-home" of the table "Operations" shows "Working…" in its "Status" column
    And the page doesn't scroll sideways
    And nothing in the table "Operations" runs outside its cell
    And no text is clipped
    When the daemon reports 4 steps of progress of the backup of "demo-home"
    Then the progress bar "Backup progress of demo-home" shows 100 percent
    And the page doesn't scroll sideways
    And nothing in the table "Operations" runs outside its cell
    And no text is clipped
    And the button "Sort by started" sits inside the column header "Started", apart from its text
    And the row header "demo-home" breaks a word only where the word is longer than a line
    And the button "Cancel demo-home" breaks a word only where the word is longer than a line
    And the page has no accessibility violations
    When the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the row "demo-home" of the table "Operations" lists "3 files" in its "Result" column
    And the page doesn't scroll sideways
    And nothing in the table "Operations" runs outside its cell
    And no text is clipped

  # The step a running operation is in is a line of its own in the Status
  # cell, above the bar when the bar shows a percentage and in place of its
  # percentage when it can't. Text a translation makes longer still wraps
  # inside the cell on a small screen with larger text (#423).

  Scenario Outline: A running backup's step fits its cell on a small screen with larger text, in a pseudolocale
    Given the daemon reports a running backup of "demo-home"
    And a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the address "/operations?locale=<locale>"
    And the page's one table has 1 row
    And the daemon reports that the backup of "demo-home" is reading the index, 3 of 10 done
    Then the page's one progress bar shows 30 percent
    And the cell of the page's one progress bar says its status and its step, both transformed
    And the page doesn't scroll sideways
    And nothing in the page's one table runs outside its cell
    And no text is clipped
    And the page has no accessibility violations
    When the daemon reports that the backup of "demo-home" is backing up, 8 done of a total not known yet
    Then the page's one progress bar shows no percentage
    And the cell of the page's one progress bar says its status and its step, both transformed
    And the page doesn't scroll sideways
    And nothing in the page's one table runs outside its cell
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  # Each plan's start button stands apart from the next one's, as the
  # controls of every other row of actions do, so a reader aiming at one plan's
  # button doesn't press the other's. On a small screen with larger text their
  # labels grow with the text, and the buttons wrap, one under the other, and
  # still stand apart.

  Scenario: Each plan's start button stands apart from the next one's
    Given the daemon reports running backups of "demo-home" and "demo-docs"
    And a viewport of 1280 by 800 pixels
    When I open the address "/operations"
    And the table "Operations" has 2 rows
    Then the buttons "Start backup of demo-home" and "Start backup of demo-docs" stand apart
    And the page has no accessibility violations

  Scenario: The start buttons' labels grow on a small screen with larger text and the buttons stand apart
    Given the daemon reports running backups of "demo-home" and "demo-docs"
    And a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the address "/operations"
    And the table "Operations" has 2 rows
    Then the label of the button "Start backup of demo-home" is as large as the page's text
    And the buttons "Start backup of demo-home" and "Start backup of demo-docs" stand apart
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: A mistake in the configuration file is shown where it is as an error alert, said assertively, by keyboard alone
    Given a daemon with a scratch demo repository and no repositories
    And someone has written a setting the daemon does not know on line 6 of its configuration file
    When I open the web UI
    And I press Tab until focus is on the link "Operations"
    And I press Enter
    Then focus is on the main heading "Operations"
    And the page shows an error alert containing "did not accept its configuration file"
    And the page shows an error alert containing "line 6: repositories.demo.color"
    And the assertive region comes to contain "did not accept its configuration file"
    And the alert marks the sender's words as English
    And the page doesn't show "demo-password"
    And the page has no accessibility violations

  Scenario: A backup refused for a mistake in the configuration file says where it is in an error alert, said assertively, by keyboard alone
    Given a demo plan "home" with a tiny scratch repository
    When I open the address "/operations"
    And I skip to the content
    And I press Tab until focus is on the button "Start backup of home"
    And someone has written a setting the daemon does not know on line 6 of its configuration file
    And I press Enter
    Then the page shows an error alert containing "line 6: repositories.demo.color"
    And the assertive region comes to contain "line 6: repositories.demo.color"
    And the alert marks the sender's words as English
    And the page doesn't show "demo-password"
    And the page has no accessibility violations
