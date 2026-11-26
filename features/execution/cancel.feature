Feature: Canceling an operation
  Canceling stops the engine. An operation the user canceled is recorded
  as canceled, never as failed, and a canceled backup writes no snapshot.
  What a canceled backup had already saved stays in the repository's
  index, so a check has nothing to warn about and the next backup reuses it.

  Scenario: A canceled operation is recorded as canceled
    Given the engine will complete the operation in 10 steps of 1 minute each
    When the agent runs the operation and the user cancels it after 3 minutes
    Then the operation ends as canceled
    And the operation took 3 minutes
    And the history shows the operation as it ended

  Scenario: A cancel made before the engine is asked for anything ends the operation as canceled
    Given the engine will complete the operation in 10 steps of 1 minute each
    When the agent runs the operation after the user has canceled it
    Then the operation ends as canceled
    And the history shows the operation as it ended

  # A file this size keeps the backup from ending before the cancel lands,
  # however long the runner takes to send it.
  @e2e
  Scenario: Canceling a backup stops its worker and writes no snapshot
    Given a new repository
    And a plan "home" backing up a folder with a 128 GiB file
    When the agent backs up the plan and the user cancels it once it has started
    Then the operation ends as canceled
    And the repository has no snapshots
    And the history shows the operation as it ended

  # The 128 GiB file keeps the backup from ending before the cancel lands,
  # however soon it has saved the new data; the next backup goes without it.
  @e2e
  Scenario: A canceled backup keeps the data it saved for the next backup
    Given a new repository that saves data in packs of 1 MiB
    And a plan "home" backing up a folder with 128 MiB of new data
    And the folder also holds a 128 GiB file
    When the agent backs up the plan and the user cancels it once the repository holds 2 packs
    Then the operation ends as canceled
    And the repository has no snapshots
    And a check of the repository finds nothing to warn about
    When the 128 GiB file is removed from the folder
    And the agent backs up the plan
    Then the backup adds less data than the folder holds
