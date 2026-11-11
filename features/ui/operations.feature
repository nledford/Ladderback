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
