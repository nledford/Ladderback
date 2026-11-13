@ui
Feature: A running operation's progress bar can be reached by keyboard and never chatters
  A running operation's progress bar is named for what it measures, a
  backup's or a check's, and for its run, as its row is, and is a stop in
  the tab order, so a keyboard user can reach it and hear how far the run
  has come whenever she wants: the step, the percentage, how much of how
  much, and, once the step has gone on long enough to tell, about how long
  is left. The bar is drawn at every report, so it moves smoothly, unless
  the reader asks for less motion, but what it tells a screen reader changes
  at most every 30 seconds while the step stays the same, so a screen reader
  that speaks value changes doesn't speak at every report. A new step is told
  at once. Nothing about progress is ever written to a live region. When the
  run ends, a bar that held focus hands it to its row's header.
  The journeys play the daemon, and move the page's clock on, so each report
  and each half-minute comes when a step says.

  Scenario: A keyboard user reaches a running backup's progress bar and hears how far it has come and how long is left
    Given the page's clock can be moved on
    And the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports that the backup of "demo-home" is backing up, 2 GiB of 20 GiB done
    And I press Tab until focus is on the progressbar "Backup progress of demo-home"
    Then the progress bar "Backup progress of demo-home" reads "Backing up… 10%, 2 GiB of 20 GiB"
    When the page's clock moves on 60 seconds
    And the daemon reports that the backup of "demo-home" is backing up, 4 GiB of 20 GiB done
    Then the progress bar "Backup progress of demo-home" reads "Backing up… 20%, 4 GiB of 20 GiB, about 8 minutes left"
    And focus stays on the progressbar "Backup progress of demo-home"
    And the live regions stay silent
    And the page has no accessibility violations

  # 2 of 10 is told; 3 and 4 come within 30 seconds of it, so they are drawn
  # and not told, until the 30 seconds are over and the latest is told.
  Scenario: A bar is drawn at every report and tells a screen reader its value at most every 30 seconds
    Given the page's clock can be moved on
    And the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports that the backup of "demo-home" is reading the index, 2 of 10 done
    Then the progress bar "Backup progress of demo-home" shows 20 percent
    When the page's clock moves on 10 seconds
    And the daemon reports that the backup of "demo-home" is reading the index, 3 of 10 done
    Then the progress bar "Backup progress of demo-home" is filled to 30 percent
    And the progress bar "Backup progress of demo-home" shows 20 percent
    And the progress bar "Backup progress of demo-home" is drawn at 30 percent
    When the page's clock moves on 10 seconds
    And the daemon reports that the backup of "demo-home" is reading the index, 4 of 10 done
    Then the progress bar "Backup progress of demo-home" is drawn at 40 percent
    And the progress bar "Backup progress of demo-home" shows 20 percent
    When the page's clock moves on 10 seconds
    Then the progress bar "Backup progress of demo-home" shows 40 percent
    And the progress bar "Backup progress of demo-home" reads "Reading the index… 40%, 4 of 10"
    And the live regions stay silent

  Scenario: A check's progress bar is named for a check, not a backup
    Given the daemon reports a running check of "demo"
    When I open the address "/operations"
    And the table "Operations" has a row "Check of demo" that shows "Running"
    And the daemon reports that the check of "demo" is checking trees, 2 of 10 done
    Then the progress bar "Check progress of demo" reads "Checking what each snapshot holds… 20%, 2 of 10"

  Scenario: Two running backups' progress bars are told apart by their plans
    Given the daemon reports running backups of "demo-home" and "demo-photos"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-photos" that shows "Running"
    And the daemon reports that the backup of "demo-home" is reading the index, 2 of 10 done
    And the daemon reports that the backup of "demo-photos" is reading the index, 5 of 10 done
    Then the progress bar "Backup progress of demo-home" shows 20 percent
    And the progress bar "Backup progress of demo-photos" shows 50 percent
    And the page has no accessibility violations

  Scenario: Focus moves to the row's header when the run of the focused progress bar ends
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports that the backup of "demo-home" is backing up, 2 GiB of 20 GiB done
    And I press Tab until focus is on the progressbar "Backup progress of demo-home"
    And the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the table "Operations" shows no progress bar
    And focus is on the row header "demo-home"

  Scenario: A bar's fill moves smoothly, and jumps to each report for a reader who asks for less motion
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports that the backup of "demo-home" is backing up, 2 GiB of 20 GiB done
    Then the fill of the progress bar "Backup progress of demo-home" moves smoothly to each report
    When the reader asks for less motion
    Then the fill of the progress bar "Backup progress of demo-home" jumps to each report

  Scenario: A new step is told at once
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports that the backup of "demo-home" is reading the index, 3 of 10 done
    Then the progress bar "Backup progress of demo-home" reads "Reading the index… 30%, 3 of 10"
    When the daemon reports that the backup of "demo-home" is backing up, 1 GiB of 20 GiB done
    Then the progress bar "Backup progress of demo-home" shows 5 percent
    And the progress bar "Backup progress of demo-home" reads "Backing up… 5%, 1 GiB of 20 GiB"

  Scenario Outline: A running backup's progress bar reads in a pseudolocale and fits a small screen with larger text
    Given the daemon reports a running backup of "demo-home"
    And a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the address "/operations?locale=<locale>"
    And the page's one table has 1 row
    And the daemon reports that the backup of "demo-home" is backing up, 2 GiB of 20 GiB done
    And I press Tab until focus is on the page's one progress bar
    Then the page's one progress bar shows 10 percent
    And the value text of the page's one progress bar is transformed
    And the page doesn't scroll sideways
    And nothing in the page's one table runs outside its cell
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |
