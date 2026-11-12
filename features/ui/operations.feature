@ui
Feature: Each row of the Operations page names its operation and says what it did
  The Operations page lists the operations of this agent: the backups of its
  plans and the maintenance of its repositories, a forget, an adoption, a
  prune and a check. A backup's row is named by its plan, and a maintenance
  row by what ran and the repository it ran on, under the alias this agent
  gave the repository. Once an operation has succeeded, its row lists what the
  operation recorded: a backup its snapshot, how many files and how much data
  it went through, what it added and how long it took, and a check what it
  found, and a row that was open as its run ended says it without a reload,
  even when the page lost its events stream on the way.
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

  Scenario: A finished backup's row shows its snapshot and what it backed up
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the address "/operations"
    Then the table "Operations" has the row "home"
    And the row "home" of the table "Operations" lists the latest snapshot of the plan "home" in its "Result" column
    And the row "home" of the table "Operations" lists "3 files" in its "Result" column
    And the page has no accessibility violations

  Scenario: A backup that ends while its row is open shows what it backed up
    Given the daemon reports a running backup of "demo-home"
    When I open the address "/operations"
    And the table "Operations" has a row "demo-home" that shows "Running"
    And the daemon reports that the backup of "demo-home" has succeeded with 3 files
    Then the row "demo-home" of the table "Operations" shows "Succeeded" in its "Status" column
    And the row "demo-home" of the table "Operations" lists "3 files" in its "Result" column

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
    And the daemon reports 4 steps of progress of the backup of "demo-home"
    Then the progress bar "Backup progress" shows 100 percent
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
    And the daemon reports 4 steps of progress of the backup of "demo-home"
    Then the progress bar "Backup progress" shows 100 percent
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
